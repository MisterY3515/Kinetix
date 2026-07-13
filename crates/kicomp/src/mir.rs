use crate::types::Type;
use crate::hir::{HirProgram, HirStatement, HirStmtKind, HirExpression, HirExprKind};
use crate::ir_hash::DeterministicHasher;

use std::hash::{Hash, Hasher};

/// Unique identifier for a local variable in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub usize);

/// Unique identifier for a basic block in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BasicBlock(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mutability {
    Not,
    Mut,
}

/// A "Place" is an LValue — a memory location that can be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub local: LocalId,
}

#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Null,
}

impl PartialEq for Constant {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Constant::Int(a), Constant::Int(b)) => a == b,
            (Constant::Float(a), Constant::Float(b)) => a.to_bits() == b.to_bits(),
            (Constant::Bool(a), Constant::Bool(b)) => a == b,
            (Constant::String(a), Constant::String(b)) => a == b,
            (Constant::Null, Constant::Null) => true,
            _ => false,
        }
    }
}
impl Eq for Constant {}

impl Hash for Constant {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Constant::Int(i) => {
                state.write_u8(0);
                i.hash(state);
            }
            Constant::Float(f) => {
                state.write_u8(1);
                f.to_bits().hash(state);
            }
            Constant::Bool(b) => {
                state.write_u8(2);
                b.hash(state);
            }
            Constant::String(s) => {
                state.write_u8(3);
                s.hash(state);
            }
            Constant::Null => {
                state.write_u8(4);
            }
        }
    }
}

/// An "Operand" is an argument to an instruction.
/// It explicitly defines ownership semantics: Move, Copy, or Borrow.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Operand {
    /// Copies a trivially copyable value (e.g., int, float).
    Copy(Place),
    /// Moves a value, transferring ownership and invalidating the source.
    Move(Place),
    /// Unused in Phase 1 (Build 10), but reserved for borrows.
    Borrow(Place, Mutability),
    /// A constant value.
    Constant(Constant),
}

