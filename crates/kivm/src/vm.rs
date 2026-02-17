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
    /// Function reference: index into program.functions
    Function(usize),
    /// Native built-in function
    NativeFn(String),
    /// Native Module (Math, System, etc.)
    NativeModule(String),
    /// Bound Method (receiver, method)
    BoundMethod(Box<Value>, Box<Value>),
    /// Map / Dictionary
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
            (Value::Array(a), Value::Array(b)) => a.partial_cmp(b), // Recursive if arrays
            // Maps are not ordered
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
            Value::Function(_) | Value::NativeFn(_) | Value::NativeModule(_) | Value::BoundMethod(_, _) => true,
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

/// A single call frame on the VM stack.
#[derive(Debug)]
struct CallFrame {
    function: CompiledFunction,
    ip: usize,
    registers: Vec<Value>,
    return_to_reg: Option<u8>,
}

impl CallFrame {
    fn new(function: CompiledFunction, args: Vec<Value>, return_to_reg: Option<u8>) -> Self {
        let num_regs = 256; // max registers per frame
        let mut registers = vec![Value::Null; num_regs];
        // Place arguments in registers 0..n
        for (i, arg) in args.into_iter().enumerate() {
            registers[i] = arg;
        }
        Self { function, ip: 0, registers, return_to_reg }
    }

    fn read_instruction(&mut self) -> Option<Instruction> {
        if self.ip < self.function.instructions.len() {
            let instr = self.function.instructions[self.ip];
            self.ip += 1;
            Some(instr)
        } else {
            None
        }
    }

    fn get_constant(&self, idx: u8) -> Value {
        match &self.function.constants[idx as usize] {
            Constant::Integer(n) => Value::Int(*n),
            Constant::Float(f) => Value::Float(*f),
            Constant::String(s) => Value::Str(s.clone()),
            Constant::Boolean(b) => Value::Bool(*b),
            Constant::Null => Value::Null,
            Constant::Function(idx) => Value::Function(*idx),
        }
    }

    fn reg(&self, idx: u8) -> &Value {
        &self.registers[idx as usize]
    }

    fn set_reg(&mut self, idx: u8, val: Value) {
        self.registers[idx as usize] = val;
    }
}

/// The KiVM virtual machine.
pub struct VM {
    call_stack: Vec<CallFrame>,
    globals: HashMap<String, Value>,
    program: CompiledProgram,
    /// Captured output for testing
    pub output: Vec<String>,
}

impl VM {
    pub fn new(program: CompiledProgram) -> Self {
        let mut globals = HashMap::new();
        // Register built-in functions
        for name in builtins::BUILTIN_NAMES {
            globals.insert(name.to_string(), Value::NativeFn(name.to_string()));
        }

        // Register Modules
        let modules = ["Math", "System", "OS", "Game", "Net", "Graph", "Data", "Audio", "UI", "Input",
                       "math", "time", "env"];
        for mod_name in modules.iter() {
            globals.insert(mod_name.to_string(), Value::NativeModule(mod_name.to_string()));
        }

        Self {
            call_stack: Vec::new(),
            globals,
            program,
            output: Vec::new(),
        }
    }

    /// Run the program and return the final value.
    pub fn run(&mut self) -> Result<Value, String> {
        let main_fn = self.program.main.clone();
        self.call_stack.push(CallFrame::new(main_fn, vec![], None));

        loop {
            let result = self.step()?;
            match result {
                StepResult::Continue => continue,
                StepResult::Halt => return Ok(Value::Null),
                StepResult::Return(val) => {
                    return Ok(val);
                }
                StepResult::Call(func, args, reg) => {
                    self.call_value(func, args, reg)?;
                }
            }
        }
    }

    pub fn call_stack_len(&self) -> usize {
        self.call_stack.len()
    }

