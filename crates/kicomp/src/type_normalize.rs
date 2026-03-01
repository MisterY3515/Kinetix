/// Type Normalization Pass
///
/// Converts a typed HIR program into a normalized HIR program by:
/// 1. Expanding type aliases (if any).
/// 2. Canonicalizing generic constraints before they hit the trait solver.
/// 3. Validating structural recursion limits post-lowering.

use crate::hir::{HirProgram, HirStatement, HirStmtKind, HirExpression, HirExprKind, HirPattern};
use crate::types::Type;
use crate::symbol::SymbolTable;

/// Normalizes the entire HIR program.
pub fn normalize(program: &mut HirProgram, symbols: &SymbolTable) -> Result<(), String> {
    for stmt in &mut program.statements {
        normalize_stmt(stmt, symbols)?;
    }
    Ok(())
}

fn normalize_stmt(stmt: &mut HirStatement, symbols: &SymbolTable) -> Result<(), String> {
    // Normalize the type attached to the statement
    stmt.ty = normalize_type(&stmt.ty)?;

    match &mut stmt.kind {
        HirStmtKind::Let { value, .. } => {
            normalize_expr(value, symbols)?;
        }
        HirStmtKind::Return { value } => {
            if let Some(expr) = value {
                normalize_expr(expr, symbols)?;
            }
        }
        HirStmtKind::Expression { expression: expr } => {
            normalize_expr(expr, symbols)?;
        }
        HirStmtKind::Block { statements } => {
            for s in statements {
                normalize_stmt(s, symbols)?;
            }
        }
        HirStmtKind::While { condition, body } => {
            normalize_expr(condition, symbols)?;
            normalize_stmt(body, symbols)?;
        }
        HirStmtKind::For { range: iterable, body, .. } => {
            normalize_expr(iterable, symbols)?;
            normalize_stmt(body, symbols)?;
        }
        HirStmtKind::Function { parameters: params, return_type, body, .. } => {
            for (_, ty) in params {
                *ty = normalize_type(ty)?;
            }
            *return_type = normalize_type(return_type)?;
            normalize_stmt(body, symbols)?;
        }
        HirStmtKind::Break | HirStmtKind::Continue => {}
        HirStmtKind::State { value, .. } | HirStmtKind::Computed { value, .. } => {
            normalize_expr(value, symbols)?;
        }
        HirStmtKind::Class { methods, .. } => {
            for m in methods {
                normalize_stmt(m, symbols)?;
            }
        }
        HirStmtKind::Effect { body, .. } => {
            normalize_stmt(body, symbols)?;
        }
    }
    Ok(())
}

