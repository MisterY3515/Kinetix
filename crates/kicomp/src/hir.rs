/// HIR — High-level Intermediate Representation.
///
/// A typed version of the AST where every node is annotated with a `Type`.
/// This representation is produced from the untyped AST after symbol resolution,
/// and serves as input to constraint collection and unification.

use crate::types::{Type, TypeVarId, parse_type_hint};
use crate::symbol::SymbolTable;
use kinetix_language::ast::{Statement, Expression};

/// A fresh type variable counter for HIR lowering.
struct FreshCounter {
    next: TypeVarId,
}

impl FreshCounter {
    fn new() -> Self { Self { next: 1 } }
    fn fresh(&mut self) -> Type {
        let id = self.next;
        self.next += 1;
        Type::Var(id)
    }
}

// ──────────────────── HIR Nodes ────────────────────

#[derive(Debug, Clone)]
pub struct HirProgram {
    pub statements: Vec<HirStatement>,
}

#[derive(Debug, Clone)]
pub struct HirStatement {
    pub kind: HirStmtKind,
    pub ty: Type,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum HirStmtKind {
    Let {
        name: String,
        mutable: bool,
        value: HirExpression,
    },
    Return {
        value: Option<HirExpression>,
    },
    Expression {
        expression: HirExpression,
    },
    Block {
        statements: Vec<HirStatement>,
    },
    Function {
        name: String,
        parameters: Vec<(String, Type)>,
        body: Box<HirStatement>,
        return_type: Type,
    },
    While {
        condition: HirExpression,
        body: Box<HirStatement>,
    },
    For {
        iterator: String,
        range: HirExpression,
        body: Box<HirStatement>,
    },
    Break,
    Continue,
}

#[derive(Debug, Clone)]
pub struct HirExpression {
    pub kind: HirExprKind,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub enum HirExprKind {
    Identifier(String),
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
    Prefix {
        operator: String,
        right: Box<HirExpression>,
    },
    Infix {
        left: Box<HirExpression>,
        operator: String,
        right: Box<HirExpression>,
    },
    If {
        condition: Box<HirExpression>,
        consequence: Box<HirStatement>,
        alternative: Option<Box<HirStatement>>,
    },
    Call {
        function: Box<HirExpression>,
        arguments: Vec<HirExpression>,
    },
    FunctionLiteral {
        parameters: Vec<(String, Type)>,
        body: Box<HirStatement>,
        return_type: Type,
    },
    ArrayLiteral(Vec<HirExpression>),
    MapLiteral(Vec<(HirExpression, HirExpression)>),
    Index {
        left: Box<HirExpression>,
        index: Box<HirExpression>,
    },
    MemberAccess {
        object: Box<HirExpression>,
        member: String,
    },
    Assign {
        target: Box<HirExpression>,
        value: Box<HirExpression>,
    },
    Range {
        start: Box<HirExpression>,
        end: Box<HirExpression>,
    },
}

// ──────────────────── AST → HIR Lowering ────────────────────

/// Lower an untyped AST program into a typed HIR program.
/// Unknown types are assigned fresh type variables for later unification.
pub fn lower_to_hir<'a>(statements: &[Statement<'a>], symbols: &SymbolTable) -> HirProgram {
    let mut fresh = FreshCounter::new();
    let mut env = std::collections::HashMap::new();
    let stmts = statements.iter()
        .map(|s| lower_statement(s, symbols, &mut fresh, &mut env))
        .collect();
    HirProgram { statements: stmts }
}

fn get_line(stmt: &Statement) -> usize {
    match stmt {
        Statement::Let { line, .. } => *line,
        Statement::Return { line, .. } => *line,
        Statement::Expression { line, .. } => *line,
        Statement::Block { line, .. } => *line,
        Statement::Function { line, .. } => *line,
        Statement::While { line, .. } => *line,
        Statement::For { line, .. } => *line,
        Statement::Class { line, .. } => *line,
        Statement::Struct { line, .. } => *line,
        Statement::Include { line, .. } => *line,
        Statement::Version { line, .. } => *line,
        Statement::Break { line } => *line,
        Statement::Continue { line } => *line,
    }
}

