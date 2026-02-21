/// Symbol Resolution Pass — builds a scope-aware symbol table from the AST.
///
/// This pass walks the untyped AST and:
/// 1. Registers all definitions (let, fn, class, struct) in a scoped symbol table.
/// 2. Reports undeclared variable errors.
/// 3. Produces a SymbolTable that the HIR lowering pass can consume.

use std::collections::HashMap;
use kinetix_language::ast::{Statement, Expression};
use crate::types::Type;
use crate::types::parse_type_hint;

/// A single symbol entry in the table.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub scope_depth: usize,
}

/// A scope-aware symbol table with nested scopes.
#[derive(Debug)]
pub struct SymbolTable {
    /// Stack of scopes; each scope maps name -> Symbol.
    scopes: Vec<HashMap<String, Symbol>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()], // global scope
        }
    }

    /// Enter a new nested scope.
    pub fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Exit the current scope.
    pub fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    /// Current scope depth (0 = global).
    pub fn depth(&self) -> usize {
        self.scopes.len() - 1
    }

    /// Define a symbol in the current scope.
    pub fn define(&mut self, name: &str, ty: Type, mutable: bool) {
        let depth = self.depth();
        let sym = Symbol {
            name: name.to_string(),
            ty,
            mutable,
            scope_depth: depth,
        };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), sym);
        }
    }

    /// Resolve a symbol by name, searching from innermost to outermost scope.
    pub fn resolve(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.get(name) {
                return Some(sym);
            }
        }
        None
    }
}

/// Walk the AST and populate a SymbolTable, returning errors for undeclared variables.
pub fn resolve_program<'a>(statements: &[Statement<'a>]) -> Result<SymbolTable, Vec<String>> {
    let mut table = SymbolTable::new();
    let mut errors = Vec::new();

    // First pass: register all top-level function and type definitions
    for stmt in statements {
        match stmt {
            Statement::Function { name, parameters, return_type, .. } => {
                let param_types: Vec<Type> = parameters.iter()
                    .map(|(_, ty)| parse_type_hint(ty))
                    .collect();
                let ret = parse_type_hint(return_type);
                table.define(name, Type::Fn(param_types, Box::new(ret)), false);
            }
            Statement::Class { name, .. } => {
                table.define(name, Type::Named(name.clone()), false);
            }
            Statement::Struct { name, .. } => {
                table.define(name, Type::Named(name.clone()), false);
            }
            _ => {}
        }
    }

    // Second pass: resolve all references
    for stmt in statements {
        resolve_statement(stmt, &mut table, &mut errors);
    }

    if errors.is_empty() {
        Ok(table)
    } else {
        Err(errors)
    }
}

fn resolve_statement<'a>(stmt: &Statement<'a>, table: &mut SymbolTable, errors: &mut Vec<String>) {
    match stmt {
        Statement::Let { name, value, mutable, type_hint, line, .. } => {
            resolve_expression(value, table, errors);
            let ty = match type_hint {
                Some(hint) => parse_type_hint(hint),
                None => Type::Var(0), // will be inferred later
            };
            table.define(name, ty, *mutable);
        }
        Statement::Function { name: _, parameters, body, .. } => {
            table.enter_scope();
            for (param_name, param_type) in parameters {
                table.define(param_name, parse_type_hint(param_type), false);
            }
            resolve_statement(body, table, errors);
            table.exit_scope();
        }
        Statement::Block { statements, .. } => {
            table.enter_scope();
            for s in statements {
                resolve_statement(s, table, errors);
            }
            table.exit_scope();
        }
        Statement::Return { value, .. } => {
            if let Some(v) = value {
                resolve_expression(v, table, errors);
            }
        }
        Statement::Expression { expression, .. } => {
            resolve_expression(expression, table, errors);
        }
        Statement::While { condition, body, .. } => {
            resolve_expression(condition, table, errors);
            resolve_statement(body, table, errors);
        }
        Statement::For { iterator, range, body, .. } => {
            resolve_expression(range, table, errors);
            table.enter_scope();
            table.define(iterator, Type::Var(0), false); // inferred
            resolve_statement(body, table, errors);
            table.exit_scope();
        }
        Statement::Class { methods, .. } => {
            for m in methods {
                resolve_statement(m, table, errors);
            }
        }
        _ => {} // Include, Version, Break, Continue, Struct — no refs to resolve
    }
}

