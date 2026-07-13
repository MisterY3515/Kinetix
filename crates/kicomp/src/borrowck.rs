use crate::mir::{MirProgram, MirFunction, BasicBlockData, StatementKind, TerminatorKind, RValue, Operand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalState {
    Uninitialized,
    Initialized,
    Moved,
}

/// A naive Borrow Checker that analyzes MIR functions node-by-node.
///
/// Build 13: Graph Traversal
/// We will walk the basic blocks to ensure there are no cycles that crash the compiler
/// and establish the state machine for each local variable.
///
/// Build 38 Phase B1: real join-point merging. A block reachable from multiple
/// predecessors (e.g. after an `if`, or a loop header) is only as safe as its
/// *least* safe incoming path -- if a local is Moved on one branch and still
/// Initialized on another, using it after the merge must be flagged, since one
/// of those paths really did move it. This is a standard forward, monotone
/// dataflow analysis (entry state per block, refined to a fixpoint via a
/// worklist), not a single linear/DFS walk.
pub struct BorrowChecker;

impl BorrowChecker {
    pub fn new() -> Self {
        Self
    }

    pub fn check_program(&mut self, program: &MirProgram) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        for func in &program.functions {
            self.check_function(func, &mut errors);
        }

        self.check_function(&program.main_block, &mut errors);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Successor blocks reachable directly from `block`'s terminator.
    fn successors(block: &BasicBlockData) -> Vec<usize> {
        match block.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::Goto(target)) => vec![target.0],
            Some(TerminatorKind::Branch { then_block, else_block, .. }) => vec![then_block.0, else_block.0],
            Some(TerminatorKind::Return(_)) | None => vec![],
        }
    }

    /// Merges two incoming states for the same block: a local is only
    /// considered Initialized at a join point if it's Initialized on *both*
    /// incoming paths; otherwise it's conservatively treated as Moved (the
    /// existing error messages already say "uninitialized or moved" for both
    /// cases, so which of the two unsafe states we pick doesn't matter).
    fn join_states(a: &[LocalState], b: &[LocalState]) -> Vec<LocalState> {
        a.iter().zip(b.iter()).map(|(&x, &y)| if x == y { x } else { LocalState::Moved }).collect()
    }

    fn apply_operand_state(op: &Operand, states: &mut [LocalState]) {
        if let Operand::Move(p) = op {
            states[p.local.0] = LocalState::Moved;
        }
    }

    fn apply_rvalue_state(rvalue: &RValue, states: &mut [LocalState]) {
        match rvalue {
            RValue::Use(op) => Self::apply_operand_state(op, states),
            RValue::UnaryOp(_, op) => Self::apply_operand_state(op, states),
            RValue::BinaryOp(_, lhs, rhs) => {
                Self::apply_operand_state(lhs, states);
                Self::apply_operand_state(rhs, states);
            }
            RValue::Call(func_op, args) => {
                Self::apply_operand_state(func_op, states);
                for arg in args {
                    Self::apply_operand_state(arg, states);
                }
            }
            RValue::Array(elems) => {
                for elem in elems {
                    Self::apply_operand_state(elem, states);
                }
            }
            RValue::Aggregate(_, ops) => {
                for op in ops {
                    Self::apply_operand_state(op, states);
                }
            }
        }
    }

    /// Simulates a block's statements starting from `states`, applying state
    /// transitions only (no error reporting) -- used while the fixpoint is
    /// still converging, since a not-yet-fully-merged entry state could
    /// otherwise produce spurious errors before all predecessors are known.
    fn simulate_block(block: &BasicBlockData, states: &mut Vec<LocalState>) {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::Assign(place, rvalue) => {
                    Self::apply_rvalue_state(rvalue, states);
                    states[place.local.0] = LocalState::Initialized;
                }
                StatementKind::Expression(rvalue) => {
                    Self::apply_rvalue_state(rvalue, states);
                }
                StatementKind::Drop(_) => {}
            }
        }
    }

    fn check_function(&mut self, func: &MirFunction, errors: &mut Vec<String>) {
        if func.basic_blocks.is_empty() { return; }

        let n = func.basic_blocks.len();
        let mut initial_states: Vec<LocalState> = vec![LocalState::Uninitialized; func.locals.len()];
        for arg in &func.args {
            initial_states[arg.0] = LocalState::Initialized;
        }

        // Entry state per block, refined to a fixpoint.
        let mut entry: Vec<Option<Vec<LocalState>>> = vec![None; n];
        entry[0] = Some(initial_states);

        let mut worklist: Vec<usize> = vec![0];
        let mut queued = vec![false; n];
        queued[0] = true;

        while let Some(block_idx) = worklist.pop() {
            queued[block_idx] = false;
            let mut states = entry[block_idx].clone().expect("entry state set before a block is processed");
            let block = &func.basic_blocks[block_idx];
            Self::simulate_block(block, &mut states);

            for succ in Self::successors(block) {
                let merged = match &entry[succ] {
                    None => states.clone(),
                    Some(existing) => Self::join_states(existing, &states),
                };
                if entry[succ].as_ref() != Some(&merged) {
                    entry[succ] = Some(merged);
                    if !queued[succ] {
                        worklist.push(succ);
                        queued[succ] = true;
                    }
                }
            }
        }

        // Final pass over every block using its converged entry state -- this is
        // where errors are actually reported.
        for block_idx in 0..n {
            let mut states = entry[block_idx].clone()
                .unwrap_or_else(|| vec![LocalState::Uninitialized; func.locals.len()]);
            let block = &func.basic_blocks[block_idx];
            for stmt in &block.statements {
                match &stmt.kind {
                    StatementKind::Assign(place, rvalue) => {
                        self.check_rvalue(rvalue, stmt.line, &func.locals, &mut states, errors);
                        states[place.local.0] = LocalState::Initialized;
                    }
                    StatementKind::Expression(rvalue) => {
                        self.check_rvalue(rvalue, stmt.line, &func.locals, &mut states, errors);
                    }
                    StatementKind::Drop(_place) => {
                        // Drop allows a Moved value (it's a no-op at runtime),
                        // but if we had strict linear checking, we'd ensure it wasn't uninitialized.
                    }
                }
            }
        }
    }

    fn check_rvalue(&self, rvalue: &RValue, line: usize, locals: &[crate::mir::LocalDecl], states: &mut [LocalState], errors: &mut Vec<String>) {
        match rvalue {
            RValue::Use(op) => self.check_operand(op, line, locals, states, errors),
            RValue::UnaryOp(_, op) => self.check_operand(op, line, locals, states, errors),
            RValue::BinaryOp(_, lhs, rhs) => {
                self.check_operand(lhs, line, locals, states, errors);
                self.check_operand(rhs, line, locals, states, errors);
            }
            RValue::Call(func_op, args) => {
                self.check_operand(func_op, line, locals, states, errors);
                for arg in args {
                    self.check_operand(arg, line, locals, states, errors);
                }
            }
            RValue::Array(elems) => {
                for elem in elems {
                    self.check_operand(elem, line, locals, states, errors);
                }
            }
            RValue::Aggregate(_, ops) => {
                for op in ops {
                    self.check_operand(op, line, locals, states, errors);
                }
            }
        }
    }

    fn check_operand(&self, op: &Operand, line: usize, locals: &[crate::mir::LocalDecl], states: &mut [LocalState], errors: &mut Vec<String>) {
        match op {
            Operand::Copy(p) => {
                if states[p.local.0] != LocalState::Initialized {
                    let name = locals[p.local.0].name.as_deref().unwrap_or("unknown");
                    errors.push(format!("Line {}: Cannot copy from an uninitialized or moved variable '{}'", line, name));
                }
            }
            Operand::Move(p) => {
                if states[p.local.0] != LocalState::Initialized {
                    let name = locals[p.local.0].name.as_deref().unwrap_or("unknown");
                    errors.push(format!("Line {}: Use of uninitialized or moved variable '{}'", line, name));
                }
                states[p.local.0] = LocalState::Moved;
            }
            Operand::Borrow(p, _) => {
                if states[p.local.0] != LocalState::Initialized {
                    let name = locals[p.local.0].name.as_deref().unwrap_or("unknown");
                    errors.push(format!("Line {}: Borrow of uninitialized or moved variable '{}'", line, name));
                }
            }
            Operand::Constant(_) => {}
        }
    }
}

