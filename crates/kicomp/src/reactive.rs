/// reactive.rs — Phase 3B Step 2: Reactive Dependency Graph
///
/// Statically analyzes the HIR to build a dependency graph between
/// `state` and `computed` declarations. This graph is used by the
/// Frame Scheduler (Step 3) to determine the topological update order.
///
/// Design:
/// - `state` nodes are *sources* (roots) in the dependency graph.
/// - `computed` nodes are *derived* values that depend on one or more `state` nodes.
/// - The graph is a DAG (Directed Acyclic Graph). Cycles are a compile-time error.
/// - Dependency resolution is purely static: we analyze identifier references
///   within each `computed` expression to find which `state` variables are read.

use std::collections::{HashMap, HashSet, VecDeque};
use crate::hir::{HirProgram, HirStatement, HirStmtKind, HirExpression, HirExprKind};

// ──────────────────── Data Structures ────────────────────

/// A single reactive node in the dependency graph.
#[derive(Debug, Clone)]
pub enum ReactiveNodeKind {
    /// A mutable reactive variable (source).
    State,
    /// An immutable derived value (depends on State nodes).
    Computed,
}

/// Metadata for a reactive variable.
#[derive(Debug, Clone)]
pub struct ReactiveNode {
    pub name: String,
    pub kind: ReactiveNodeKind,
    pub line: usize,
}

/// The complete reactive dependency graph extracted from the HIR.
#[derive(Debug)]
pub struct ReactiveGraph {
    /// All reactive nodes indexed by name.
    pub nodes: HashMap<String, ReactiveNode>,
    /// Adjacency list: computed_name → set of state names it depends on.
    pub dependencies: HashMap<String, HashSet<String>>,
    /// Reverse adjacency: state_name -> set of computed names that depend on it.
    pub dependents: HashMap<String, HashSet<String>>,
    /// Topological order for update propagation (states first, then computed in dependency order).
    pub update_order: Vec<String>,
}

impl ReactiveGraph {
    pub fn to_compiled(&self) -> crate::ir::CompiledReactiveGraph {
        let mut compiled = crate::ir::CompiledReactiveGraph::new();
        
        for (name, node) in &self.nodes {
            let kind = match node.kind {
                ReactiveNodeKind::State => crate::ir::ReactiveNodeKind::State,
                ReactiveNodeKind::Computed => crate::ir::ReactiveNodeKind::Computed,
            };
            compiled.nodes.insert(name.clone(), crate::ir::ReactiveNodeMetadata {
                name: node.name.clone(),
                kind,
                line: node.line,
            });
        }
        
        compiled.dependencies = self.dependencies.clone();
        compiled.dependents = self.dependents.clone();
        compiled.update_order = self.update_order.clone();
        
        compiled
    }
}

// ──────────────────── Graph Construction ────────────────────

/// Analyze a HIR program and build the reactive dependency graph.
/// Returns Ok(ReactiveGraph) if the graph is valid, or Err(String) if cycles are detected.
pub fn build_reactive_graph(program: &HirProgram) -> Result<ReactiveGraph, String> {
    let mut nodes = HashMap::new();
    let mut computed_exprs: Vec<(String, &HirExpression, usize)> = Vec::new();

    // Pass 1: Collect all state and computed declarations
    collect_reactive_nodes(&program.statements, &mut nodes, &mut computed_exprs);

    // If there are no reactive nodes, return an empty graph (no-op)
    if nodes.is_empty() {
        return Ok(ReactiveGraph {
            nodes,
            dependencies: HashMap::new(),
            dependents: HashMap::new(),
            update_order: Vec::new(),
        });
    }

    // Pass 2: For each computed node, resolve which state variables it references
    let state_names: HashSet<String> = nodes.iter()
        .filter(|(_, n)| matches!(n.kind, ReactiveNodeKind::State))
        .map(|(name, _)| name.clone())
        .collect();

    let mut dependencies: HashMap<String, HashSet<String>> = HashMap::new();
    let mut dependents: HashMap<String, HashSet<String>> = HashMap::new();

    for (computed_name, expr, line) in &computed_exprs {
        let mut deps = HashSet::new();
        collect_identifier_refs(expr, &state_names, &mut deps);

        if deps.is_empty() {
            // A computed that doesn't depend on any state is valid but functionally
            // equivalent to a `let`. We allow it silently.
        }

        // Check for self-reference (computed depending on itself)
        if deps.contains(computed_name) {
            return Err(format!(
                "Line {}: computed variable '{}' cannot reference itself (cycle detected)",
                line, computed_name
            ));
        }

        // Build reverse map
        for state_name in &deps {
            dependents
                .entry(state_name.clone())
                .or_insert_with(HashSet::new)
                .insert(computed_name.clone());
        }

        dependencies.insert(computed_name.clone(), deps);
    }

    // Pass 3: Topological sort (Kahn's algorithm) for update order
    let update_order = topological_sort(&nodes, &dependencies)?;

    Ok(ReactiveGraph {
        nodes,
        dependencies,
        dependents,
        update_order,
    })
}

