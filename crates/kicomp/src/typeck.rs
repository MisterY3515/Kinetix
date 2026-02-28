/// Type Checker — Constraint Collection & Robinson Unification.
///
/// This module walks the HIR and:
/// 1. Collects type constraints (equations that must hold).
/// 2. Solves them via Robinson's unification algorithm.
/// 3. Produces a Substitution that maps type variables to concrete types.

use crate::types::{Type, TypeVarId, Substitution};
use crate::hir::*;

/// A type constraint: two types that must unify.
#[derive(Debug, Clone)]
pub struct Constraint {
    pub left: Type,
    pub right: Type,
    pub line: usize,
}

impl Constraint {
    pub fn new(left: Type, right: Type, line: usize) -> Self {
        Self { left, right, line }
    }
}

/// A type error produced during unification.
#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub line: usize,
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)
    }
}

/// The Type Context holds the global state for type checking.
pub struct TypeContext {
    next_var: TypeVarId,
    pub substitution: Substitution,
}

impl TypeContext {
    pub fn new() -> Self {
        Self {
            next_var: 10000, // start high to avoid collision with HIR lowering vars
            substitution: Substitution::new(),
        }
    }

    /// Generate a fresh type variable.
    pub fn fresh_var(&mut self) -> Type {
        let id = self.next_var;
        self.next_var += 1;
        Type::Var(id)
    }

    /// Collect constraints from the entire HIR program.
    pub fn collect_constraints(&mut self, program: &HirProgram) -> Vec<Constraint> {
        let mut constraints = Vec::new();
        for stmt in &program.statements {
            self.collect_stmt(stmt, &mut constraints);
        }
        constraints
    }

    fn collect_stmt(&mut self, stmt: &HirStatement, constraints: &mut Vec<Constraint>) {
        match &stmt.kind {
            HirStmtKind::Let { name: _, mutable: _, value } => {
                self.collect_expr(value, stmt.line, constraints);
                // The Let's type must match the value's type
                constraints.push(Constraint::new(stmt.ty.clone(), value.ty.clone(), stmt.line));
            }
            HirStmtKind::State { value, .. } | HirStmtKind::Computed { value, .. } => {
                self.collect_expr(value, stmt.line, constraints);
                constraints.push(Constraint::new(stmt.ty.clone(), value.ty.clone(), stmt.line));
            }
            HirStmtKind::Effect { body, .. } => {
                self.collect_stmt(body, constraints);
            }
            HirStmtKind::Return { value } => {
                if let Some(v) = value {
                    self.collect_expr(v, stmt.line, constraints);
                }
            }
            HirStmtKind::Expression { expression } => {
                self.collect_expr(expression, stmt.line, constraints);
            }
            HirStmtKind::Block { statements } => {
                for s in statements {
                    self.collect_stmt(s, constraints);
                }
            }
            HirStmtKind::Function { parameters: _, body, return_type, .. } => {
                self.collect_stmt(body, constraints);
                // If body is a block with a return, the return value type must match return_type
                if let HirStmtKind::Block { statements } = &body.kind {
                    for s in statements {
                        if let HirStmtKind::Return { value: Some(v) } = &s.kind {
                            constraints.push(Constraint::new(return_type.clone(), v.ty.clone(), s.line));
                        }
                    }
                }
            }
            HirStmtKind::Class { methods, .. } => {
                for m in methods {
                    self.collect_stmt(m, constraints);
                }
            }
            HirStmtKind::While { condition, body } => {
                self.collect_expr(condition, stmt.line, constraints);
                // Condition must be bool
                constraints.push(Constraint::new(Type::Bool, condition.ty.clone(), stmt.line));
                self.collect_stmt(body, constraints);
            }
            HirStmtKind::For { range, body, .. } => {
                self.collect_expr(range, stmt.line, constraints);
                self.collect_stmt(body, constraints);
            }
            HirStmtKind::Break | HirStmtKind::Continue => {}
        }
    }

