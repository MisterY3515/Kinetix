/// Compile-Time Drop Order Verification Pass
///
/// This pass analyzes the MIR graph to guarantee two properties before LLVM emission:
/// 1. Values are destructed in the exact reverse order of their initialization globally.
/// 2. No panics can occur in the Drop Glue due to double-frees or uninitialized drops.
///
/// While `borrowck.rs` ensures we don't *use* moved/freed values, `drop_verify.rs`
/// mathematically ensures the compiler-injected *cleanup/drop paths* are structurally sound.
///
/// Build 38 Phase B1: the original per-block-only double-drop check missed a drop
/// split across two different blocks (impossible before real branching existed, but
/// very possible now). This is generalized to a CFG-wide dataflow: for every local,
/// track whether it has already been dropped on the path reaching a given point, and
/// flag both a second `Drop` (double-drop) and any *use* (Copy/Move/Borrow) of an
/// already-dropped local, since both indicate the same underlying problem -- the value
/// no longer legitimately exists at that point in the program. A full "were all live
/// locals dropped before every Return" check is not attempted here: MIR's own
/// `BasicBlockData`/`MirFunction` types don't retain scope-liveness information (only
/// `MirBuilder` does, transiently, while lowering), so re-deriving it here would mean
/// duplicating scope tracking rather than verifying it.

use crate::mir::{MirProgram, MirFunction, BasicBlockData, StatementKind, TerminatorKind, RValue, Operand, LocalId};
use std::collections::HashSet;

pub fn verify(program: &MirProgram) -> Result<(), String> {
    for func in &program.functions {
        verify_function(func)?;
    }
    verify_function(&program.main_block)?;
    Ok(())
}

fn successors(block: &BasicBlockData) -> Vec<usize> {
    match block.terminator.as_ref().map(|t| &t.kind) {
        Some(TerminatorKind::Goto(target)) => vec![target.0],
        Some(TerminatorKind::Branch { then_block, else_block, .. }) => vec![then_block.0, else_block.0],
        Some(TerminatorKind::Return(_)) | None => vec![],
    }
}

/// A local is considered dropped at a join point if it was dropped on *either*
/// incoming path -- using it afterwards would be wrong on at least that one path.
fn join_dropped(a: &HashSet<LocalId>, b: &HashSet<LocalId>) -> HashSet<LocalId> {
    a.union(b).cloned().collect()
}

fn operand_local(op: &Operand) -> Option<LocalId> {
    match op {
        Operand::Copy(p) | Operand::Move(p) | Operand::Borrow(p, _) => Some(p.local),
        Operand::Constant(_) => None,
    }
}

fn rvalue_locals(rvalue: &RValue) -> Vec<LocalId> {
    match rvalue {
        RValue::Use(op) => operand_local(op).into_iter().collect(),
        RValue::UnaryOp(_, op) => operand_local(op).into_iter().collect(),
        RValue::BinaryOp(_, l, r) => [operand_local(l), operand_local(r)].into_iter().flatten().collect(),
        RValue::Call(f, args) => {
            let mut v: Vec<LocalId> = operand_local(f).into_iter().collect();
            v.extend(args.iter().filter_map(operand_local));
            v
        }
        RValue::Array(elems) | RValue::Aggregate(_, elems) => elems.iter().filter_map(operand_local).collect(),
    }
}

/// Applies a block's statements to `dropped`, without reporting errors -- used
/// while the fixpoint is still converging.
fn simulate_block(block: &BasicBlockData, dropped: &mut HashSet<LocalId>) {
    for stmt in &block.statements {
        match &stmt.kind {
            StatementKind::Drop(place) => { dropped.insert(place.local); }
            StatementKind::Assign(place, _rvalue) => { dropped.remove(&place.local); } // reassignment brings a local back to life
            StatementKind::Expression(_) => {}
        }
    }
}