// ──────────────────── HIR Traversal ────────────────────

/// Walk HIR statements to collect reactive node declarations.
fn collect_reactive_nodes<'a>(
    statements: &'a [HirStatement],
    nodes: &mut HashMap<String, ReactiveNode>,
    computed_exprs: &mut Vec<(String, &'a HirExpression, usize)>,
) {
    for stmt in statements {
        match &stmt.kind {
            HirStmtKind::Let { name: _, mutable: _, value: _ } => {}
            HirStmtKind::State { name, value: _ } => {
                nodes.insert(name.clone(), ReactiveNode {
                    name: name.clone(),
                    kind: ReactiveNodeKind::State,
                    line: stmt.line,
                });
            }
            HirStmtKind::Computed { name, value } => {
                nodes.insert(name.clone(), ReactiveNode {
                    name: name.clone(),
                    kind: ReactiveNodeKind::Computed,
                    line: stmt.line,
                });
                computed_exprs.push((name.clone(), value, stmt.line));
            }
            HirStmtKind::Effect { .. } => {}
            HirStmtKind::Block { statements: inner } => {
                collect_reactive_nodes(inner, nodes, computed_exprs);
            }
            HirStmtKind::Function { body, .. } => {
                // Recurse into function body directly without cloning
                if let HirStmtKind::Block { statements: inner } = &body.kind {
                    collect_reactive_nodes(inner, nodes, computed_exprs);
                }
            }
            HirStmtKind::Class { methods, .. } => {
                collect_reactive_nodes(methods, nodes, computed_exprs);
            }
            _ => {}
        }
    }
}

/// Collect all identifier references within an expression that match known state names.
fn collect_identifier_refs(
    expr: &HirExpression,
    state_names: &HashSet<String>,
    refs: &mut HashSet<String>,
) {
    match &expr.kind {
        HirExprKind::Identifier(name) => {
            if state_names.contains(name) {
                refs.insert(name.clone());
            }
        }
        HirExprKind::Prefix { right, .. } => {
            collect_identifier_refs(right, state_names, refs);
        }
        HirExprKind::Infix { left, right, .. } => {
            collect_identifier_refs(left, state_names, refs);
            collect_identifier_refs(right, state_names, refs);
        }
        HirExprKind::If { condition, consequence, alternative } => {
            collect_identifier_refs(condition, state_names, refs);
            collect_stmt_refs(consequence, state_names, refs);
            if let Some(alt) = alternative {
                collect_stmt_refs(alt, state_names, refs);
            }
        }
        HirExprKind::Call { function, arguments } => {
            collect_identifier_refs(function, state_names, refs);
            for arg in arguments {
                collect_identifier_refs(arg, state_names, refs);
            }
        }
        HirExprKind::MethodCall { object, arguments, .. } => {
            collect_identifier_refs(object, state_names, refs);
            for arg in arguments {
                collect_identifier_refs(arg, state_names, refs);
            }
        }
        HirExprKind::MemberAccess { object, .. } => {
            collect_identifier_refs(object, state_names, refs);
        }
        HirExprKind::Index { left, index } => {
            collect_identifier_refs(left, state_names, refs);
            collect_identifier_refs(index, state_names, refs);
        }
        HirExprKind::ArrayLiteral(elems) => {
            for e in elems {
                collect_identifier_refs(e, state_names, refs);
            }
        }
        HirExprKind::StructLiteral(_, field_list) => {
            for (_, field_expr) in field_list {
                collect_identifier_refs(field_expr, state_names, refs);
            }
        }
        HirExprKind::Assign { target, value } => {
            collect_identifier_refs(target, state_names, refs);
            collect_identifier_refs(value, state_names, refs);
        }
        HirExprKind::Match { value, arms } => {
            collect_identifier_refs(value, state_names, refs);
            for (_, arm_body) in arms {
                collect_stmt_refs(arm_body, state_names, refs);
            }
        }
        HirExprKind::Range { start, end } => {
            collect_identifier_refs(start, state_names, refs);
            collect_identifier_refs(end, state_names, refs);
        }
        HirExprKind::FunctionLiteral { body, .. } => {
            collect_stmt_refs(body, state_names, refs);
        }
        // Literals do not reference any state
        HirExprKind::Integer(_) | HirExprKind::Float(_) | HirExprKind::String(_)
        | HirExprKind::Boolean(_) | HirExprKind::Null | HirExprKind::MapLiteral(_) => {}
    }
}

