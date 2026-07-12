/// KiComp Compiler: walks the AST and emits register-based bytecode.

use kinetix_language::ast::{Statement, Expression};
use crate::ir::*;
use std::collections::HashMap;

/// Current build version of the compiler/VM. Reads the same KINETIX_BUILD
/// env var the build scripts set (falls back to 37 for plain `cargo build`
/// invocations that don't set it) -- previously this was a second, separate
/// hardcoded number that silently drifted out of sync with KINETIX_BUILD.
pub const CURRENT_BUILD: i64 = match option_env!("KINETIX_BUILD") {
    Some(s) => parse_build_number(s),
    None => 37,
};

const fn parse_build_number(s: &str) -> i64 {
    let bytes = s.as_bytes();
    let mut result: i64 = 0;
    let mut i = 0;
    while i < bytes.len() {
        result = result * 10 + (bytes[i] - b'0') as i64;
        i += 1;
    }
    result
}

#[derive(Debug, Clone, Copy)]
struct LocalInfo {
    reg: u16,
    moved: bool,
}

/// Scope for tracking local variable slots.
#[derive(Debug)]
struct Scope {
    locals: HashMap<String, LocalInfo>,
    next_register: u16,
}

impl Scope {
    fn new(start_register: u16) -> Self {
        Self {
            locals: HashMap::new(),
            next_register: start_register,
        }
    }

    fn define(&mut self, name: &str) -> u16 {
        let reg = self.next_register;
        self.locals.insert(name.to_string(), LocalInfo { reg, moved: false });
        self.next_register += 1;
        reg
    }
}

/// Tracks the backpatch targets for `break`/`continue` inside one loop, so
/// nested loops each patch their own jumps to their own exit/continue point.
struct LoopContext {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
}

