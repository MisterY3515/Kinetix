use crate::types::Type;
use crate::hir::HirPattern;
use crate::symbol::SymbolTable;

/// Validates that a list of match arms exhaustively covers the structural space of `match_ty`.
/// For Kinetix Phase 2, we implement a 1D mapping matrix to evaluate Enum variant coverage
/// and Primitive infinite domain coverage.
pub fn check_exhaustiveness(match_ty: &Type, arms: &[HirPattern], symbols: &SymbolTable) -> Result<(), String> {
    // 1. Wildcard (`_`) or unconditional binding (`x`) provides immediate exhaustive coverage.
    for arm in arms {
        match arm {
            HirPattern::Wildcard | HirPattern::Binding(_) => return Ok(()),
            _ => {}
        }
    }

    match match_ty {
        Type::Bool => {
            let mut has_true = false;
            let mut has_false = false;
            for arm in arms {
                if let HirPattern::Literal(lit) = arm {
                    if let crate::hir::HirExprKind::Boolean(b) = lit.kind {
                        if b { has_true = true; } else { has_false = true; }
                    }
                }
            }
            if !has_true { return Err("Missing coverage for: true".to_string()); }
            if !has_false { return Err("Missing coverage for: false".to_string()); }
            Ok(())
        }
        Type::Custom { name, .. } => {
            // For Kinetix Phase 2, we only fully support Option and Result builtins as exhaustively checked enums.
            // User-defined enums require AST variant lookup, which we simulate for Option/Result.
            let variants = match name.as_str() {
                "Option" => vec!["Some", "None"],
                "Result" => vec!["Ok", "Err"],
                _ => {
                    // For custom user structs/classes, a wildcard or binding is required.
                    return Err(format!("Non-exhaustive match. Type '{}' requires a catch-all bound", name));
                }
            };
            
            let mut covered_variants = std::collections::HashSet::new();
            for arm in arms {
                if let HirPattern::Variant { name: var_name, .. } = arm {
                    covered_variants.insert(var_name.clone());
                }
            }
            
            for var in variants {
                if !covered_variants.contains(var) {
                    return Err(format!("Missing coverage for variant: {}", var));
                }
            }
            Ok(())
        }
        Type::Int | Type::Float | Type::Str => {
            // Infinite domains cannot be exhaustively matched by structural literals alone.
            Err(format!("Non-exhaustive match. Add a `_` arm to cover all cases for type {}", match_ty))
        }
        _ => Err(format!("Cannot match against type {:?}", match_ty)),
    }
}

pub fn check_program_exhaustiveness(
    hir: &crate::hir::HirProgram,
    symbols: &SymbolTable,
    sub: &crate::types::Substitution,
) -> Result<(), String> {
    for stmt in &hir.statements {
        check_statement(stmt, symbols, sub)?;
    }
    Ok(())
}

fn check_statement(
    stmt: &crate::hir::HirStatement,
    symbols: &SymbolTable,
    sub: &crate::types::Substitution,
) -> Result<(), String> {
    use crate::hir::HirStmtKind;
    match &stmt.kind {
        HirStmtKind::Let { value, .. } | HirStmtKind::Return { value: Some(value) } => {
            check_expression(value, symbols, sub)?;
        }
        HirStmtKind::Expression { expression } => {
            check_expression(expression, symbols, sub)?;
        }
        HirStmtKind::Block { statements } => {
            for s in statements {
                check_statement(s, symbols, sub)?;
            }
        }
        HirStmtKind::Function { body, .. } => {
            check_statement(body, symbols, sub)?;
        }
        HirStmtKind::While { condition, body } => {
            check_expression(condition, symbols, sub)?;
            check_statement(body, symbols, sub)?;
        }
        HirStmtKind::For { range, body, .. } => {
            check_expression(range, symbols, sub)?;
            check_statement(body, symbols, sub)?;
        }
        _ => {}
    }
    Ok(())
}

fn check_expression(
    expr: &crate::hir::HirExpression,
    symbols: &SymbolTable,
    sub: &crate::types::Substitution,
) -> Result<(), String> {
    use crate::hir::HirExprKind;
    match &expr.kind {
        HirExprKind::If { condition, consequence, alternative } => {
            check_expression(condition, symbols, sub)?;
            check_statement(consequence, symbols, sub)?;
            if let Some(alt) = alternative {
                check_statement(alt, symbols, sub)?;
            }
        }
        HirExprKind::Match { value, arms } => {
            check_expression(value, symbols, sub)?;
            let resolved_ty = sub.apply(&value.ty);
            let patterns: Vec<HirPattern> = arms.iter().map(|(p, _)| p.clone()).collect();
            check_exhaustiveness(&resolved_ty, &patterns, symbols)?;
            
            for (_, body) in arms {
                check_statement(body, symbols, sub)?;
            }
        }
        HirExprKind::Prefix { right, .. } => check_expression(right, symbols, sub)?,
        HirExprKind::Infix { left, right, .. } => {
            check_expression(left, symbols, sub)?;
            check_expression(right, symbols, sub)?;
        }
        HirExprKind::Call { function, arguments } => {
            check_expression(function, symbols, sub)?;
            for arg in arguments {
                check_expression(arg, symbols, sub)?;
            }
        }
        HirExprKind::FunctionLiteral { body, .. } => {
            check_statement(body, symbols, sub)?;
        }
        HirExprKind::ArrayLiteral(elems) => {
            for e in elems { check_expression(e, symbols, sub)?; }
        }
        HirExprKind::MapLiteral(pairs) => {
            for (k, v) in pairs {
                check_expression(k, symbols, sub)?;
                check_expression(v, symbols, sub)?;
            }
        }
        HirExprKind::Index { left, index } => {
            check_expression(left, symbols, sub)?;
            check_expression(index, symbols, sub)?;
        }
        HirExprKind::MemberAccess { object, .. } => {
            check_expression(object, symbols, sub)?;
        }
        HirExprKind::Assign { target, value } => {
            check_expression(target, symbols, sub)?;
            check_expression(value, symbols, sub)?;
        }
        HirExprKind::Range { start, end } => {
            check_expression(start, symbols, sub)?;
            check_expression(end, symbols, sub)?;
        }
        _ => {} // primitive literals etc.
    }
    Ok(())
}
