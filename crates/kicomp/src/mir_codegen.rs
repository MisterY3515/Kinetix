/// MIR-consuming codegen (Build 38 Phase B2).
///
/// Lowers a (post-B1, structurally real) MIR CFG straight to the same
/// `Opcode`/`Instruction` bytecode shape `compiler.rs` produces by walking the
/// AST, so the result can run on the same VM and through the same `opt.rs`
/// peephole optimizer unchanged. This is a new, opt-in path: nothing in the
/// default pipeline calls into this module, and `compiler.rs` is untouched.
///
/// This module does **not** run the MIR validators itself -- callers are
/// expected to have already run `borrowck`/`mono_validate`/`drop_verify`/
/// `ssa_validate` (exactly as the existing pipeline does before discarding
/// MIR today) before compiling it to bytecode.
///
/// Known, deliberate gaps (not attempted here, matching pre-existing MIR
/// lowering gaps in `mir.rs`): `RValue::Aggregate` (struct literals) fails to
/// compile with an explicit error, since `mir.rs`'s `HirExprKind::StructLiteral`
/// lowering discards field names (only positional values survive), so there
/// is no way to reproduce `compiler.rs`'s named-field `Map` + `__class__`
/// shape. `Match`/`MemberAccess`/`FunctionLiteral`/`MapLiteral`/complex
/// (non-identifier) assignment targets were already stubbed to a `Null`
/// placeholder by `mir.rs` itself before this module existed, so they
/// silently compile to `LoadNull` here too, same as any other constant.

use crate::mir::{
    MirProgram, MirFunction, MirStatement, StatementKind, Terminator, TerminatorKind,
    RValue, Operand, LocalId, Constant as MirConstant,
};
use crate::ir::{CompiledProgram, CompiledFunction, Instruction, Opcode, Constant as IrConstant};
use crate::types::Type;

/// Lowers a full `MirProgram` (already validated) to a `CompiledProgram`
/// runnable by the same VM `compiler.rs`'s output runs on.
pub fn compile_mir_program(mir: &MirProgram) -> Result<CompiledProgram, String> {
    let mut program = CompiledProgram::new();

    let mut func_indices = Vec::with_capacity(mir.functions.len());
    for mir_func in &mir.functions {
        let compiled = compile_function(mir_func)?;
        func_indices.push(program.functions.len());
        program.functions.push(compiled);
    }

    // Register every top-level MIR function as a global up front, mirroring
    // `compiler.rs::compile_function`'s own `SetGlobal` registration -- except
    // all at once, before any of `main`'s own statements run, rather than
    // at each function's point in program order. MIR doesn't preserve where a
    // `Function` statement appeared relative to other top-level statements
    // (`MirBuilder`'s `HirStmtKind::Function` arm pushes straight to
    // `self.functions`, with no matching statement left behind in the
    // enclosing block -- see that arm's doc comment), so there is no position
    // to reproduce faithfully. This is a deliberate, documented simplification:
    // a call to a function declared *later* in the source would resolve here
    // even though `compiler.rs` would reject it (or read stale state) --
    // acceptable for a differential-testing path where test programs simply
    // declare functions before use.
    let functions = &mir.functions;
    let indices = &func_indices;
    let main = compile_function_with_prologue(&mir.main_block, |cg| {
        for (mir_func, &idx) in functions.iter().zip(indices) {
            let reg = cg.alloc_scratch();
            let func_const = cg.func.add_constant(IrConstant::Function(idx));
            cg.emit(Instruction::ab(Opcode::LoadConst, reg, func_const));
            let name_const = cg.func.add_constant(IrConstant::String(mir_func.name.clone()));
            cg.emit(Instruction::ab(Opcode::SetGlobal, name_const, reg));
        }
    })?;

    program.main = main;
    Ok(program)
}

fn compile_function(mir_func: &MirFunction) -> Result<CompiledFunction, String> {
    compile_function_with_prologue(mir_func, |_| {})
}

