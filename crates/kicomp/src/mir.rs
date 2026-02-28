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
    /// Return from the function.
    Return,
    /// Unconditional jump to a basic block.
    Goto(BasicBlock),
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

pub struct MirBuilder<'a> {
    locals: Vec<LocalDecl>,
    basic_blocks: Vec<BasicBlockData>,
    current_block: BasicBlock,
    local_env: HashMap<String, LocalId>,
    substitution: &'a Substitution,
    scopes: Vec<Vec<LocalId>>,
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

    pub fn build(mut self, hir: &HirProgram) -> (MirFunction, Vec<MirFunction>) {
        for stmt in &hir.statements {
            self.lower_statement(stmt);
        }
        
        // Drop root scope variables before returning
        self.drop_current_scope(0); // 0 is line number for function exit

        self.basic_blocks[self.current_block.0].terminator = Some(Terminator {
            kind: TerminatorKind::Return,
            line: 0,
        });

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
                sub_builder.basic_blocks[sub_builder.current_block.0].terminator = Some(Terminator {
                    kind: TerminatorKind::Return,
                    line: stmt.line,
                });
                
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
            // For Build 10, we'll implement a subset (Let, Expr, Block).
            // Loops and Ifs will be expanded as needed.
            _ => {}
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
                    // Fallback for unresolved globals (builtins)
                    RValue::Use(Operand::Constant(Constant::Null))
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
}