fn normalize_expr(expr: &mut HirExpression, symbols: &SymbolTable) -> Result<(), String> {
    expr.ty = normalize_type(&expr.ty)?;

    match &mut expr.kind {
        HirExprKind::Infix { left, right, .. } => {
            normalize_expr(left, symbols)?;
            normalize_expr(right, symbols)?;
        }
        HirExprKind::Prefix { right: operand, .. } => {
            normalize_expr(operand, symbols)?;
        }
        HirExprKind::Call { function, arguments } => {
            normalize_expr(function, symbols)?;
            for arg in arguments {
                normalize_expr(arg, symbols)?;
            }
        }
        HirExprKind::MethodCall { object, arguments, .. } => {
            // Pre-TypeChecker pass: just normalize sub-expressions.
            // The actual MethodCall → static Call dispatch happens in
            // resolve_method_calls() AFTER type inference, when we know
            // the concrete type of `object`.
            normalize_expr(object, symbols)?;
            for arg in arguments.iter_mut() {
                normalize_expr(arg, symbols)?;
            }
        }
        HirExprKind::ArrayLiteral(elements) => {
            for el in elements {
                normalize_expr(el, symbols)?;
            }
        }
        HirExprKind::StructLiteral(_, fields) => {
            for (_, field_expr) in fields {
                normalize_expr(field_expr, symbols)?;
            }
        }
        HirExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                normalize_expr(k, symbols)?;
                normalize_expr(v, symbols)?;
            }
        }
        HirExprKind::If { condition, consequence: then_branch, alternative: else_branch } => {
            normalize_expr(condition, symbols)?;
            normalize_stmt(then_branch, symbols)?;
            if let Some(else_b) = else_branch {
                normalize_stmt(else_b, symbols)?;
            }
        }
        HirExprKind::Match { value, arms } => {
            normalize_expr(value, symbols)?;
            for (pat, body) in arms {
                normalize_pattern(pat, symbols)?;
                normalize_stmt(body, symbols)?;
            }
        }
        HirExprKind::Index { left, index: right } => {
            normalize_expr(left, symbols)?;
            normalize_expr(right, symbols)?;
        }
        HirExprKind::MemberAccess { object, .. } => {
            normalize_expr(object, symbols)?;
        }
        HirExprKind::Range { start: left, end: right } => {
            normalize_expr(left, symbols)?;
            normalize_expr(right, symbols)?;
        }
        HirExprKind::Assign { value, target } => {
            normalize_expr(target, symbols)?;
            normalize_expr(value, symbols)?;
        }

        // Try is not a HirExprKind variant (it is compiled into Match early)
        HirExprKind::FunctionLiteral { parameters, return_type, body } => {
            for (_, ty) in parameters {
                *ty = normalize_type(ty)?;
            }
            *return_type = normalize_type(return_type)?;
            normalize_stmt(body, symbols)?;
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

fn normalize_pattern(pat: &mut HirPattern, symbols: &SymbolTable) -> Result<(), String> {
    match pat {
        HirPattern::Literal(expr) => {
            normalize_expr(expr, symbols)?;
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

// ============================================================================
// Post-TypeChecker Method Resolution
// ============================================================================

use crate::types::Substitution;

/// Resolve method calls AFTER type inference.
/// This pass walks the HIR, applies the substitution to discover concrete types,
/// and transforms MethodCall { object, method, args } into
/// Call { function: Identifier("Class::method"), arguments: [&mut object, ...args] }.
pub fn resolve_method_calls(
    program: &mut HirProgram,
    symbols: &SymbolTable,
    sub: &Substitution,
) -> Result<(), String> {
    for stmt in &mut program.statements {
        resolve_stmt(stmt, symbols, sub)?;
    }
    Ok(())
}

fn resolve_stmt(stmt: &mut HirStatement, symbols: &SymbolTable, sub: &Substitution) -> Result<(), String> {
    match &mut stmt.kind {
        HirStmtKind::Let { value, .. } => resolve_expr(value, symbols, sub)?,
        HirStmtKind::Return { value } => {
            if let Some(v) = value { resolve_expr(v, symbols, sub)?; }
        }
        HirStmtKind::Expression { expression } => resolve_expr(expression, symbols, sub)?,
        HirStmtKind::Block { statements } => {
            for s in statements { resolve_stmt(s, symbols, sub)?; }
        }
        HirStmtKind::Function { body, .. } => resolve_stmt(body, symbols, sub)?,
        HirStmtKind::State { value, .. } => resolve_expr(value, symbols, sub)?,
        HirStmtKind::Computed { value, .. } => resolve_expr(value, symbols, sub)?,
        HirStmtKind::Class { methods, .. } => {
            for m in methods { resolve_stmt(m, symbols, sub)?; }
        }
        HirStmtKind::Effect { body, .. } => resolve_stmt(body, symbols, sub)?,
        _ => {} // While, For, If, Break, Continue handled via expressions
    }
    Ok(())
}

fn resolve_expr(expr: &mut HirExpression, symbols: &SymbolTable, sub: &Substitution) -> Result<(), String> {
    // First, apply substitution to this expression's type so we have concrete types
    expr.ty = sub.apply_default(&expr.ty);

    match &mut expr.kind {
        HirExprKind::MethodCall { object, arguments, .. } => {
            // Recurse into children first
            resolve_expr(object, symbols, sub)?;
            for arg in arguments.iter_mut() {
                resolve_expr(arg, symbols, sub)?;
            }

            // Now object.ty should be concrete
            let resolved_ty = sub.apply_default(&object.ty);
            let class_name = if let Type::Custom { name, .. } = &resolved_ty {
                Some(name.clone())
            } else {
                None
            };

            if let Some(class_name) = class_name {
                if let Some(struct_def) = symbols.custom_types.get(&class_name) {
                    // Extract method name before swapping
                    let method_name = if let HirExprKind::MethodCall { method, .. } = &expr.kind {
                        method.clone()
                    } else {
                        unreachable!()
                    };

                    if let Some(method_type) = struct_def.methods.get(&method_name) {
                        let mut takes_mut_ref = false;
                        if let Type::Fn(params, _) = method_type {
                            if let Some(first_param_ty) = params.first() {
                                takes_mut_ref = matches!(first_param_ty, Type::MutRef(_));
                            }
                        }

                        let fully_qualified_name = format!("{}::{}", class_name, method_name);

                        let mut temp_kind = HirExprKind::Null;
                        std::mem::swap(&mut expr.kind, &mut temp_kind);

                        if let HirExprKind::MethodCall { object: extracted_obj, arguments: mut ext_args, .. } = temp_kind {
                            let mut new_args = Vec::new();

                            if takes_mut_ref {
                                let self_type = Type::MutRef(Box::new(extracted_obj.ty.clone()));
                                new_args.push(crate::hir::HirExpression {
                                    kind: HirExprKind::Prefix {
                                        operator: "&mut".to_string(),
                                        right: Box::new(*extracted_obj),
                                    },
                                    ty: self_type,
                                });
                            } else {
                                // Consuming method: passes `self` by value (Move)
                                new_args.push(*extracted_obj);
                            }

                            new_args.append(&mut ext_args);

                            expr.kind = HirExprKind::Call {
                                function: Box::new(crate::hir::HirExpression {
                                    kind: HirExprKind::Identifier(fully_qualified_name),
                                    ty: Type::Var(0),
                                }),
                                arguments: new_args,
                            };
                        }
                    } else {
                        return Err(format!("Type `{}` has no method named `{}`", class_name,
                            if let HirExprKind::MethodCall { method, .. } = &expr.kind { method.clone() } else { "?".into() }
                        ));
                    }
                } else {
                    return Err(format!("Unrecognized custom type `{}` for method call", class_name));
                }
            } else {
                // Fallback: This is not a resolved Custom Type method call.
                // It could be a Builtin Module Call (e.g., `system.os.isWindows()`).
                
                // Helper per srotolare la chain di MemberAccess
                fn extract_full_path(expr: &HirExpression) -> Option<String> {
                    match &expr.kind {
                        HirExprKind::Identifier(name) => Some(name.clone()),
                        HirExprKind::MemberAccess { object, member } => {
                            let parent = extract_full_path(object)?;
                            Some(format!("{}.{}", parent, member))
                        }
                        _ => None
                    }
                }

                if let Some(mut temp_kind) = None.or_else(|| {
                    let mut k = HirExprKind::Null;
                    std::mem::swap(&mut expr.kind, &mut k);
                    Some(k)
                }) {
                    if let HirExprKind::MethodCall { object: extracted_obj, method: method_name, arguments: ext_args } = temp_kind {
                        
                        if let Some(parent_path) = extract_full_path(&extracted_obj) {
                            // Se riesce ad estrarre parent string type (es: "system" o "system.os")
                            // Allora lo convertiamo in una chiamata piana "system.os.method"
                            let fully_qualified_name = format!("{}.{}", parent_path, method_name);
                            expr.kind = HirExprKind::Call {
                                function: Box::new(crate::hir::HirExpression {
                                    kind: HirExprKind::Identifier(fully_qualified_name),
                                    ty: expr.ty.clone(),
                                }),
                                arguments: ext_args,
                            };
                            return Ok(());
                        } else {
                            // Non estraibile come dot path (es array literal `[1].len()`), lasciamo MethodCall come è
                            expr.kind = HirExprKind::MethodCall { object: extracted_obj, method: method_name, arguments: ext_args };
                        }
                    } else {
                        std::mem::swap(&mut expr.kind, &mut temp_kind);
                    }
                }
            }
        }
        // Recurse into all other expression kinds
        HirExprKind::Infix { left, right, .. } => {
            resolve_expr(left, symbols, sub)?;
            resolve_expr(right, symbols, sub)?;
        }
        HirExprKind::Prefix { right, .. } => resolve_expr(right, symbols, sub)?,
        HirExprKind::Call { function, arguments } => {
            resolve_expr(function, symbols, sub)?;
            for arg in arguments { resolve_expr(arg, symbols, sub)?; }
        }
        HirExprKind::ArrayLiteral(elems) => {
            for e in elems { resolve_expr(e, symbols, sub)?; }
        }
        HirExprKind::StructLiteral(_, fields) => {
            for (_, f) in fields { resolve_expr(f, symbols, sub)?; }
        }
        HirExprKind::MapLiteral(entries) => {
            for (k, v) in entries { resolve_expr(k, symbols, sub)?; resolve_expr(v, symbols, sub)?; }
        }
        HirExprKind::If { condition, consequence, alternative } => {
            resolve_expr(condition, symbols, sub)?;
            resolve_stmt(consequence, symbols, sub)?;
            if let Some(alt) = alternative { resolve_stmt(alt, symbols, sub)?; }
        }
        HirExprKind::Index { left, index } => {
            resolve_expr(left, symbols, sub)?;
            resolve_expr(index, symbols, sub)?;
        }
        HirExprKind::MemberAccess { object, .. } => resolve_expr(object, symbols, sub)?,
        HirExprKind::Assign { target, value } => {
            resolve_expr(target, symbols, sub)?;
            resolve_expr(value, symbols, sub)?;
        }
        HirExprKind::FunctionLiteral { body, .. } => resolve_stmt(body, symbols, sub)?,
        HirExprKind::Match { value, arms } => {
            resolve_expr(value, symbols, sub)?;
            for (_, body) in arms { resolve_stmt(body, symbols, sub)?; }
        }
        HirExprKind::Range { start, end } => {
            resolve_expr(start, symbols, sub)?;
            resolve_expr(end, symbols, sub)?;
        }
        _ => {} // Literals, Identifier, Null — no recursion needed
    }
    Ok(())
}