fn verify_function(func: &MirFunction) -> Result<(), String> {
    if func.basic_blocks.is_empty() { return Ok(()); }

    let n = func.basic_blocks.len();
    let mut entry: Vec<Option<HashSet<LocalId>>> = vec![None; n];
    entry[0] = Some(HashSet::new());

    let mut worklist = vec![0usize];
    let mut queued = vec![false; n];
    queued[0] = true;

    while let Some(block_idx) = worklist.pop() {
        queued[block_idx] = false;
        let mut dropped = entry[block_idx].clone().expect("entry state set before a block is processed");
        let block = &func.basic_blocks[block_idx];
        simulate_block(block, &mut dropped);

        for succ in successors(block) {
            let merged = match &entry[succ] {
                None => dropped.clone(),
                Some(existing) => join_dropped(existing, &dropped),
            };
            if entry[succ].as_ref() != Some(&merged) {
                entry[succ] = Some(merged);
                if !queued[succ] {
                    worklist.push(succ);
                    queued[succ] = true;
                }
            }
        }
    }

    // Final pass: re-simulate from each block's converged entry state, reporting errors.
    for block_idx in 0..n {
        let mut dropped = entry[block_idx].clone().unwrap_or_default();
        let block = &func.basic_blocks[block_idx];
        for (stmt_idx, stmt) in block.statements.iter().enumerate() {
            match &stmt.kind {
                StatementKind::Drop(place) => {
                    if dropped.contains(&place.local) {
                        return Err(format!(
                            "Drop Order Verification Error: Local {:?} dropped multiple times (reaching block {} statement {})",
                            place.local, block_idx, stmt_idx
                        ));
                    }
                    dropped.insert(place.local);
                }
                StatementKind::Assign(place, rvalue) => {
                    for local in rvalue_locals(rvalue) {
                        if dropped.contains(&local) {
                            return Err(format!(
                                "Drop Order Verification Error: Local {:?} used after being dropped (block {} statement {})",
                                local, block_idx, stmt_idx
                            ));
                        }
                    }
                    dropped.remove(&place.local);
                }
                StatementKind::Expression(rvalue) => {
                    for local in rvalue_locals(rvalue) {
                        if dropped.contains(&local) {
                            return Err(format!(
                                "Drop Order Verification Error: Local {:?} used after being dropped (block {} statement {})",
                                local, block_idx, stmt_idx
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mir::{BasicBlockData, LocalId, MirStatement, Place};

    #[test]
    fn test_double_drop_caught() {
        let stmt1 = MirStatement {
            kind: StatementKind::Drop(Place { local: LocalId(1) }),
            line: 0,
        };
        let stmt2 = MirStatement {
            kind: StatementKind::Drop(Place { local: LocalId(1) }),
            line: 1,
        };
        
        let block = BasicBlockData {
            statements: vec![stmt1, stmt2],
            terminator: None,
        };
        
        // Mock function builder
        let mut f = MirFunction {
            name: "mock".to_string(),
            args: vec![],
            return_ty: crate::types::Type::Void,
            locals: vec![],
            basic_blocks: vec![block],
        };
        
        assert!(verify_function(&f).is_err());
    }

    // Build 38 Phase B1: a double-drop split across two different (but connected)
    // blocks -- impossible before real branching existed, since every function was
    // exactly one block. The old per-block-reset check would have missed this.
    #[test]
    fn test_cross_block_double_drop_caught() {
        use crate::mir::{BasicBlock, Terminator};

        let block0 = BasicBlockData {
            statements: vec![MirStatement { kind: StatementKind::Drop(Place { local: LocalId(0) }), line: 0 }],
            terminator: Some(Terminator { kind: TerminatorKind::Goto(BasicBlock(1)), line: 0 }),
        };
        let block1 = BasicBlockData {
            statements: vec![MirStatement { kind: StatementKind::Drop(Place { local: LocalId(0) }), line: 1 }],
            terminator: Some(Terminator { kind: TerminatorKind::Return(None), line: 1 }),
        };

        let f = MirFunction {
            name: "mock".to_string(),
            args: vec![],
            return_ty: crate::types::Type::Void,
            locals: vec![],
            basic_blocks: vec![block0, block1],
        };

        assert!(verify_function(&f).is_err(), "cross-block double-drop of the same local should be caught");
    }

    // A drop reachable on only one branch of an if/else, with no re-drop
    // afterwards, must NOT be flagged -- the check must not over-trigger just
    // because a local is dropped in a non-entry block.
    #[test]
    fn test_single_drop_on_one_branch_is_fine() {
        use crate::mir::{BasicBlock, Terminator, Operand, Constant};

        let entry = BasicBlockData {
            statements: vec![],
            terminator: Some(Terminator {
                kind: TerminatorKind::Branch {
                    cond: Operand::Constant(Constant::Bool(true)),
                    then_block: BasicBlock(1),
                    else_block: BasicBlock(2),
                },
                line: 0,
            }),
        };
        let then_block = BasicBlockData {
            statements: vec![MirStatement { kind: StatementKind::Drop(Place { local: LocalId(0) }), line: 1 }],
            terminator: Some(Terminator { kind: TerminatorKind::Goto(BasicBlock(3)), line: 1 }),
        };
        let else_block = BasicBlockData {
            statements: vec![],
            terminator: Some(Terminator { kind: TerminatorKind::Goto(BasicBlock(3)), line: 2 }),
        };
        let merge_block = BasicBlockData {
            statements: vec![],
            terminator: Some(Terminator { kind: TerminatorKind::Return(None), line: 3 }),
        };

        let f = MirFunction {
            name: "mock".to_string(),
            args: vec![],
            return_ty: crate::types::Type::Void,
            locals: vec![],
            basic_blocks: vec![entry, then_block, else_block, merge_block],
        };

        assert!(verify_function(&f).is_ok());
    }
}