/// An RValue represents a computation that produces a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RValue {
    /// Yields the value of an operand.
    Use(Operand),
    /// Binary operation.
    BinaryOp(String, Operand, Operand),
    /// Unary operation.
    UnaryOp(String, Operand),
    /// Function call.
    Call(Operand, Vec<Operand>),
    /// Array construction.
    Array(Vec<Operand>),
    /// Struct or Aggregate construction.
    Aggregate(String, Vec<Operand>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StatementKind {
    /// Assingment: Place = RValue
    Assign(Place, RValue),
    /// Execute an RValue for side effects without assigning the result.
    Expression(RValue),
    /// Drop a value (free memory/resources).
    Drop(Place),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MirStatement {
    pub kind: StatementKind,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TerminatorKind {
    /// Return from the function, with the value being returned (if any). The
    /// validators (borrowck/drop_verify/ssa_validate) never needed this payload
    /// in Phase B1 -- they only cared about the returned expression being
    /// evaluated as a *use* beforehand -- but Phase B2's MIR-consuming codegen
    /// needs to know which register/operand to actually emit as the return
    /// value, so it's carried here instead of being discarded.
    Return(Option<Operand>),
    /// Unconditional jump to a basic block.
    Goto(BasicBlock),
    /// Conditional branch: jump to `then_block` if `cond` is truthy, else `else_block`.
    Branch {
        cond: Operand,
        then_block: BasicBlock,
        else_block: BasicBlock,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Terminator {
    pub kind: TerminatorKind,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BasicBlockData {
    pub statements: Vec<MirStatement>,
    pub terminator: Option<Terminator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalDecl {
    pub name: Option<String>,
    pub ty: Type,
    pub mutability: Mutability,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MirFunction {
    pub name: String,
    pub args: Vec<LocalId>,
    pub return_ty: Type,
    pub locals: Vec<LocalDecl>,
    pub basic_blocks: Vec<BasicBlockData>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MirProgram {
    pub functions: Vec<MirFunction>,
    pub main_block: MirFunction,
}

impl MirFunction {
    pub fn compute_hash(&self) -> u64 {
        let mut hasher = DeterministicHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl MirProgram {
    pub fn compute_hash(&self) -> u64 {
        let mut hasher = DeterministicHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

/// Helper function to determine if a type is trivially copyable.
pub fn is_trivially_copyable(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::Void | Type::Fn(_, _) | Type::Ref(_) => true,
        Type::Str | Type::Array(_) | Type::Map(_, _) | Type::Custom { .. } | Type::Var(_) | Type::MutRef(_) => false,
    }
}

use crate::types::Substitution;
use std::collections::HashMap;

/// Tracks the Goto targets for `break`/`continue` inside the loop currently
/// being lowered (innermost last), mirroring `Compiler::loop_stack` in
/// `compiler.rs` -- except MIR blocks are allocated up front, so no
/// backpatching is needed: the targets are known before the body is lowered.
struct MirLoopContext {
    continue_target: BasicBlock,
    break_target: BasicBlock,
}

pub struct MirBuilder<'a> {
    locals: Vec<LocalDecl>,
    basic_blocks: Vec<BasicBlockData>,
    current_block: BasicBlock,
    local_env: HashMap<String, LocalId>,
    substitution: &'a Substitution,
    scopes: Vec<Vec<LocalId>>,
    loop_stack: Vec<MirLoopContext>,
    pub functions: Vec<MirFunction>,
}

impl<'a> MirBuilder<'a> {
    pub fn new(substitution: &'a Substitution) -> Self {
        let entry_block = BasicBlockData { statements: vec![], terminator: None };
        Self {
            locals: vec![],
            basic_blocks: vec![entry_block],
            current_block: BasicBlock(0),
            local_env: HashMap::new(),
            substitution,
            scopes: vec![vec![]], // the root function scope
            loop_stack: vec![],
            functions: vec![],
        }
    }

    fn push_local(&mut self, name: Option<String>, ty: Type, mutability: Mutability) -> LocalId {
        let resolved_ty = self.substitution.apply_default(&ty);
        let id = LocalId(self.locals.len());
        self.locals.push(LocalDecl { name: name.clone(), ty: resolved_ty, mutability });
        if let Some(n) = name {
            self.local_env.insert(n, id);
        }
        if let Some(current_scope) = self.scopes.last_mut() {
            current_scope.push(id);
        }
        id
    }

    fn push_statement(&mut self, stmt: MirStatement) {
        self.basic_blocks[self.current_block.0].statements.push(stmt);
    }

    /// Allocates a new, empty basic block and returns its id. Does not switch
    /// `current_block` -- callers do that explicitly once they're ready to lower
    /// statements into it.
    fn new_block(&mut self) -> BasicBlock {
        let id = BasicBlock(self.basic_blocks.len());
        self.basic_blocks.push(BasicBlockData { statements: vec![], terminator: None });
        id
    }

    /// Sets `current_block`'s terminator, unless it already has one -- once a
    /// block is terminated it stays terminated; later cleanup code (e.g. scope
    /// drops on exit) may still append trailing statements after the terminator
    /// is set, which is fine since `statements`/`terminator` are separate fields.
    fn terminate_current(&mut self, kind: TerminatorKind, line: usize) {
        let block = &mut self.basic_blocks[self.current_block.0];
        if block.terminator.is_none() {
            block.terminator = Some(Terminator { kind, line });
        }
    }

    fn current_block_terminated(&self) -> bool {
        self.basic_blocks[self.current_block.0].terminator.is_some()
    }

    /// Like `lower_expression_to_operand`, but for a bare local it borrows the
    /// place instead of copying/moving it -- used where the caller only needs
    /// to *read through* the value (e.g. indexing into an array) without
    /// consuming it, so a second read later in the same scope isn't flagged
    /// as a use of a moved value.
    fn lower_expression_to_borrowed_operand(&mut self, expr: &HirExpression) -> Operand {
        if let HirExprKind::Identifier(name) = &expr.kind {
            if let Some(&local_id) = self.local_env.get(name) {
                return Operand::Borrow(Place { local: local_id }, Mutability::Not);
            }
        }
        self.lower_expression_to_operand(expr)
    }

    pub fn build(mut self, hir: &HirProgram) -> (MirFunction, Vec<MirFunction>) {
        for stmt in &hir.statements {
            if self.current_block_terminated() { break; }
            self.lower_statement(stmt);
        }

        // Drop root scope variables before returning
        self.drop_current_scope(0); // 0 is line number for function exit

        self.terminate_current(TerminatorKind::Return(None), 0);

        let main_fn = MirFunction {
            name: "<main>".to_string(),
            args: vec![],
            return_ty: Type::Void,
            locals: self.locals,
            basic_blocks: self.basic_blocks,
        };
        (main_fn, self.functions)
    }

    fn drop_current_scope(&mut self, line: usize) {
        if let Some(exiting_scope) = self.scopes.pop() {
            for local_id in exiting_scope.into_iter().rev() {
                let ty = &self.locals[local_id.0].ty;
                if !is_trivially_copyable(ty) {
                    self.push_statement(MirStatement {
                        kind: StatementKind::Drop(Place { local: local_id }),
                        line,
                    });
                }
            }
        }
    }

    fn lower_statement(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStmtKind::Let { name, mutable, value } => {
                let mutability = if *mutable { Mutability::Mut } else { Mutability::Not };
                let local_id = self.push_local(Some(name.clone()), stmt.ty.clone(), mutability);
                let place = Place { local: local_id };
                
                let rvalue = self.lower_expression_to_rvalue(value);
                self.push_statement(MirStatement {
                    kind: StatementKind::Assign(place, rvalue),
                    line: stmt.line,
                });
            }
            HirStmtKind::Expression { expression } => {
                let rvalue = self.lower_expression_to_rvalue(expression);
                self.push_statement(MirStatement {
                    kind: StatementKind::Expression(rvalue),
                    line: stmt.line,
                });
            }
            HirStmtKind::Block { statements } => {
                self.scopes.push(vec![]);
                for s in statements {
                    if self.current_block_terminated() { break; }
                    self.lower_statement(s);
                }
                self.drop_current_scope(stmt.line);
            }
            HirStmtKind::Function { name, parameters, body, return_type } => {
                let mut sub_builder = MirBuilder::new(self.substitution);
                let mut arg_ids = Vec::new();
                for (param_name, ty) in parameters {
                    let id = sub_builder.push_local(Some(param_name.clone()), ty.clone(), Mutability::Not);
                    arg_ids.push(id);
                }
                sub_builder.lower_statement(body);
                sub_builder.drop_current_scope(stmt.line);
                sub_builder.terminate_current(TerminatorKind::Return(None), stmt.line);

                let mir_fn = MirFunction {
                    name: name.clone(),
                    args: arg_ids,
                    return_ty: self.substitution.apply_default(return_type),
                    locals: sub_builder.locals,
                    basic_blocks: sub_builder.basic_blocks,
                };
                self.functions.push(mir_fn);
                // Also pull up any deeply nested functions
                self.functions.extend(sub_builder.functions);
            }
            HirStmtKind::Class { methods, .. } => {
                for m in methods {
                    self.lower_statement(m);
                }
            }
            HirStmtKind::While { condition, body } => {
                let header = self.new_block();
                self.terminate_current(TerminatorKind::Goto(header), stmt.line);
                self.current_block = header;

                let cond_op = self.lower_expression_to_operand(condition);
                let body_block = self.new_block();
                let exit_block = self.new_block();
                self.terminate_current(
                    TerminatorKind::Branch { cond: cond_op, then_block: body_block, else_block: exit_block },
                    stmt.line,
                );

                self.current_block = body_block;
                self.loop_stack.push(MirLoopContext { continue_target: header, break_target: exit_block });
                self.lower_statement(body);
                self.loop_stack.pop();
                if !self.current_block_terminated() {
                    self.terminate_current(TerminatorKind::Goto(header), stmt.line);
                }

                self.current_block = exit_block;
            }
            HirStmtKind::For { iterator, range, body } => {
                // Evaluate the iterable once into a temp place (borrowed, not moved,
                // so it can still be indexed on every iteration below).
                let iter_ty = range.ty.clone();
                let iter_local = self.push_local(None, iter_ty, Mutability::Not);
                let iter_place = Place { local: iter_local };
                let range_rvalue = self.lower_expression_to_rvalue(range);
                self.push_statement(MirStatement {
                    kind: StatementKind::Assign(iter_place.clone(), range_rvalue),
                    line: stmt.line,
                });

                let idx_local = self.push_local(None, Type::Int, Mutability::Mut);
                let idx_place = Place { local: idx_local };
                self.push_statement(MirStatement {
                    kind: StatementKind::Assign(idx_place.clone(), RValue::Use(Operand::Constant(Constant::Int(0)))),
                    line: stmt.line,
                });

                // len(iter) -- modeled the same way any other unresolved builtin call
                // lowers in this MIR today (see `HirExprKind::Identifier`'s fallback):
                // the callee is an unresolved global, carried as a name string.
                let len_local = self.push_local(None, Type::Int, Mutability::Not);
                let len_place = Place { local: len_local };
                let len_call = RValue::Call(
                    Operand::Constant(Constant::String("len".to_string())),
                    vec![Operand::Borrow(iter_place.clone(), Mutability::Not)],
                );
                self.push_statement(MirStatement {
                    kind: StatementKind::Assign(len_place.clone(), len_call),
                    line: stmt.line,
                });

                let header = self.new_block();
                self.terminate_current(TerminatorKind::Goto(header), stmt.line);
                self.current_block = header;

                let cond_local = self.push_local(None, Type::Bool, Mutability::Not);
                let cond_place = Place { local: cond_local };
                self.push_statement(MirStatement {
                    kind: StatementKind::Assign(
                        cond_place.clone(),
                        RValue::BinaryOp("<".to_string(), Operand::Copy(idx_place.clone()), Operand::Copy(len_place.clone())),
                    ),
                    line: stmt.line,
                });

                let body_block = self.new_block();
                let exit_block = self.new_block();
                self.terminate_current(
                    TerminatorKind::Branch { cond: Operand::Copy(cond_place), then_block: body_block, else_block: exit_block },
                    stmt.line,
                );

                self.current_block = body_block;
                // iterator := iter[idx] -- shadow-safe: save/restore any previous
                // binding for this name so a same-named outer local isn't leaked
                // past the loop (mirrors the compiler.rs Phase A scope-leak fix).
                let iter_var_local = self.push_local(Some(iterator.clone()), Type::Int, Mutability::Not);
                self.push_statement(MirStatement {
                    kind: StatementKind::Assign(
                        Place { local: iter_var_local },
                        RValue::BinaryOp("[]".to_string(), Operand::Borrow(iter_place.clone(), Mutability::Not), Operand::Copy(idx_place.clone())),
                    ),
                    line: stmt.line,
                });

                let increment_block = self.new_block();
                self.loop_stack.push(MirLoopContext { continue_target: increment_block, break_target: exit_block });
                self.lower_statement(body);
                self.loop_stack.pop();
                if !self.current_block_terminated() {
                    self.terminate_current(TerminatorKind::Goto(increment_block), stmt.line);
                }

                self.current_block = increment_block;
                self.push_statement(MirStatement {
                    kind: StatementKind::Assign(
                        idx_place.clone(),
                        RValue::BinaryOp("+".to_string(), Operand::Copy(idx_place.clone()), Operand::Constant(Constant::Int(1))),
                    ),
                    line: stmt.line,
                });
                self.terminate_current(TerminatorKind::Goto(header), stmt.line);

                self.current_block = exit_block;
            }
            HirStmtKind::Break => {
                // A `break`/`continue` outside any loop is a real error, but it's
                // already caught at the bytecode-codegen layer (`compiler.rs`'s own
                // `loop_stack` check), which runs independently over the same AST --
                // no need to duplicate that check in this (validation-only) MIR pass.
                if let Some(ctx) = self.loop_stack.last() {
                    let target = ctx.break_target;
                    self.terminate_current(TerminatorKind::Goto(target), stmt.line);
                }
            }
            HirStmtKind::Continue => {
                if let Some(ctx) = self.loop_stack.last() {
                    let target = ctx.continue_target;
                    self.terminate_current(TerminatorKind::Goto(target), stmt.line);
                }
            }
            HirStmtKind::Return { value } => {
                let ret_operand = value.as_ref().map(|val_expr| self.lower_expression_to_operand(val_expr));
                self.terminate_current(TerminatorKind::Return(ret_operand), stmt.line);
            }
            // State/Computed/Effect are intentionally not lowered into MIR at all
            // (see `check_reactive_isolation` in ssa_validate.rs).
            HirStmtKind::State { .. } | HirStmtKind::Computed { .. } | HirStmtKind::Effect { .. } => {}
        }
    }

    /// Lowers `stmt` the same way `lower_statement` does, except that if it is
    /// (or ends in) a trailing `Expression` statement, that expression's value is
    /// assigned into `result_place` instead of being discarded. Used to give
    /// if-expressions (`let x = if cond { 1 } else { 2 }`) a real value instead of
    /// the previous `Null` placeholder.
    fn lower_statement_as_value(&mut self, stmt: &HirStatement, result_place: Option<&Place>) {
        match &stmt.kind {
            HirStmtKind::Block { statements } => {
                self.scopes.push(vec![]);
                if let Some((last, rest)) = statements.split_last() {
                    for s in rest {
                        if self.current_block_terminated() { break; }
                        self.lower_statement(s);
                    }
                    if !self.current_block_terminated() {
                        self.lower_statement_as_value(last, result_place);
                    }
                }
                self.drop_current_scope(stmt.line);
            }
            HirStmtKind::Expression { expression } => {
                let rvalue = self.lower_expression_to_rvalue(expression);
                match result_place {
                    Some(place) => {
                        self.push_statement(MirStatement {
                            kind: StatementKind::Assign(place.clone(), rvalue),
                            line: stmt.line,
                        });
                    }
                    None => {
                        self.push_statement(MirStatement { kind: StatementKind::Expression(rvalue), line: stmt.line });
                    }
                }
            }
            HirStmtKind::Let { name, .. } => {
                // This HIR types a `Block` as its *last statement's* type regardless
                // of kind (see `hir.rs`'s `Statement::Block` lowering), so a trailing
                // `let` also counts as "the block's value" here, not just a trailing
                // expression. Lower the binding normally, then additionally propagate
                // the freshly-bound local's value into `result_place`.
                self.lower_statement(stmt);
                if let Some(place) = result_place {
                    if let Some(&local_id) = self.local_env.get(name) {
                        let local_ty = self.locals[local_id.0].ty.clone();
                        let operand = if is_trivially_copyable(&local_ty) {
                            Operand::Copy(Place { local: local_id })
                        } else {
                            Operand::Move(Place { local: local_id })
                        };
                        self.push_statement(MirStatement {
                            kind: StatementKind::Assign(place.clone(), RValue::Use(operand)),
                            line: stmt.line,
                        });
                    }
                }
            }
            _ => self.lower_statement(stmt),
        }
    }

    fn lower_expression_to_rvalue(&mut self, expr: &HirExpression) -> RValue {
        match &expr.kind {
            HirExprKind::Integer(v) => RValue::Use(Operand::Constant(Constant::Int(*v))),
            HirExprKind::Float(v) => RValue::Use(Operand::Constant(Constant::Float(*v))),
            HirExprKind::Boolean(v) => RValue::Use(Operand::Constant(Constant::Bool(*v))),
            HirExprKind::String(v) => RValue::Use(Operand::Constant(Constant::String(v.clone()))),
            HirExprKind::Null => RValue::Use(Operand::Constant(Constant::Null)),
            HirExprKind::Identifier(name) => {
                if let Some(&local_id) = self.local_env.get(name) {
                    let place = Place { local: local_id };
                    let resolved_ty = self.substitution.apply_default(&expr.ty);
                    if is_trivially_copyable(&resolved_ty) {
                        RValue::Use(Operand::Copy(place))
                    } else {
                        // Explicit ownership transfer!
                        RValue::Use(Operand::Move(place))
                    }
                } else {
                    // Unresolved global (a top-level function name or a builtin like
                    // `len`) -- MIR doesn't track a symbol table for these, so the
                    // name itself is carried through as a string constant. This is
                    // the same convention `monomorphize.rs`'s `monomorphize_rvalue`
                    // already expects a `Call`'s function operand to use (see its
                    // `Operand::Constant(Constant::String(func_name))` match), just
                    // not previously produced by this lowering. Codegen resolves it
                    // via `GetGlobal` by name instead of the AST-walking compiler's
                    // symbol-table lookup.
                    RValue::Use(Operand::Constant(Constant::String(name.clone())))
                }
            }
            HirExprKind::Infix { left, operator, right } => {
                let l_place = self.lower_expression_to_operand(left);
                let r_place = self.lower_expression_to_operand(right);
                RValue::BinaryOp(operator.clone(), l_place, r_place)
            }
            HirExprKind::Prefix { operator, right } => {
                if operator == "&" || operator == "&mut" {
                    if let HirExprKind::Identifier(ref name) = right.kind {
                        if let Some(&local_id) = self.local_env.get(name) {
                            let place = Place { local: local_id };
                            let mutability = if operator == "&mut" { Mutability::Mut } else { Mutability::Not };
                            return RValue::Use(Operand::Borrow(place, mutability));
                        }
                    }
                    // Fallback to evaluating into a temporary and borrowing that
                    let r_place_operand = self.lower_expression_to_operand(right);
                    if let Operand::Move(place) | Operand::Copy(place) = r_place_operand {
                        let mutability = if operator == "&mut" { Mutability::Mut } else { Mutability::Not };
                        return RValue::Use(Operand::Borrow(place, mutability));
                    }
                }
                let r_place = self.lower_expression_to_operand(right);
                RValue::UnaryOp(operator.clone(), r_place)
            }
            HirExprKind::Call { function, arguments } => {
                let func_op = self.lower_expression_to_operand(function);
                let arg_ops: Vec<Operand> = arguments.iter()
                    .map(|a| self.lower_expression_to_operand(a))
                    .collect();
                RValue::Call(func_op, arg_ops)
            }
            HirExprKind::ArrayLiteral(elems) => {
                let ops: Vec<Operand> = elems.iter()
                    .map(|e| self.lower_expression_to_operand(e))
                    .collect();
                RValue::Array(ops)
            }
            HirExprKind::StructLiteral(name, fields) => {
                let ops: Vec<Operand> = fields.iter()
                    .map(|(_, e)| self.lower_expression_to_operand(e))
                    .collect();
                RValue::Aggregate(name.clone(), ops)
            }
            HirExprKind::MethodCall { .. } => {
                unreachable!("MethodCall should have been statically dispatched to Call in Type Normalizer.");
            }
            HirExprKind::If { condition, consequence, alternative } => {
                let cond_op = self.lower_expression_to_operand(condition);
                let result_ty = self.substitution.apply_default(&expr.ty);
                let result_place = if result_ty == Type::Void {
                    None
                } else {
                    Some(Place { local: self.push_local(None, expr.ty.clone(), Mutability::Not) })
                };

                let then_block = self.new_block();
                let else_block = self.new_block();
                let merge_block = self.new_block();
                self.terminate_current(
                    TerminatorKind::Branch { cond: cond_op, then_block, else_block },
                    0,
                );

                self.current_block = then_block;
                self.lower_statement_as_value(consequence, result_place.as_ref());
                if !self.current_block_terminated() {
                    self.terminate_current(TerminatorKind::Goto(merge_block), 0);
                }

                self.current_block = else_block;
                match alternative {
                    Some(alt) => self.lower_statement_as_value(alt, result_place.as_ref()),
                    None => {
                        // No `else` -- this branch implicitly produces nothing. If
                        // `result_place` is `Some` here, the *then* branch must
                        // diverge (HIR only types a non-Void if-without-else when
                        // the true branch never falls through, e.g.
                        // `if cond { return x }`), so this value is never actually
                        // read on any live path -- but the merge block below
                        // unconditionally reads `result_place`, so it must still be
                        // initialized to *something* here or the borrow checker
                        // correctly (but spuriously, from the source program's
                        // point of view) flags it as read-before-init.
                        if let Some(place) = &result_place {
                            self.push_statement(MirStatement {
                                kind: StatementKind::Assign(place.clone(), RValue::Use(Operand::Constant(Constant::Null))),
                                line: 0,
                            });
                        }
                    }
                }
                if !self.current_block_terminated() {
                    self.terminate_current(TerminatorKind::Goto(merge_block), 0);
                }

                self.current_block = merge_block;

                match result_place {
                    Some(place) => {
                        if is_trivially_copyable(&result_ty) {
                            RValue::Use(Operand::Copy(place))
                        } else {
                            RValue::Use(Operand::Move(place))
                        }
                    }
                    None => RValue::Use(Operand::Constant(Constant::Null)),
                }
            }
            HirExprKind::Index { left, index } => {
                let l_op = self.lower_expression_to_borrowed_operand(left);
                let i_op = self.lower_expression_to_operand(index);
                RValue::BinaryOp("[]".to_string(), l_op, i_op)
            }
            HirExprKind::Range { start, end } => {
                // Same convention as `"[]"` above: reuses `BinaryOp` with an
                // operator tag that doesn't correspond to a source-level infix
                // operator, rather than adding a dedicated RValue variant just
                // for this one construct. `for x in a..b` (the only place a
                // range is used in MIR-representable code today) evaluates this
                // once into a place it then indexes into every iteration.
                let s_op = self.lower_expression_to_operand(start);
                let e_op = self.lower_expression_to_operand(end);
                RValue::BinaryOp("..".to_string(), s_op, e_op)
            }
            HirExprKind::Assign { target, value } => {
                // Only simple identifier targets are modeled (the common `x = x + 1`
                // loop-body reassignment pattern); complex targets (`arr[i] = v`,
                // `obj.field = v`) fall through to the existing Null placeholder below,
                // same as before this change.
                if let HirExprKind::Identifier(name) = &target.kind {
                    if let Some(&local_id) = self.local_env.get(name) {
                        let place = Place { local: local_id };
                        let rvalue = self.lower_expression_to_rvalue(value);
                        self.push_statement(MirStatement {
                            kind: StatementKind::Assign(place, rvalue),
                            line: 0,
                        });
                        return RValue::Use(Operand::Constant(Constant::Null));
                    }
                }
                RValue::Use(Operand::Constant(Constant::Null))
            }
            _ => RValue::Use(Operand::Constant(Constant::Null)), // placeholder for Match, MemberAccess, etc.
        }
    }

    fn lower_expression_to_operand(&mut self, expr: &HirExpression) -> Operand {
        let rvalue = self.lower_expression_to_rvalue(expr);
        // Create a temporary local for the intermediate result
        let temp_id = self.push_local(None, expr.ty.clone(), Mutability::Not);
        let place = Place { local: temp_id };
        self.push_statement(MirStatement {
            kind: StatementKind::Assign(place.clone(), rvalue),
            line: 0, // temp
        });
        
        let resolved_ty = self.substitution.apply_default(&expr.ty);
        if is_trivially_copyable(&resolved_ty) {
            Operand::Copy(place)
        } else {
            Operand::Move(place)
        }
    }
}

pub fn lower_to_mir(hir: &HirProgram, substitution: &Substitution) -> MirProgram {
    let builder = MirBuilder::new(substitution);
    let (main_fn, functions) = builder.build(hir);
    MirProgram {
        functions,
        main_block: main_fn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use kinetix_language::lexer::Lexer;
    use kinetix_language::parser::Parser;
    use crate::symbol::resolve_program;
    use crate::hir::lower_to_hir as ast_to_hir;
    use crate::typeck::TypeContext;

    fn compile_to_mir(src: &str) -> MirProgram {
        let arena = Bump::new();
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, &arena);
        let program = parser.parse_program();
        let symbols = resolve_program(&program.statements).unwrap();
        let traits = crate::trait_solver::TraitEnvironment::new();
        let hir = ast_to_hir(&program.statements, &symbols, &traits);
        
        let mut ctx = TypeContext::new();
        let constraints = ctx.collect_constraints(&hir);
        ctx.solve(&constraints).unwrap();
        
        lower_to_mir(&hir, &ctx.substitution)
    }

    #[test]
    fn test_mir_generates_moves_for_strings() {
        let mir = compile_to_mir("let a = \"hello\"\nlet b = a");
        let basic_block = &mir.main_block.basic_blocks[0];
        
        let mut found_move = false;
        for stmt in &basic_block.statements {
            if let StatementKind::Assign(_, RValue::Use(Operand::Move(_))) = &stmt.kind {
                found_move = true;
            }
        }
        assert!(found_move, "Expected a Move operand for String transfer");
    }

    #[test]
    fn test_mir_generates_copies_for_ints() {
        let mir = compile_to_mir("let x = 42\nlet y = x");
        let basic_block = &mir.main_block.basic_blocks[0];
        
        let mut found_copy = false;
        for stmt in &basic_block.statements {
            if let StatementKind::Assign(_, RValue::Use(Operand::Copy(_))) = &stmt.kind {
                found_copy = true;
            }
        }
        assert!(found_copy, "Expected a Copy operand for Int transfer");
    }

    #[test]
    fn test_mir_generates_borrows() {
        let mir = compile_to_mir("let x = 42\nlet y = &x\nlet z = &mut x");
        let basic_block = &mir.main_block.basic_blocks[0];
        
        let mut found_immutable_borrow = false;
        let mut found_mutable_borrow = false;
        
        for stmt in &basic_block.statements {
            if let StatementKind::Assign(_, RValue::Use(Operand::Borrow(_, Mutability::Not))) = &stmt.kind {
                found_immutable_borrow = true;
            }
            if let StatementKind::Assign(_, RValue::Use(Operand::Borrow(_, Mutability::Mut))) = &stmt.kind {
                found_mutable_borrow = true;
            }
        }
        
        assert!(found_immutable_borrow, "Expected an immutable Borrow operand");
        assert!(found_mutable_borrow, "Expected a mutable Borrow operand");
    }

    #[test]
    fn test_mir_generates_drops_at_scope_exit() {
        let mir = compile_to_mir("{ let x = \"hello\" }");
        let basic_block = &mir.main_block.basic_blocks[0];
        
        let mut found_drop = false;
        for stmt in &basic_block.statements {
            if let StatementKind::Drop(_) = &stmt.kind {
                found_drop = true;
            }
        }
        
        assert!(found_drop, "Expected a Drop statement at the end of the block for the string variable");
    }

    // ── Build 38 Phase B1: real multi-block CFG lowering ──────────────────

    #[test]
    fn test_mir_if_produces_multiple_blocks_and_branch_terminator() {
        let mir = compile_to_mir("let x = 1\nif x > 0 {\n    let y = 1\n} else {\n    let y = 2\n}");
        assert!(mir.main_block.basic_blocks.len() > 1, "expected more than one block for an if/else");
        let has_branch = mir.main_block.basic_blocks.iter().any(|b| {
            matches!(b.terminator.as_ref().map(|t| &t.kind), Some(TerminatorKind::Branch { .. }))
        });
        assert!(has_branch, "expected a Branch terminator for the if/else condition");
    }

    #[test]
    fn test_mir_if_expression_yields_real_value_not_null_placeholder() {
        // Previously any if-expression lowered to a bare `Null` placeholder RValue;
        // now it should produce a real value via a result place assigned in both arms.
        let mir = compile_to_mir("let x = 1\nlet z = if x > 0 { 1 } else { 2 }");
        assert!(mir.main_block.basic_blocks.len() > 1);
        assert!(crate::borrowck::check_mir(&mir).is_ok());
        assert!(crate::ssa_validate::validate(&mir).is_ok());
    }

    #[test]
    fn test_mir_while_produces_header_body_exit_blocks() {
        let mir = compile_to_mir("mut x = 3\nwhile x > 0 {\n    x = x - 1\n}");
        assert!(mir.main_block.basic_blocks.len() >= 3, "expected at least header/body/exit blocks");
        let has_branch = mir.main_block.basic_blocks.iter().any(|b| {
            matches!(b.terminator.as_ref().map(|t| &t.kind), Some(TerminatorKind::Branch { .. }))
        });
        assert!(has_branch);
    }

    #[test]
    fn test_mir_for_loop_produces_blocks() {
        let mir = compile_to_mir("for i in 0..3 {\n    let y = i\n}");
        assert!(mir.main_block.basic_blocks.len() >= 3);
    }

    #[test]
    fn test_mir_nested_break_continue_passes_all_validators() {
        let mir = compile_to_mir(
            "for i in 0..5 {\n    if i > 2 {\n        break\n    }\n    if i > 1 {\n        continue\n    }\n}"
        );
        assert!(crate::borrowck::check_mir(&mir).is_ok());
        assert!(crate::ssa_validate::validate(&mir).is_ok());
        assert!(crate::drop_verify::verify(&mir).is_ok());
        assert!(crate::mono_validate::validate(&mir).is_ok());
    }

    // A call to a name MIR can't resolve locally (a top-level function or a
    // builtin) must keep the callee's name so codegen can later emit a
    // GetGlobal for it -- previously this collapsed to `Constant::Null`,
    // silently destroying the callee's identity.
    #[test]
    fn test_mir_call_to_unresolved_global_carries_name_not_null() {
        let mir = compile_to_mir("fn foo(x: int) -> int { return x }\nlet y = foo(1)");
        let has_named_callee = mir.main_block.basic_blocks.iter().any(|b| {
            b.statements.iter().any(|s| matches!(
                &s.kind,
                StatementKind::Assign(_, RValue::Use(Operand::Constant(Constant::String(name)))) if name == "foo"
            ))
        });
        assert!(has_named_callee, "callee name 'foo' should survive MIR lowering as a string constant");
    }

    #[test]
    fn test_mir_for_loop_len_call_carries_name_not_null() {
        let mir = compile_to_mir("for i in 0..3 {\n    let y = i\n}");
        let has_len_callee = mir.main_block.basic_blocks.iter().any(|b| {
            b.statements.iter().any(|s| matches!(
                &s.kind,
                StatementKind::Assign(_, RValue::Call(Operand::Constant(Constant::String(name)), _)) if name == "len"
            ))
        });
        assert!(has_len_callee, "the for-loop's implicit len() bounds check should carry its callee name");
    }
}