pub fn check_mir(program: &MirProgram) -> Result<(), Vec<String>> {
    let mut borrowck = BorrowChecker::new();
    borrowck.check_program(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use kinetix_language::lexer::Lexer;
    use kinetix_language::parser::Parser;
    use crate::symbol::resolve_program;
    use crate::hir::lower_to_hir;
    use crate::typeck::TypeContext;
    use crate::mir::lower_to_mir;

    fn compile_to_mir(src: &str) -> MirProgram {
        let arena = Bump::new();
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, &arena);
        let program = parser.parse_program();
        let symbols = resolve_program(&program.statements).unwrap();
        let traits = crate::trait_solver::TraitEnvironment::new();
        let hir = lower_to_hir(&program.statements, &symbols, &traits);
        let mut ctx = TypeContext::new();
        let constraints = ctx.collect_constraints(&hir);
        ctx.solve(&constraints).unwrap();
        lower_to_mir(&hir, &ctx.substitution)
    }

    #[test]
    fn test_borrowck_traversal() {
        let mir = compile_to_mir("let x = 42\nlet y = x\nlet z = x");
        let result = check_mir(&mir);
        assert!(result.is_ok(), "CFG traversal or copy check failed for Int");
    }

    #[test]
    fn test_borrowck_rejects_use_after_move() {
        let mir = compile_to_mir("let a = \"hello\"\nlet b = a\nlet c = a");
        let result = check_mir(&mir);
        assert!(result.is_err(), "Borrow Checker failed to catch use-after-move");
        let errs = result.unwrap_err();
        assert!(errs[0].contains("Use of uninitialized or moved variable"));
    }

    // Build 38 Phase B1: real join-point merging. `s` is moved on the `if` branch
    // but not on the `else` branch; using it unconditionally after the merge must
    // be rejected, since the `if` branch really did move it away.
    #[test]
    fn test_borrowck_rejects_use_after_move_on_only_one_branch() {
        let mir = compile_to_mir(
            "let s = \"hello\"\nlet cond = true\nif cond {\n    let t = s\n} else {\n    let z = \"world\"\n}\nlet u = s"
        );
        let result = check_mir(&mir);
        assert!(result.is_err(), "Borrow Checker failed to catch a move on only one incoming branch");
    }

    // The mirror-image positive case: `s` is moved on *both* branches, so it's
    // consistently Moved at the merge -- but nothing reads it afterwards, so this
    // must still pass (a join-point check must not falsely reject sound code).
    #[test]
    fn test_borrowck_accepts_move_on_both_branches_when_unused_after() {
        let mir = compile_to_mir(
            "let s = \"hello\"\nlet cond = true\nif cond {\n    let t = s\n} else {\n    let u = s\n}"
        );
        assert!(check_mir(&mir).is_ok());
    }
}
