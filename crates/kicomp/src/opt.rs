/// Kinetix Bytecode Optimizer — Build 35
/// Operates on `CompiledProgram` after compilation, before serialization/execution.
/// Each pass is a pure transformation on the instruction stream.

use crate::ir::{CompiledProgram, CompiledFunction, Instruction, Opcode, Constant};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Run all optimization passes on a compiled program.
pub fn optimize(program: &mut CompiledProgram) {
    optimize_function(&mut program.main);
    for func in program.functions.iter_mut() {
        optimize_function(func);
    }
    program.is_optimized = true;
}

/// Run all optimization passes on a single function.
fn optimize_function(func: &mut CompiledFunction) {
    dead_code_elimination(func);
    constant_folding(func);
    redundant_load_elimination(func);
    jump_threading(func);
    nop_elimination(func);
    drop_redundancy_elimination(func);
}

// ─── Pass 1: Dead Code Elimination ──────────────────────────────────────────
/// Remove instructions after unconditional Return/ReturnVoid/Halt
/// until the next jump target, effectively trimming unreachable code.

fn dead_code_elimination(func: &mut CompiledFunction) {
    // First: collect all jump targets so we know which instructions are reachable entry points
    let mut jump_targets = std::collections::HashSet::new();
    for instr in &func.instructions {
        match instr.opcode {
            Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue => {
                jump_targets.insert(instr.a as usize);
            }
            _ => {}
        }
    }

    let mut dead = false;
    for i in 0..func.instructions.len() {
        if dead {
            // If this instruction is a jump target, it's reachable again
            if jump_targets.contains(&i) {
                dead = false;
            } else {
                // Mark as Nop (will be cleaned up by nop_elimination)
                func.instructions[i] = Instruction::a_only(Opcode::Nop, 0);
            }
        }

        match func.instructions[i].opcode {
            Opcode::Return | Opcode::ReturnVoid | Opcode::Halt => {
                dead = true;
            }
            Opcode::Jump => {
                // Unconditional jump also makes following code dead
                dead = true;
            }
            _ => {}
        }
    }
}

// ─── Pass 2: Constant Folding ───────────────────────────────────────────────
/// Detect patterns like:
///   LoadConst rA, constIdx1  (integer/float)
///   LoadConst rB, constIdx2  (integer/float)
///   Add/Sub/Mul/Div rC, rA, rB
/// And replace with a single LoadConst rC, folded_result.

fn constant_folding(func: &mut CompiledFunction) {
    // Track which register holds which constant index (after a LoadConst)
    let mut reg_const: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();

    let len = func.instructions.len();
    let mut i = 0;
    while i < len {
        let instr = func.instructions[i];

        match instr.opcode {
            Opcode::LoadConst => {
                reg_const.insert(instr.a, instr.b as usize);
            }
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod => {
                // Check if both operands are known constants
                if let (Some(&ci_b), Some(&ci_c)) = (reg_const.get(&instr.b), reg_const.get(&instr.c)) {
                    if let Some(folded) = fold_arithmetic(&func.constants, ci_b, ci_c, instr.opcode) {
                        let new_idx = func.add_constant(folded);
                        func.instructions[i] = Instruction::ab(Opcode::LoadConst, instr.a, new_idx);
                        reg_const.insert(instr.a, new_idx as usize);
                        i += 1;
                        continue;
                    }
                }
                // Result register is no longer a known constant
                reg_const.remove(&instr.a);
            }
            // Any write to a register invalidates its tracked constant
            _ => {
                if writes_to_register(instr.opcode) {
                    reg_const.remove(&instr.a);
                }
                // Control flow invalidates all tracking
                match instr.opcode {
                    Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue
                    | Opcode::Call | Opcode::TailCall | Opcode::Return | Opcode::ReturnVoid => {
                        reg_const.clear();
                    }
                    _ => {}
                }
            }
        }

        i += 1;
    }
}

