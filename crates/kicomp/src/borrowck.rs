use crate::mir::{MirProgram, MirFunction, StatementKind, TerminatorKind, RValue, Operand};
use std::collections::HashSet;

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

    fn check_function(&mut self, func: &MirFunction, errors: &mut Vec<String>) {
        if func.basic_blocks.is_empty() { return; }
        
        // Ensure we don't infinitely loop on CFG cycles.
        let mut visited = HashSet::new();
        let mut worklist = vec![0]; // Start at basic block 0
        
        let mut states: Vec<LocalState> = vec![LocalState::Uninitialized; func.locals.len()];
        for arg in &func.args {
            states[arg.0] = LocalState::Initialized;
        }
        
        while let Some(block_idx) = worklist.pop() {
            if !visited.insert(block_idx) {
                continue; // Already processed
            }
            
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
            
            if let Some(term) = &block.terminator {
                match &term.kind {
                    TerminatorKind::Goto(target) => {
                        worklist.push(target.0);
                    }
                    TerminatorKind::Return => {
                        // Exit path
                    }
                    // Add SwitchInt handling later if needed
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
}
