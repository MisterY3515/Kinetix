/// KiComp Compiler: walks the AST and emits register-based bytecode.

use kinetix_language::ast::{Statement, Expression};
use crate::ir::*;
use std::collections::HashMap;

/// Current build version of the compiler/VM.
pub const CURRENT_BUILD: i64 = 23;

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

/// The main compiler struct.
pub struct Compiler {
    pub program: CompiledProgram,
    scopes: Vec<Scope>,
    next_temp: u16,
    max_temp: u16,
    /// Current source line number being compiled (for line_map).
    pub current_line: u32,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            program: CompiledProgram::new(),
            scopes: vec![Scope::new(0)],
            next_temp: 0,
            max_temp: 0,
            current_line: 1,
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

        for stmt in statements {
            self.compile_statement(stmt)?;
            if let Some(scope) = self.scopes.last() {
                self.next_temp = scope.next_register;
            }
        }
        self.program.main.locals = self.max_temp;
        self.emit_instr(Instruction::a_only(Opcode::Halt, 0));
        Ok(&self.program)
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
                        let mut new_params = vec![("self".to_string(), "Object".to_string())];
                        new_params.extend_from_slice(parameters); // copy the rest
                        let flat_name = format!("{}::{}", class_name, method_name);
                        self.compile_function(&flat_name, &new_params, body)?;
                    }
                }
            }
            Statement::Struct { .. } 
            | Statement::Enum { .. } | Statement::Trait { .. } | Statement::Impl { .. } => {
                // Deferred to M4 / Phase 2
            }
            Statement::Break { .. } | Statement::Continue { .. } => {
                // Handled by loop context (M4)
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
        self.current_scope_mut().define(name);
        if self.current_scope_mut().next_register > self.max_temp {
             self.max_temp = self.current_scope_mut().next_register; 
        }

        Ok(())
    }

    fn compile_while(&mut self, condition: &Expression<'_>, body: &Statement<'_>) -> Result<(), String> {
        let loop_start = self.current_fn().instructions.len();
        let cond_reg = self.compile_expression(condition)?;
        let jump_idx = self.emit_instr(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));

        if let Statement::Block { statements, .. } = body {
            for s in statements {
                self.compile_statement(s)?;
                if let Some(scope) = self.scopes.last() {
                    self.next_temp = scope.next_register;
                }
            }
        }

        self.emit_instr(Instruction::a_only(Opcode::Jump, loop_start as u16));
        let exit_pos = self.current_fn().instructions.len();
        self.current_fn().instructions[jump_idx].a = exit_pos as u16;

        Ok(())
    }

    fn compile_for(&mut self, variable: &str, iterable: &Expression<'_>, body: &Statement<'_>) -> Result<(), String> {
        let iter_reg = self.compile_expression(iterable)?;
        let idx_reg = self.alloc_register();
        let var_reg = self.current_scope_mut().define(variable);
        if self.current_scope_mut().next_register > self.max_temp { self.max_temp = self.current_scope_mut().next_register; }

        let zero_const = self.current_fn().add_constant(Constant::Integer(0));
        self.emit_instr(Instruction::ab(Opcode::LoadConst, idx_reg, zero_const));

        let loop_start = self.current_fn().instructions.len();
        self.emit_instr(Instruction::new(Opcode::GetIndex, var_reg, iter_reg, idx_reg));
        let jump_idx = self.emit_instr(Instruction::ab(Opcode::JumpIfFalse, 0, var_reg));

        if let Statement::Block { statements, .. } = body {
            for s in statements {
                self.compile_statement(s)?;
            }
        }

        let one_const = self.current_fn().add_constant(Constant::Integer(1));
        let one_reg = self.alloc_register();
        self.emit_instr(Instruction::ab(Opcode::LoadConst, one_reg, one_const));
        self.emit_instr(Instruction::new(Opcode::Add, idx_reg, idx_reg, one_reg));
        self.emit_instr(Instruction::a_only(Opcode::Jump, loop_start as u16));

        let exit_pos = self.current_fn().instructions.len();
        self.current_fn().instructions[jump_idx].a = exit_pos as u16;

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
                
                // Track if target is an Identifier (to check Reactive Graph)
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
                    }
                    Expression::Index { left, index } => {
                        let obj_reg = self.compile_expression(left)?;
                        let idx_reg = self.compile_expression(index)?;
                        self.emit_instr(Instruction::new(Opcode::SetIndex, obj_reg, idx_reg, val_reg));
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

                Ok(val_reg)
            }
            Expression::Call { function, arguments } => {
                // Intrinsic: print(x)
                if let Expression::Identifier(name) = *function {
                    if name == "print" && arguments.len() == 1 {
                        let arg_reg = self.compile_expression(&arguments[0])?;
                        self.emit_instr(Instruction::a_only(Opcode::Print, arg_reg));
                        let null_reg = self.alloc_register();
                        self.emit_instr(Instruction::a_only(Opcode::LoadNull, null_reg));
                        return Ok(null_reg);
                    }
                }

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
                        p.starts_with("system.os.")
                            || p.starts_with("system.info.")
                    });

                    if is_multilevel_builtin {
                        let flat_name = full_path.unwrap();
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
                        return Ok(call_reg);
                    }
                }

                let orig_func_reg = self.compile_expression(function)?;
                let call_reg = self.alloc_register();
                self.emit_instr(Instruction::ab(Opcode::SetLocal, call_reg, orig_func_reg));
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
                Ok(call_reg)
            }
            Expression::If { condition, consequence, alternative } => {
                let cond_reg = self.compile_expression(condition)?;
                let result_reg = self.alloc_register();
                let jump_else = self.emit_instr(Instruction::ab(Opcode::JumpIfFalse, 0, cond_reg));

                if let Statement::Block { statements, .. } = *consequence {
                    for s in statements {
                        self.compile_statement(s)?;
                    }
                }

                if let Some(alt) = alternative {
                    let jump_end = self.emit_instr(Instruction::a_only(Opcode::Jump, 0));
                    let else_pos = self.current_fn().instructions.len();
                    self.current_fn().instructions[jump_else].a = else_pos as u16;

                    if let Statement::Block { statements, .. } = *alt {
                        for s in statements {
                            self.compile_statement(s)?;
                        }
                    }

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
            Expression::Range { .. } | Expression::MapLiteral(_) | Expression::Match { .. } => {
                let reg = self.alloc_register();
                self.emit_instr(Instruction::a_only(Opcode::LoadNull, reg));
                Ok(reg)
            }
        }
    }
}