    pub fn step(&mut self) -> Result<StepResult, String> {
        let frame = self.call_stack.last_mut()
            .ok_or("Call stack empty")?;

        let instr = match frame.read_instruction() {
            Some(i) => i,
            None => return Ok(StepResult::Halt),
        };

        match instr.opcode {
            Opcode::LoadConst => {
                let val = frame.get_constant(instr.b);
                frame.set_reg(instr.a, val);
            }
            Opcode::LoadNull => {
                frame.set_reg(instr.a, Value::Null);
            }
            Opcode::LoadTrue => {
                frame.set_reg(instr.a, Value::Bool(true));
            }
            Opcode::LoadFalse => {
                frame.set_reg(instr.a, Value::Bool(false));
            }

            // Arithmetic
            Opcode::Add => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a + b),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 + b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a + *b as f64),
                    (Value::Str(a), Value::Str(b)) => Value::Str(format!("{}{}", a, b)),
                    (Value::Str(a), b) => Value::Str(format!("{}{}", a, b)),
                    (a, Value::Str(b)) => Value::Str(format!("{}{}", a, b)),
                    _ => return Err(format!("Cannot add {:?} and {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Sub => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a - b),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a - b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 - b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a - *b as f64),
                    _ => return Err(format!("Cannot subtract {:?} and {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Mul => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a * b),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a * b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 * b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a * *b as f64),
                    _ => return Err(format!("Cannot multiply {:?} and {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Div => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => {
                        if *b == 0 { return Err("Division by zero".to_string()); }
                        Value::Int(a / b)
                    }
                    (Value::Float(a), Value::Float(b)) => Value::Float(a / b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 / b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a / *b as f64),
                    _ => return Err(format!("Cannot divide {:?} by {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Mod => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a % b),
                    _ => return Err(format!("Cannot mod {:?} by {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Neg => {
                let val = frame.reg(instr.b).clone();
                let result = match val {
                    Value::Int(n) => Value::Int(-n),
                    Value::Float(f) => Value::Float(-f),
                    _ => return Err(format!("Cannot negate {:?}", val)),
                };
                frame.set_reg(instr.a, result);
            }

            // Comparison
            Opcode::Eq => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Bool(a == b),
                    (Value::Float(a), Value::Float(b)) => Value::Bool(a == b),
                    (Value::Str(a), Value::Str(b)) => Value::Bool(a == b),
                    (Value::Bool(a), Value::Bool(b)) => Value::Bool(a == b),
                    (Value::Null, Value::Null) => Value::Bool(true),
                    _ => Value::Bool(false),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Neq => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Bool(a != b),
                    (Value::Float(a), Value::Float(b)) => Value::Bool(a != b),
                    (Value::Str(a), Value::Str(b)) => Value::Bool(a != b),
                    (Value::Bool(a), Value::Bool(b)) => Value::Bool(a != b),
                    _ => Value::Bool(true),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Lt => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Bool(a < b),
                    (Value::Float(a), Value::Float(b)) => Value::Bool(a < b),
                    _ => return Err(format!("Cannot compare {:?} < {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Gt => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Bool(a > b),
                    (Value::Float(a), Value::Float(b)) => Value::Bool(a > b),
                    _ => return Err(format!("Cannot compare {:?} > {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Lte => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Bool(a <= b),
                    (Value::Float(a), Value::Float(b)) => Value::Bool(a <= b),
                    _ => return Err(format!("Cannot compare {:?} <= {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::Gte => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => Value::Bool(a >= b),
                    (Value::Float(a), Value::Float(b)) => Value::Bool(a >= b),
                    _ => return Err(format!("Cannot compare {:?} >= {:?}", left, right)),
                };
                frame.set_reg(instr.a, result);
            }

            // Logical
            Opcode::Not => {
                let val = frame.reg(instr.b).clone();
                frame.set_reg(instr.a, Value::Bool(!val.is_truthy()));
            }
            Opcode::And => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                frame.set_reg(instr.a, Value::Bool(left.is_truthy() && right.is_truthy()));
            }
            Opcode::Or => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                frame.set_reg(instr.a, Value::Bool(left.is_truthy() || right.is_truthy()));
            }

            Opcode::Concat => {
                let left = frame.reg(instr.b).clone();
                let right = frame.reg(instr.c).clone();
                frame.set_reg(instr.a, Value::Str(format!("{}{}", left, right)));
            }

            // Variables
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
                    Value::Str(s) => s,
                    _ => return Err("GetGlobal: expected string constant".to_string()),
                };
                let val = self.globals.get(&name).cloned().unwrap_or(Value::Null);
                let frame = self.call_stack.last_mut().unwrap();
                frame.set_reg(instr.a, val);
            }
            Opcode::SetGlobal => {
                let name = match frame.get_constant(instr.a) {
                    Value::Str(s) => s,
                    _ => return Err("SetGlobal: expected string constant".to_string()),
                };
                let val = frame.reg(instr.b).clone();
                self.globals.insert(name, val);
            }

            // Arrays
            Opcode::MakeArray => {
                let start = instr.a as usize;
                let count = instr.b as usize;
                let frame = self.call_stack.last().unwrap();
                let elements: Vec<Value> = (start..start + count)
                    .map(|i| frame.registers[i].clone())
                    .collect();
                let frame = self.call_stack.last_mut().unwrap();
                frame.set_reg(instr.a, Value::Array(elements));
            }
            Opcode::GetIndex => {
                let arr = frame.reg(instr.b).clone();
                let idx = frame.reg(instr.c).clone();
                match (&arr, &idx) {
                    (Value::Array(a), Value::Int(i)) => {
                        let val = a.get(*i as usize).cloned().unwrap_or(Value::Null);
                        frame.set_reg(instr.a, val);
                    }
                    (Value::Map(m), Value::Str(key)) => {
                        let val = m.get(key).cloned().unwrap_or(Value::Null);
                        frame.set_reg(instr.a, val);
                    }
                    _ => {
                        frame.set_reg(instr.a, Value::Null);
                    }
                }
            }
            Opcode::SetIndex => {
                // A[B] = C
                let val = frame.reg(instr.c).clone();
                let index_val = frame.reg(instr.b).clone();
                match &mut frame.registers[instr.a as usize] {
                     Value::Array(arr) => {
                         let idx = index_val.as_int().unwrap_or(0) as usize;
                         if idx < arr.len() {
                             arr[idx] = val;
                         }
                     }
                     Value::Map(map) => {
                         if let Value::Str(key) = index_val {
                             map.insert(key, val);
                         }
                     }
                     _ => {}
                }
            }

            // Members
            Opcode::GetMember => {
                let obj = frame.reg(instr.b).clone();
                let member_name = match frame.get_constant(instr.c) {
                    Value::Str(s) => s,
                    _ => return Err("GetMember: expected string constant".to_string()),
                };

                let result = match obj {
                    Value::NativeModule(name) => {
                         // Math.PI check
                         if name == "Math" && member_name == "PI" {
                             Value::Float(std::f64::consts::PI)
                         } else {
                             Value::NativeFn(format!("{}.{}", name, member_name))
                         }
                    },
                    Value::Str(_) => {
                        let method = Value::NativeFn(format!("str.{}", member_name));
                        Value::BoundMethod(Box::new(obj), Box::new(method))
                    },
                    Value::Array(_) => {
                        let method = Value::NativeFn(format!("array.{}", member_name));
                        Value::BoundMethod(Box::new(obj), Box::new(method))
                    },
                    Value::Map(ref map) => {
                        // Check methods first? or properties?
                        // For data objects, priority is usually the key.
                        if let Some(val) = map.get(&member_name) {
                            val.clone()
                        } else {
                            // Map methods? keys(), values(), len()...
                            // For now, return Null if key not found, or maybe check 'map.len'?
                            Value::Null
                        }
                    },
                    _ => Value::Null,
                };
                frame.set_reg(instr.a, result);
            }
            Opcode::SetMember => {
                // Read-only for now
                return Err("SetMember not implemented".to_string());
            }

            // Control flow
            Opcode::Jump => {
                let frame = self.call_stack.last_mut().unwrap();
                frame.ip = instr.a as usize;
            }
            Opcode::JumpIfFalse => {
                let val = frame.reg(instr.b).clone();
                if !val.is_truthy() {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.ip = instr.a as usize;
                }
            }
            Opcode::JumpIfTrue => {
                let val = frame.reg(instr.b).clone();
                if val.is_truthy() {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.ip = instr.a as usize;
                }
            }

            // Functions
            Opcode::Call => {
                let func_val = frame.reg(instr.a).clone();
                let arg_count = instr.b as usize;
                let mut args = Vec::with_capacity(arg_count);
                for i in 0..arg_count {
                    args.push(frame.reg(instr.a + 1 + i as u8).clone());
                }

                return Ok(StepResult::Call(func_val, args, instr.a));
            }
            Opcode::Return => {
                let val = frame.reg(instr.a).clone();
                let return_to = frame.return_to_reg;
                self.call_stack.pop();
                if self.call_stack.is_empty() {
                    return Ok(StepResult::Return(val));
                }
                // Write return value to caller's register
                if let Some(reg) = return_to {
                    let caller = self.call_stack.last_mut().unwrap();
                    caller.set_reg(reg, val);
                }
                return Ok(StepResult::Continue);
            }
            Opcode::ReturnVoid => {
                let return_to = frame.return_to_reg;
                self.call_stack.pop();
                if self.call_stack.is_empty() {
                    return Ok(StepResult::Return(Value::Null));
                }
                // Write Null to caller's register to signal void return
                if let Some(reg) = return_to {
                    let caller = self.call_stack.last_mut().unwrap();
                    caller.set_reg(reg, Value::Null);
                }
                return Ok(StepResult::Continue);
            }
            Opcode::MakeClosure => {
                // Placeholder
                frame.set_reg(instr.a, Value::Null);
            }

            // Built-in
            Opcode::Print => {
                let val = frame.reg(instr.a).clone();
                let text = format!("{}", val);
                println!("{}", text);
                self.output.push(text);
            }

            Opcode::Pop | Opcode::Nop => {}

            Opcode::Halt => {
                return Ok(StepResult::Halt);
            }
        }

        Ok(StepResult::Continue)
    }

    pub fn call_value(&mut self, func: Value, mut args: Vec<Value>, return_reg: u8) -> Result<(), String> {
        match func {
            Value::BoundMethod(receiver, method) => {
                // Prepend receiver to args
                args.insert(0, *receiver);
                // Call the underlying method
                self.call_value(*method, args, return_reg)
            }
            Value::Function(func_idx) => {
                let func = self.program.functions[func_idx].clone();
                self.call_stack.push(CallFrame::new(func, args, Some(return_reg)));
                Ok(())
            }
            Value::NativeFn(name) => {
                let result = builtins::call_builtin(&name, &args, self)?;
                let frame = self.call_stack.last_mut().unwrap();
                frame.set_reg(return_reg, result);
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
    Call(Value, Vec<Value>, u8),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_simple_program(instructions: Vec<Instruction>, constants: Vec<Constant>) -> CompiledProgram {
        let mut prog = CompiledProgram::new();
        prog.main.instructions = instructions;
        prog.main.constants = constants;
        prog
    }

    #[test]
    fn test_vm_load_const_halt() {
        let prog = make_simple_program(
            vec![
                Instruction::ab(Opcode::LoadConst, 0, 0),
                Instruction::a_only(Opcode::Halt, 0),
            ],
            vec![Constant::Integer(42)],
        );
        let mut vm = VM::new(prog);
        vm.run().expect("VM should halt successfully");
    }

    #[test]
    fn test_vm_add() {
        let prog = make_simple_program(
            vec![
                Instruction::ab(Opcode::LoadConst, 0, 0), // r0 = 10
                Instruction::ab(Opcode::LoadConst, 1, 1), // r1 = 20
                Instruction::new(Opcode::Add, 2, 0, 1),   // r2 = r0 + r1
                Instruction::a_only(Opcode::Print, 2),     // print r2
                Instruction::a_only(Opcode::Halt, 0),
            ],
            vec![Constant::Integer(10), Constant::Integer(20)],
        );
        let mut vm = VM::new(prog);
        vm.run().expect("VM should run");
        assert_eq!(vm.output, vec!["30"]);
    }

    #[test]
    fn test_vm_comparison() {
        let prog = make_simple_program(
            vec![
                Instruction::ab(Opcode::LoadConst, 0, 0), // r0 = 5
                Instruction::ab(Opcode::LoadConst, 1, 1), // r1 = 10
                Instruction::new(Opcode::Lt, 2, 0, 1),    // r2 = 5 < 10
                Instruction::a_only(Opcode::Print, 2),     // print true
                Instruction::a_only(Opcode::Halt, 0),
            ],
            vec![Constant::Integer(5), Constant::Integer(10)],
        );
        let mut vm = VM::new(prog);
        vm.run().expect("VM should run");
        assert_eq!(vm.output, vec!["true"]);
    }

    #[test]
    fn test_vm_jump_if_false() {
        let prog = make_simple_program(
            vec![
                Instruction::a_only(Opcode::LoadFalse, 0),        // r0 = false
                Instruction::ab(Opcode::JumpIfFalse, 4, 0),       // if !r0 jump to 4
                Instruction::ab(Opcode::LoadConst, 1, 0),         // r1 = "skipped" (should skip)
                Instruction::a_only(Opcode::Print, 1),
                Instruction::ab(Opcode::LoadConst, 1, 1),         // r1 = "reached"
                Instruction::a_only(Opcode::Print, 1),
                Instruction::a_only(Opcode::Halt, 0),
            ],
            vec![
                Constant::String("skipped".to_string()),
                Constant::String("reached".to_string()),
            ],
        );
        let mut vm = VM::new(prog);
        vm.run().expect("VM should run");
        assert_eq!(vm.output, vec!["reached"]);
    }

    #[test]
    fn test_vm_string_concat() {
        let prog = make_simple_program(
            vec![
                Instruction::ab(Opcode::LoadConst, 0, 0), // "Hello "
                Instruction::ab(Opcode::LoadConst, 1, 1), // "World"
                Instruction::new(Opcode::Add, 2, 0, 1),   // "Hello World"
                Instruction::a_only(Opcode::Print, 2),
                Instruction::a_only(Opcode::Halt, 0),
            ],
            vec![
                Constant::String("Hello ".to_string()),
                Constant::String("World".to_string()),
            ],
        );
        let mut vm = VM::new(prog);
        vm.run().expect("VM should run");
        assert_eq!(vm.output, vec!["Hello World"]);
    }
}