fn lower_statement<'a>(stmt: &Statement<'a>, symbols: &SymbolTable, fresh: &mut FreshCounter, env: &mut std::collections::HashMap<String, Type>) -> HirStatement {
    let line = get_line(stmt);
    match stmt {
        Statement::Let { name, mutable, type_hint, value, .. } => {
            let val = lower_expression(value, symbols, fresh, env);
            let ty = match type_hint {
                Some(hint) => parse_type_hint(hint),
                None => fresh.fresh(),
            };
            // Register this variable in the type environment
            env.insert(name.clone(), ty.clone());
            HirStatement {
                kind: HirStmtKind::Let { name: name.clone(), mutable: *mutable, value: val },
                ty,
                line,
            }
        }
        Statement::Return { value, .. } => {
            let val = value.as_ref().map(|v| lower_expression(v, symbols, fresh, env));
            let ty = match &val {
                Some(e) => e.ty.clone(),
                None => Type::Void,
            };
            HirStatement { kind: HirStmtKind::Return { value: val }, ty, line }
        }
        Statement::Expression { expression, .. } => {
            let expr = lower_expression(expression, symbols, fresh, env);
            let ty = expr.ty.clone();
            HirStatement { kind: HirStmtKind::Expression { expression: expr }, ty, line }
        }
        Statement::Block { statements, .. } => {
            let stmts: Vec<HirStatement> = statements.iter()
                .map(|s| lower_statement(s, symbols, fresh, env))
                .collect();
            let ty = stmts.last().map(|s| s.ty.clone()).unwrap_or(Type::Void);
            HirStatement { kind: HirStmtKind::Block { statements: stmts }, ty, line }
        }
        Statement::Function { name, parameters, body, return_type, .. } => {
            let params: Vec<(String, Type)> = parameters.iter()
                .map(|(n, t)| (n.clone(), parse_type_hint(t)))
                .collect();
            let ret = parse_type_hint(return_type);
            
            // Function scope clone to prevent leaking params into global
            let mut func_env = env.clone();
            for (p_name, p_ty) in &params {
                func_env.insert(p_name.clone(), p_ty.clone());
            }

            let hir_body = Box::new(lower_statement(body, symbols, fresh, &mut func_env));
            HirStatement {
                kind: HirStmtKind::Function { name: name.clone(), parameters: params, body: hir_body, return_type: ret.clone() },
                ty: Type::Void, // function definitions don't produce a value
                line,
            }
        }
        Statement::While { condition, body, .. } => {
            let cond = lower_expression(condition, symbols, fresh, env);
            let b = Box::new(lower_statement(body, symbols, fresh, env));
            HirStatement { kind: HirStmtKind::While { condition: cond, body: b }, ty: Type::Void, line }
        }
        Statement::For { iterator, range, body, .. } => {
            let r = lower_expression(range, symbols, fresh, env);
            
            // Scope iter var
            let mut for_env = env.clone();
            for_env.insert(iterator.clone(), Type::Int); // Iterators over Ranges are Int
            let b = Box::new(lower_statement(body, symbols, fresh, &mut for_env));
            
            HirStatement {
                kind: HirStmtKind::For { iterator: iterator.clone(), range: r, body: b },
                ty: Type::Void, line,
            }
        }
        Statement::Break { .. } => HirStatement { kind: HirStmtKind::Break, ty: Type::Void, line },
        Statement::Continue { .. } => HirStatement { kind: HirStmtKind::Continue, ty: Type::Void, line },
        // Class, Struct, Include, Version — skip for now (handled in later phases)
        _ => HirStatement { kind: HirStmtKind::Break, ty: Type::Void, line },
    }
}

