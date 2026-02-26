/// KiVM Virtual Machine: register-based bytecode interpreter.

use kinetix_kicomp::ir::*;
use crate::builtins;
use std::collections::HashMap;
use std::fmt;

/// Runtime value in the VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
    Array(Vec<Value>),
    Function(usize),
    NativeFn(String),
    NativeModule(String),
    BoundMethod(Box<Value>, Box<Value>),
    Map(HashMap<String, Value>),
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
            (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Str(a), Value::Str(b)) => a.partial_cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
            (Value::Array(a), Value::Array(b)) => a.partial_cmp(b),
            (Value::Map(_), Value::Map(_)) => None,
            _ => None,
        }
    }
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Null => false,
            Value::Array(a) => !a.is_empty(),
            Value::Map(m) => !m.is_empty(),
            _ => true,
        }
    }
    
    pub fn as_int(&self) -> Result<i64, String> {
        match self {
            Value::Int(n) => Ok(*n),
            Value::Float(f) => Ok(*f as i64),
            _ => Err(format!("Expected int, got {:?}", self)),
        }
    }

    pub fn as_float(&self) -> Result<f64, String> {
        match self {
            Value::Float(f) => Ok(*f),
            Value::Int(n) => Ok(*n as f64),
            _ => Err(format!("Expected float, got {:?}", self)),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(v) => write!(f, "{}", v),
            Value::Str(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Null => write!(f, "null"),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                   if i > 0 { write!(f, ", ")?; }
                   write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Function(idx) => write!(f, "<fn@{}>", idx),
            Value::NativeFn(name) => write!(f, "<native:{}>", name),
            Value::NativeModule(name) => write!(f, "<module:{}>", name),
            Value::BoundMethod(_, method) => write!(f, "<bound:{}>", method),
        }
    }
}

#[derive(Debug)]
struct CallFrame {
    function: CompiledFunction,
    ip: usize,
    registers: Vec<Value>,
    return_to_reg: Option<u16>,
}

impl CallFrame {
    fn new(function: CompiledFunction, args: Vec<Value>, return_to_reg: Option<u16>) -> Self {
        let num_regs = std::cmp::max(function.locals as usize, function.arity as usize);
        let safe_num_regs = if num_regs == 0 { 256 } else { num_regs };
        let mut registers = vec![Value::Null; safe_num_regs];
        
        for (i, arg) in args.into_iter().enumerate() {
            if i < registers.len() {
                registers[i] = arg;
            }
        }
        
        Self {
            function,
            ip: 0,
            registers,
            return_to_reg,
        }
    }

    fn reg(&self, idx: u16) -> &Value {
        &self.registers[idx as usize]
    }

    fn reg_mut(&mut self, idx: u16) -> &mut Value {
        &mut self.registers[idx as usize]
    }

    fn set_reg(&mut self, idx: u16, val: Value) {
        if (idx as usize) < self.registers.len() {
            self.registers[idx as usize] = val;
        } else {
             while self.registers.len() <= idx as usize {
                 self.registers.push(Value::Null);
             }
             self.registers[idx as usize] = val;
        }
    }

    fn get_constant(&self, idx: u16) -> &Constant {
        &self.function.constants[idx as usize]
    }
}

pub struct VM {
    program: CompiledProgram,
    call_stack: Vec<CallFrame>,
    globals: HashMap<String, Value>,
    pub output: Vec<String>,
    
    // Reactive Core Data
    state_values: HashMap<String, Value>,
    dirty_states: std::collections::HashSet<String>,
}

impl VM {
    pub fn new(program: CompiledProgram) -> Self {
        let mut globals = HashMap::new();
        for name in crate::builtins::BUILTIN_NAMES {
            globals.insert(name.to_string(), Value::NativeFn(name.to_string()));
        }

        Self {
            program,
            call_stack: Vec::new(),
            globals,
            output: Vec::new(),
            state_values: HashMap::new(),
            dirty_states: std::collections::HashSet::new(),
        }
    }

