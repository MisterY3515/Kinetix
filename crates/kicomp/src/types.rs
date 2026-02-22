/// Kinetix Type System — core type representation for Hindley-Milner inference.

use std::collections::HashMap;
use std::fmt;

/// Unique identifier for a type variable (used during unification).
pub type TypeVarId = u32;

/// The core type representation for Kinetix.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Primitive types
    Int,
    Float,
    Bool,
    Str,
    Void,

    /// Function type: (param_types) -> return_type
    Fn(Vec<Type>, Box<Type>),

    /// Homogeneous array: Array<T>
    Array(Box<Type>),

    /// Map: Map<K, V>
    Map(Box<Type>, Box<Type>),

    /// Immutable reference: &T
    Ref(Box<Type>),

    /// Mutable reference: &mut T
    MutRef(Box<Type>),

    /// Unification variable (fresh, to be solved by HM)
    Var(TypeVarId),

    /// Named / user-defined type (class, struct, trait — resolved later), with optional generic args
    Custom { name: String, args: Vec<Type> },
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Void => write!(f, "void"),
            Type::Fn(params, ret) => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", p)?;
                }
                write!(f, ") -> {}", ret)
            }
            Type::Array(inner) => write!(f, "Array<{}>", inner),
            Type::Map(k, v) => write!(f, "Map<{}, {}>", k, v),
            Type::Ref(inner) => write!(f, "&{}", inner),
            Type::MutRef(inner) => write!(f, "&mut {}", inner),
            Type::Var(id) => write!(f, "?T{}", id),
            Type::Custom { name, args } => {
                write!(f, "{}", name)?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
        }
    }
}

/// A substitution maps type variables to their resolved types.
#[derive(Debug, Clone, Default)]
pub struct Substitution {
    map: HashMap<TypeVarId, Type>,
}

impl Substitution {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    /// Bind a type variable to a type.
    pub fn bind(&mut self, var: TypeVarId, ty: Type) {
        self.map.insert(var, ty);
    }

    /// Look up a type variable.
    pub fn lookup(&self, var: TypeVarId) -> Option<&Type> {
        self.map.get(&var)
    }

    /// Apply this substitution to a type, recursively resolving variables.
    pub fn apply(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(id) => {
                if let Some(resolved) = self.map.get(id) {
                    // Chase the substitution chain
                    self.apply(resolved)
                } else {
                    ty.clone()
                }
            }
            Type::Fn(params, ret) => {
                let params = params.iter().map(|p| self.apply(p)).collect();
                let ret = Box::new(self.apply(ret));
                Type::Fn(params, ret)
            }
            Type::Array(inner) => Type::Array(Box::new(self.apply(inner))),
            Type::Map(k, v) => Type::Map(Box::new(self.apply(k)), Box::new(self.apply(v))),
            Type::Ref(inner) => Type::Ref(Box::new(self.apply(inner))),
            Type::MutRef(inner) => Type::MutRef(Box::new(self.apply(inner))),
            Type::Custom { name, args } => {
                let mapped_args = args.iter().map(|a| self.apply(a)).collect();
                Type::Custom { name: name.clone(), args: mapped_args }
            }
            _ => ty.clone(), // Primitives (Int, Float, Bool, Str, Void)
        }
    }
}

/// Convert a Kinetix type-hint string (from parser) to a Type.
pub fn parse_type_hint(hint: &str) -> Type {
    match hint {
        "int" => Type::Int,
        "float" => Type::Float,
        "bool" => Type::Bool,
        "str" | "string" => Type::Str,
        "void" | "" => Type::Void, // default return type
        other => {
            // Very naive generic type hint parsing for AST string hints like "Option<int>"
            if let Some(start) = other.find('<') {
                if other.ends_with('>') {
                    let name = other[..start].to_string();
                    let inner_str = &other[start+1..other.len()-1];
                    // Handle single generic argument for now in type hints
                    let inner_ty = if inner_str.is_empty() { vec![] } else { vec![parse_type_hint(inner_str.trim())] };
                    return Type::Custom { name, args: inner_ty };
                }
            }
            Type::Custom { name: other.to_string(), args: vec![] }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_display() {
        assert_eq!(Type::Int.to_string(), "int");
        assert_eq!(Type::Fn(vec![Type::Int, Type::Float], Box::new(Type::Bool)).to_string(), "fn(int, float) -> bool");
        assert_eq!(Type::Array(Box::new(Type::Str)).to_string(), "Array<str>");
        assert_eq!(Type::Var(42).to_string(), "?T42");
    }

    #[test]
    fn test_substitution_apply() {
        let mut sub = Substitution::new();
        sub.bind(0, Type::Int);
        sub.bind(1, Type::Var(0)); // chain: ?T1 -> ?T0 -> int

        assert_eq!(sub.apply(&Type::Var(0)), Type::Int);
        assert_eq!(sub.apply(&Type::Var(1)), Type::Int); // chased
        assert_eq!(sub.apply(&Type::Var(99)), Type::Var(99)); // unbound
    }

    #[test]
    fn test_substitution_apply_fn() {
        let mut sub = Substitution::new();
        sub.bind(0, Type::Int);
        let fn_ty = Type::Fn(vec![Type::Var(0)], Box::new(Type::Var(0)));
        assert_eq!(sub.apply(&fn_ty), Type::Fn(vec![Type::Int], Box::new(Type::Int)));
    }

    #[test]
    fn test_parse_type_hint() {
        assert_eq!(parse_type_hint("int"), Type::Int);
        assert_eq!(parse_type_hint("float"), Type::Float);
        assert_eq!(parse_type_hint("string"), Type::Str);
        assert_eq!(parse_type_hint("void"), Type::Void);
        assert_eq!(parse_type_hint(""), Type::Void);
        assert_eq!(parse_type_hint("MyClass"), Type::Custom { name: "MyClass".to_string(), args: vec![] });
        assert_eq!(parse_type_hint("Option<int>"), Type::Custom { name: "Option".to_string(), args: vec![Type::Int] });
    }
}
