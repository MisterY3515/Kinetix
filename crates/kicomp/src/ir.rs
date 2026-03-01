/// Kinetix Intermediate Representation (IR)
/// Register-based bytecode format for KiVM.

use serde::{Serialize, Deserialize};

/// Each opcode encodes an operation for the register-based VM.
/// Instruction format: (Opcode, A, B, C) where A/B/C are register indices
/// or indices into the constant pool depending on the opcode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Opcode {
    /// Load constant pool[B] into register A
    LoadConst = 0,
    /// Load null into register A
    LoadNull = 1,
    /// Load true into register A
    LoadTrue = 2,
    /// Load false into register A
    LoadFalse = 3,

    // Arithmetic: A = B op C
    Add = 10,
    Sub = 11,
    Mul = 12,
    Div = 13,
    Mod = 14,
    /// Negate: A = -B
    Neg = 15,

    // Comparison: A = B op C  (result is bool)
    Eq = 20,
    Neq = 21,
    Lt = 22,
    Gt = 23,
    Lte = 24,
    Gte = 25,

    // Logical
    Not = 30,
    /// And: A = B && C
    And = 31,
    /// Or: A = B || C
    Or = 32,

    // String
    /// Concat: A = B + C (string concat)
    Concat = 35,

    // Variables
    /// Get local variable at slot B into register A
    GetLocal = 40,
    /// Set local variable at slot A from register B
    SetLocal = 41,
    /// Get global variable (name in const pool[B]) into register A
    GetGlobal = 42,
    /// Set global variable (name in const pool[A]) from register B
    SetGlobal = 43,

    // Reactive Core
    /// Set reactive state (name in const pool[A]) from register B
    SetState = 44,
    /// Init computed value (name in const pool[A]) from register B
    InitComputed = 45,
    /// Update existing reactive state (name in const pool[A]) with register B
    UpdateState = 46,
    /// Init reactive effect (dependencies array in A, closure in register B)
    InitEffect = 47,

    // Object/Array
    /// Get member: A = B.const[C] (field name in constant pool)
    GetMember = 50,
    /// Set member: A.const[B] = C
    SetMember = 51,
    /// Get index: A = B[C]
    GetIndex = 52,
    /// Set index: A[B] = C
    SetIndex = 53,
    /// Make array with B elements starting from register A, result in A
    MakeArray = 54,
    
    /// Make map with B key-value pairs from registers A..A+B*2
    MakeMap = 55,
    /// Make range [B..C) -> A
    MakeRange = 56,
    /// Get Iterator: A = iter(B)
    GetIter = 57,
    /// Advance Iterator: A = next(B), jump to C if done
    IterNext = 58,

    // Control flow
    /// Jump to instruction at offset A (absolute)
    Jump = 60,
    /// Jump to offset A if register B is falsy
    JumpIfFalse = 61,
    /// Jump to offset A if register B is truthy
    JumpIfTrue = 62,

    // Functions
    /// Call function in register A with B arguments (args in A+1..A+B), result in A
    Call = 70,
    /// Return value in register A
    Return = 71,
    /// Return void
    ReturnVoid = 72,
    /// Create closure: A = closure(const[B]) capturing C registers
    MakeClosure = 73,
    /// Tail Call: Reuse current frame for recursive call
    TailCall = 74,
    /// Load method: A = BoundMethod(object: B, method_name_idx: C)
    LoadMethod = 75,

    // Built-in operations
    /// Print register A
    Print = 80,

    // VM control
    /// Pop register A (discard value, mark as free)
    Pop = 90,
    /// No operation
    Nop = 91,
    /// Halt execution
    Halt = 255,
}

/// A single bytecode instruction: opcode + 3 operands.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Instruction {
    pub opcode: Opcode,
    pub a: u16,
    pub b: u16,
    pub c: u16,
}

impl Instruction {
    pub fn new(opcode: Opcode, a: u16, b: u16, c: u16) -> Self {
        Self { opcode, a, b, c }
    }

    /// Shorthand for opcodes that only use register A
    pub fn a_only(opcode: Opcode, a: u16) -> Self {
        Self { opcode, a, b: 0, c: 0 }
    }

    /// Shorthand for opcodes that use A and B
    pub fn ab(opcode: Opcode, a: u16, b: u16) -> Self {
        Self { opcode, a, b, c: 0 }
    }
}

/// Runtime value stored in the constant pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
    Function(usize),
    /// Compiled class descriptor
    Class {
        name: String,
        methods: Vec<usize>, // indices into functions
        fields: Vec<String>,
        parent: Option<String>,
    },
}

/// A compiled function: its bytecode, constants, and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledFunction {
    pub name: String,
    pub arity: u16,          // number of parameters
    pub locals: u16,         // number of local variable slots
    pub instructions: Vec<Instruction>,
    pub constants: Vec<Constant>,
    pub param_names: Vec<String>,
    /// Maps each instruction index to a source line number (1-based).
    #[serde(default)]
    pub line_map: Vec<u32>,
}

impl CompiledFunction {
    pub fn new(name: String, arity: u16) -> Self {
        Self {
            name,
            arity,
            locals: 0,
            instructions: vec![],
            constants: vec![],
            param_names: vec![],
            line_map: vec![],
        }
    }

    /// Add a constant and return its index.
    pub fn add_constant(&mut self, c: Constant) -> u16 {
        // Deduplicate
        for (i, existing) in self.constants.iter().enumerate() {
            if existing == &c {
                return i as u16;
            }
        }
        let idx = self.constants.len() as u16;
        self.constants.push(c);
        idx
    }

    /// Emit an instruction and return its index.
    pub fn emit(&mut self, instr: Instruction) -> usize {
        let idx = self.instructions.len();
        self.instructions.push(instr);
        idx
    }
}

/// Runtime metadata for a reactive node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReactiveNodeKind {
    State,
    Computed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactiveNodeMetadata {
    pub name: String,
    pub kind: ReactiveNodeKind,
    pub line: usize,
}

/// A serialized reactive dependency graph for the VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledReactiveGraph {
    pub nodes: std::collections::HashMap<String, ReactiveNodeMetadata>,
    pub dependencies: std::collections::HashMap<String, std::collections::HashSet<String>>,
    pub dependents: std::collections::HashMap<String, std::collections::HashSet<String>>,
    pub update_order: Vec<String>,
}

impl CompiledReactiveGraph {
    pub fn new() -> Self {
        Self {
            nodes: std::collections::HashMap::new(),
            dependencies: std::collections::HashMap::new(),
            dependents: std::collections::HashMap::new(),
            update_order: vec![],
        }
    }
}

/// A compiled program: a list of functions + a top-level "main" chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledProgram {
    pub main: CompiledFunction,
    pub functions: Vec<CompiledFunction>,
    pub version: String,
    pub reactive_graph: CompiledReactiveGraph,
    /// Static VTable: maps (class_name, method_name) → function_index
    #[serde(default)]
    pub vtable: std::collections::HashMap<String, std::collections::HashMap<String, usize>>,
}

impl CompiledProgram {
    pub fn new() -> Self {
        Self {
            main: CompiledFunction::new("<main>".to_string(), 0),
            functions: vec![],
            version: "0.1.0".to_string(), // will be updated by compiler
            reactive_graph: CompiledReactiveGraph::new(),
            vtable: std::collections::HashMap::new(),
        }
    }
}
