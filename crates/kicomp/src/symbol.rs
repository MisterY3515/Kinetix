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

/// A registry for struct and class definitions.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub parent: Option<String>,
    pub fields: std::collections::HashMap<String, Type>,
    pub methods: std::collections::HashMap<String, Type>,
}

/// A registry for enum definitions: the ordered variant list (name + optional
/// payload type), used by `exhaustiveness.rs` to check variant coverage and by
/// `hir.rs` to tell a nullary-variant pattern (`None`) apart from a catch-all
/// binding pattern (`x`) -- both parse as a bare `Expression::Identifier`.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<(String, Option<Type>)>,
}

/// A scope-aware symbol table with nested scopes.
#[derive(Debug)]
pub struct SymbolTable {
    /// Stack of scopes; each scope maps name -> Symbol.
    scopes: Vec<HashMap<String, Symbol>>,
    next_var: u32,
    pub custom_types: HashMap<String, StructDef>,
    pub enums: HashMap<String, EnumDef>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()], // global scope
            next_var: 1,
            custom_types: HashMap::new(),
            enums: HashMap::new(),
        }
    }

    /// True if `name` is a known variant with no payload (e.g. `None`, or a
    /// user-declared nullary variant like `Red` in `enum Color { Red, ... }`).
    /// Used to classify a match arm's bare identifier as a variant pattern
    /// rather than a catch-all binding.
    pub fn is_nullary_variant(&self, name: &str) -> bool {
        self.enums.values().any(|e| e.variants.iter().any(|(vn, payload)| vn == name && payload.is_none()))
    }

    pub fn fresh_var(&mut self) -> Type {
        let id = self.next_var;
        self.next_var += 1;
        Type::Var(id)
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

    // Register built-in modules in the global scope
    let builtins = ["math", "system", "data", "graph", "net", "crypto", "audio"];
    for b in builtins {
        table.define(b, Type::Custom { name: b.to_string(), args: vec![] }, false);
    }
    table.define("println", Type::Fn(vec![Type::Var(0)], Box::new(Type::Void)), false);
    table.define("print", Type::Fn(vec![Type::Var(0)], Box::new(Type::Void)), false);

    // Global builtins from kivm::builtins::BUILTIN_NAMES (bare, non-dotted names only --
    // dotted names like "Math.abs"/"system.os.name" are dispatched via MemberAccess and
    // never resolved as identifiers, so they don't need a symbol table entry).
    // Signatures are intentionally permissive (Type::Var for anything dynamically-typed)
    // and match each builtin's primary call arity; a handful of builtins accept an
    // optional trailing argument (input, assert, stop/exit, pad_left/pad_right's pad
    // char, min/max's 2-arg numeric form) which is not modeled here and will still fail
    // symbol/type resolution if used -- known boundary, see Gestione/roadmap.md.
    for (name, ty) in [
        ("input", Type::Fn(vec![Type::Str], Box::new(Type::Str))),
        ("len", Type::Fn(vec![Type::Var(0)], Box::new(Type::Int))),
        ("typeof", Type::Fn(vec![Type::Var(0)], Box::new(Type::Str))),
        ("assert", Type::Fn(vec![Type::Bool], Box::new(Type::Void))),
        ("str", Type::Fn(vec![Type::Var(0)], Box::new(Type::Str))),
        ("int", Type::Fn(vec![Type::Var(0)], Box::new(Type::Int))),
        ("float", Type::Fn(vec![Type::Var(0)], Box::new(Type::Float))),
        ("bool", Type::Fn(vec![Type::Var(0)], Box::new(Type::Bool))),
        ("byte", Type::Fn(vec![Type::Var(0)], Box::new(Type::Int))),
        ("char", Type::Fn(vec![Type::Var(0)], Box::new(Type::Str))),
        ("stop", Type::Fn(vec![], Box::new(Type::Void))),
        ("exit", Type::Fn(vec![], Box::new(Type::Void))),
        ("copy", Type::Fn(vec![Type::Var(0)], Box::new(Type::Var(0)))),

        ("to_upper", Type::Fn(vec![Type::Str], Box::new(Type::Str))),
        ("to_lower", Type::Fn(vec![Type::Str], Box::new(Type::Str))),
        ("trim", Type::Fn(vec![Type::Str], Box::new(Type::Str))),
        ("split", Type::Fn(vec![Type::Str, Type::Str], Box::new(Type::Array(Box::new(Type::Str))))),
        ("replace", Type::Fn(vec![Type::Str, Type::Str, Type::Str], Box::new(Type::Str))),
        ("contains", Type::Fn(vec![Type::Var(0), Type::Var(1)], Box::new(Type::Bool))),
        ("starts_with", Type::Fn(vec![Type::Str, Type::Str], Box::new(Type::Bool))),
        ("ends_with", Type::Fn(vec![Type::Str, Type::Str], Box::new(Type::Bool))),
        ("pad_left", Type::Fn(vec![Type::Str, Type::Int], Box::new(Type::Str))),
        ("pad_right", Type::Fn(vec![Type::Str, Type::Int], Box::new(Type::Str))),
        ("join", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Str], Box::new(Type::Str))),

        ("push", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Var(0)], Box::new(Type::Array(Box::new(Type::Var(0)))))),
        ("pop", Type::Fn(vec![Type::Array(Box::new(Type::Var(0)))], Box::new(Type::Array(Box::new(Type::Var(0)))))),
        ("remove_at", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Int], Box::new(Type::Array(Box::new(Type::Var(0)))))),
        ("insert", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Int, Type::Var(0)], Box::new(Type::Array(Box::new(Type::Var(0)))))),
        ("reverse", Type::Fn(vec![Type::Array(Box::new(Type::Var(0)))], Box::new(Type::Array(Box::new(Type::Var(0)))))),
        ("sort", Type::Fn(vec![Type::Array(Box::new(Type::Var(0)))], Box::new(Type::Array(Box::new(Type::Var(0)))))),
        ("min", Type::Fn(vec![Type::Array(Box::new(Type::Var(0)))], Box::new(Type::Var(0)))),
        ("max", Type::Fn(vec![Type::Array(Box::new(Type::Var(0)))], Box::new(Type::Var(0)))),
        ("any", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Var(1)], Box::new(Type::Bool))),
        ("all", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Var(1)], Box::new(Type::Bool))),

        ("range", Type::Fn(vec![Type::Int, Type::Int], Box::new(Type::Array(Box::new(Type::Int))))),
        ("enumerate", Type::Fn(vec![Type::Array(Box::new(Type::Var(0)))], Box::new(Type::Array(Box::new(Type::Var(1)))))),
        ("zip", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Array(Box::new(Type::Var(1)))], Box::new(Type::Array(Box::new(Type::Var(2)))))),
        ("map", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Var(1)], Box::new(Type::Array(Box::new(Type::Var(2)))))),
        ("filter", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Var(1)], Box::new(Type::Array(Box::new(Type::Var(0)))))),
        ("reduce", Type::Fn(vec![Type::Array(Box::new(Type::Var(0))), Type::Var(1), Type::Var(2)], Box::new(Type::Var(2)))),
    ] {
        table.define(name, ty, false);
    }

    // M2 Builtins
    let t = Type::Var(1); // Generic T
    let e = Type::Var(2); // Generic E
    
    // Option<T>
    let option_t = Type::Custom { name: "Option".to_string(), args: vec![t.clone()] };
    table.define("Option", option_t.clone(), false);
    table.define("Some", Type::Fn(vec![t.clone()], Box::new(option_t.clone())), false);
    table.define("None", option_t.clone(), false); // Note: None in Rust is highly polymorphic, keeping it simple for now
    table.enums.insert("Option".to_string(), EnumDef {
        name: "Option".to_string(),
        variants: vec![("Some".to_string(), Some(t.clone())), ("None".to_string(), None)],
    });

    // Result<T,E>
    let result_t = Type::Custom { name: "Result".to_string(), args: vec![t.clone(), e.clone()] };
    table.define("Result", result_t.clone(), false);
    table.define("Ok", Type::Fn(vec![t.clone()], Box::new(result_t.clone())), false);
    table.define("Err", Type::Fn(vec![e.clone()], Box::new(result_t.clone())), false);
    table.enums.insert("Result".to_string(), EnumDef {
        name: "Result".to_string(),
        variants: vec![("Ok".to_string(), Some(t.clone())), ("Err".to_string(), Some(e.clone()))],
    });

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
            Statement::Class { name, parent, fields, methods, .. } => {
                let mut field_map = std::collections::HashMap::new();
                for (_, f_name, f_type) in fields {
                    field_map.insert(f_name.clone(), parse_type_hint(f_type));
                }
                let mut method_map = std::collections::HashMap::new();
                for m in methods {
                    if let Statement::Function { name: m_name, parameters, return_type, .. } = m {
                        let param_types: Vec<Type> = parameters.iter()
                            .map(|(_, ty)| parse_type_hint(ty))
                            .collect();
                        let ret = parse_type_hint(return_type);
                        method_map.insert(m_name.clone(), Type::Fn(param_types, Box::new(ret)));
                    }
                }
                table.custom_types.insert(name.clone(), StructDef {
                    name: name.clone(),
                    parent: parent.clone(),
                    fields: field_map,
                    methods: method_map,
                });
                table.define(name, Type::Custom { name: name.clone(), args: vec![] }, false);
            }
            Statement::Struct { name, fields, .. } => {
                let mut field_map = std::collections::HashMap::new();
                for (f_name, f_type) in fields {
                    field_map.insert(f_name.clone(), parse_type_hint(f_type));
                }
                table.custom_types.insert(name.clone(), StructDef {
                    name: name.clone(),
                    parent: None,
                    fields: field_map,
                    methods: std::collections::HashMap::new(),
                });
                table.define(name, Type::Custom { name: name.clone(), args: vec![] }, false);
            }
            Statement::Enum { name, generics, variants, .. } => {
                // Ordered (name, fresh Type::Var) pairs -- a Vec, not a HashMap,
                // to keep multi-generic enums' argument order deterministic.
                let generic_vars: Vec<(String, Type)> = generics.iter()
                    .map(|g| (g.clone(), table.fresh_var()))
                    .collect();
                let enum_ty = Type::Custom {
                    name: name.clone(),
                    args: generic_vars.iter().map(|(_, t)| t.clone()).collect(),
                };

                let mut variant_defs = Vec::new();
                for (vname, payload) in variants {
                    let payload_ty = payload.as_ref().map(|p| {
                        generic_vars.iter().find(|(g, _)| g == p)
                            .map(|(_, t)| t.clone())
                            .unwrap_or_else(|| parse_type_hint(p))
                    });
                    match &payload_ty {
                        Some(pty) => table.define(vname, Type::Fn(vec![pty.clone()], Box::new(enum_ty.clone())), false),
                        None => table.define(vname, enum_ty.clone(), false),
                    }
                    variant_defs.push((vname.clone(), payload_ty));
                }
                table.enums.insert(name.clone(), EnumDef { name: name.clone(), variants: variant_defs });
                table.define(name, enum_ty, false);
            }
            _ => {}
        }
    }

    // Second sub-pass: merge `impl` block methods into their target type's
    // method map. Run as its own pass (not folded into the loop above) so it
    // doesn't matter whether the `impl` block appears before or after the
    // struct/class/enum declaration it targets in the source file -- by this
    // point every class/struct is already registered in `custom_types`, and
    // an `impl` targeting an enum (which never gets a `custom_types` entry of
    // its own) defensively creates one so method-call resolution still finds it.
    for stmt in statements {
        if let Statement::Impl { target_name, methods, .. } = stmt {
            let mut method_map = std::collections::HashMap::new();
            for m in methods {
                if let Statement::Function { name: m_name, parameters, return_type, .. } = m {
                    let param_types: Vec<Type> = parameters.iter()
                        .map(|(_, ty)| parse_type_hint(ty))
                        .collect();
                    let ret = parse_type_hint(return_type);
                    method_map.insert(m_name.clone(), Type::Fn(param_types, Box::new(ret)));
                }
            }
            match table.custom_types.get_mut(target_name) {
                Some(def) => def.methods.extend(method_map),
                None => {
                    table.custom_types.insert(target_name.clone(), StructDef {
                        name: target_name.clone(),
                        parent: None,
                        fields: std::collections::HashMap::new(),
                        methods: method_map,
                    });
                }
            }
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
    let line = match stmt {
        Statement::Let { line, .. } => *line,
        Statement::Return { line, .. } => *line,
        Statement::Expression { line, .. } => *line,
        Statement::Block { line, .. } => *line,
        Statement::Function { line, .. } => *line,
        Statement::While { line, .. } => *line,
        Statement::For { line, .. } => *line,
        Statement::Class { line, .. } => *line,
        Statement::Struct { line, .. } => *line,
        Statement::Enum { line, .. } => *line,
        Statement::Trait { line, .. } => *line,
        Statement::Impl { line, .. } => *line,
        Statement::Include { line, .. } => *line,
        Statement::Version { line, .. } => *line,
        Statement::Break { line } => *line,
        Statement::Continue { line } => *line,
        Statement::State { line, .. } => *line,
        Statement::Computed { line, .. } => *line,
        Statement::Effect { line, .. } => *line,
    };

    match stmt {
        Statement::Let { name, value, mutable, type_hint, .. } => {
            resolve_expression(value, table, errors, line);
            let ty = match type_hint {
                Some(hint) => parse_type_hint(hint),
                None => table.fresh_var(), // unique inference variable
            };
            table.define(name, ty, *mutable);
        }
        Statement::Effect { body, .. } => {
            resolve_statement(body, table, errors);
        }
        Statement::Function { parameters, body, .. } => {
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
                resolve_expression(v, table, errors, line);
            }
        }
        Statement::Expression { expression, .. } => {
            resolve_expression(expression, table, errors, line);
        }
        Statement::While { condition, body, .. } => {
            resolve_expression(condition, table, errors, line);
            resolve_statement(body, table, errors);
        }
        Statement::For { iterator, range, body, .. } => {
            resolve_expression(range, table, errors, line);
            table.enter_scope();
            let iterator_ty = table.fresh_var();
            table.define(iterator, iterator_ty, false); // inferred
            resolve_statement(body, table, errors);
            table.exit_scope();
        }
        Statement::Class { methods, .. } => {
            for m in methods {
                resolve_statement(m, table, errors);
            }
        }
        Statement::State { name, value, type_hint, .. } => {
            resolve_expression(value, table, errors, line);
            let ty = match type_hint {
                Some(hint) => parse_type_hint(hint),
                None => table.fresh_var(),
            };
            table.define(name, ty, true); // state vars are implicitly mutable
        }
        Statement::Computed { name, value, type_hint, .. } => {
            resolve_expression(value, table, errors, line);
            let ty = match type_hint {
                Some(hint) => parse_type_hint(hint),
                None => table.fresh_var(),
            };
            table.define(name, ty, false); // computed vars are immutable
        }
        _ => {} // Include, Version, Break, Continue, Struct — no refs to resolve
    }
}