    pub fn call_stack_len(&self) -> usize {
        self.call_stack.len()
    }

    pub fn run(&mut self) -> Result<(), String> {
        let mut ticks = 0;
        const MAX_TICKS: usize = 1000; // Prevent infinite reactive loops

        // Tick loop (Frame Scheduler)
        loop {
            let main_args = vec![];
            let main_frame = CallFrame::new(self.program.main.clone(), main_args, None);
            self.call_stack.push(main_frame);
            
            // Clear dirty tracking for this frame
            self.dirty_states.clear();

            // Inner execution loop
            loop {
                if self.call_stack.is_empty() {
                    break;
                }
                
                let result = match self.step() {
                    Ok(r) => r,
                    Err(e) => return Err(self.runtime_error(&e)),
                };
                match result {
                    StepResult::Continue => {},
                    StepResult::Halt => break,
                    StepResult::Return(val) => {
                        let popped = self.call_stack.pop().expect("Stack underflow");
                        if let Some(reg) = popped.return_to_reg {
                            if let Some(parent) = self.call_stack.last_mut() {
                                parent.set_reg(reg, val);
                            }
                        } else {
                            // Main return -> break inner loop
                            break;
                        }
                    },
                    StepResult::Call(func, args, dest_reg) => {
                         self.call_value(func, args, Some(dest_reg))?;
                    },
                    StepResult::TailCall(func, args) => {
                        let popped = self.call_stack.pop().expect("Stack underflow");
                        let ret_reg = popped.return_to_reg;
                        self.call_value(func, args, ret_reg)?;
                    }
                }
            } // end inner execution loop

            // Frame finished. Check reactive topology.
            if self.dirty_states.is_empty() {
                // Stable state reached. Normal exit.
                break;
            }

            ticks += 1;
            if ticks >= MAX_TICKS {
                return Err("Runtime Error: Infinite reactive loop detected (Max ticks 1000 exceeded). Verify that state variables do not cyclically update each other.".into());
            }

            // In Step 4 (MIR Caching) we will check the Dependency Graph here.
            // For Step 3, the Tick loop simply re-evaluates the "frame" (Main) to 
            // compute the new Derived nodes from the new State.
            self.call_stack.clear(); // Ensure clean state before re-running
        }

        Ok(())
    }

    /// Build a detailed runtime error string with function name, line number, and message.
    fn runtime_error(&self, msg: &str) -> String {
        if let Some(frame) = self.call_stack.last() {
            let fn_name = &frame.function.name;
            // ip has already been incremented by step(), so the faulting instruction is ip - 1
            let ip = if frame.ip > 0 { frame.ip - 1 } else { 0 };
            let line = frame.function.line_map.get(ip).copied().unwrap_or(0);
            if line > 0 {
                format!("[line {}] in {}: {}", line, fn_name, msg)
            } else {
                format!("in {}: {}", fn_name, msg)
            }
        } else {
            msg.to_string()
        }
    }