/// The main compiler struct.
pub struct Compiler {
    pub program: CompiledProgram,
    scopes: Vec<Scope>,
    next_temp: u16,
    max_temp: u16,
    /// Current source line number being compiled (for line_map).
    pub current_line: u32,
    /// Names of no-payload enum variants (`Red` in `enum Color { Red, ... }`,
    /// or the built-in `None`), collected by a pre-scan in `compile()`. A bare
    /// identifier match-arm pattern is ambiguous at the AST level (`None` vs a
    /// catch-all binding `x`) -- this set disambiguates it, mirroring the fix
    /// applied to the same ambiguity in `hir.rs` (`SymbolTable::is_nullary_variant`).
    known_nullary_variants: std::collections::HashSet<String>,
    /// Stack of enclosing loops, innermost last, so `break`/`continue` patch
    /// against the nearest loop only.
    loop_stack: Vec<LoopContext>,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            program: CompiledProgram::new(),
            scopes: vec![Scope::new(0)],
            next_temp: 0,
            max_temp: 0,
            current_line: 1,
            known_nullary_variants: std::collections::HashSet::new(),
            loop_stack: vec![],
        }
    }

    pub fn compile(
        &mut self,
        statements: &[Statement<'_>],
        reactive_graph: Option<crate::ir::CompiledReactiveGraph>,
    ) -> Result<&CompiledProgram, String> {
        if let Some(rg) = reactive_graph {
            self.program.reactive_graph = rg;
        }

        // Pre-scan: collect nullary variant names before compiling anything,
        // so a match expression can classify a bare-identifier pattern
        // correctly regardless of where its enum is declared in the file.
        self.known_nullary_variants.insert("None".to_string());
        for stmt in statements {
            if let Statement::Enum { variants, .. } = stmt {
                for (vname, payload) in variants {
                    if payload.is_none() {
                        self.known_nullary_variants.insert(vname.clone());
                    }
                }
            }
        }

        // Built-in Option/Result constructors: always available at runtime,
        // regardless of whether the user also declares `enum Option<T> {...}`
        // themselves (symbol.rs pre-registers Some/None/Ok/Err unconditionally
        // too). A user's own enum declaration, if present, simply re-emits
        // equivalent globals later at its point in program order.
        let none_reg = self.emit_nullary_variant_value("Option", "None");
        let none_const = self.current_fn().add_constant(Constant::String("None".to_string()));
        self.emit_instr(Instruction::ab(Opcode::SetGlobal, none_const, none_reg));
        self.compile_variant_constructor("Option", "Some");
        self.compile_variant_constructor("Result", "Ok");
        self.compile_variant_constructor("Result", "Err");

        for stmt in statements {
            self.compile_statement(stmt)?;
            if let Some(scope) = self.scopes.last() {
                self.next_temp = scope.next_register;
            }
        }
        self.program.main.locals = self.max_temp;
        self.emit_instr(Instruction::a_only(Opcode::Halt, 0));

        // Static VTable Build Post-Monomorphization equivalent for AST pipeline
        self.program.vtable = crate::vtable::build_vtable(&self.program);

        Ok(&self.program)
    }

    /// Builds a tagged enum instance (`Value::Map` with `__enum__`/`__variant__`/
    /// `__payload__` keys, mirroring the `__class__` convention for class
    /// instances) for a no-payload variant, in the *current* function. Returns
    /// the register holding it.
    fn emit_nullary_variant_value(&mut self, enum_name: &str, variant_name: &str) -> u16 {
        let reg = self.alloc_register();
        self.emit_instr(Instruction::ab(Opcode::MakeMap, reg, 0));

        let enum_key = self.current_fn().add_constant(Constant::String("__enum__".to_string()));
        let enum_val = self.current_fn().add_constant(Constant::String(enum_name.to_string()));
        let enum_val_reg = self.alloc_register();
        self.emit_instr(Instruction::ab(Opcode::LoadConst, enum_val_reg, enum_val));
        self.emit_instr(Instruction::new(Opcode::SetMember, reg, enum_key, enum_val_reg));

        let variant_key = self.current_fn().add_constant(Constant::String("__variant__".to_string()));
        let variant_val = self.current_fn().add_constant(Constant::String(variant_name.to_string()));
        let variant_val_reg = self.alloc_register();
        self.emit_instr(Instruction::ab(Opcode::LoadConst, variant_val_reg, variant_val));
        self.emit_instr(Instruction::new(Opcode::SetMember, reg, variant_key, variant_val_reg));

        let payload_key = self.current_fn().add_constant(Constant::String("__payload__".to_string()));
        let null_reg = self.alloc_register();
        self.emit_instr(Instruction::a_only(Opcode::LoadNull, null_reg));
        self.emit_instr(Instruction::new(Opcode::SetMember, reg, payload_key, null_reg));

        reg
    }

    /// Compiles a synthetic 1-arity function `EnumName::VariantName` that
    /// builds a tagged enum instance around its single argument (the payload),
    /// and registers it as a global under `variant_name` -- the same "flattened
    /// name as global" convention `compile_function` uses for class methods.
    fn compile_variant_constructor(&mut self, enum_name: &str, variant_name: &str) {
        let func_name = format!("{}::{}", enum_name, variant_name);
        let mut func = CompiledFunction::new(func_name, 1);
        func.param_names = vec!["payload".to_string()];

        let saved_main = std::mem::replace(&mut self.program.main, func);
        let saved_temp = self.next_temp;
        let saved_max = self.max_temp;
        self.next_temp = 0;
        self.max_temp = 0;

        self.scopes.push(Scope::new(0));
        self.current_scope_mut().define("payload"); // register 0
        self.next_temp = 1;
        self.max_temp = 1;

        let map_reg = self.emit_nullary_variant_value(enum_name, variant_name);
        let payload_key = self.current_fn().add_constant(Constant::String("__payload__".to_string()));
        self.emit_instr(Instruction::new(Opcode::SetMember, map_reg, payload_key, 0));
        self.emit_instr(Instruction::a_only(Opcode::Return, map_reg));
        self.scopes.pop();

        let mut compiled_func = std::mem::replace(&mut self.program.main, saved_main);
        compiled_func.locals = self.max_temp;
        self.next_temp = saved_temp;
        self.max_temp = saved_max;

        let func_idx = self.program.functions.len();
        self.program.functions.push(compiled_func);

        let name_const = self.current_fn().add_constant(Constant::String(variant_name.to_string()));
        let reg = self.alloc_register();
        let idx_const = self.current_fn().add_constant(Constant::Function(func_idx));
        self.emit_instr(Instruction::ab(Opcode::LoadConst, reg, idx_const));
        self.emit_instr(Instruction::ab(Opcode::SetGlobal, name_const, reg));
    }

    fn current_fn(&mut self) -> &mut CompiledFunction {
        &mut self.program.main
    }

    /// Emit an instruction and record the current source line in line_map.
    fn emit_instr(&mut self, instr: Instruction) -> usize {
        let line = self.current_line;
        let func = &mut self.program.main;
        let idx = func.instructions.len();
        func.instructions.push(instr);
        func.line_map.push(line);
        idx
    }

    #[allow(dead_code)]
    fn current_scope(&self) -> &Scope {
        self.scopes.last().expect("no scope")
    }

    fn current_scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().expect("no scope")
    }

    fn alloc_register(&mut self) -> u16 {
        let r = self.next_temp;
        self.next_temp += 1;
        if self.next_temp > self.max_temp {
            self.max_temp = self.next_temp;
        }
        r
    }

    fn resolve_use(&mut self, name: &str) -> Result<Option<u16>, String> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.locals.get_mut(name) {
                // The Borrow Checker (borrowck.rs) acts as the authority on ownership.
                return Ok(Some(info.reg));
            }
        }
        Ok(None)
    }

    fn resolve_assign(&mut self, name: &str) -> Option<u16> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.locals.get_mut(name) {
                info.moved = false; // Revitalize
                return Some(info.reg);
            }
        }
        None
    }

    /// Emits the store instructions to write `val_reg` into `target` (Identifier/
    /// MemberAccess/Index), plus the reactive State tick if `target` is a known
    /// State node. Shared by `Expression::Assign` and by mutating array method
    /// calls (`arr.push(x)`) that need to write their result back into `arr`.
    fn emit_assign_to_target(&mut self, target: &Expression<'_>, val_reg: u16) -> Result<(), String> {
        let mut target_name = None;

        match target {
            Expression::Identifier(name) => {
                target_name = Some(name.clone());
                if let Some(slot) = self.resolve_assign(name) {
                    self.emit_instr(Instruction::ab(Opcode::SetLocal, slot, val_reg));
                } else {
                    let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                    self.emit_instr(Instruction::ab(Opcode::SetGlobal, name_idx, val_reg));
                }
            }
            Expression::MemberAccess { object, member } => {
                let obj_reg = self.compile_expression(object)?;
                let member_idx = self.current_fn().add_constant(Constant::String(member.clone()));
                self.emit_instr(Instruction::new(Opcode::SetMember, obj_reg, member_idx, val_reg));
                self.writeback_global_root(object, obj_reg);
            }
            Expression::Index { left, index } => {
                let obj_reg = self.compile_expression(left)?;
                let idx_reg = self.compile_expression(index)?;
                self.emit_instr(Instruction::new(Opcode::SetIndex, obj_reg, idx_reg, val_reg));
                self.writeback_global_root(left, obj_reg);
            }
            _ => return Err("Invalid assignment target".into()),
        }

        // --- REACTIVE STATE TRACKING ---
        // If the user manually mutates a known State, tell the VM so it can tick
        if let Some(name) = target_name {
            if let Some(node) = self.program.reactive_graph.nodes.get(&name) {
                if matches!(node.kind, crate::ir::ReactiveNodeKind::State) {
                    let name_idx = self.current_fn().add_constant(Constant::String(name));
                    self.emit_instr(Instruction::ab(Opcode::UpdateState, name_idx, val_reg));
                }
            }
        }

        Ok(())
    }

    /// `SetMember`/`SetIndex` mutate the register holding the container in
    /// place. For a local, reading it via `Identifier` returns that same
    /// persistent register (see `resolve_use`), so the mutation is already
    /// visible. For a *global* (top-level `let`/`mut`), reading it emits a
    /// `GetGlobal` that copies the value into a fresh temporary -- mutating
    /// that copy is invisible until it's written back with `SetGlobal`. Only
    /// handles the case where `root` is a plain identifier; nested containers
    /// (`a.b.field = x`) are unaffected since they were already unreachable
    /// (no parser support for member-access assignment targets).
    fn writeback_global_root(&mut self, root: &Expression<'_>, reg: u16) {
        if let Expression::Identifier(name) = root {
            if self.resolve_use(name).ok().flatten().is_none() {
                let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                self.emit_instr(Instruction::ab(Opcode::SetGlobal, name_idx, reg));
            }
        }
    }

    /// Compiles `stmt` as a value-producing block: if its last statement is an
    /// expression-statement, that expression's value ends up in `result_reg`;
    /// otherwise `result_reg` stays `Null`. Used to give `if`/`match` a real
    /// value in expression position (`let x = if cond { 1 } else { 2 }`),
    /// which previously left `result_reg` uninitialized. Pushes/pops its own
    /// scope when `stmt` is a `Block`, so locals declared inside a branch
    /// don't leak into the enclosing scope.
    fn compile_block_as_value(&mut self, stmt: &Statement<'_>, result_reg: u16) -> Result<(), String> {
        self.emit_instr(Instruction::a_only(Opcode::LoadNull, result_reg));

        let write_last = |c: &mut Self, s: &Statement<'_>| -> Result<(), String> {
            if let Statement::Expression { expression, .. } = s {
                let val_reg = c.compile_expression(expression)?;
                if val_reg != result_reg {
                    c.emit_instr(Instruction::ab(Opcode::SetLocal, result_reg, val_reg));
                }
                Ok(())
            } else {
                c.compile_statement(s)
            }
        };

        if let Statement::Block { statements, .. } = stmt {
            self.scopes.push(Scope::new(self.next_temp));
            for (i, s) in statements.iter().enumerate() {
                if i + 1 == statements.len() {
                    write_last(self, s)?;
                } else {
                    self.compile_statement(s)?;
                    if let Some(scope) = self.scopes.last() {
                        self.next_temp = scope.next_register;
                    }
                }
            }
            self.scopes.pop();
        } else {
            write_last(self, stmt)?;
        }
        Ok(())
    }

    // ========== Statements ==========

    fn compile_statement(&mut self, stmt: &Statement<'_>) -> Result<(), String> {
        // Update current_line from the AST node
        match stmt {
            Statement::Let { line, .. } | Statement::Return { line, .. }
            | Statement::Expression { line, .. } | Statement::Block { line, .. }
            | Statement::Function { line, .. } | Statement::While { line, .. }
            | Statement::For { line, .. } | Statement::Include { line, .. }
            | Statement::Class { line, .. } | Statement::Struct { line, .. }
            | Statement::Enum { line, .. } | Statement::Trait { line, .. } | Statement::Impl { line, .. }
            | Statement::Break { line } | Statement::Continue { line }
            | Statement::Version { line, .. }
            | Statement::State { line, .. } | Statement::Computed { line, .. } | Statement::Effect { line, .. } => {
                self.current_line = *line as u32;
            }
        }
        match stmt {
            Statement::State { name, value, .. } => {
                let reg = self.compile_expression(value)?;
                let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                self.emit_instr(Instruction::ab(Opcode::SetState, name_idx, reg));
                
                if self.scopes.len() == 1 {
                    self.emit_instr(Instruction::ab(Opcode::SetGlobal, name_idx, reg));
                } else {
                    let slot = self.current_scope_mut().define(name);
                    if self.current_scope_mut().next_register > self.max_temp {
                        self.max_temp = self.current_scope_mut().next_register; 
                    }
                    if slot != reg {
                        self.emit_instr(Instruction::ab(Opcode::SetLocal, slot, reg));
                    }
                }
            }
            Statement::Computed { name, value, .. } => {
                let func_name = format!("$computed_{}", name);
                
                let saved_temp = self.next_temp;
                let saved_max = self.max_temp;
                let saved_main = std::mem::replace(&mut self.program.main, CompiledFunction::new(func_name.clone(), 0));
                self.next_temp = 0;
                self.max_temp = 0;
                self.scopes.push(Scope::new(0));
                
                let ret_reg = self.compile_expression(value)?;
                self.emit_instr(Instruction::a_only(Opcode::Return, ret_reg));
                
                self.scopes.pop();
                
                let mut compiled_func = std::mem::replace(&mut self.program.main, saved_main);
                compiled_func.locals = self.max_temp;
                self.next_temp = saved_temp;
                self.max_temp = saved_max;
                
                let func_idx = self.program.functions.len();
                self.program.functions.push(compiled_func);
                
                let closure_reg = self.alloc_register();
                let idx_const = self.current_fn().add_constant(Constant::Function(func_idx));
                self.emit_instr(Instruction::ab(Opcode::LoadConst, closure_reg, idx_const));
                self.emit_instr(Instruction::ab(Opcode::MakeClosure, closure_reg, 0));
                
                let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                self.emit_instr(Instruction::ab(Opcode::InitComputed, name_idx, closure_reg));
                
                if self.scopes.len() == 1 {
                    self.emit_instr(Instruction::ab(Opcode::SetGlobal, name_idx, closure_reg));
                } else {
                    let slot = self.current_scope_mut().define(name);
                    if self.current_scope_mut().next_register > self.max_temp {
                        self.max_temp = self.current_scope_mut().next_register; 
                    }
                    if slot != closure_reg {
                        self.emit_instr(Instruction::ab(Opcode::SetLocal, slot, closure_reg));
                    }
                }
            }
            Statement::Effect { dependencies, body, .. } => {
                let func_name = format!("$effect_{}", self.program.functions.len());
                
                let saved_temp = self.next_temp;
                let saved_max = self.max_temp;
                let saved_main = std::mem::replace(&mut self.program.main, CompiledFunction::new(func_name, 0));
                self.next_temp = 0;
                self.max_temp = 0;
                self.scopes.push(Scope::new(0));
                
                if let Statement::Block { statements, .. } = body {
                    for s in statements {
                        self.compile_statement(s)?;
                        if let Some(scope) = self.scopes.last() {
                            self.next_temp = scope.next_register;
                        }
                    }
                } else {
                    self.compile_statement(body)?;
                }
                
                self.emit_instr(Instruction::a_only(Opcode::ReturnVoid, 0));
                self.scopes.pop();
                
                let mut compiled_func = std::mem::replace(&mut self.program.main, saved_main);
                compiled_func.locals = self.max_temp;
                self.next_temp = saved_temp;
                self.max_temp = saved_max;
                
                let func_idx = self.program.functions.len();
                self.program.functions.push(compiled_func);
                
                let closure_reg = self.alloc_register();
                let idx_const = self.current_fn().add_constant(Constant::Function(func_idx));
                self.emit_instr(Instruction::ab(Opcode::LoadConst, closure_reg, idx_const));
                self.emit_instr(Instruction::ab(Opcode::MakeClosure, closure_reg, 0));
                
                let deps_reg = if dependencies.is_empty() {
                    let r = self.alloc_register();
                    self.emit_instr(Instruction::a_only(Opcode::LoadNull, r));
                    r
                } else {
                    let base_reg = self.next_temp;
                    for dep in dependencies {
                        let r = self.alloc_register();
                        let idx = self.current_fn().add_constant(Constant::String(dep.clone()));
                        self.emit_instr(Instruction::ab(Opcode::LoadConst, r, idx));
                    }
                    let arr_reg = self.alloc_register();
                    self.emit_instr(Instruction::ab(Opcode::MakeArray, base_reg, dependencies.len() as u16));
                    // Reset temp registers used for dependency strings
                    self.next_temp = base_reg + 1;
                    arr_reg
                };
                
                self.emit_instr(Instruction::ab(Opcode::InitEffect, deps_reg, closure_reg));
            }
            Statement::Let { name, value, mutable: _, type_hint: _, .. } => {
                let reg = self.compile_expression(value)?;
                if self.scopes.len() == 1 {
                    // Global scope -> SetGlobal
                    let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                    self.emit_instr(Instruction::ab(Opcode::SetGlobal, name_idx, reg));
                } else {
                    // Local scope
                    let slot = self.current_scope_mut().define(name);
                    if self.current_scope_mut().next_register > self.max_temp {
                        self.max_temp = self.current_scope_mut().next_register; 
                    }
                    if slot != reg {
                        self.emit_instr(Instruction::ab(Opcode::SetLocal, slot, reg));
                    }
                }
            }
            Statement::Function { name, parameters, body, return_type: _, .. } => {
                self.compile_function(name, parameters, body)?;
            }
            Statement::Return { value, .. } => {
                if let Some(val) = value {
                    // TCO: if the return value is a function call, emit TailCall instead
                    if let Expression::Call { function, arguments } = val {
                        // Compile the function reference
                        let func_reg = self.compile_expression(function)?;
                        let call_reg = self.alloc_register();
                        self.emit_instr(Instruction::ab(Opcode::SetLocal, call_reg, func_reg));
                        for (i, arg) in arguments.iter().enumerate() {
                            let expected_reg = call_reg + 1 + i as u16;
                            let arg_reg = self.compile_expression(arg)?;
                            if arg_reg != expected_reg {
                                while self.next_temp <= expected_reg {
                                    self.alloc_register();
                                }
                                self.emit_instr(Instruction::ab(Opcode::SetLocal, expected_reg, arg_reg));
                            }
                        }
                        self.emit_instr(Instruction::ab(Opcode::TailCall, call_reg, arguments.len() as u16));
                    } else {
                        let reg = self.compile_expression(val)?;
                        self.emit_instr(Instruction::a_only(Opcode::Return, reg));
                    }
                } else {
                    self.emit_instr(Instruction::a_only(Opcode::ReturnVoid, 0));
                }
            }
            Statement::Expression { expression, .. } => {
                self.compile_expression(expression)?;
            }
            Statement::Block { statements, .. } => {
                self.scopes.push(Scope::new(self.next_temp));
                for s in statements {
                    self.compile_statement(s)?;
                    if let Some(scope) = self.scopes.last() {
                        self.next_temp = scope.next_register;
                    }
                }
                self.scopes.pop();
            }
            Statement::While { condition, body, .. } => {
                self.compile_while(condition, body)?;
            }
            Statement::For { iterator, range, body, .. } => {
                self.compile_for(iterator, range, body)?;
            }
            Statement::Include { .. } => {
                // Includes resolved at higher level
            }
            Statement::Class { name: class_name, methods, .. } => {
                for method in methods {
                    if let Statement::Function { name: method_name, parameters, body, .. } = method {
                        let flat_name = format!("{}::{}", class_name, method_name);
                        self.compile_function(&flat_name, parameters, body)?;
                    }
                }
            }
            Statement::Enum { name, variants, .. } => {
                // Each variant becomes a global: a no-payload variant (`Red`)
                // is a plain tagged value; a payload variant (`Circle(f)`) is
                // a 1-arity constructor function -- see
                // `emit_nullary_variant_value`/`compile_variant_constructor`.
                for (variant_name, payload) in variants {
                    if payload.is_some() {
                        self.compile_variant_constructor(name, variant_name);
                    } else {
                        let reg = self.emit_nullary_variant_value(name, variant_name);
                        let name_const = self.current_fn().add_constant(Constant::String(variant_name.clone()));
                        self.emit_instr(Instruction::ab(Opcode::SetGlobal, name_const, reg));
                    }
                }
            }
            Statement::Impl { target_name, methods, .. } => {
                // Same flattened-global-per-method convention as `Statement::Class`,
                // so `LoadMethod`'s existing dispatch (vtable, falling back to a
                // `"Target::method"` global lookup) picks these up for any
                // instance whose `__class__` matches `target_name`. `trait_name`/
                // generics are compile-time-only (already checked by
                // `trait_solver.rs`), no codegen needed for them.
                for method in methods {
                    if let Statement::Function { name: method_name, parameters, body, .. } = method {
                        let flat_name = format!("{}::{}", target_name, method_name);
                        self.compile_function(&flat_name, parameters, body)?;
                    }
                }
            }
            // `struct`/`trait` declarations have no runtime footprint of their
            // own: a struct's instantiation is `Expression::StructLiteral`
            // (compiled independently of how/whether the type was declared),
            // and a trait is a pure compile-time interface (method signatures
            // only, no bodies -- those live in `impl` blocks, above).
            Statement::Struct { .. } | Statement::Trait { .. } => {}
            Statement::Break { .. } => {
                let jump_idx = self.emit_instr(Instruction::a_only(Opcode::Jump, 0));
                match self.loop_stack.last_mut() {
                    Some(ctx) => ctx.break_jumps.push(jump_idx),
                    None => return Err("'break' used outside of a loop".to_string()),
                }
            }
            Statement::Continue { .. } => {
                let jump_idx = self.emit_instr(Instruction::a_only(Opcode::Jump, 0));
                match self.loop_stack.last_mut() {
                    Some(ctx) => ctx.continue_jumps.push(jump_idx),
                    None => return Err("'continue' used outside of a loop".to_string()),
                }
            }
            Statement::Version { build, .. } => {
                if *build > CURRENT_BUILD {
                    eprintln!("Warning: Script requires build {}, but you are running build {}. Some features may not work.", build, CURRENT_BUILD);
                }
            }

        }
        Ok(())
    }

    fn compile_function(
        &mut self,
        name: &str,
        parameters: &[(String, String)],
        body: &Statement<'_>,
    ) -> Result<(), String> {
        let mut func = CompiledFunction::new(name.to_string(), parameters.len() as u16);
        func.param_names = parameters.iter().map(|(n, _)| n.clone()).collect();

        // Save state
        let saved_main = std::mem::replace(&mut self.program.main, func);
        let saved_temp = self.next_temp;
        let saved_max = self.max_temp;
        self.next_temp = 0;
        self.max_temp = 0;

        // Parameters occupy registers 0..arity
        self.scopes.push(Scope::new(0));
        for (pname, _) in parameters {
            self.current_scope_mut().define(pname);
            self.next_temp += 1;
        }
        if self.next_temp > self.max_temp { self.max_temp = self.next_temp; }

        // Compile body
        if let Statement::Block { statements, .. } = body {
            for s in statements {
                self.compile_statement(s)?;
                if let Some(scope) = self.scopes.last() {
                    self.next_temp = scope.next_register;
                }
            }
        }

        // Implicit return void
        self.emit_instr(Instruction::a_only(Opcode::ReturnVoid, 0));
        self.scopes.pop();

        // Restore state
        let mut compiled_func = std::mem::replace(&mut self.program.main, saved_main);
        compiled_func.locals = self.max_temp;
        
        self.next_temp = saved_temp;
        self.max_temp = saved_max;

        let func_idx = self.program.functions.len();
        self.program.functions.push(compiled_func);

        // Store reference as global
        let name_const = self.current_fn().add_constant(Constant::String(name.to_string()));
        let reg = self.alloc_register();
        let idx_const = self.current_fn().add_constant(Constant::Function(func_idx));
        self.emit_instr(Instruction::ab(Opcode::LoadConst, reg, idx_const));
        self.emit_instr(Instruction::ab(Opcode::SetGlobal, name_const, reg));

        Ok(())
    }

    fn compile_while(&mut self, condition: &Expression<'_>, body: &Statement<'_>) -> Result<(), String> {
        let loop_start = self.current_fn().instructions.len();
        let cond_reg = self.compile_expression(condition)?;
        let jump_idx = self.emit_instr(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));

        self.loop_stack.push(LoopContext { break_jumps: vec![], continue_jumps: vec![] });
        if let Statement::Block { statements, .. } = body {
            for s in statements {
                self.compile_statement(s)?;
                if let Some(scope) = self.scopes.last() {
                    self.next_temp = scope.next_register;
                }
            }
        }
        let ctx = self.loop_stack.pop().expect("pushed above");
        for idx in &ctx.continue_jumps {
            self.current_fn().instructions[*idx].a = loop_start as u16;
        }

        self.emit_instr(Instruction::a_only(Opcode::Jump, loop_start as u16));
        let exit_pos = self.current_fn().instructions.len();
        self.current_fn().instructions[jump_idx].a = exit_pos as u16;
        for idx in &ctx.break_jumps {
            self.current_fn().instructions[*idx].a = exit_pos as u16;
        }

        Ok(())
    }

    fn compile_for(&mut self, variable: &str, iterable: &Expression<'_>, body: &Statement<'_>) -> Result<(), String> {
        let iter_reg = self.compile_expression(iterable)?;

        // Compute the length once via the `len` builtin instead of relying on
        // the fetched element's own truthiness to detect loop end -- that
        // treated any falsy element (0, false, "", null) as a spurious "end
        // of array", silently truncating the iteration.
        let len_name_idx = self.current_fn().add_constant(Constant::String("len".to_string()));
        let len_func_reg = self.alloc_register();
        self.emit_instr(Instruction::ab(Opcode::GetGlobal, len_func_reg, len_name_idx));
        let call_reg = self.alloc_register();
        self.emit_instr(Instruction::ab(Opcode::SetLocal, call_reg, len_func_reg));
        let expected_arg_reg = call_reg + 1;
        if iter_reg != expected_arg_reg {
            while self.next_temp <= expected_arg_reg {
                self.alloc_register();
            }
            self.emit_instr(Instruction::ab(Opcode::SetLocal, expected_arg_reg, iter_reg));
        }
        self.emit_instr(Instruction::ab(Opcode::Call, call_reg, 1));
        let len_reg = call_reg;

        let idx_reg = self.alloc_register();
        let zero_const = self.current_fn().add_constant(Constant::Integer(0));
        self.emit_instr(Instruction::ab(Opcode::LoadConst, idx_reg, zero_const));

        // The loop variable is a scope-tracked local (so the body can reference
        // it by name), but `Scope::define` hands out registers from its own
        // counter (`next_register`), independent of `next_temp` (the temp
        // counter used by `alloc_register` above) -- without resyncing first,
        // `var_reg` could alias `iter_reg`/`len_func_reg`/`idx_reg`. This was a
        // latent bug masked until now by `GetIndex` being unimplemented, so no
        // `for` loop ever ran far enough to hit the corruption.
        if self.next_temp > self.current_scope_mut().next_register {
            self.current_scope_mut().next_register = self.next_temp;
        }
        // `define` inserts into the enclosing scope's locals map (compile_for
        // never pushes its own Scope), so without saving/restoring whatever
        // was bound to `variable` before the loop, the loop variable would
        // permanently shadow it afterwards -- e.g. `for i in 0..5 {}` followed
        // later by `mut i = 0` would keep resolving `i` to the loop's stale
        // register instead of the new global.
        let previous_binding = self.current_scope_mut().locals.get(variable).copied();
        let var_reg = self.current_scope_mut().define(variable);
        self.next_temp = self.current_scope_mut().next_register;
        if self.next_temp > self.max_temp {
            self.max_temp = self.next_temp;
        }

        let loop_start = self.current_fn().instructions.len();
        let cond_reg = self.alloc_register();
        self.emit_instr(Instruction::new(Opcode::Lt, cond_reg, idx_reg, len_reg));
        let jump_idx = self.emit_instr(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));
        self.emit_instr(Instruction::new(Opcode::GetIndex, var_reg, iter_reg, idx_reg));

        self.loop_stack.push(LoopContext { break_jumps: vec![], continue_jumps: vec![] });
        if let Statement::Block { statements, .. } = body {
            for s in statements {
                self.compile_statement(s)?;
            }
        }
        let ctx = self.loop_stack.pop().expect("pushed above");

        match previous_binding {
            Some(info) => { self.current_scope_mut().locals.insert(variable.to_string(), info); }
            None => { self.current_scope_mut().locals.remove(variable); }
        }

        // `continue` must land here so the index still gets incremented --
        // jumping straight to loop_start would re-check the same index forever.
        let increment_start = self.current_fn().instructions.len();
        for idx in &ctx.continue_jumps {
            self.current_fn().instructions[*idx].a = increment_start as u16;
        }

        let one_const = self.current_fn().add_constant(Constant::Integer(1));
        let one_reg = self.alloc_register();
        self.emit_instr(Instruction::ab(Opcode::LoadConst, one_reg, one_const));
        self.emit_instr(Instruction::new(Opcode::Add, idx_reg, idx_reg, one_reg));
        self.emit_instr(Instruction::a_only(Opcode::Jump, loop_start as u16));

        let exit_pos = self.current_fn().instructions.len();
        self.current_fn().instructions[jump_idx].a = exit_pos as u16;
        for idx in &ctx.break_jumps {
            self.current_fn().instructions[*idx].a = exit_pos as u16;
        }

        Ok(())
    }

    // ========== Expressions ==========

    fn compile_expression(&mut self, expr: &Expression<'_>) -> Result<u16, String> {
        match expr {
            Expression::Integer(val) => {
                let reg = self.alloc_register();
                let idx = self.current_fn().add_constant(Constant::Integer(*val));
                self.emit_instr(Instruction::ab(Opcode::LoadConst, reg, idx));
                Ok(reg)
            }
            Expression::Try { value } => self.compile_expression(value), // TEMPORARY stub
            Expression::Float(val) => {
                let reg = self.alloc_register();
                let idx = self.current_fn().add_constant(Constant::Float(*val));
                self.emit_instr(Instruction::ab(Opcode::LoadConst, reg, idx));
                Ok(reg)
            }
            Expression::String(val) => {
                let reg = self.alloc_register();
                let idx = self.current_fn().add_constant(Constant::String(val.clone()));
                self.emit_instr(Instruction::ab(Opcode::LoadConst, reg, idx));
                Ok(reg)
            }
            Expression::Boolean(val) => {
                let reg = self.alloc_register();
                let opcode = if *val { Opcode::LoadTrue } else { Opcode::LoadFalse };
                self.emit_instr(Instruction::a_only(opcode, reg));
                Ok(reg)
            }
            Expression::StructLiteral { name, fields, .. } => {
                let obj_reg = self.alloc_register();
                self.emit_instr(Instruction::ab(Opcode::MakeMap, obj_reg, 0));
                
                // Add __class__ hidden field
                let class_key_idx = self.current_fn().add_constant(Constant::String("__class__".to_string()));
                let class_val_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                let class_val_reg = self.alloc_register();
                self.emit_instr(Instruction::ab(Opcode::LoadConst, class_val_reg, class_val_idx));
                self.emit_instr(Instruction::new(Opcode::SetMember, obj_reg, class_key_idx, class_val_reg));

                for (fname, expr) in fields {
                    let val_reg = self.compile_expression(expr)?;
                    let name_idx = self.current_fn().add_constant(Constant::String(fname.clone()));
                    self.emit_instr(Instruction::new(Opcode::SetMember, obj_reg, name_idx, val_reg));
                }
                Ok(obj_reg)
            }
            Expression::Null => {
                let reg = self.alloc_register();
                self.emit_instr(Instruction::a_only(Opcode::LoadNull, reg));
                Ok(reg)
            }
            Expression::Identifier(name) => {
                if let Some(reg) = self.resolve_use(name)? {
                    return Ok(reg);
                }
                // Global lookup (Globals are strict-const or unsafe-shared, we allow access)
                let reg = self.alloc_register();
                let name_idx = self.current_fn().add_constant(Constant::String(name.clone()));
                self.emit_instr(Instruction::ab(Opcode::GetGlobal, reg, name_idx));
                Ok(reg)
            }
            Expression::Prefix { operator, right } => {
                let right_reg = self.compile_expression(right)?;
                if operator == "&" || operator == "&mut" {
                    // This VM has no pointer/aliasing value: locals ARE registers and
                    // calls clone arguments into the callee's frame, so a reference is
                    // erased to the same register as its operand. `&mut` additionally
                    // marks the argument, at its call site, for write-back of the
                    // call's return value (see Expression::Call below) -- the only
                    // channel by which a callee can appear to mutate it.
                    return Ok(right_reg);
                }
                let result = self.alloc_register();
                let opcode = match operator.as_str() {
                    "-" => Opcode::Neg,
                    "!" => Opcode::Not,
                    _ => return Err(format!("Unknown prefix operator: {}", operator)),
                };
                self.emit_instr(Instruction::ab(opcode, result, right_reg));
                Ok(result)
            }
            Expression::Infix { left, operator, right } => {
                let left_reg = self.compile_expression(left)?;
                let right_reg = self.compile_expression(right)?;
                let result = self.alloc_register();
                let opcode = match operator.as_str() {
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
                    _ => return Err(format!("Unknown infix operator: {}", operator)),
                };
                self.emit_instr(Instruction::new(opcode, result, left_reg, right_reg));
                Ok(result)
            }
            Expression::Assign { target, value } => {
                let val_reg = self.compile_expression(value)?;
                self.emit_assign_to_target(target, val_reg)?;
                Ok(val_reg)
            }
            Expression::Call { function, arguments } => {

                // Helper: flatten multi-level MemberAccess into a dot-separated string
                fn stringify_member_access(expr: &Expression) -> Option<String> {
                    match expr {
                        Expression::Identifier(name) => Some(name.clone()),
                        Expression::MemberAccess { object, member } => {
                            let parent = stringify_member_access(object)?;
                            Some(format!("{}.{}", parent, member))
                        }
                        _ => None,
                    }
                }

                // Module builtins vs Method calling on Instance
                if let Expression::MemberAccess { object, member } = *function {
                    // First, check for multi-level builtin calls like system.os.isWindows()
                    let full_path = stringify_member_access(function);
                    let is_multilevel_builtin = full_path.as_ref().map_or(false, |p| {
                        p.starts_with("system.") || p.starts_with("Math.") || p.starts_with("math.")
                    });

                    if is_multilevel_builtin {
                        let flat_name = full_path.unwrap();
                        let call_reg = self.alloc_register();
                        let name_idx = self.current_fn().add_constant(Constant::String(flat_name));
                        self.emit_instr(Instruction::ab(Opcode::GetGlobal, call_reg, name_idx));
                        for (i, arg) in arguments.iter().enumerate() {
                            let expected_reg = call_reg + 1 + i as u16;
                            let arg_reg = self.compile_expression(arg)?;
                            if arg_reg != expected_reg {
                                while self.next_temp <= expected_reg {
                                    self.alloc_register();
                                }
                                self.emit_instr(Instruction::ab(Opcode::SetLocal, expected_reg, arg_reg));
                            }
                        }
                        self.emit_instr(Instruction::ab(Opcode::Call, call_reg, arguments.len() as u16));
                        return Ok(call_reg);
                    }

                    let mut is_local_obj = false;
                    if let Expression::Identifier(name) = &**object {
                        let is_capitalized = name.chars().next().unwrap_or('a').is_uppercase();
                        if self.resolve_use(name)?.is_some() || self.program.reactive_graph.nodes.contains_key(name) {
                            is_local_obj = true;
                        } else if !is_capitalized {
                            // Se è minuscolo ed è globale (es. 'let p = Point...; p.greet()'), NON è un module
                            is_local_obj = true;
                        }
                    } else {
                        is_local_obj = true; // e.g. get_obj().method()
                    }

                    if !is_local_obj {
                        let Expression::Identifier(module_name) = &**object else { unreachable!() };
                        let mut chars = module_name.chars();
                        let cap_module = match chars.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                        };
                        let flat_name = format!("{}.{}", cap_module, member);
                        let call_reg = self.alloc_register();
                        let name_idx = self.current_fn().add_constant(Constant::String(flat_name));
                        self.emit_instr(Instruction::ab(Opcode::LoadConst, call_reg, name_idx));
                        for (i, arg) in arguments.iter().enumerate() {
                            let expected_reg = call_reg + 1 + i as u16;
                            let arg_reg = self.compile_expression(arg)?;
                            if arg_reg != expected_reg {
                                while self.next_temp <= expected_reg {
                                    self.alloc_register();
                                }
                                self.emit_instr(Instruction::ab(Opcode::SetLocal, expected_reg, arg_reg));
                            }
                        }
                        self.emit_instr(Instruction::ab(Opcode::Call, call_reg, arguments.len() as u16));
                        return Ok(call_reg);
                    } else {
                        // OOP Method Call
                        let obj_reg = self.compile_expression(object)?;
                        let method_idx = self.current_fn().add_constant(Constant::String(member.clone()));
                        let call_reg = self.alloc_register();
                        self.emit_instr(Instruction::new(Opcode::LoadMethod, call_reg, obj_reg, method_idx));
                        
                        for (i, arg) in arguments.iter().enumerate() {
                            let expected_reg = call_reg + 1 + i as u16;
                            let arg_reg = self.compile_expression(arg)?;
                            if arg_reg != expected_reg {
                                while self.next_temp <= expected_reg {
                                    self.alloc_register();
                                }
                                self.emit_instr(Instruction::ab(Opcode::SetLocal, expected_reg, arg_reg));
                            }
                        }
                        self.emit_instr(Instruction::ab(Opcode::Call, call_reg, arguments.len() as u16));

                        // Array mutating methods (push/pop/remove_at/insert/reverse/sort) are
                        // implemented natively as functional (return a new array) -- write the
                        // result back into the receiver so the call also mutates it in place.
                        if matches!(member.as_ref(), "push" | "pop" | "remove_at" | "insert" | "reverse" | "sort") {
                            self.emit_assign_to_target(object, call_reg)?;
                        }

                        return Ok(call_reg);
                    }
                }

                let orig_func_reg = self.compile_expression(function)?;
                let call_reg = self.alloc_register();
                self.emit_instr(Instruction::ab(Opcode::SetLocal, call_reg, orig_func_reg));
                let mut mut_ref_target: Option<&Expression<'_>> = None;
                for (i, arg) in arguments.iter().enumerate() {
                    let expected_reg = call_reg + 1 + i as u16;
                    let arg_reg = self.compile_expression(arg)?;
                    if arg_reg != expected_reg {
                        while self.next_temp <= expected_reg {
                            self.alloc_register();
                        }
                        self.emit_instr(Instruction::ab(Opcode::SetLocal, expected_reg, arg_reg));
                    }
                    if let Expression::Prefix { operator, right } = arg {
                        if operator == "&mut" {
                            if !matches!(**right, Expression::Identifier(_) | Expression::MemberAccess { .. } | Expression::Index { .. }) {
                                return Err("'&mut' requires a mutable place (a variable, field, or index expression)".to_string());
                            }
                            if mut_ref_target.is_some() {
                                return Err("at most one '&mut' argument is supported per call (the callee has a single return value to write back)".to_string());
                            }
                            mut_ref_target = Some(right);
                        }
                    }
                }
                self.emit_instr(Instruction::ab(Opcode::Call, call_reg, arguments.len() as u16));

                // `&mut x` at a call site is sugar for `x = f(...)`: this VM clones
                // arguments into the callee's frame (no aliasing), so the only way a
                // function can appear to mutate a by-reference argument is by
                // returning its new value, which is written back into the argument's
                // place here.
                if let Some(target) = mut_ref_target {
                    self.emit_assign_to_target(target, call_reg)?;
                }

                Ok(call_reg)
            }
            Expression::If { condition, consequence, alternative } => {
                let cond_reg = self.compile_expression(condition)?;
                let result_reg = self.alloc_register();
                let jump_else = self.emit_instr(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));

                self.compile_block_as_value(consequence, result_reg)?;

                if let Some(alt) = alternative {
                    let jump_end = self.emit_instr(Instruction::a_only(Opcode::Jump, 0));
                    let else_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_else].a = else_pos as u16;

                    self.compile_block_as_value(alt, result_reg)?;

                    let end_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_end].a = end_pos as u16;
                } else {
                    let end_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_else].a = end_pos as u16;
                }

                Ok(result_reg)
            }
            Expression::Index { left, index } => {
                let left_reg = self.compile_expression(left)?;
                let idx_reg = self.compile_expression(index)?;
                let result = self.alloc_register();
                self.emit_instr(Instruction::new(Opcode::GetIndex, result, left_reg, idx_reg));
                Ok(result)
            }
            Expression::MemberAccess { object, member } => {
                let obj_reg = self.compile_expression(object)?;
                let name_idx = self.current_fn().add_constant(Constant::String(member.clone()));
                let result = self.alloc_register();
                self.emit_instr(Instruction::new(Opcode::GetMember, result, obj_reg, name_idx));
                Ok(result)
            }
            Expression::ArrayLiteral(elements) => {
                let start_reg = self.next_temp;
                for elem in elements {
                    self.compile_expression(elem)?;
                }
                self.emit_instr(Instruction::ab(Opcode::MakeArray, start_reg, elements.len() as u16));
                Ok(start_reg)
            }
            Expression::FunctionLiteral { parameters, body, return_type: _ } => {
                let name = format!("<lambda_{}>", self.program.functions.len());
                self.compile_function(&name, parameters, body)?;
                let reg = self.alloc_register();
                let func_idx = self.program.functions.len() - 1;
                let idx = self.current_fn().add_constant(Constant::Function(func_idx));
                self.emit_instr(Instruction::ab(Opcode::LoadConst, reg, idx));
                Ok(reg)
            }
            Expression::Match { value, arms } => self.compile_match(value, arms),
            Expression::Range { start, end } => {
                let start_reg = self.compile_expression(start)?;
                let end_reg = self.compile_expression(end)?;
                let result = self.alloc_register();
                self.emit_instr(Instruction::new(Opcode::MakeRange, result, start_reg, end_reg));
                Ok(result)
            }
            Expression::MapLiteral(_) => {
                let reg = self.alloc_register();
                self.emit_instr(Instruction::a_only(Opcode::LoadNull, reg));
                Ok(reg)
            }
        }
    }

    /// Defines `name` as a new local bound to `value_reg` in the *current*
    /// scope, keeping `next_temp` in sync with the scope's own counter (see
    /// `compile_function`'s parameter registration for the same pattern) --
    /// `Scope::define` alone would silently let a later `alloc_register()`
    /// reuse the same slot, aliasing the binding.
    fn bind_local(&mut self, name: &str, value_reg: u16) {
        let slot = self.current_scope_mut().define(name);
        if self.next_temp <= slot {
            self.next_temp = slot + 1;
        }
        if self.next_temp > self.max_temp {
            self.max_temp = self.next_temp;
        }
        if slot != value_reg {
            self.emit_instr(Instruction::ab(Opcode::SetLocal, slot, value_reg));
        }
    }

    /// Real bytecode for `match` (Phase 2 ADTs: previously a `LoadNull` stub).
    /// Each arm's pattern is classified exactly as `hir.rs` classifies
    /// `HirPattern` (same nullary-variant-vs-binding disambiguation via
    /// `known_nullary_variants`), then compiled to a runtime test:
    /// - Wildcard/Binding: no test, always matches.
    /// - Literal: `Eq` against the compiled literal.
    /// - Variant: `Eq` on the tagged value's `__variant__` field, with an
    ///   optional payload binding read from `__payload__`.
    /// All arms write into the same `result_reg` (via `compile_block_as_value`)
    /// and jump to the end, giving `match` a real value in expression position.
    fn compile_match(&mut self, value: &Expression<'_>, arms: &[(Expression<'_>, &Statement<'_>)]) -> Result<u16, String> {
        let value_reg = self.compile_expression(value)?;
        let result_reg = self.alloc_register();
        self.emit_instr(Instruction::a_only(Opcode::LoadNull, result_reg));

        let mut end_jumps = Vec::new();

        for (pattern, body) in arms {
            use crate::pattern::ArmPattern;
            let classified = crate::pattern::classify_pattern(pattern, |n| self.known_nullary_variants.contains(n));

            match classified {
                ArmPattern::Wildcard => {
                    self.compile_block_as_value(body, result_reg)?;
                    end_jumps.push(self.emit_instr(Instruction::a_only(Opcode::Jump, 0)));
                }
                ArmPattern::Binding(name) => {
                    self.scopes.push(Scope::new(self.next_temp));
                    self.bind_local(&name, value_reg);
                    self.compile_block_as_value(body, result_reg)?;
                    self.scopes.pop();
                    end_jumps.push(self.emit_instr(Instruction::a_only(Opcode::Jump, 0)));
                }
                ArmPattern::Literal(lit_expr) => {
                    let lit_reg = self.compile_expression(lit_expr)?;
                    let cond_reg = self.alloc_register();
                    self.emit_instr(Instruction::new(Opcode::Eq, cond_reg, value_reg, lit_reg));
                    let jump_next = self.emit_instr(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));

                    self.compile_block_as_value(body, result_reg)?;
                    end_jumps.push(self.emit_instr(Instruction::a_only(Opcode::Jump, 0)));

                    let next_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_next].a = next_pos as u16;
                }
                ArmPattern::Variant { name, binding } => {
                    let variant_key = self.current_fn().add_constant(Constant::String("__variant__".to_string()));
                    let tag_reg = self.alloc_register();
                    self.emit_instr(Instruction::new(Opcode::GetMember, tag_reg, value_reg, variant_key));
                    let name_idx = self.current_fn().add_constant(Constant::String(name));
                    let name_reg = self.alloc_register();
                    self.emit_instr(Instruction::ab(Opcode::LoadConst, name_reg, name_idx));
                    let cond_reg = self.alloc_register();
                    self.emit_instr(Instruction::new(Opcode::Eq, cond_reg, tag_reg, name_reg));
                    let jump_next = self.emit_instr(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));

                    self.scopes.push(Scope::new(self.next_temp));
                    if let Some(bname) = binding {
                        let payload_key = self.current_fn().add_constant(Constant::String("__payload__".to_string()));
                        let payload_reg = self.alloc_register();
                        self.emit_instr(Instruction::new(Opcode::GetMember, payload_reg, value_reg, payload_key));
                        self.bind_local(&bname, payload_reg);
                    }
                    self.compile_block_as_value(body, result_reg)?;
                    self.scopes.pop();

                    end_jumps.push(self.emit_instr(Instruction::a_only(Opcode::Jump, 0)));
                    let next_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_next].a = next_pos as u16;
                }
            }
        }

        let end_pos = self.current_fn().instructions.len();
        for idx in end_jumps {
            self.current_fn().instructions[idx].a = end_pos as u16;
        }

        Ok(result_reg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    fn try_compile_source(source: &str) -> Result<CompiledProgram, String> {
        let arena = Bump::new();
        let lexer = kinetix_language::lexer::Lexer::new(source);
        let mut parser = kinetix_language::parser::Parser::new(lexer, &arena);
        let ast = parser.parse_program();
        assert!(parser.errors.is_empty(), "parse errors: {:?}", parser.errors);
        let mut compiler = Compiler::new();
        compiler.compile(&ast.statements, None).map(|p| p.clone())
    }

    fn compile_source(source: &str) -> CompiledProgram {
        try_compile_source(source).expect("compile should succeed")
    }

    #[test]
    fn test_range_compiles_to_make_range_not_load_null() {
        let program = compile_source("let r = 0..5;");
        assert!(
            program.main.instructions.iter().any(|i| i.opcode == Opcode::MakeRange),
            "Expression::Range should emit MakeRange, not fall through to the LoadNull stub"
        );
    }

    #[test]
    fn test_break_emits_jump_patched_to_loop_exit() {
        let program = compile_source("mut i = 0\nwhile i < 10 {\n    break\n}\n");
        let instrs = &program.main.instructions;
        let halt_idx = instrs.len() - 1;
        assert_eq!(instrs[halt_idx].opcode, Opcode::Halt);

        let jumps: Vec<&Instruction> = instrs.iter().filter(|i| i.opcode == Opcode::Jump).collect();
        assert_eq!(jumps.len(), 2, "expected the break's jump and the loop-back jump");
        assert_eq!(
            jumps[0].a as usize, halt_idx,
            "break should be patched to the loop's exit point, right before Halt"
        );
    }

    #[test]
    fn test_continue_in_for_loop_targets_increment_not_condition() {
        let program = compile_source("for x in [1, 2, 3] {\n    continue\n}\n");
        let instrs = &program.main.instructions;
        let jump_indices: Vec<usize> = instrs.iter().enumerate()
            .filter(|(_, i)| i.opcode == Opcode::Jump)
            .map(|(idx, _)| idx)
            .collect();
        assert_eq!(jump_indices.len(), 2, "expected the continue's jump and the loop-back jump");

        let continue_jump = &instrs[jump_indices[0]];
        let loop_back_jump = &instrs[jump_indices[1]];
        // continue must NOT target the condition check (that would skip the
        // index increment and loop forever on the same element).
        assert_ne!(
            continue_jump.a, loop_back_jump.a,
            "continue must not target the condition check (infinite loop bug)"
        );
        // continue's target should be the index increment, which sits strictly
        // between the body (where `continue` was emitted) and the unconditional
        // loop-back jump.
        assert!(
            continue_jump.a as usize > jump_indices[0] && continue_jump.a as usize <= jump_indices[1],
            "continue should target the increment step, not jump backward or past the loop-back jump"
        );
    }

    #[test]
    fn test_break_outside_loop_is_compile_error() {
        let result = try_compile_source("break\n");
        assert!(result.is_err(), "bare top-level `break` should be a compile error");
    }

    #[test]
    fn test_continue_outside_loop_is_compile_error() {
        let result = try_compile_source("continue\n");
        assert!(result.is_err(), "bare top-level `continue` should be a compile error");
    }
}