/// Compiles one `MirFunction`, first invoking `prologue` (used only by
/// `compile_mir_program`'s global-function-registration step for `main`) so
/// its instructions land before block 0's, keeping block-offset bookkeeping
/// correct without any post-hoc offset shifting.
fn compile_function_with_prologue(
    mir_func: &MirFunction,
    prologue: impl FnOnce(&mut FnCodegen),
) -> Result<CompiledFunction, String> {
    let mut func = CompiledFunction::new(mir_func.name.clone(), mir_func.args.len() as u16);
    func.param_names = mir_func.args.iter()
        .map(|id| mir_func.locals[id.0].name.clone().unwrap_or_default())
        .collect();

    let locals_len = mir_func.locals.len() as u16;
    let mut cg = FnCodegen {
        func,
        mir_locals: &mir_func.locals,
        next_scratch: locals_len,
        max_register: locals_len,
        current_line: 0,
        block_offsets: vec![0; mir_func.basic_blocks.len()],
        pending_jumps: vec![],
    };

    prologue(&mut cg);
    cg.next_scratch = locals_len;

    for (i, block) in mir_func.basic_blocks.iter().enumerate() {
        cg.block_offsets[i] = cg.func.instructions.len();
        for stmt in &block.statements {
            cg.current_line = stmt.line;
            cg.compile_statement(stmt)?;
            cg.next_scratch = locals_len;
        }
        cg.compile_terminator(block.terminator.as_ref())?;
        cg.next_scratch = locals_len;
    }

    let jumps = std::mem::take(&mut cg.pending_jumps);
    for (instr_idx, target_block) in jumps {
        cg.func.instructions[instr_idx].a = cg.block_offsets[target_block] as u16;
    }

    cg.func.locals = cg.max_register;
    Ok(cg.func)
}

/// Per-function codegen state. Register model: MIR already ANF-normalizes
/// every intermediate value into a dedicated `LocalId` during lowering (see
/// `mir.rs`'s `push_local`/`lower_expression_to_operand`), so each MIR local
/// maps 1:1 onto a VM register (`LocalId(n)` -> register `n`) -- no separate
/// slot-assignment pass is needed the way `compiler.rs` needs one (it walks
/// raw AST subexpressions, which have no equivalent pre-assigned identity).
/// A bump-allocated scratch range starting right after the last local's
/// register is used only for the handful of cases where bytecode needs a
/// register that no MIR `Place` backs: materializing a bare `Operand::Constant`
/// and staging a call's contiguous callee+argument registers.
struct FnCodegen<'a> {
    func: CompiledFunction,
    mir_locals: &'a [crate::mir::LocalDecl],
    next_scratch: u16,
    max_register: u16,
    current_line: usize,
    /// `block_offsets[i]` = instruction index where MIR block `i`'s first
    /// instruction landed.
    block_offsets: Vec<usize>,
    /// `(instruction index whose `.a` is a placeholder, target MIR block
    /// index)` -- patched once every block's start offset is known, since MIR
    /// blocks are pre-allocated and compiled in vector order (no backpatching
    /// idiom needed the way `compiler.rs` needs one for forward jumps).
    pending_jumps: Vec<(usize, usize)>,
}

impl<'a> FnCodegen<'a> {
    fn reg_of(&self, local: LocalId) -> u16 {
        local.0 as u16
    }

    fn alloc_scratch(&mut self) -> u16 {
        let r = self.next_scratch;
        self.next_scratch += 1;
        if self.next_scratch > self.max_register {
            self.max_register = self.next_scratch;
        }
        r
    }

    fn emit(&mut self, instr: Instruction) -> usize {
        let idx = self.func.emit(instr);
        self.func.line_map.push(self.current_line as u32);
        idx
    }

    fn compile_statement(&mut self, stmt: &MirStatement) -> Result<(), String> {
        match &stmt.kind {
            StatementKind::Assign(place, rvalue) => {
                let dst = self.reg_of(place.local);
                let is_fn_target = matches!(self.mir_locals[place.local.0].ty, Type::Fn(_, _));
                self.compile_rvalue_into(dst, rvalue, is_fn_target)
            }
            StatementKind::Expression(rvalue) => {
                let dst = self.alloc_scratch();
                self.compile_rvalue_into(dst, rvalue, false)
            }
            // No runtime counterpart: ownership is enforced by `borrowck`/
            // `drop_verify` at MIR validation time, not by a bytecode
            // instruction (`compiler.rs`'s own output has no equivalent
            // "drop" opcode either).
            StatementKind::Drop(_) => Ok(()),
        }
    }