fn resolve_expression<'a>(expr: &Expression<'a>, table: &mut SymbolTable, errors: &mut Vec<String>, line: usize) {
    match expr {
        Expression::Identifier(name) => {
            if table.resolve(name).is_none() {
                errors.push(format!("Line {}: Undeclared variable: '{}'", line, name));
            }
        }
        Expression::Prefix { right, .. } => {
            resolve_expression(right, table, errors, line);
        }
        Expression::Try { value } => {
            resolve_expression(value, table, errors, line);
        }
        Expression::Infix { left, right, .. } => {
            resolve_expression(left, table, errors, line);
            resolve_expression(right, table, errors, line);
        }
        Expression::If { condition, consequence, alternative } => {
            resolve_expression(condition, table, errors, line);
            resolve_statement(consequence, table, errors);
            if let Some(alt) = alternative {
                resolve_statement(alt, table, errors);
            }
        }
        Expression::Call { function, arguments } => {
            resolve_expression(function, table, errors, line);
            for arg in arguments {
                resolve_expression(arg, table, errors, line);
            }
        }
        Expression::StructLiteral { fields, .. } => {
            for (_, field_expr) in fields {
                resolve_expression(field_expr, table, errors, line);
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
            for e in elems { resolve_expression(e, table, errors, line); }
        }
        Expression::MapLiteral(pairs) => {
            for (k, v) in pairs {
                resolve_expression(k, table, errors, line);
                resolve_expression(v, table, errors, line);
            }
        }
        Expression::Index { left, index } => {
            resolve_expression(left, table, errors, line);
            resolve_expression(index, table, errors, line);
        }
        Expression::MemberAccess { object, .. } => {
            resolve_expression(object, table, errors, line);
        }
        Expression::Assign { target, value } => {
            resolve_expression(target, table, errors, line);
            resolve_expression(value, table, errors, line);
        }
        Expression::Match { value, arms } => {
            resolve_expression(value, table, errors, line);
            for (pattern, body) in arms {
                // A binding pattern (`x`) or a variant payload binding
                // (`Circle(r)`) introduces a new name scoped to this arm's
                // body -- define it before resolving the body, or it would
                // (incorrectly) fail as an undeclared variable. Wildcards and
                // literal patterns introduce nothing.
                table.enter_scope();
                match crate::pattern::classify_pattern(pattern, |n| table.is_nullary_variant(n)) {
                    crate::pattern::ArmPattern::Binding(name) => {
                        let fv = table.fresh_var();
                        table.define(&name, fv, false);
                    }
                    crate::pattern::ArmPattern::Variant { binding: Some(bname), .. } => {
                        let fv = table.fresh_var();
                        table.define(&bname, fv, false);
                    }
                    crate::pattern::ArmPattern::Literal(lit) => {
                        resolve_expression(lit, table, errors, line);
                    }
                    _ => {}
                }
                resolve_statement(body, table, errors);
                table.exit_scope();
            }
        }
        Expression::Range { start, end } => {
            resolve_expression(start, table, errors, line);
            resolve_expression(end, table, errors, line);
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
