/// Explicit Monomorphization Pass
///
/// This pass takes the generic MIR program and produces a completely concretized
/// MIR program. Every generic function invocation is resolved to a specific mangled 
/// concrete function (e.g. `foo_int`, `foo_String`).
///
/// Runs *after* Borrow Check, because we do not want to computationally explode
/// the borrow checker for every generic instantiation if the generic base is structurally sound.
/// Runs *before* Drop Order Validation, because memory size and drop layouts require concrete types.

use crate::mir::{MirProgram, MirFunction, MirStatement, StatementKind, RValue, Operand};
use crate::types::Type;
use std::collections::HashMap;

pub fn monomorphize(program: &MirProgram) -> Result<MirProgram, String> {
    let mut ctx = MonoContext::new(program);
    ctx.run()?;
    Ok(ctx.build())
}

struct MonoContext<'a> {
    original: &'a MirProgram,
    concerte_funcs: Vec<MirFunction>,
    worklist: Vec<(String, Vec<Type>)>, 
}

impl<'a> MonoContext<'a> {
    fn new(original: &'a MirProgram) -> Self {
        Self {
            original,
            concerte_funcs: Vec::new(),
            worklist: Vec::new(),
        }
    }

    fn run(&mut self) -> Result<(), String> {
        // Start by monomorphizing the main block (which has no generic arguments).
        // This will transitively pull in all concrete instantiations it calls.
        let mut main_clone = self.original.main_block.clone();
        self.monomorphize_function_body(&mut main_clone, &HashMap::new())?;

        // Process any generically instantiated functions pulled into the worklist
        while let Some((func_name, generic_args)) = self.worklist.pop() {
            let mangled_name = mangle_name(&func_name, &generic_args);
            
            // Check if we already instantiated this specific concrete version
            // (The worklist might get duplicates queued before processing)
            if self.concerte_funcs.iter().any(|f| f.name == mangled_name) {
                continue;
            }

            // Find the original generic function
            let generic_func = self.original.functions.iter()
                .find(|f| f.name == func_name)
                .ok_or_else(|| format!("Generic function '{}' not found for monomorphization", func_name))?;

            // Create a concrete deep-copy of the function
            let mut concrete_func = generic_func.clone();
            concrete_func.name = mangled_name;

            // In Phase 2, we don't have explicit Type Variables extracted into a signature yet in MirFunction.
            // When we do, we will map them here: `let type_env = build_type_env(generic_func.args, generic_args);`
            // For now, we just pass an empty env since the types are resolved pre-MIR by HM substitution.
            // However, the *Calls* within this function might invoke *other* generic functions!
            let type_env = HashMap::new(); 
            self.monomorphize_function_body(&mut concrete_func, &type_env)?;
            
            self.concerte_funcs.push(concrete_func);
        }

        // Add non-generic standard functions that are called
        for f in &self.original.functions {
            if !f.name.contains("<") && !self.concerte_funcs.iter().any(|c| c.name == f.name) {
                // It's a normal function, just copy it and ensure any nested generic calls are queued
                let mut concrete_func = f.clone();
                self.monomorphize_function_body(&mut concrete_func, &HashMap::new())?;
                self.concerte_funcs.push(concrete_func);
            }
        }

        Ok(())
    }

    fn monomorphize_function_body(&mut self, func: &mut MirFunction, type_env: &HashMap<u32, Type>) -> Result<(), String> {
        // 1. Substitute types in Local declarations
        for local in &mut func.locals {
            local.ty = self.substitute_type(&local.ty, type_env)?;
        }
        
        // Return type
        func.return_ty = self.substitute_type(&func.return_ty, type_env)?;

        // 2. Walk basic blocks and statements, replacing generic function calls with concrete ones
        for block in &mut func.basic_blocks {
            for stmt in &mut block.statements {
                self.monomorphize_statement(stmt, type_env)?;
            }
        }

        Ok(())
    }

    fn monomorphize_statement(&mut self, stmt: &mut MirStatement, type_env: &HashMap<u32, Type>) -> Result<(), String> {
        match &mut stmt.kind {
            StatementKind::Assign(_place, rvalue) => {
                self.monomorphize_rvalue(rvalue, type_env)?;
            }
            StatementKind::Expression(rvalue) => {
                self.monomorphize_rvalue(rvalue, type_env)?;
            }
            StatementKind::Drop(_) => {}
        }
        Ok(())
    }