    pub fn step(&mut self) -> Result<StepResult, String> {
        let frame_idx = self.call_stack.len() - 1;
        let frame = &mut self.call_stack[frame_idx];

        if frame.ip >= frame.function.instructions.len() {
             return Ok(StepResult::Return(Value::Null));
        }

        let instr = frame.function.instructions[frame.ip];
        frame.ip += 1;

        match instr.opcode {
            Opcode::LoadConst => {
                let c = frame.get_constant(instr.b).clone();
                let val = match c {
                    Constant::Integer(i) => Value::Int(i),
                    Constant::Float(f) => Value::Float(f),
                    Constant::String(s) => Value::Str(s),
                    Constant::Boolean(b) => Value::Bool(b),
                    Constant::Null => Value::Null,
                    Constant::Function(idx) => Value::Function(idx),
                    Constant::Class { name, .. } => {
                         let mut map = HashMap::new();
                         map.insert("__class_name__".to_string(), Value::Str(name));
                         Value::Map(map)
                    }
                };
                frame.set_reg(instr.a, val);
            }
            Opcode::LoadNull => frame.set_reg(instr.a, Value::Null),
            Opcode::LoadTrue => frame.set_reg(instr.a, Value::Bool(true)),
            Opcode::LoadFalse => frame.set_reg(instr.a, Value::Bool(false)),

            Opcode::Add => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                match (left, right) {
                    (Value::Int(a), Value::Int(b)) => frame.set_reg(instr.a, Value::Int(a + b)),
                    (Value::Float(a), Value::Float(b)) => frame.set_reg(instr.a, Value::Float(a + b)),
                    (Value::Int(a), Value::Float(b)) => frame.set_reg(instr.a, Value::Float(a as f64 + b)),
                    (Value::Float(a), Value::Int(b)) => frame.set_reg(instr.a, Value::Float(a + b as f64)),
                    (Value::Str(a), Value::Str(b)) => frame.set_reg(instr.a, Value::Str(a + &b)),
                    _ => return Err("Invalid types for Add".into()),
                }
            }
            Opcode::Sub => {
                 let left = frame.reg(instr.b).as_int()?;
                 let right = frame.reg(instr.c).as_int()?;
                 frame.set_reg(instr.a, Value::Int(left - right));
            }
            Opcode::Mul => {
                 let left = frame.reg(instr.b).as_int()?;
                 let right = frame.reg(instr.c).as_int()?;
                 frame.set_reg(instr.a, Value::Int(left * right));
            }
            Opcode::Div => {
                 let left = frame.reg(instr.b).as_int()?;
                 let right = frame.reg(instr.c).as_int()?;
                 if right == 0 { return Err("Division by zero".into()); }
                 frame.set_reg(instr.a, Value::Int(left / right));
            }
            Opcode::Mod => {
                 let left = frame.reg(instr.b).as_int()?;
                 let right = frame.reg(instr.c).as_int()?;
                 if right == 0 { return Err("Division by zero".into()); }
                 frame.set_reg(instr.a, Value::Int(left % right));
            }
            Opcode::Eq => {
                let left = frame.reg(instr.b);
                let right = frame.reg(instr.c);
                frame.set_reg(instr.a, Value::Bool(left == right));
            }
            Opcode::Lt => {
                 let left = frame.reg(instr.b);
                 let right = frame.reg(instr.c);
                 frame.set_reg(instr.a, Value::Bool(left < right));
            }
            Opcode::Gt => {
                 let left = frame.reg(instr.b);
                 let right = frame.reg(instr.c);
                 frame.set_reg(instr.a, Value::Bool(left > right));
            }
            Opcode::Lte => {
                 let left = frame.reg(instr.b);
                 let right = frame.reg(instr.c);
                 frame.set_reg(instr.a, Value::Bool(left <= right));
            }
            Opcode::Gte => {
                 let left = frame.reg(instr.b);
                 let right = frame.reg(instr.c);
                 frame.set_reg(instr.a, Value::Bool(left >= right));
            }
            Opcode::Neq => {
                 let left = frame.reg(instr.b);
                 let right = frame.reg(instr.c);
                 frame.set_reg(instr.a, Value::Bool(left != right));
            }
            Opcode::And => {
                 let left = frame.reg(instr.b).is_truthy();
                 let right = frame.reg(instr.c).is_truthy();
                 frame.set_reg(instr.a, Value::Bool(left && right));
            }
            Opcode::Or => {
                 let left = frame.reg(instr.b).is_truthy();
                 let right = frame.reg(instr.c).is_truthy();
                 frame.set_reg(instr.a, Value::Bool(left || right));
            }

            Opcode::Print => {
                let val = frame.reg(instr.a);
                let out = format!("{}", val);
                println!("{}", out);
                self.output.push(out);
            }

            Opcode::GetLocal => {
                let val = frame.reg(instr.b).clone();
                frame.set_reg(instr.a, val);
            }
            Opcode::SetLocal => {
                let val = frame.reg(instr.b).clone();
                frame.set_reg(instr.a, val);
            }
            Opcode::GetGlobal => {
                let name = match frame.get_constant(instr.b) {
                    Constant::String(s) => s.clone(),
                    _ => return Err("GetGlobal: expected string constant".into()),
                };
                if let Some(val) = self.globals.get(&name) {
                    frame.set_reg(instr.a, val.clone());
                } else {
                    return Err(format!("Undefined global: {}", name));
                }
            }
            Opcode::SetGlobal => {
                let name = match frame.get_constant(instr.a) {
                     Constant::String(s) => s.clone(),
                    _ => return Err("SetGlobal: expected string constant".into()),
                };
                let val = frame.reg(instr.b).clone();
                self.globals.insert(name, val);
            }
            
            Opcode::SetState => {
                let name = match frame.get_constant(instr.a) {
                    Constant::String(s) => s.clone(),
                    _ => return Err("SetState: expected string constant".into()),
                };
                let current_eval_val = frame.reg(instr.b).clone();
                
                // Reactive update tracking logic
                if let Some(existing_val) = self.state_values.get(&name) {
                    // Tick > 0. Restore the persisted state value!
                    // Overwrite the register with the source-of-truth from the previous tick, 
                    // before it gets saved into the local/global slot by the subsequent Opcode.
                    frame.set_reg(instr.b, existing_val.clone());
                } else {
                    // First time initialization
                    self.state_values.insert(name.clone(), current_eval_val);
                    self.dirty_states.insert(name);
                }
            }
            Opcode::UpdateState => {
                let name = match frame.get_constant(instr.a) {
                    Constant::String(s) => s.clone(),
                    _ => return Err("UpdateState: expected string constant".into()),
                };
                let val = frame.reg(instr.b).clone();
                // User mutated the state explicitly. Track it to trigger next reactive tick.
                self.state_values.insert(name.clone(), val);
                self.dirty_states.insert(name);
            }
            Opcode::InitComputed => {
                // Computed values are initialized in the main flow. 
                // In Step 4 we will use this tag to conditionally skip re-computation using MIR versions.
                // For now, it's a no-op marker that the VM ignores execution-wise.
            }
            Opcode::InitEffect => {
                // Same as Computed, ignored by execution flow for now until reactive step is complete.
            }
            
            Opcode::SetMember => {
                let val = frame.reg(instr.c).clone();
                let member_name = match frame.get_constant(instr.b) {
                    Constant::String(s) => s.clone(),
                     _ => return Err("SetMember: name must be string".into()),
                };
                let target = frame.reg_mut(instr.a);
                match target {
                    Value::Map(map) => { map.insert(member_name, val); },
                    _ => return Err("SetMember: target not a map".into()),
                }
            }
            Opcode::GetMember => {
                 let member_name = match frame.get_constant(instr.c) {
                    Constant::String(s) => s.clone(),
                     _ => return Err("GetMember: name must be string".into()),
                };
                let obj = frame.reg(instr.b);
                match obj {
                    Value::Map(map) => {
                        if let Some(val) = map.get(&member_name) {
                            frame.set_reg(instr.a, val.clone());
                        } else {
                            frame.set_reg(instr.a, Value::Null);
                        }
                    },
                    _ => return Err("GetMember: target not a map".into()),
                }
            }
            Opcode::MakeArray => {
                let start_reg = instr.a;
                let count = instr.b as usize;
                let mut arr = Vec::with_capacity(count);
                for i in 0..count {
                    arr.push(frame.reg(start_reg + i as u16).clone());
                }
                frame.set_reg(instr.a, Value::Array(arr));
            }
            Opcode::MakeMap => {
                let count = instr.b as u16; 
                let start_reg = instr.a;
                let mut map = HashMap::new();
                for i in 0..count {
                     let k_reg = start_reg + (i * 2);
                     let v_reg = start_reg + (i * 2) + 1;
                     let key = frame.reg(k_reg).clone();
                     let val = frame.reg(v_reg).clone();
                     let k_str = match key {
                         Value::Str(s) => s,
                         _ => return Err("Map key must be string".into()),
                     };
                     map.insert(k_str, val);
                }
                frame.set_reg(instr.a, Value::Map(map));
            }
            Opcode::MakeRange => {
                let start = frame.reg(instr.b).as_int()?;
                let end = frame.reg(instr.c).as_int()?;
                let mut chars = Vec::new();
                for i in start..end {
                    chars.push(Value::Int(i));
                }
                frame.set_reg(instr.a, Value::Array(chars));
            }

            Opcode::Jump => {
                frame.ip = instr.a as usize;
            }
            Opcode::JumpIfFalse => {
                let cond = frame.reg(instr.b);
                if !cond.is_truthy() {
                    frame.ip = instr.a as usize;
                }
            }
            Opcode::JumpIfTrue => {
                 let cond = frame.reg(instr.b);
                if cond.is_truthy() {
                    frame.ip = instr.a as usize;
                }
            }

            Opcode::Call => {
                let func_val = frame.reg(instr.a).clone();
                let arg_count = instr.b as usize;
                let mut args = Vec::with_capacity(arg_count);
                for i in 0..arg_count {
                    let arg_reg = instr.a + 1 + i as u16;
                    args.push(frame.reg(arg_reg).clone());
                }
                return Ok(StepResult::Call(func_val, args, instr.a));
            }

            Opcode::TailCall => {
                let func_val = frame.reg(instr.a).clone();
                let arg_count = instr.b as usize;
                let mut args = Vec::with_capacity(arg_count);
                for i in 0..arg_count {
                    let arg_reg = instr.a + 1 + i as u16;
                    args.push(frame.reg(arg_reg).clone());
                }
                return Ok(StepResult::TailCall(func_val, args));
            }

            Opcode::Return => {
                let val = frame.reg(instr.a).clone();
                return Ok(StepResult::Return(val));
            }
            Opcode::ReturnVoid => {
                return Ok(StepResult::Return(Value::Null));
            }

            Opcode::Halt => return Ok(StepResult::Halt),
            
            _ => { return Err(format!("Opcode {:?} unimplemented", instr.opcode)); }
        }