fn fold_arithmetic(constants: &[Constant], a: usize, b: usize, op: Opcode) -> Option<Constant> {
    if a >= constants.len() || b >= constants.len() {
        return None;
    }
    match (&constants[a], &constants[b]) {
        (Constant::Integer(va), Constant::Integer(vb)) => {
            let result = match op {
                Opcode::Add => va.checked_add(*vb)?,
                Opcode::Sub => va.checked_sub(*vb)?,
                Opcode::Mul => va.checked_mul(*vb)?,
                Opcode::Div => {
                    if *vb == 0 { return None; }
                    va.checked_div(*vb)?
                }
                Opcode::Mod => {
                    if *vb == 0 { return None; }
                    va.checked_rem(*vb)?
                }
                _ => return None,
            };
            Some(Constant::Integer(result))
        }
        (Constant::Float(va), Constant::Float(vb)) => {
            let result = match op {
                Opcode::Add => va + vb,
                Opcode::Sub => va - vb,
                Opcode::Mul => va * vb,
                Opcode::Div => {
                    if *vb == 0.0 { return None; }
                    va / vb
                }
                Opcode::Mod => {
                    if *vb == 0.0 { return None; }
                    va % vb
                }
                _ => return None,
            };
            Some(Constant::Float(result))
        }
        _ => None,
    }
}

// ─── Pass 3: Redundant Load Elimination ─────────────────────────────────────
/// If the same LoadConst/LoadTrue/LoadFalse/LoadNull loads the same value
/// into the same register consecutively, eliminate the duplicate.

fn redundant_load_elimination(func: &mut CompiledFunction) {
    // Track what each register currently holds (opcode + operand)
    let mut reg_state: std::collections::HashMap<u16, (Opcode, u16)> = std::collections::HashMap::new();

    for i in 0..func.instructions.len() {
        let instr = func.instructions[i];
        match instr.opcode {
            Opcode::LoadConst => {
                let key = (Opcode::LoadConst, instr.b);
                if reg_state.get(&instr.a) == Some(&key) {
                    // Duplicate load, eliminate
                    func.instructions[i] = Instruction::a_only(Opcode::Nop, 0);
                } else {
                    reg_state.insert(instr.a, key);
                }
            }
            Opcode::LoadTrue | Opcode::LoadFalse | Opcode::LoadNull => {
                let key = (instr.opcode, 0);
                if reg_state.get(&instr.a) == Some(&key) {
                    func.instructions[i] = Instruction::a_only(Opcode::Nop, 0);
                } else {
                    reg_state.insert(instr.a, key);
                }
            }
            // Any other write invalidates the register
            _ => {
                if writes_to_register(instr.opcode) {
                    reg_state.remove(&instr.a);
                }
                // Control flow invalidates all
                match instr.opcode {
                    Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue
                    | Opcode::Call | Opcode::TailCall | Opcode::Return | Opcode::ReturnVoid => {
                        reg_state.clear();
                    }
                    _ => {}
                }
            }
        }
    }
}

// ─── Pass 4: Jump Threading ─────────────────────────────────────────────────
/// If a Jump targets another unconditional Jump, chain to the final target.

fn jump_threading(func: &mut CompiledFunction) {
    let len = func.instructions.len();
    for i in 0..len {
        let instr = func.instructions[i];
        match instr.opcode {
            Opcode::Jump => {
                let final_target = resolve_jump_chain(&func.instructions, instr.a as usize, len);
                func.instructions[i].a = final_target as u16;
            }
            Opcode::JumpIfFalse | Opcode::JumpIfTrue => {
                let final_target = resolve_jump_chain(&func.instructions, instr.a as usize, len);
                func.instructions[i].a = final_target as u16;
            }
            _ => {}
        }
    }
}

fn resolve_jump_chain(instructions: &[Instruction], mut target: usize, len: usize) -> usize {
    let mut visited = std::collections::HashSet::new();
    while target < len {
        if !visited.insert(target) {
            break; // Cycle detected, stop
        }
        if instructions[target].opcode == Opcode::Jump {
            target = instructions[target].a as usize;
        } else {
            break;
        }
    }
    target
}

// ─── Pass 5: Nop Elimination ────────────────────────────────────────────────
/// Remove all Nop instructions and adjust jump targets accordingly.

