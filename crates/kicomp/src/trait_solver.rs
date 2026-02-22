use crate::types::{Type, parse_type_hint};
use kinetix_language::ast::Statement;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Type>, // Types of the parameters
    pub return_ty: Type,
}

#[derive(Debug, Clone)]
pub struct TraitDef {
    pub name: String,
    pub generics: Vec<String>,
    pub methods: Vec<TraitMethod>,
}

#[derive(Debug, Clone)]
pub struct ImplDef {
    pub target_name: String,
    pub generics: Vec<String>,
    pub trait_name: Option<String>,
    pub methods: HashMap<String, TraitMethod>,
}

#[derive(Debug)]
pub struct TraitEnvironment {
    pub traits: HashMap<String, TraitDef>,
    // Maps a TargetTypeName -> List of Impls for that type
    pub impls: HashMap<String, Vec<ImplDef>>,
}

impl TraitEnvironment {
    pub fn new() -> Self {
        Self {
            traits: HashMap::new(),
            impls: HashMap::new(),
        }
    }

    pub fn register_trait(&mut self, stmt: &Statement) -> Result<(), String> {
        if let Statement::Trait { name, generics, methods, .. } = stmt {
            let mut trait_methods = Vec::new();
            for (m_name, m_params, m_ret) in methods {
                let params: Vec<Type> = m_params.iter().map(|(_, t)| parse_type_hint(t)).collect();
                let ret = parse_type_hint(m_ret);
                trait_methods.push(TraitMethod {
                    name: m_name.clone(),
                    params,
                    return_ty: ret,
                });
            }
            let def = TraitDef {
                name: name.clone(),
                generics: generics.clone(),
                methods: trait_methods,
            };
            self.traits.insert(name.clone(), def);
        }
        Ok(())
    }

    pub fn register_impl(&mut self, stmt: &Statement) -> Result<(), String> {
        if let Statement::Impl { target_name, generics, trait_name, methods, .. } = stmt {
            let mut impl_methods = HashMap::new();
            for m in methods {
                if let Statement::Function { name: m_name, parameters, return_type, .. } = m {
                    let params: Vec<Type> = parameters.iter().map(|(_, t)| parse_type_hint(t)).collect();
                    let ret = parse_type_hint(return_type);
                    impl_methods.insert(m_name.clone(), TraitMethod {
                        name: m_name.clone(),
                        params,
                        return_ty: ret,
                    });
                }
            }

            let def = ImplDef {
                target_name: target_name.clone(),
                generics: generics.clone(),
                trait_name: trait_name.clone(),
                methods: impl_methods,
            };

            // Orphan rules / Coherence check
            if let Some(existing_impls) = self.impls.get(target_name) {
                for existing in existing_impls {
                    if existing.trait_name == *trait_name {
                        if let Some(t_name) = trait_name {
                            return Err(format!("Overlapping implementations of trait '{}' for type '{}'", t_name, target_name));
                        } else {
                            // Multiple inherent impls are allowed, but method names cannot overlap
                            for new_method in def.methods.keys() {
                                if existing.methods.contains_key(new_method) {
                                    return Err(format!("Duplicate definition of inherent method '{}' for type '{}'", new_method, target_name));
                                }
                            }
                        }
                    }
                }
            }

            self.impls.entry(target_name.clone()).or_insert_with(Vec::new).push(def);
        }
        Ok(())
    }

    /// Resolve a method for a given target type.
    pub fn resolve_method(&self, target_type_name: &str, method_name: &str) -> Option<TraitMethod> {
        if let Some(impls) = self.impls.get(target_type_name) {
            for imp in impls {
                if let Some(method) = imp.methods.get(method_name) {
                    return Some(method.clone());
                }
            }
        }
        None
    }

    /// Detects cycles in trait definitions to prevent semantic DOS and infinite resolution loops.
    pub fn validate_cycles(&self) -> Result<(), String> {
        let mut visited = std::collections::HashSet::new();
        let mut path = Vec::new();

        for trait_name in self.traits.keys() {
            if let Err(cycle) = self.check_trait_deps(trait_name, &mut visited, &mut path) {
                return Err(format!("Trait Cycle Detected: {}", cycle));
            }
        }
        Ok(())
    }

    fn check_trait_deps(&self, current: &str, visited: &mut std::collections::HashSet<String>, path: &mut Vec<String>) -> Result<(), String> {
        if path.iter().any(|p| p == current) {
            path.push(current.to_string());
            return Err(path.join(" -> "));
        }
        if visited.contains(current) {
            return Ok(());
        }
        path.push(current.to_string());
        visited.insert(current.to_string());

        if let Some(t_def) = self.traits.get(current) {
            for method in &t_def.methods {
                for param in &method.params {
                    self.trace_type_deps(param, visited, path)?;
                }
                self.trace_type_deps(&method.return_ty, visited, path)?;
            }
        }

        path.pop();
        Ok(())
    }

    fn trace_type_deps(&self, ty: &Type, visited: &mut std::collections::HashSet<String>, path: &mut Vec<String>) -> Result<(), String> {
        match ty {
            Type::Custom { name, args } => {
                if self.traits.contains_key(name) {
                    self.check_trait_deps(name, visited, path)?;
                }
                for arg in args {
                    self.trace_type_deps(arg, visited, path)?;
                }
            }
            Type::Array(inner) | Type::Ref(inner) | Type::MutRef(inner) => self.trace_type_deps(inner, visited, path)?,
            Type::Fn(params, ret) => {
                for p in params {
                    self.trace_type_deps(p, visited, path)?;
                }
                self.trace_type_deps(ret, visited, path)?;
            }
            Type::Map(k, v) => {
                self.trace_type_deps(k, visited, path)?;
                self.trace_type_deps(v, visited, path)?;
            }
            _ => {}
        }
        Ok(())
    }
}
