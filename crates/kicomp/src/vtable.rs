/// Static VTable Dispatch Module
///
/// Builds a compile-time method dispatch table after monomorphization.
/// Maps (class_name, method_name) → function_index for O(1) method resolution.
///
/// This module runs *after* monomorphization to ensure all generic instantiations
/// are resolved before building the VTable.

use std::collections::HashMap;
use crate::ir::{CompiledProgram, Constant};

/// A VTable entry: maps method names to function indices for a given class.
pub type ClassVTable = HashMap<String, usize>;

/// The complete VTable map: class_name → { method_name → function_index }
pub type VTableMap = HashMap<String, ClassVTable>;

/// Build the static VTable from a compiled program.
///
/// Scans all `Constant::Class` entries in the program's constant pools
/// and maps each method to its function index for O(1) dispatch.
pub fn build_vtable(program: &CompiledProgram) -> VTableMap {
    let mut vtable_map: VTableMap = HashMap::new();

    // Scan main function constants for class definitions
    scan_constants(&program.main.constants, &program.functions, &mut vtable_map);

    // Scan all function constants
    for func in &program.functions {
        scan_constants(&func.constants, &program.functions, &mut vtable_map);
    }

    vtable_map
}

/// Scan a constant pool for Class definitions and populate the VTable.
fn scan_constants(
    constants: &[Constant],
    functions: &[crate::ir::CompiledFunction],
    vtable_map: &mut VTableMap,
) {
    for constant in constants {
        if let Constant::Class { name, methods, fields: _, parent: _ } = constant {
            let class_vtable = vtable_map.entry(name.clone()).or_insert_with(HashMap::new);

            for &method_idx in methods {
                if method_idx < functions.len() {
                    let func = &functions[method_idx];
                    // Method name is stored as the function name, potentially qualified
                    // Strip the class prefix if present (e.g., "Point.greet" → "greet")
                    let method_name = if func.name.contains('.') {
                        func.name.split('.').last().unwrap_or(&func.name).to_string()
                    } else {
                        func.name.clone()
                    };
                    class_vtable.insert(method_name, method_idx);
                }
            }
        }
    }
}

/// Resolve a method call using the VTable.
/// Returns the function index if found.
pub fn resolve_method(vtable: &VTableMap, class_name: &str, method_name: &str) -> Option<usize> {
    vtable.get(class_name).and_then(|ct| ct.get(method_name).copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{CompiledFunction, CompiledProgram, CompiledReactiveGraph, Constant};

    #[test]
    fn test_build_vtable_empty() {
        let program = CompiledProgram::new();
        let vtable = build_vtable(&program);
        assert!(vtable.is_empty());
    }

    #[test]
    fn test_build_vtable_with_class() {
        let mut program = CompiledProgram::new();
        
        // Add a method function
        let greet_fn = CompiledFunction::new("Point.greet".to_string(), 1);
        program.functions.push(greet_fn);
        
        // Add a class constant referencing the method
        program.main.constants.push(Constant::Class {
            name: "Point".to_string(),
            methods: vec![0], // index 0 in functions
            fields: vec!["x".to_string(), "y".to_string()],
            parent: None,
        });
        
        let vtable = build_vtable(&program);
        assert!(vtable.contains_key("Point"));
        assert_eq!(resolve_method(&vtable, "Point", "greet"), Some(0));
    }

    #[test]
    fn test_resolve_method_not_found() {
        let vtable: VTableMap = HashMap::new();
        assert_eq!(resolve_method(&vtable, "Point", "greet"), None);
    }
}