fn nop_elimination(func: &mut CompiledFunction) {
    if func.instructions.iter().all(|i| i.opcode != Opcode::Nop) {
        return; // Nothing to do
    }

    // Build a mapping: old_index → new_index
    let mut index_map: Vec<usize> = Vec::with_capacity(func.instructions.len());
    let mut new_idx = 0usize;
    for instr in &func.instructions {
        index_map.push(new_idx);
        if instr.opcode != Opcode::Nop {
            new_idx += 1;
        }
    }

    // Update jump targets
    for instr in func.instructions.iter_mut() {
        match instr.opcode {
            Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue => {
                let old_target = instr.a as usize;
                if old_target < index_map.len() {
                    instr.a = index_map[old_target] as u16;
                }
            }
            Opcode::IterNext => {
                let old_target = instr.c as usize;
                if old_target < index_map.len() {
                    instr.c = index_map[old_target] as u16;
                }
            }
            _ => {}
        }
    }

    // Remove Nops
    func.instructions.retain(|i| i.opcode != Opcode::Nop);

    // Also compact line_map if it exists
    if !func.line_map.is_empty() && func.line_map.len() == index_map.len() {
        let old_line_map = std::mem::take(&mut func.line_map);
        func.line_map = old_line_map.into_iter()
            .zip(func.instructions.iter()) // won't work because we already removed nops
            .map(|(line, _)| line)
            .collect();
    }
}

// ─── Pass 6: Drop Redundancy Elimination ────────────────────────────────────
/// Remove Pop instructions on registers that are immediately overwritten
/// without being read in between.

fn drop_redundancy_elimination(func: &mut CompiledFunction) {
    let len = func.instructions.len();
    for i in 0..len {
        if func.instructions[i].opcode != Opcode::Pop {
            continue;
        }
        let popped_reg = func.instructions[i].a;

        // Look at the next non-Nop instruction
        if let Some(next_idx) = (i + 1..len).find(|&j| func.instructions[j].opcode != Opcode::Nop) {
            let next = func.instructions[next_idx];
            // If the next instruction writes to the same register without reading it
            if writes_to_register(next.opcode) && next.a == popped_reg
                && !reads_register_b(next.opcode, popped_reg, next.b)
                && !reads_register_c(next.opcode, popped_reg, next.c)
            {
                func.instructions[i] = Instruction::a_only(Opcode::Nop, 0);
            }
        }
    }

    // Clean up any newly introduced Nops
    if func.instructions.iter().any(|i| i.opcode == Opcode::Nop) {
        nop_elimination(func);
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Returns true if the opcode writes a result to register A.
fn writes_to_register(op: Opcode) -> bool {
    matches!(op,
        Opcode::LoadConst | Opcode::LoadNull | Opcode::LoadTrue | Opcode::LoadFalse
        | Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod | Opcode::Neg
        | Opcode::Eq | Opcode::Neq | Opcode::Lt | Opcode::Gt | Opcode::Lte | Opcode::Gte
        | Opcode::Not | Opcode::And | Opcode::Or
        | Opcode::Concat
        | Opcode::GetLocal | Opcode::GetGlobal
        | Opcode::GetMember | Opcode::GetIndex
        | Opcode::MakeArray | Opcode::MakeMap | Opcode::MakeRange
        | Opcode::GetIter | Opcode::IterNext
        | Opcode::Call | Opcode::TailCall
        | Opcode::MakeClosure | Opcode::LoadMethod
    )
}

fn reads_register_b(op: Opcode, reg: u16, b: u16) -> bool {
    if b != reg { return false; }
    // Most arithmetic/comparison opcodes read B
    matches!(op,
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod
        | Opcode::Eq | Opcode::Neq | Opcode::Lt | Opcode::Gt | Opcode::Lte | Opcode::Gte
        | Opcode::And | Opcode::Or | Opcode::Concat
        | Opcode::Neg | Opcode::Not
        | Opcode::GetMember | Opcode::GetIndex
        | Opcode::SetLocal | Opcode::JumpIfFalse | Opcode::JumpIfTrue
    )
}

fn reads_register_c(op: Opcode, reg: u16, c: u16) -> bool {
    if c != reg { return false; }
    matches!(op,
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod
        | Opcode::Eq | Opcode::Neq | Opcode::Lt | Opcode::Gt | Opcode::Lte | Opcode::Gte
        | Opcode::And | Opcode::Or | Opcode::Concat
        | Opcode::GetMember | Opcode::GetIndex
        | Opcode::SetMember | Opcode::SetIndex
    )
}
