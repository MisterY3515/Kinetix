/// Kinetix MIR Optimizer — Build 35
/// Operates on MIR-level structures before bytecode lowering.

use crate::mir::{MirFunction, MirProgram, TerminatorKind};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Run all MIR-level optimization passes.
pub fn optimize_mir(program: &mut MirProgram) {
    optimize_mir_function(&mut program.main_block);
    for func in program.functions.iter_mut() {
        optimize_mir_function(func);
    }
}

fn optimize_mir_function(func: &mut MirFunction) {
    cfg_simplification(func);
    drop_redundancy(func);
}

// ─── Pass 1: Control Flow Simplification ────────────────────────────────────
/// Merge single-predecessor blocks and remove empty blocks that only Goto.

fn cfg_simplification(func: &mut MirFunction) {
    // Simple pass: remove empty blocks that only contain a Goto terminator
    // and redirect predecessors to the target.
    let len = func.basic_blocks.len();
    if len <= 1 {
        return;
    }

    // Build a redirect map for empty blocks
    let mut redirects: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (i, block) in func.basic_blocks.iter().enumerate() {
        if block.statements.is_empty() {
            if let Some(ref term) = block.terminator {
                if let TerminatorKind::Goto(target) = term.kind {
                    redirects.insert(i, target.0);
                }
            }
        }
    }

    // Follow chains (in case of A→B→C where both A and B are empty)
    let mut resolved: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (&src, &dst) in &redirects {
        let mut target = dst;
        let mut visited = std::collections::HashSet::new();
        visited.insert(src);
        while let Some(&next) = redirects.get(&target) {
            if !visited.insert(target) {
                break; // Cycle
            }
            target = next;
        }
        resolved.insert(src, target);
    }

    // Apply redirects to all terminators
    for block in func.basic_blocks.iter_mut() {
        if let Some(ref mut term) = block.terminator {
            if let TerminatorKind::Goto(ref mut target) = term.kind {
                if let Some(&new_target) = resolved.get(&target.0) {
                    target.0 = new_target;
                }
            }
        }
    }
}

// ─── Pass 2: Drop Redundancy ────────────────────────────────────────────────
/// Remove duplicate Drop statements within the same block for the same Place.

fn drop_redundancy(func: &mut MirFunction) {
    for block in func.basic_blocks.iter_mut() {
        let mut seen_drops = std::collections::HashSet::new();
        block.statements.retain(|stmt| {
            match &stmt.kind {
                crate::mir::StatementKind::Drop(place) => {
                    // Use the local id as key for dedup
                    let key = place.local.0;
                    seen_drops.insert(key)
                    // insert returns true if the value was not already present → keep it
                }
                _ => true,
            }
        });
    }
}