    fn collect_expr(&mut self, expr: &HirExpression, line: usize, constraints: &mut Vec<Constraint>) {
        match &expr.kind {
            HirExprKind::Infix { left, operator, right } => {
                self.collect_expr(left, line, constraints);
                self.collect_expr(right, line, constraints);
                // Both operands must have the same type
                constraints.push(Constraint::new(left.ty.clone(), right.ty.clone(), line));
                // For comparison operators, result is bool
                match operator.as_str() {
                    "==" | "!=" | "<" | ">" | "<=" | ">=" => {
                        constraints.push(Constraint::new(expr.ty.clone(), Type::Bool, line));
                    }
                    // For arithmetic, result type matches operand type
                    "+" | "-" | "*" | "/" | "%" => {
                        constraints.push(Constraint::new(expr.ty.clone(), left.ty.clone(), line));
                    }
                    // Logical operators
                    "&&" | "||" => {
                        constraints.push(Constraint::new(left.ty.clone(), Type::Bool, line));
                        constraints.push(Constraint::new(right.ty.clone(), Type::Bool, line));
                        constraints.push(Constraint::new(expr.ty.clone(), Type::Bool, line));
                    }
                    _ => {}
                }
            }
            HirExprKind::Prefix { right, operator } => {
                self.collect_expr(right, line, constraints);
                match operator.as_str() {
                    "!" => constraints.push(Constraint::new(expr.ty.clone(), Type::Bool, line)),
                    "-" => constraints.push(Constraint::new(expr.ty.clone(), right.ty.clone(), line)),
                    "&" => constraints.push(Constraint::new(expr.ty.clone(), Type::Ref(Box::new(right.ty.clone())), line)),
                    "&mut" => constraints.push(Constraint::new(expr.ty.clone(), Type::MutRef(Box::new(right.ty.clone())), line)),
                    _ => {}
                }
            }
            HirExprKind::Call { function, arguments } => {
                self.collect_expr(function, line, constraints);
                for arg in arguments {
                    self.collect_expr(arg, line, constraints);
                }
                // Constrain: function type must be Fn([arg types...], return_type)
                // and expr.ty must be the return type
                let arg_types: Vec<Type> = arguments.iter().map(|a| a.ty.clone()).collect();
                let expected_fn = Type::Fn(arg_types, Box::new(expr.ty.clone()));
                constraints.push(Constraint::new(function.ty.clone(), expected_fn, line));
            }
            HirExprKind::MethodCall { object, arguments, .. } => {
                self.collect_expr(object, line, constraints);
                for arg in arguments {
                    self.collect_expr(arg, line, constraints);
                }

                // --- M2.6 GLOBAL ENFORCEMENT: Fallible Builtins return Result<T,E> ---
                // If the object is a recognized builtin capability module, type-check the Result wrapper.
                if let Type::Custom { name, .. } = &object.ty {
                    if name == "data" || name == "system" || name == "net" || name == "os" {
                        let method_str = if let HirExprKind::MethodCall { method, .. } = &expr.kind { method.as_str() } else { "" };
                        
                        // Exclusion list for info methods safely mapped internally:
                        let safe = ["uptime", "cpu_usage", "memory_free", "os_name", "os_version"];
                        
                        if !safe.contains(&method_str) {
                            // Enforce Result<OkType, String>
                            let ok_ty = self.fresh_var();
                            let result_ty = Type::Custom {
                                name: "Result".into(),
                                args: vec![ok_ty, Type::Custom { name: "String".into(), args: vec![] }],
                            };
                            constraints.push(Constraint::new(expr.ty.clone(), result_ty, line));
                        }
                    }
                }
            }
            HirExprKind::If { condition, consequence, alternative } => {
                self.collect_expr(condition, line, constraints);
                constraints.push(Constraint::new(Type::Bool, condition.ty.clone(), line));
                self.collect_stmt(consequence, constraints);
                if let Some(alt) = alternative {
                    self.collect_stmt(alt, constraints);
                    // Both branches must have the same type
                    constraints.push(Constraint::new(consequence.ty.clone(), alt.ty.clone(), line));
                }
            }
            HirExprKind::Assign { target, value } => {
                self.collect_expr(target, line, constraints);
                self.collect_expr(value, line, constraints);
                constraints.push(Constraint::new(target.ty.clone(), value.ty.clone(), line));
            }
            HirExprKind::Index { left, index } => {
                self.collect_expr(left, line, constraints);
                self.collect_expr(index, line, constraints);
                // Index must be int
                constraints.push(Constraint::new(index.ty.clone(), Type::Int, line));
                // left must be Array<T> where T = expr.ty
                constraints.push(Constraint::new(left.ty.clone(), Type::Array(Box::new(expr.ty.clone())), line));
            }
            HirExprKind::ArrayLiteral(elems) => {
                for e in elems { self.collect_expr(e, line, constraints); }
                // All elements must have the same type
                if elems.len() >= 2 {
                    let first = &elems[0].ty;
                    for e in &elems[1..] {
                        constraints.push(Constraint::new(first.clone(), e.ty.clone(), line));
                    }
                }
            }
            HirExprKind::MapLiteral(pairs) => {
                for (k, v) in pairs {
                    self.collect_expr(k, line, constraints);
                    self.collect_expr(v, line, constraints);
                }
            }
            HirExprKind::MemberAccess { object, .. } => {
                self.collect_expr(object, line, constraints);
            }
            HirExprKind::Range { start, end } => {
                self.collect_expr(start, line, constraints);
                self.collect_expr(end, line, constraints);
                constraints.push(Constraint::new(start.ty.clone(), Type::Int, line));
                constraints.push(Constraint::new(end.ty.clone(), Type::Int, line));
            }
            HirExprKind::FunctionLiteral { body, .. } => {
                self.collect_stmt(body, constraints);
            }
            // Literals and identifiers — no constraints to add
            _ => {}
        }
    }