fn resolve_expression<'a>(expr: &Expression<'a>, table: &mut SymbolTable, errors: &mut Vec<String>) {
    match expr {
        Expression::Identifier(name) => {
            // Built-in modules are always valid
            if name == "println" || name == "print" || name.starts_with("math.")
                || name.starts_with("system.") || name.starts_with("data.")
                || name.starts_with("graph.") || name.starts_with("net.")
                || name.starts_with("crypto.") || name.starts_with("audio.") {
                return;
            }
            if table.resolve(name).is_none() {
                errors.push(format!("Undeclared variable: '{}'", name));
            }
        }
        Expression::Prefix { right, .. } => {
            resolve_expression(right, table, errors);
        }
        Expression::Infix { left, right, .. } => {
            resolve_expression(left, table, errors);
            resolve_expression(right, table, errors);
        }
        Expression::If { condition, consequence, alternative } => {
            resolve_expression(condition, table, errors);
            resolve_statement(consequence, table, errors);
            if let Some(alt) = alternative {
                resolve_statement(alt, table, errors);
            }
        }
        Expression::Call { function, arguments } => {
            resolve_expression(function, table, errors);
            for arg in arguments {
                resolve_expression(arg, table, errors);
            }
        }
        Expression::FunctionLiteral { parameters, body, .. } => {
            table.enter_scope();
            for (pname, ptype) in parameters {
                table.define(pname, parse_type_hint(ptype), false);
            }
            resolve_statement(body, table, errors);
            table.exit_scope();
        }
        Expression::ArrayLiteral(elems) => {
            for e in elems { resolve_expression(e, table, errors); }
        }
        Expression::MapLiteral(pairs) => {
            for (k, v) in pairs {
                resolve_expression(k, table, errors);
                resolve_expression(v, table, errors);
            }
        }
        Expression::Index { left, index } => {
            resolve_expression(left, table, errors);
            resolve_expression(index, table, errors);
        }
        Expression::MemberAccess { object, .. } => {
            resolve_expression(object, table, errors);
        }
        Expression::Assign { target, value } => {
            resolve_expression(target, table, errors);
            resolve_expression(value, table, errors);
        }
        Expression::Match { value, arms } => {
            resolve_expression(value, table, errors);
            for (pattern, body) in arms {
                resolve_expression(pattern, table, errors);
                resolve_statement(body, table, errors);
            }
        }
        Expression::Range { start, end } => {
            resolve_expression(start, table, errors);
            resolve_expression(end, table, errors);
        }
        // Literals: no resolution needed
        Expression::Integer(_) | Expression::Float(_) | Expression::String(_)
        | Expression::Boolean(_) | Expression::Null => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use kinetix_language::lexer::Lexer;
    use kinetix_language::parser::Parser;

    fn parse_and_resolve(src: &str) -> Result<SymbolTable, Vec<String>> {
        let arena = Bump::new();
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, &arena);
        let program = parser.parse_program();
        assert!(parser.errors.is_empty(), "Parser errors: {:?}", parser.errors);
        resolve_program(&program.statements)
    }

    #[test]
    fn test_basic_resolution() {
        let result = parse_and_resolve("let x = 10\nlet y = x + 1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_undeclared_variable() {
        let result = parse_and_resolve("let x = y + 1");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("Undeclared variable: 'y'"));
    }

    #[test]
    fn test_function_scope() {
        let result = parse_and_resolve(
            "fn add(a: int, b: int) -> int { return a + b }\nlet r = add(1, 2)"
        );
        assert!(result.is_ok());
    }
}