/// Helper to extract identifier references from statements (for nested blocks).
fn collect_stmt_refs(
    stmt: &HirStatement,
    state_names: &HashSet<String>,
    refs: &mut HashSet<String>,
) {
    match &stmt.kind {
        HirStmtKind::Let { value, .. } => {
            collect_identifier_refs(value, state_names, refs);
        }
        HirStmtKind::State { value, .. } | HirStmtKind::Computed { value, .. } => {
            collect_identifier_refs(value, state_names, refs);
        }
        HirStmtKind::Effect { body, .. } => {
            collect_stmt_refs(body, state_names, refs);
        }
        HirStmtKind::Return { value } => {
            if let Some(v) = value {
                collect_identifier_refs(v, state_names, refs);
            }
        }
        HirStmtKind::Expression { expression } => {
            collect_identifier_refs(expression, state_names, refs);
        }
        HirStmtKind::Block { statements } => {
            for s in statements {
                collect_stmt_refs(s, state_names, refs);
            }
        }
        HirStmtKind::While { condition, body } => {
            collect_identifier_refs(condition, state_names, refs);
            collect_stmt_refs(body, state_names, refs);
        }
        HirStmtKind::For { range, body, .. } => {
            collect_identifier_refs(range, state_names, refs);
            collect_stmt_refs(body, state_names, refs);
        }
        HirStmtKind::Function { body, .. } => {
            collect_stmt_refs(body, state_names, refs);
        }
        HirStmtKind::Class { methods, .. } => {
            for m in methods {
                collect_stmt_refs(m, state_names, refs);
            }
        }
        HirStmtKind::Break | HirStmtKind::Continue => {}
    }
}

// ──────────────────── Topological Sort ────────────────────

/// Topological sort using Kahn's algorithm.
/// Returns the update order: state nodes first, then computed nodes in dependency order.
fn topological_sort(
    nodes: &HashMap<String, ReactiveNode>,
    dependencies: &HashMap<String, HashSet<String>>,
) -> Result<Vec<String>, String> {
    let mut order = Vec::new();
    
    // State nodes have no dependencies, they go first
    let mut state_nodes: Vec<String> = nodes.iter()
        .filter(|(_, n)| matches!(n.kind, ReactiveNodeKind::State))
        .map(|(name, _)| name.clone())
        .collect();
    state_nodes.sort(); // deterministic order
    order.extend(state_nodes);

    // Kahn's for computed nodes
    let computed_names: Vec<String> = nodes.iter()
        .filter(|(_, n)| matches!(n.kind, ReactiveNodeKind::Computed))
        .map(|(name, _)| name.clone())
        .collect();

    if computed_names.is_empty() {
        return Ok(order);
    }

    // Build in-degree map (only for computed-to-computed edges)
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    for name in &computed_names {
        let deps = dependencies.get(name).cloned().unwrap_or_default();
        // Count only computed dependencies (state dependencies are already resolved)
        let computed_deps: usize = deps.iter()
            .filter(|d| nodes.get(*d).map(|n| matches!(n.kind, ReactiveNodeKind::Computed)).unwrap_or(false))
            .count();
        in_degree.insert(name.clone(), computed_deps);
    }

    let mut queue: VecDeque<String> = in_degree.iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| name.clone())
        .collect();

    let mut sorted_computed = Vec::new();
    while let Some(name) = queue.pop_front() {
        sorted_computed.push(name.clone());
        // For each computed that depends on `name`, decrement in-degree
        for (other_name, other_deps) in dependencies.iter() {
            if other_deps.contains(&name) {
                if let Some(deg) = in_degree.get_mut(other_name) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(other_name.clone());
                    }
                }
            }
        }
    }

    if sorted_computed.len() != computed_names.len() {
        return Err("Cycle detected in computed dependency graph. Computed values cannot form circular references.".to_string());
    }

    order.extend(sorted_computed);
    Ok(order)
}