fn lower_expression<'a>(expr: &Expression<'a>, symbols: &SymbolTable, fresh: &mut FreshCounter, env: &mut std::collections::HashMap<String, Type>) -> HirExpression {
    match expr {
        Expression::Integer(v) => HirExpression { kind: HirExprKind::Integer(*v), ty: Type::Int },
        Expression::Float(v) => HirExpression { kind: HirExprKind::Float(*v), ty: Type::Float },
        Expression::String(v) => HirExpression { kind: HirExprKind::String(v.clone()), ty: Type::Str },
        Expression::Boolean(v) => HirExpression { kind: HirExprKind::Boolean(*v), ty: Type::Bool },
        Expression::Null => HirExpression { kind: HirExprKind::Null, ty: Type::Void },
        Expression::Identifier(name) => {
            let ty = if name == "println" || name == "print" {
                // println/print accept any argument, so we give them a fresh type variable parameter
                Type::Fn(vec![fresh.fresh()], Box::new(Type::Void))
            } else if let Some(t) = env.get(name) {
                // Prefer the HIR-level type environment so unification uses the same type
                // variable across all usages of a Let-defined variable.
                t.clone()
            } else if let Some(t) = symbols.resolve(name).map(|s| s.ty.clone()) {
                // Fallback: builtins, functions, classes registered in the SymbolTable.
                t
            } else {
                let fr = fresh.fresh();
                env.insert(name.clone(), fr.clone());
                fr
            };
            HirExpression { kind: HirExprKind::Identifier(name.clone()), ty }
        }
        Expression::Prefix { operator, right } => {
            let r = lower_expression(right, symbols, fresh, env);
            let ty = match operator.as_str() {
                "&" => Type::Ref(Box::new(r.ty.clone())),
                "&mut" => Type::MutRef(Box::new(r.ty.clone())),
                _ => r.ty.clone(),
            };
            HirExpression {
                kind: HirExprKind::Prefix { operator: operator.clone(), right: Box::new(r) },
                ty,
            }
        }
        Expression::Infix { left, operator, right } => {
            let l = lower_expression(left, symbols, fresh, env);
            let r = lower_expression(right, symbols, fresh, env);
            let ty = fresh.fresh(); // will be constrained later
            HirExpression {
                kind: HirExprKind::Infix { left: Box::new(l), operator: operator.clone(), right: Box::new(r) },
                ty,
            }
        }
        Expression::If { condition, consequence, alternative } => {
            let cond = lower_expression(condition, symbols, fresh, env);
            let cons = lower_statement(consequence, symbols, fresh, env);
            let alt = alternative.as_ref().map(|a| Box::new(lower_statement(a, symbols, fresh, env)));
            let ty = cons.ty.clone();
            HirExpression {
                kind: HirExprKind::If { condition: Box::new(cond), consequence: Box::new(cons), alternative: alt },
                ty,
            }
        }
        Expression::Call { function, arguments } => {
            let func = lower_expression(function, symbols, fresh, env);
            let args: Vec<HirExpression> = arguments.iter()
                .map(|a| lower_expression(a, symbols, fresh, env))
                .collect();
            let ty = fresh.fresh(); // return type inferred later
            HirExpression {
                kind: HirExprKind::Call { function: Box::new(func), arguments: args },
                ty,
            }
        }
        Expression::FunctionLiteral { parameters, body, return_type } => {
            let params: Vec<(String, Type)> = parameters.iter()
                .map(|(n, t)| (n.clone(), parse_type_hint(t)))
                .collect();
            let ret = parse_type_hint(return_type);
            
            let mut func_env = env.clone();
            for (p_name, p_ty) in &params {
                func_env.insert(p_name.clone(), p_ty.clone());
            }

            let hir_body = lower_statement(body, symbols, fresh, &mut func_env);
            let param_types: Vec<Type> = params.iter().map(|(_, t)| t.clone()).collect();
            HirExpression {
                kind: HirExprKind::FunctionLiteral { parameters: params, body: Box::new(hir_body), return_type: ret.clone() },
                ty: Type::Fn(param_types, Box::new(ret)),
            }
        }
        Expression::ArrayLiteral(elems) => {
            let hir_elems: Vec<HirExpression> = elems.iter()
                .map(|e| lower_expression(e, symbols, fresh, env))
                .collect();
            let elem_ty = hir_elems.first().map(|e| e.ty.clone()).unwrap_or_else(|| fresh.fresh());
            HirExpression {
                kind: HirExprKind::ArrayLiteral(hir_elems),
                ty: Type::Array(Box::new(elem_ty)),
            }
        }
        Expression::MapLiteral(pairs) => {
            let hir_pairs: Vec<(HirExpression, HirExpression)> = pairs.iter()
                .map(|(k, v)| (lower_expression(k, symbols, fresh, env), lower_expression(v, symbols, fresh, env)))
                .collect();
            let (kt, vt) = hir_pairs.first()
                .map(|(k, v)| (k.ty.clone(), v.ty.clone()))
                .unwrap_or_else(|| (fresh.fresh(), fresh.fresh()));
            HirExpression {
                kind: HirExprKind::MapLiteral(hir_pairs),
                ty: Type::Map(Box::new(kt), Box::new(vt)),
            }
        }
        Expression::Index { left, index } => {
            let l = lower_expression(left, symbols, fresh, env);
            let i = lower_expression(index, symbols, fresh, env);
            let ty = fresh.fresh();
            HirExpression {
                kind: HirExprKind::Index { left: Box::new(l), index: Box::new(i) },
                ty,
            }
        }
        Expression::MemberAccess { object, member } => {
            let obj = lower_expression(object, symbols, fresh, env);
            let mut ty = fresh.fresh();

            // Intercept built-in standard library methods so Type Checker knows their signature
            if let HirExprKind::Identifier(ref obj_name) = obj.kind {
                match obj_name.as_str() {
                    "math" => match member.as_str() {
                        "sin" | "cos" | "tan" | "sqrt" | "abs" | "floor" | "ceil" | "round" 
                        | "asin" | "acos" | "atan" | "log" | "log10" | "exp" => {
                            ty = Type::Fn(vec![Type::Float], Box::new(Type::Float));
                        }
                        "pow" | "atan2" => {
                            ty = Type::Fn(vec![Type::Float, Type::Float], Box::new(Type::Float));
                        }
                        _ => {}
                    },
                    "system" => match member.as_str() {
                        "time" => ty = Type::Fn(vec![], Box::new(Type::Float)),
                        "exit" => ty = Type::Fn(vec![Type::Int], Box::new(Type::Void)),
                        _ => {}
                    },
                    "auth" => match member.as_str() {
                        "login" => ty = Type::Fn(vec![Type::Str, Type::Str], Box::new(Type::Bool)),
                        "role" => ty = Type::Fn(vec![], Box::new(Type::Str)),
                        _ => {}
                    },
                    _ => {}
                }
            }

            HirExpression {
                kind: HirExprKind::MemberAccess { object: Box::new(obj), member: member.clone() },
                ty,
            }
        }
        Expression::Assign { target, value } => {
            let t = lower_expression(target, symbols, fresh, env);
            let v = lower_expression(value, symbols, fresh, env);
            let ty = v.ty.clone();
            HirExpression {
                kind: HirExprKind::Assign { target: Box::new(t), value: Box::new(v) },
                ty,
            }
        }
        Expression::Range { start, end } => {
            let s = lower_expression(start, symbols, fresh, env);
            let e = lower_expression(end, symbols, fresh, env);
            HirExpression {
                kind: HirExprKind::Range { start: Box::new(s), end: Box::new(e) },
                ty: Type::Array(Box::new(Type::Int)), // ranges are int arrays
            }
        }
        Expression::Match { .. } => {
            // Match expressions will be fully supported in Phase 2
            HirExpression { kind: HirExprKind::Null, ty: fresh.fresh() }
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

    fn lower(src: &str) -> HirProgram {
        let arena = Bump::new();
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, &arena);
        let program = parser.parse_program();
        assert!(parser.errors.is_empty(), "Parser errors: {:?}", parser.errors);
        let symbols = resolve_program(&program.statements).expect("Symbol resolution failed");
        lower_to_hir(&program.statements, &symbols)
    }

    #[test]
    fn test_lower_let() {
        let hir = lower("let x = 42");
        assert_eq!(hir.statements.len(), 1);
        match &hir.statements[0].kind {
            HirStmtKind::Let { name, .. } => assert_eq!(name, "x"),
            other => panic!("Expected Let, got {:?}", other),
        }
    }

    #[test]
    fn test_lower_function() {
        let hir = lower("fn add(a: int, b: int) -> int { return a + b }");
        assert_eq!(hir.statements.len(), 1);
        match &hir.statements[0].kind {
            HirStmtKind::Function { name, parameters, return_type, .. } => {
                assert_eq!(name, "add");
                assert_eq!(parameters.len(), 2);
                assert_eq!(return_type, &Type::Int);
            }
            other => panic!("Expected Function, got {:?}", other),
        }
    }

    #[test]
    fn test_literal_types() {
        let hir = lower("let a = 10\nlet b = 3.14\nlet c = true\nlet d = \"hello\"");
        // Check that the expressions within each Let have the correct types
        for (i, expected) in [(0, Type::Int), (1, Type::Float), (2, Type::Bool), (3, Type::Str)] {
            match &hir.statements[i].kind {
                HirStmtKind::Let { value, .. } => assert_eq!(value.ty, expected, "Statement {}", i),
                _ => panic!("Expected Let"),
            }
        }
    }
}
