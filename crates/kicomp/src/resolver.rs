/// resolver.rs — Offline Dependency Resolver (Build 33)
///
/// Resolves local path dependencies declared in `.kicomp` files.
/// Builds a topological ordering (DAG) of modules for compilation.

use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use crate::project::{ProjectConfig, Dependency, DependencySource};

// ─── Data Structures ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ResolvedModule {
    /// Logical name of the dependency (as declared in .kicomp)
    pub name: String,
    /// Absolute path to the module's entry file
    pub entry_path: PathBuf,
    /// Source code content
    pub source: String,
}

#[derive(Debug)]
pub enum ResolveError {
    NotFound(String),
    Cycle(Vec<String>),
    Io(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound(msg) => write!(f, "Dependency not found: {}", msg),
            ResolveError::Cycle(chain) => write!(f, "Circular dependency detected: {}", chain.join(" → ")),
            ResolveError::Io(msg) => write!(f, "IO error resolving dependency: {}", msg),
        }
    }
}

// ─── Resolver ────────────────────────────────────────────────────────────

/// Resolve all dependencies declared in the project configuration.
/// Returns a topologically sorted list of modules (dependencies first, then project entry).
pub fn resolve_dependencies(config: &ProjectConfig) -> Result<Vec<ResolvedModule>, ResolveError> {
    let mut resolved: Vec<ResolvedModule> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut in_stack: HashSet<String> = HashSet::new();

    // Resolve each dependency via DFS
    for dep in &config.dependencies {
        resolve_dep(dep, &mut resolved, &mut visited, &mut in_stack)?;
    }

    // Finally, add the project entry point itself
    let entry_source = std::fs::read_to_string(&config.entry)
        .map_err(|e| ResolveError::Io(format!("Cannot read entry '{}': {}", config.entry.display(), e)))?;

    resolved.push(ResolvedModule {
        name: config.name.clone(),
        entry_path: config.entry.clone(),
        source: entry_source,
    });

    Ok(resolved)
}

fn resolve_dep(
    dep: &Dependency,
    resolved: &mut Vec<ResolvedModule>,
    visited: &mut HashSet<String>,
    in_stack: &mut HashSet<String>,
) -> Result<(), ResolveError> {
    if visited.contains(&dep.name) {
        return Ok(()); // Already resolved
    }

    if in_stack.contains(&dep.name) {
        // Cycle detected — build the chain for display
        let chain: Vec<String> = in_stack.iter().cloned().collect();
        return Err(ResolveError::Cycle(chain));
    }

    in_stack.insert(dep.name.clone());

    let (entry_path, source) = match &dep.source {
        DependencySource::Local(path) => {
            // Look for either a direct .kix file or a directory with main.kix
            let resolved_path = if path.extension().map_or(false, |e| e == "kix") {
                path.clone()
            } else if path.is_dir() {
                // Look for main.kix or lib.kix inside the directory
                let main = path.join("main.kix");
                let lib = path.join("lib.kix");
                if main.exists() {
                    main
                } else if lib.exists() {
                    lib
                } else {
                    return Err(ResolveError::NotFound(format!(
                        "Dependency '{}': directory '{}' has no main.kix or lib.kix",
                        dep.name, path.display()
                    )));
                }
            } else {
                // Try appending .kix
                let with_ext = path.with_extension("kix");
                if with_ext.exists() {
                    with_ext
                } else {
                    return Err(ResolveError::NotFound(format!(
                        "Dependency '{}': path '{}' not found",
                        dep.name, path.display()
                    )));
                }
            };

            let content = std::fs::read_to_string(&resolved_path)
                .map_err(|e| ResolveError::Io(format!(
                    "Cannot read dependency '{}' at '{}': {}",
                    dep.name, resolved_path.display(), e
                )))?;

            (resolved_path, content)
        }
    };

    in_stack.remove(&dep.name);
    visited.insert(dep.name.clone());

    resolved.push(ResolvedModule {
        name: dep.name.clone(),
        entry_path,
        source,
    });

    Ok(())
}

/// Combine sources from resolved modules into a single compilation unit.
/// Each module source is prepended with a comment marker for debug traceability.
pub fn combine_sources(modules: &[ResolvedModule]) -> String {
    let mut combined = String::new();
    for module in modules {
        combined.push_str(&format!("// --- module: {} ({})\n", module.name, module.entry_path.display()));
        combined.push_str(&module.source);
        combined.push_str("\n\n");
    }
    combined
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combine_sources() {
        let modules = vec![
            ResolvedModule {
                name: "lib_a".into(),
                entry_path: PathBuf::from("libs/a/main.kix"),
                source: "fn helper() { return 42 }".into(),
            },
            ResolvedModule {
                name: "main".into(),
                entry_path: PathBuf::from("src/main.kix"),
                source: "println(helper())".into(),
            },
        ];
        let combined = combine_sources(&modules);
        assert!(combined.contains("module: lib_a"));
        assert!(combined.contains("module: main"));
        assert!(combined.contains("fn helper()"));
    }
}