    /// Solve all collected constraints via Robinson unification.
    pub fn solve(&mut self, constraints: &[Constraint]) -> Result<(), Vec<TypeError>> {
        let mut errors = Vec::new();
        for c in constraints {
            if let Err(msg) = self.unify(&c.left, &c.right) {
                errors.push(TypeError { message: msg, line: c.line });
            }
        }
        // M2.5 Generic Instantiation Depth Limit (DOS protection)
        if let Err(msg) = self.substitution.check_depth_limit(32) {
            errors.push(TypeError { message: msg, line: 0 }); // Global error
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    /// Robinson unification: make two types equal under the current substitution.
    fn unify(&mut self, a: &Type, b: &Type) -> Result<(), String> {
        let a = self.substitution.apply(a);
        let b = self.substitution.apply(b);

        match (&a, &b) {
            // Already equal
            _ if a == b => Ok(()),

            // Var binds to anything (occurs check)
            (Type::Var(id), _) => {
                if self.occurs(*id, &b) {
                    return Err(format!("Infinite type: ?T{} occurs in {}", id, b));
                }
                self.substitution.bind(*id, b);
                Ok(())
            }
            (_, Type::Var(id)) => {
                if self.occurs(*id, &a) {
                    return Err(format!("Infinite type: ?T{} occurs in {}", id, a));
                }
                self.substitution.bind(*id, a);
                Ok(())
            }

            // Function types: unify param-by-param and return
            (Type::Fn(p1, r1), Type::Fn(p2, r2)) => {
                if p1.len() != p2.len() {
                    return Err(format!("Arity mismatch: expected {} params, got {}", p1.len(), p2.len()));
                }
                for (pa, pb) in p1.iter().zip(p2.iter()) {
                    self.unify(pa, pb)?;
                }
                self.unify(r1, r2)
            }

            // Structural types
            (Type::Array(a), Type::Array(b)) => self.unify(a, b),
            (Type::Map(k1, v1), Type::Map(k2, v2)) => {
                self.unify(k1, k2)?;
                self.unify(v1, v2)
            }
            (Type::Ref(a), Type::Ref(b)) => self.unify(a, b),
            (Type::MutRef(a), Type::MutRef(b)) => self.unify(a, b),


            // Custom types (named + generic args)
            (Type::Custom { name: n1, args: a1 }, Type::Custom { name: n2, args: a2 }) if n1 == n2 && a1.len() == a2.len() => {
                for (arg1, arg2) in a1.iter().zip(a2.iter()) {
                    self.unify(arg1, arg2)?;
                }
                Ok(())
            }

            // Mismatch
            _ => Err(format!("Type mismatch: {} vs {}", a, b)),
        }
    }

    /// Occurs check: does variable `var` appear anywhere inside `ty`?
    fn occurs(&self, var: TypeVarId, ty: &Type) -> bool {
        match ty {
            Type::Var(id) => *id == var,
            Type::Fn(params, ret) => {
                params.iter().any(|p| self.occurs(var, p)) || self.occurs(var, ret)
            }
            Type::Array(inner) => self.occurs(var, inner),
            Type::Map(k, v) => self.occurs(var, k) || self.occurs(var, v),
            Type::Ref(inner) => self.occurs(var, inner),
            Type::MutRef(inner) => self.occurs(var, inner),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use kinetix_language::lexer::Lexer;
    use kinetix_language::parser::Parser;
    use crate::symbol::resolve_program;
    use crate::hir::lower_to_hir;

    fn check(src: &str) -> Result<Substitution, Vec<TypeError>> {
        let arena = Bump::new();
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, &arena);
        let program = parser.parse_program();
        assert!(parser.errors.is_empty(), "Parser errors: {:?}", parser.errors);
        let symbols = resolve_program(&program.statements).expect("Symbol resolution failed");
        let traits = crate::trait_solver::TraitEnvironment::new();
        let hir = lower_to_hir(&program.statements, &symbols, &traits);
        let mut ctx = TypeContext::new();
        let constraints = ctx.collect_constraints(&hir);
        ctx.solve(&constraints)?;
        Ok(ctx.substitution)
    }

    #[test]
    fn test_simple_let_int() {
        let sub = check("let x: int = 42").unwrap();
        // No errors, int = int is trivially satisfied
        assert_eq!(sub.apply(&Type::Int), Type::Int);
    }

    #[test]
    fn test_simple_infix() {
        let _sub = check("let x = 10 + 20").unwrap();
        // Both sides are Int, result is Int — no errors
    }

    #[test]
    fn test_unification_basic() {
        let mut ctx = TypeContext::new();
        ctx.unify(&Type::Int, &Type::Int).unwrap();
        let v = ctx.fresh_var();
        ctx.unify(&v, &Type::Float).unwrap();
        assert_eq!(ctx.substitution.apply(&v), Type::Float);
    }

    #[test]
    fn test_unification_mismatch() {
        let mut ctx = TypeContext::new();
        let result = ctx.unify(&Type::Int, &Type::Bool);
        assert!(result.is_err());
    }

    #[test]
    fn test_function_constraint() {
        let _sub = check("fn add(a: int, b: int) -> int { return a + b }").unwrap();
    }

    #[test]
    fn test_occurs_check() {
        let mut ctx = TypeContext::new();
        let v = ctx.fresh_var();
        // Trying to unify ?T with Array<?T> should fail (infinite type)
        let result = ctx.unify(&v, &Type::Array(Box::new(v.clone())));
        assert!(result.is_err());
    }
}