        Ok(StepResult::Continue)
    }

    pub fn call_value(&mut self, func: Value, mut args: Vec<Value>, return_reg: Option<u16>) -> Result<(), String> {
        match func {
            Value::BoundMethod(receiver, method) => {
                args.insert(0, *receiver);
                self.call_value(*method, args, return_reg)
            }
            Value::Function(func_idx) => {
                let func = self.program.functions[func_idx].clone();
                self.call_stack.push(CallFrame::new(func, args, return_reg));
                Ok(())
            }
            Value::NativeFn(name) => {
                let result = builtins::call_builtin(&name, &args, self)?;
                if let Some(reg) = return_reg {
                    if let Some(frame) = self.call_stack.last_mut() {
                        frame.set_reg(reg, result);
                    }
                }
                Ok(())
            }
            Value::Str(name) => {
                let result = builtins::call_builtin(&name, &args, self)
                    .map_err(|_| format!("Cannot call Str('{}') (not expecting a native function)", name))?;
                if let Some(reg) = return_reg {
                    if let Some(frame) = self.call_stack.last_mut() {
                        frame.set_reg(reg, result);
                    }
                }
                Ok(())
            }
            _ => Err(format!("Cannot call {:?}", func)),
        }
    }
}

#[derive(Debug)]
pub enum StepResult {
    Continue,
    Halt,
    Return(Value),
    Call(Value, Vec<Value>, u16),
    TailCall(Value, Vec<Value>),
}