    fn compile_terminator(&mut self, term: Option<&Terminator>) -> Result<(), String> {
        let term = term.ok_or_else(|| {
            "mir_codegen: basic block has no terminator (malformed MIR)".to_string()
        })?;
        self.current_line = term.line;
        match &term.kind {
            TerminatorKind::Return(Some(op)) => {
                let reg = self.operand_to_register(op);
                self.emit(Instruction::a_only(Opcode::Return, reg));
            }
            TerminatorKind::Return(None) => {
                self.emit(Instruction::a_only(Opcode::ReturnVoid, 0));
            }
            TerminatorKind::Goto(target) => {
                let idx = self.emit(Instruction::a_only(Opcode::Jump, 0));
                self.pending_jumps.push((idx, target.0));
            }
            TerminatorKind::Branch { cond, then_block, else_block } => {
                let cond_reg = self.operand_to_register(cond);
                let jf_idx = self.emit(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));
                self.pending_jumps.push((jf_idx, else_block.0));
                let j_idx = self.emit(Instruction::a_only(Opcode::Jump, 0));
                self.pending_jumps.push((j_idx, then_block.0));
            }
        }
        Ok(())
    }

    fn compile_rvalue_into(&mut self, dst: u16, rvalue: &RValue, is_fn_target: bool) -> Result<(), String> {
        match rvalue {
            RValue::Use(Operand::Constant(MirConstant::String(name))) if is_fn_target => {
                let name_idx = self.func.add_constant(IrConstant::String(name.clone()));
                self.emit(Instruction::ab(Opcode::GetGlobal, dst, name_idx));
            }
            RValue::Use(op) => self.load_operand_into(op, dst),
            RValue::BinaryOp(op, l, r) => {
                let l_reg = self.operand_to_register(l);
                let r_reg = self.operand_to_register(r);
                let opcode = binop_to_opcode(op)?;
                self.emit(Instruction::new(opcode, dst, l_reg, r_reg));
            }
            RValue::UnaryOp(op, r) => {
                let r_reg = self.operand_to_register(r);
                let opcode = unop_to_opcode(op)?;
                self.emit(Instruction::ab(opcode, dst, r_reg));
            }
            RValue::Call(func_op, args) => {
                let result_reg = self.compile_call(func_op, args);
                if result_reg != dst {
                    self.emit(Instruction::ab(Opcode::SetLocal, dst, result_reg));
                }
            }
            RValue::Array(elems) => self.compile_array_into(dst, elems),
            RValue::Aggregate(name, _fields) => {
                return Err(format!(
                    "mir_codegen: struct/aggregate literal '{}' is not supported -- \
                     MIR's RValue::Aggregate does not preserve field names, so it \
                     cannot be lowered to compiler.rs's named-field Map shape",
                    name
                ));
            }
        }
        Ok(())
    }

    /// Resolves an operand to a register holding its value. `Copy`/`Move`/
    /// `Borrow` all read the same register their `Place` already owns --
    /// this VM has no aliasing distinct from a register slot (`compiler.rs`
    /// erases references the same way; see its `Prefix` `&`/`&mut` handling).
    /// A bare `Constant` (rare -- see the module doc comment for the only two
    /// spots MIR currently produces one) is materialized into a fresh scratch
    /// register.
    fn operand_to_register(&mut self, op: &Operand) -> u16 {
        match op {
            Operand::Copy(p) | Operand::Move(p) | Operand::Borrow(p, _) => self.reg_of(p.local),
            Operand::Constant(c) => {
                let reg = self.alloc_scratch();
                self.load_constant_into(c, reg);
                reg
            }
        }
    }

    fn load_operand_into(&mut self, op: &Operand, dst: u16) {
        match op {
            Operand::Copy(p) | Operand::Move(p) | Operand::Borrow(p, _) => {
                let src = self.reg_of(p.local);
                if src != dst {
                    self.emit(Instruction::ab(Opcode::SetLocal, dst, src));
                }
            }
            Operand::Constant(c) => self.load_constant_into(c, dst),
        }
    }

    fn load_constant_into(&mut self, c: &MirConstant, dst: u16) {
        match c {
            MirConstant::Int(v) => {
                let idx = self.func.add_constant(IrConstant::Integer(*v));
                self.emit(Instruction::ab(Opcode::LoadConst, dst, idx));
            }
            MirConstant::Float(v) => {
                let idx = self.func.add_constant(IrConstant::Float(*v));
                self.emit(Instruction::ab(Opcode::LoadConst, dst, idx));
            }
            MirConstant::Bool(v) => {
                let opcode = if *v { Opcode::LoadTrue } else { Opcode::LoadFalse };
                self.emit(Instruction::a_only(opcode, dst));
            }
            MirConstant::String(s) => {
                let idx = self.func.add_constant(IrConstant::String(s.clone()));
                self.emit(Instruction::ab(Opcode::LoadConst, dst, idx));
            }
            MirConstant::Null => {
                self.emit(Instruction::a_only(Opcode::LoadNull, dst));
            }
        }
    }

    /// Emits a call, returning the register holding its result. The callee's
    /// name reaches here as a bare `Operand::Constant(String(name))` only for
    /// the `for`-loop's synthetic `len()` bounds check (the only place
    /// `mir.rs` builds a `Call` operand directly rather than through
    /// `lower_expression_to_operand`); every other named call already went
    /// through an `Assign` whose `is_fn_target` branch above resolved the
    /// global into a place's register, so it arrives here as an ordinary
    /// `Copy`/`Move`.
    fn compile_call(&mut self, func_op: &Operand, args: &[Operand]) -> u16 {
        let call_reg = self.alloc_scratch();
        match func_op {
            Operand::Constant(MirConstant::String(name)) => {
                let name_idx = self.func.add_constant(IrConstant::String(name.clone()));
                self.emit(Instruction::ab(Opcode::GetGlobal, call_reg, name_idx));
            }
            _ => self.load_operand_into(func_op, call_reg),
        }
        // VM calling convention: the callee occupies `call_reg`, arguments
        // occupy `call_reg+1..=call_reg+n` contiguously (mirrors
        // `compiler.rs`'s own `Expression::Call` lowering exactly).
        for arg in args {
            let expected_reg = self.alloc_scratch();
            self.load_operand_into(arg, expected_reg);
        }
        self.emit(Instruction::ab(Opcode::Call, call_reg, args.len() as u16));
        call_reg
    }

    /// `MakeArray` reads `n` contiguous registers starting at its `A` operand
    /// (see `compiler.rs`'s `Expression::ArrayLiteral`), but MIR's elements
    /// are already-materialized locals scattered arbitrarily across the
    /// register space -- unlike `compiler.rs`, which computes each element
    /// fresh into naturally-contiguous temps. Stage a fresh contiguous range
    /// first, same idea as `compile_call`'s argument staging.
    fn compile_array_into(&mut self, dst: u16, elems: &[Operand]) {
        let start = self.alloc_scratch();
        if !elems.is_empty() {
            self.load_operand_into(&elems[0], start);
            for el in &elems[1..] {
                let reg = self.alloc_scratch();
                self.load_operand_into(el, reg);
            }
        }
        self.emit(Instruction::ab(Opcode::MakeArray, start, elems.len() as u16));
        if start != dst {
            self.emit(Instruction::ab(Opcode::SetLocal, dst, start));
        }
    }
}

