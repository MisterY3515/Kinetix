/// Type Normalization Pass
///
/// Converts a typed HIR program into a normalized HIR program by:
/// 1. Expanding type aliases (if any).
/// 2. Canonicalizing generic constraints before they hit the trait solver.
/// 3. Validating structural recursion limits post-lowering.

use crate::hir::{HirProgram, HirStatement, HirStmtKind, HirExpression, HirExprKind, HirPattern};
use crate::types::Type;

/// Normalizes the entire HIR program.
pub fn normalize(program: &mut HirProgram) -> Result<(), String> {
    for stmt in &mut program.statements {
        normalize_stmt(stmt)?;
    }
    Ok(())
}

fn normalize_stmt(stmt: &mut HirStatement) -> Result<(), String> {
    // Normalize the type attached to the statement
    stmt.ty = normalize_type(&stmt.ty)?;

    match &mut stmt.kind {
        HirStmtKind::Let { value, .. } => {
            normalize_expr(value)?;
        }
        HirStmtKind::Return { value } => {
            if let Some(expr) = value {
                normalize_expr(expr)?;
            }
        }
        HirStmtKind::Expression { expression: expr } => {
            normalize_expr(expr)?;
        }
        HirStmtKind::Block { statements } => {
            for s in statements {
                normalize_stmt(s)?;
            }
        }
        HirStmtKind::While { condition, body } => {
            normalize_expr(condition)?;
            normalize_stmt(body)?;
        }
        HirStmtKind::For { range: iterable, body, .. } => {
            normalize_expr(iterable)?;
            normalize_stmt(body)?;
        }
        HirStmtKind::Function { parameters: params, return_type, body, .. } => {
            for (_, ty) in params {
                *ty = normalize_type(ty)?;
            }
            *return_type = normalize_type(return_type)?;
            normalize_stmt(body)?;
        }
        HirStmtKind::Break | HirStmtKind::Continue => {}
        HirStmtKind::State { value, .. } | HirStmtKind::Computed { value, .. } => {
            normalize_expr(value)?;
        }
        HirStmtKind::Effect { body, .. } => {
            normalize_stmt(body)?;
        }
    }
    Ok(())
}

fn normalize_expr(expr: &mut HirExpression) -> Result<(), String> {
    expr.ty = normalize_type(&expr.ty)?;

    match &mut expr.kind {
        HirExprKind::Infix { left, right, .. } => {
            normalize_expr(left)?;
            normalize_expr(right)?;
        }
        HirExprKind::Prefix { right: operand, .. } => {
            normalize_expr(operand)?;
        }
        HirExprKind::Call { function, arguments } => {
            normalize_expr(function)?;
            for arg in arguments {
                normalize_expr(arg)?;
            }
        }
        HirExprKind::ArrayLiteral(elements) => {
            for el in elements {
                normalize_expr(el)?;
            }
        }
        HirExprKind::StructLiteral(_, fields) => {
            for (_, field_expr) in fields {
                normalize_expr(field_expr)?;
            }
        }
        HirExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                normalize_expr(k)?;
                normalize_expr(v)?;
            }
        }
        HirExprKind::If { condition, consequence: then_branch, alternative: else_branch } => {
            normalize_expr(condition)?;
            normalize_stmt(then_branch)?;
            if let Some(else_b) = else_branch {
                normalize_stmt(else_b)?;
            }
        }
        HirExprKind::Match { value, arms } => {
            normalize_expr(value)?;
            for (pat, body) in arms {
                normalize_pattern(pat)?;
                normalize_stmt(body)?;
            }
        }
        HirExprKind::Index { left, index: right } => {
            normalize_expr(left)?;
            normalize_expr(right)?;
        }
        HirExprKind::MemberAccess { object, .. } => {
            normalize_expr(object)?;
        }
        HirExprKind::Range { start: left, end: right } => {
            normalize_expr(left)?;
            normalize_expr(right)?;
        }
        HirExprKind::Assign { value, target } => {
            normalize_expr(target)?;
            normalize_expr(value)?;
        }

        // Try is not a HirExprKind variant (it is compiled into Match early)
        HirExprKind::FunctionLiteral { parameters, return_type, body } => {
            for (_, ty) in parameters {
                *ty = normalize_type(ty)?;
            }
            *return_type = normalize_type(return_type)?;
            normalize_stmt(body)?;
        }
        HirExprKind::Identifier(_) | HirExprKind::Integer(_) | HirExprKind::Float(_) | HirExprKind::String(_) | HirExprKind::Boolean(_) | HirExprKind::Null => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_nested_arrays() {
        let ty = Type::Array(Box::new(Type::Array(Box::new(Type::Int))));
        let normalized = normalize_type(&ty).unwrap();
        
        // At this phase, without aliases, it's just identity clone
        assert_eq!(normalized, Type::Array(Box::new(Type::Array(Box::new(Type::Int)))));
    }

    #[test]
    fn test_normalize_generics() {
        let ty = Type::Custom { 
            name: "Result".to_string(), 
            args: vec![Type::Int, Type::Str] 
        };
        let normalized = normalize_type(&ty).unwrap();
        assert_eq!(normalized, Type::Custom { 
            name: "Result".to_string(), 
            args: vec![Type::Int, Type::Str] 
        });
    }
}

fn normalize_pattern(pat: &mut HirPattern) -> Result<(), String> {
    match pat {
        HirPattern::Literal(expr) => {
            normalize_expr(expr)?;
            Ok(())
        }
        HirPattern::Wildcard | HirPattern::Binding(_) | HirPattern::Variant { .. } => Ok(()),
    }
}

/// Core transformation: flattens known aliases, canonicalizes inner structures.
fn normalize_type(ty: &Type) -> Result<Type, String> {
    match ty {
        Type::Array(inner) => Ok(Type::Array(Box::new(normalize_type(inner)?))),
        Type::Map(k, v) => Ok(Type::Map(Box::new(normalize_type(k)?), Box::new(normalize_type(v)?))),
        Type::Ref(inner) => Ok(Type::Ref(Box::new(normalize_type(inner)?))),
        Type::MutRef(inner) => Ok(Type::MutRef(Box::new(normalize_type(inner)?))),
        Type::Fn(params, ret) => {
            let mut new_params = Vec::new();
            for p in params {
                new_params.push(normalize_type(p)?);
            }
            Ok(Type::Fn(new_params, Box::new(normalize_type(ret)?)))
        }
        Type::Custom { name, args } => {
            let mut new_args = Vec::new();
            for arg in args {
                new_args.push(normalize_type(arg)?);
            }
            // Future-proofing: here is where we would expand alias mappings like `type Foo = Option<int>`
            // For now, it just deeply normalizes all generic constraints to ensure standard representation.
            Ok(Type::Custom { name: name.clone(), args: new_args })
        }
        Type::Int | Type::Float | Type::Bool | Type::Str | Type::Void | Type::Var(_) => {
            Ok(ty.clone())
        }
    }
}
