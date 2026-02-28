/// HIR Integrity Validation Pass (Build 20)
///
/// Walks the typed HIR tree and catches structural violations before MIR lowering.
///
/// Checks enforced:
/// 1. **Duplicate function parameters** — Two params with the same name.
/// 2. **Unreachable statements** — Code after unconditional `return` or `break` in a block.
///
/// Note: Type::Var in the HIR is expected (Hindley-Milner). Unresolved type variables
/// are caught later by mono_validate after MIR lowering + monomorphization.

use crate::hir::{HirProgram, HirStatement, HirStmtKind, HirExpression, HirExprKind, HirPattern};
use crate::types::Type;

/// Validate an entire HIR program.
/// Returns `Ok(())` if the HIR is structurally sound, or a list of diagnostic errors.
pub fn validate(program: &HirProgram) -> Result<(), Vec<String>> {
    let mut errors: Vec<String> = Vec::new();

    for stmt in &program.statements {
        validate_statement(stmt, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_statement(stmt: &HirStatement, errors: &mut Vec<String>) {
    match &stmt.kind {
        HirStmtKind::Let { value, .. }
        | HirStmtKind::State { value, .. }
        | HirStmtKind::Computed { value, .. } => {
            validate_expression(value, errors);
        }

        HirStmtKind::Function { name, parameters, body, .. } => {
            // Check for duplicate parameter names
            check_duplicate_params(name, parameters, stmt.line, errors);
            validate_statement(body, errors);
        }

        HirStmtKind::Class { methods, .. } => {
            for method in methods {
                validate_statement(method, errors);
            }
        }

        HirStmtKind::Block { statements } => {
            check_unreachable_stmts(statements, errors);
            for s in statements {
                validate_statement(s, errors);
            }
        }

        HirStmtKind::Return { value } => {
            if let Some(expr) = value {
                validate_expression(expr, errors);
            }
        }

        HirStmtKind::Expression { expression } => {
            validate_expression(expression, errors);
        }

        HirStmtKind::While { condition, body } => {
            validate_expression(condition, errors);
            validate_statement(body, errors);
        }

        HirStmtKind::For { range, body, .. } => {
            validate_expression(range, errors);
            validate_statement(body, errors);
        }

        HirStmtKind::Effect { body, .. } => {
            validate_statement(body, errors);
        }

        HirStmtKind::Break | HirStmtKind::Continue => {}
    }
}

fn validate_expression(expr: &HirExpression, errors: &mut Vec<String>) {
    match &expr.kind {
        HirExprKind::Integer(_) | HirExprKind::Float(_) | HirExprKind::String(_)
        | HirExprKind::Boolean(_) | HirExprKind::Null | HirExprKind::Identifier(_) => {}

        HirExprKind::Prefix { right, .. } => {
            validate_expression(right, errors);
        }

        HirExprKind::Infix { left, right, .. } => {
            validate_expression(left, errors);
            validate_expression(right, errors);
        }

        HirExprKind::If { condition, consequence, alternative } => {
            validate_expression(condition, errors);
            validate_statement(consequence, errors);
            if let Some(alt) = alternative {
                validate_statement(alt, errors);
            }
        }

        HirExprKind::Call { function, arguments } => {
            validate_expression(function, errors);
            for arg in arguments {
                validate_expression(arg, errors);
            }
        }

        HirExprKind::FunctionLiteral { parameters, body, .. } => {
            check_duplicate_params("<lambda>", parameters, 0, errors);
            validate_statement(body, errors);
        }

        HirExprKind::ArrayLiteral(elems) => {
            for e in elems {
                validate_expression(e, errors);
            }
        }

        HirExprKind::StructLiteral(_, fields) => {
            for (_, val) in fields {
                validate_expression(val, errors);
            }
        }

        HirExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                validate_expression(k, errors);
                validate_expression(v, errors);
            }
        }

        HirExprKind::Index { left, index } => {
            validate_expression(left, errors);
            validate_expression(index, errors);
        }

        HirExprKind::MethodCall { object, arguments, .. } => {
            validate_expression(object, errors);
            for arg in arguments {
                validate_expression(arg, errors);
            }
        }

        HirExprKind::MemberAccess { object, .. } => {
            validate_expression(object, errors);
        }

        HirExprKind::Assign { target, value } => {
            validate_expression(target, errors);
            validate_expression(value, errors);
        }

        HirExprKind::Range { start, end } => {
            validate_expression(start, errors);
            validate_expression(end, errors);
        }

        HirExprKind::Match { value, arms } => {
            validate_expression(value, errors);
            for (pattern, body) in arms {
                validate_pattern(pattern, errors);
                validate_statement(body, errors);
            }
        }
    }
}

fn validate_pattern(pattern: &HirPattern, errors: &mut Vec<String>) {
    match pattern {
        HirPattern::Literal(expr) => validate_expression(expr, errors),
        HirPattern::Variant { .. } | HirPattern::Wildcard | HirPattern::Binding(_) => {}
    }
}


/// Check for duplicate parameter names in a function.
fn check_duplicate_params(fn_name: &str, params: &[(String, Type)], line: usize, errors: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    for (name, _) in params {
        if !seen.insert(name.as_str()) {
            errors.push(format!(
                "HIR Integrity Error (line {}): Duplicate parameter '{}' in function '{}'",
                line, name, fn_name
            ));
        }
    }
}

/// Check for unreachable statements after an unconditional `return` or `break`.
fn check_unreachable_stmts(stmts: &[HirStatement], errors: &mut Vec<String>) {
    for (i, stmt) in stmts.iter().enumerate() {
        let is_terminal = matches!(&stmt.kind,
            HirStmtKind::Return { .. } | HirStmtKind::Break | HirStmtKind::Continue
        );
        if is_terminal && i + 1 < stmts.len() {
            let next = &stmts[i + 1];
            errors.push(format!(
                "HIR Integrity Warning (line {}): Unreachable statement after {:?} at line {}",
                next.line,
                match &stmt.kind {
                    HirStmtKind::Return { .. } => "return",
                    HirStmtKind::Break => "break",
                    HirStmtKind::Continue => "continue",
                    _ => "terminal",
                },
                stmt.line
            ));
            break; // Only report the first unreachable statement
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::*;
    use crate::types::Type;

    fn make_expr(kind: HirExprKind, ty: Type) -> HirExpression {
        HirExpression { kind, ty }
    }

    fn make_stmt(kind: HirStmtKind, ty: Type, line: usize) -> HirStatement {
        HirStatement { kind, ty, line }
    }

    #[test]
    fn test_valid_program() {
        let program = HirProgram {
            statements: vec![
                make_stmt(
                    HirStmtKind::Let {
                        name: "x".to_string(),
                        mutable: false,
                        value: make_expr(HirExprKind::Integer(42), Type::Int),
                    },
                    Type::Int,
                    1,
                ),
            ],
        };
        assert!(validate(&program).is_ok());
    }

    #[test]
    fn test_type_var_accepted_in_hir() {
        // Type::Var is expected in HIR (Hindley-Milner) — resolved during MIR lowering
        let program = HirProgram {
            statements: vec![
                make_stmt(
                    HirStmtKind::Let {
                        name: "x".to_string(),
                        mutable: false,
                        value: make_expr(HirExprKind::Integer(42), Type::Var(99)),
                    },
                    Type::Var(99),
                    1,
                ),
            ],
        };
        assert!(validate(&program).is_ok());
    }

    #[test]
    fn test_duplicate_params() {
        let program = HirProgram {
            statements: vec![
                make_stmt(
                    HirStmtKind::Function {
                        name: "bad_fn".to_string(),
                        parameters: vec![
                            ("a".to_string(), Type::Int),
                            ("b".to_string(), Type::Int),
                            ("a".to_string(), Type::Float), // duplicate
                        ],
                        body: Box::new(make_stmt(
                            HirStmtKind::Block { statements: vec![] },
                            Type::Void,
                            2,
                        )),
                        return_type: Type::Void,
                    },
                    Type::Void,
                    1,
                ),
            ],
        };
        let result = validate(&program);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(errs.iter().any(|e| e.contains("Duplicate parameter 'a'")));
    }

    #[test]
    fn test_unreachable_after_return() {
        let program = HirProgram {
            statements: vec![
                make_stmt(
                    HirStmtKind::Block {
                        statements: vec![
                            make_stmt(HirStmtKind::Return { value: None }, Type::Void, 1),
                            make_stmt(
                                HirStmtKind::Let {
                                    name: "dead".to_string(),
                                    mutable: false,
                                    value: make_expr(HirExprKind::Integer(1), Type::Int),
                                },
                                Type::Int,
                                2,
                            ),
                        ],
                    },
                    Type::Void,
                    1,
                ),
            ],
        };
        let result = validate(&program);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(errs.iter().any(|e| e.contains("Unreachable")));
    }
}