// ──────────────────── AST-Level Collection ────────────────────

/// Direct AST analysis to collect state/computed declarations.
/// This is the primary mechanism since HIR currently lowers both to Let.
pub fn collect_reactive_from_ast<'a>(
    statements: &'a [kinetix_language::ast::Statement<'a>],
) -> (HashMap<String, ReactiveNode>, Vec<(String, usize)>) {
    let mut nodes = HashMap::new();
    let mut computed_names = Vec::new();

    for stmt in statements {
        match stmt {
            kinetix_language::ast::Statement::State { name, line, .. } => {
                nodes.insert(name.clone(), ReactiveNode {
                    name: name.clone(),
                    kind: ReactiveNodeKind::State,
                    line: *line,
                });
            }
            kinetix_language::ast::Statement::Computed { name, line, .. } => {
                nodes.insert(name.clone(), ReactiveNode {
                    name: name.clone(),
                    kind: ReactiveNodeKind::Computed,
                    line: *line,
                });
                computed_names.push((name.clone(), *line));
            }
            kinetix_language::ast::Statement::Block { statements: inner, .. } => {
                let (inner_nodes, inner_computed) = collect_reactive_from_ast(inner);
                nodes.extend(inner_nodes);
                computed_names.extend(inner_computed);
            }
            kinetix_language::ast::Statement::Function { body, .. } => {
                if let kinetix_language::ast::Statement::Block { statements: inner, .. } = body {
                    let (inner_nodes, inner_computed) = collect_reactive_from_ast(inner);
                    nodes.extend(inner_nodes);
                    computed_names.extend(inner_computed);
                }
            }
            _ => {}
        }
    }

    (nodes, computed_names)
}

// ──────────────────── Tests ────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_graph() {
        let program = HirProgram { statements: vec![] };
        let graph = build_reactive_graph(&program).unwrap();
        assert!(graph.nodes.is_empty());
        assert!(graph.update_order.is_empty());
    }

    #[test]
    fn test_collect_reactive_from_ast_basic() {
        use kinetix_language::lexer::Lexer;
        use kinetix_language::parser::Parser;
        use bumpalo::Bump;

        let source = r#"
            state counter = 0;
            computed doubled = counter * 2;
            let normal = 42;
        "#;

        let lexer = Lexer::new(source);
        let arena = Bump::new();
        let mut parser = Parser::new(lexer, &arena);
        let program = parser.parse_program();
        assert!(parser.errors.is_empty(), "Parse errors: {:?}", parser.errors);

        let (nodes, computed_names) = collect_reactive_from_ast(&program.statements);

        assert_eq!(nodes.len(), 2, "Should have 2 reactive nodes (state + computed)");
        assert!(nodes.contains_key("counter"), "Should contain 'counter' state");
        assert!(nodes.contains_key("doubled"), "Should contain 'doubled' computed");
        assert!(!nodes.contains_key("normal"), "'normal' let should not be reactive");
        assert!(matches!(nodes["counter"].kind, ReactiveNodeKind::State));
        assert!(matches!(nodes["doubled"].kind, ReactiveNodeKind::Computed));
        assert_eq!(computed_names.len(), 1);
        assert_eq!(computed_names[0].0, "doubled");
    }
}