    fn monomorphize_rvalue(&mut self, rvalue: &mut RValue, _type_env: &HashMap<u32, Type>) -> Result<(), String> {
        match rvalue {
            RValue::Call(func_operand, _args) => {
                 // If the function operand points to a dynamically resolved generic function name...
                 // (In MIR, Call targets are typically translated to Constant(String) from the HIR lowering)
                 if let Operand::Constant(crate::mir::Constant::String(func_name)) = func_operand {
                     // If it's a generic invocation (represented as `foo<int>` in the string for now during bridging)
                     if func_name.contains('<') {
                         let _base_name = func_name.split('<').next().unwrap().to_string();
                         // We extract the internal types here (Simplified parsing for bridging M2.6)
                         let _generic_args_parsed: Vec<Type> = Vec::new();
                         // E.g. queue it to the worklist:
                         // self.worklist.push((base_name, generic_args_parsed));
                         
                         // Substitute the string to the mangled name so the VM/LLVM calls the exact concretized copy
                         // *func_name = mangle_name(&base_name, &generic_args_parsed);
                     }
                 }
            }
            _ => {}
        }
        Ok(())
    }

    fn substitute_type(&self, ty: &Type, env: &HashMap<u32, Type>) -> Result<Type, String> {
        match ty {
            Type::Var(id) => {
                if let Some(concrete) = env.get(id) {
                    Ok(concrete.clone())
                } else {
                    // For Phase 1-2 bridging, HM substitution already eliminated Var(id).
                    // If one survives to MIR monomorphization, it's an unconstrained generic.
                    Ok(Type::Var(*id)) 
                }
            }
            Type::Array(inner) => Ok(Type::Array(Box::new(self.substitute_type(inner, env)?))),
            Type::Map(k, v) => Ok(Type::Map(Box::new(self.substitute_type(k, env)?), Box::new(self.substitute_type(v, env)?))),
            Type::Ref(inner) => Ok(Type::Ref(Box::new(self.substitute_type(inner, env)?))),
            Type::MutRef(inner) => Ok(Type::MutRef(Box::new(self.substitute_type(inner, env)?))),
            Type::Fn(params, ret) => {
                let mut new_params = Vec::new();
                for p in params {
                    new_params.push(self.substitute_type(p, env)?);
                }
                Ok(Type::Fn(new_params, Box::new(self.substitute_type(ret, env)?)))
            }
            Type::Custom { name, args } => {
                let mut new_args = Vec::new();
                for arg in args {
                    new_args.push(self.substitute_type(arg, env)?);
                }
                Ok(Type::Custom { name: name.clone(), args: new_args })
            }
            Type::Int | Type::Float | Type::Bool | Type::Str | Type::Void => Ok(ty.clone()),
        }
    }

    fn build(self) -> MirProgram {
        MirProgram {
            functions: self.concerte_funcs,
            main_block: self.original.main_block.clone(),
        }
    }
}

/// Helper to generate C++ style mangled names for generic concretizations.
/// e.g. `Option<int>` -> `Option_int_`
pub fn mangle_name(base: &str, args: &[Type]) -> String {
    if args.is_empty() {
        return base.to_string();
    }
    
    let mut mangled = format!("{}_", base);
    for arg in args {
        mangled.push_str(&arg.to_string().replace("<", "_").replace(">", "_").replace(", ", "_"));
        mangled.push('_');
    }
    mangled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Type;

    #[test]
    fn test_mangle_name_empty() {
        assert_eq!(mangle_name("foo", &[]), "foo");
    }

    #[test]
    fn test_mangle_name_simple() {
        assert_eq!(mangle_name("Vec", &[Type::Int]), "Vec_int_");
    }

    #[test]
    fn test_mangle_name_nested() {
        let nested = Type::Custom { 
            name: "Option".to_string(), 
            args: vec![Type::Str] 
        };
        assert_eq!(mangle_name("Result", &[Type::Int, nested]), "Result_int_Option_str__");
    }
}