fn binop_to_opcode(op: &str) -> Result<Opcode, String> {
    Ok(match op {
        "+" => Opcode::Add,
        "-" => Opcode::Sub,
        "*" => Opcode::Mul,
        "/" => Opcode::Div,
        "%" => Opcode::Mod,
        "==" => Opcode::Eq,
        "!=" => Opcode::Neq,
        "<" => Opcode::Lt,
        ">" => Opcode::Gt,
        "<=" => Opcode::Lte,
        ">=" => Opcode::Gte,
        "&&" => Opcode::And,
        "||" => Opcode::Or,
        "[]" => Opcode::GetIndex,
        ".." => Opcode::MakeRange,
        _ => return Err(format!("mir_codegen: unsupported binary operator '{}'", op)),
    })
}

fn unop_to_opcode(op: &str) -> Result<Opcode, String> {
    Ok(match op {
        "-" => Opcode::Neg,
        "!" => Opcode::Not,
        _ => return Err(format!("mir_codegen: unsupported unary operator '{}'", op)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use kinetix_language::lexer::Lexer;
    use kinetix_language::parser::Parser;
    use crate::symbol::resolve_program;
    use crate::hir::lower_to_hir as ast_to_hir;
    use crate::typeck::TypeContext;
    use crate::mir::lower_to_mir;

    fn compile_source_to_mir(src: &str) -> MirProgram {
        let arena = Bump::new();
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, &arena);
        let program = parser.parse_program();
        let symbols = resolve_program(&program.statements).unwrap();
        let traits = crate::trait_solver::TraitEnvironment::new();
        let hir = ast_to_hir(&program.statements, &symbols, &traits);

        let mut ctx = TypeContext::new();
        let constraints = ctx.collect_constraints(&hir);
        ctx.solve(&constraints).unwrap();

        lower_to_mir(&hir, &ctx.substitution)
    }

    fn assert_all_validators_pass(mir: &MirProgram) {
        assert!(crate::borrowck::check_mir(mir).is_ok());
        assert!(crate::ssa_validate::validate(mir).is_ok());
        assert!(crate::drop_verify::verify(mir).is_ok());
        assert!(crate::mono_validate::validate(mir).is_ok());
    }

    #[test]
    fn test_compiles_simple_arithmetic() {
        let mir = compile_source_to_mir("let a = 1 + 2 * 3");
        assert_all_validators_pass(&mir);
        let program = compile_mir_program(&mir).unwrap();
        assert!(program.main.instructions.iter().any(|i| i.opcode == Opcode::Mul));
        assert!(program.main.instructions.iter().any(|i| i.opcode == Opcode::Add));
    }

    #[test]
    fn test_if_branch_offsets_are_patched_to_real_instructions() {
        let mir = compile_source_to_mir("let x = if true { 1 } else { 2 }");
        assert_all_validators_pass(&mir);
        let program = compile_mir_program(&mir).unwrap();
        for instr in &program.main.instructions {
            if matches!(instr.opcode, Opcode::Jump | Opcode::JumpIfFalse) {
                assert!(
                    (instr.a as usize) < program.main.instructions.len(),
                    "jump target {} out of bounds ({} instructions)",
                    instr.a,
                    program.main.instructions.len()
                );
            }
        }
    }

    #[test]
    fn test_while_loop_compiles_with_backward_jump() {
        let mir = compile_source_to_mir("let mut i = 0\nwhile i < 3 {\n    i = i + 1\n}");
        assert_all_validators_pass(&mir);
        let program = compile_mir_program(&mir).unwrap();
        let jump_targets: Vec<u16> = program.main.instructions.iter()
            .filter(|i| i.opcode == Opcode::Jump)
            .map(|i| i.a)
            .collect();
        assert!(jump_targets.iter().enumerate().any(|(idx, &target)| (target as usize) <= idx),
            "expected at least one backward jump (the loop header re-check)");
    }

    #[test]
    fn test_function_call_resolves_via_get_global() {
        let mir = compile_source_to_mir("fn foo(x: int) -> int { return x + 1 }\nlet y = foo(41)");
        assert_all_validators_pass(&mir);
        let program = compile_mir_program(&mir).unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "foo");
        assert!(program.main.instructions.iter().any(|i| i.opcode == Opcode::GetGlobal));
        assert!(program.main.instructions.iter().any(|i| i.opcode == Opcode::Call));
    }

    #[test]
    fn test_for_loop_len_call_resolves_via_get_global() {
        let mir = compile_source_to_mir("for i in 0..3 {\n    let y = i\n}");
        assert_all_validators_pass(&mir);
        let program = compile_mir_program(&mir).unwrap();
        assert!(program.main.instructions.iter().any(|i| i.opcode == Opcode::GetGlobal));
    }

    #[test]
    fn test_recursive_function_with_early_return_and_no_else_passes_borrowck() {
        // A no-`else` if whose `then` branch diverges (`return`) is typed
        // non-Void by HIR (the diverging branch unifies with any type), so MIR
        // allocates a result place for it -- this reproduces a real Phase B1
        // bug where the implicit empty `else` never initialized that place
        // before falling through to the merge block, making borrowck reject
        // completely ordinary early-return code as a use-before-init.
        let mir = compile_source_to_mir(
            "fn fact(n: int) -> int {\n    if n <= 1 {\n        return 1\n    }\n    return n * fact(n - 1)\n}\nprintln(fact(5))"
        );
        assert_all_validators_pass(&mir);
    }

    #[test]
    fn test_for_loop_range_compiles_to_make_range_not_null() {
        // `for i in 0..N` previously left `HirExprKind::Range` unhandled,
        // silently lowering the iterable to `Constant::Null` -- the loop's
        // block structure still validated fine (Phase B1 was validation-only,
        // so it never noticed the iterated value was semantically wrong).
        let mir = compile_source_to_mir("let mut total = 0\nfor i in 0..5 {\n    total = total + i\n}");
        assert_all_validators_pass(&mir);
        let program = compile_mir_program(&mir).unwrap();
        assert!(program.main.instructions.iter().any(|i| i.opcode == Opcode::MakeRange));
    }

    #[test]
    fn test_struct_literal_is_a_clean_error_not_silent_wrong_output() {
        let mir = compile_source_to_mir("struct Point { x: int, y: int }\nlet p = Point { x: 1, y: 2 }");
        assert_all_validators_pass(&mir);
        assert!(compile_mir_program(&mir).is_err());
    }
}
