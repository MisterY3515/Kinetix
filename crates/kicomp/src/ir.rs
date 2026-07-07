/// Kinetix Intermediate Representation (IR)
/// Register-based bytecode format for KiVM.

use serde::{Serialize, Deserialize};

/// Each opcode encodes an operation for the register-based VM.
/// Instruction format: (Opcode, A, B, C) where A/B/C are register indices
/// or indices into the constant pool depending on the opcode.
// Build 37: discriminants are dense and sequential (0..N) rather than the
// previous banded scheme (0, 10, 20, ..., 255) -- this is a register-based
// VM's opcode dispatch match, and a dense range lets rustc/LLVM lower it to
// a single flat jump table instead of a sparse switch. The explicit gaps
// served no other purpose: the .exki format serializes `CompiledProgram` as
// JSON (see exn.rs), which encodes enum variants by name, not by numeric
// discriminant, and nothing else in the codebase casts an Opcode to its u8
// value -- verified via grep before this change. New opcodes should just be
// appended at the end; there's no serialization reason to reserve gaps.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Opcode {
    /// Load constant pool[B] into register A
    LoadConst,
    /// Load null into register A
    LoadNull,
    /// Load true into register A
    LoadTrue,
    /// Load false into register A
    LoadFalse,

    // Arithmetic: A = B op C
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    /// Negate: A = -B
    Neg,

    // Comparison: A = B op C  (result is bool)
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,

    // Logical
    Not,
    /// And: A = B && C
    And,
    /// Or: A = B || C
    Or,

    // String
    /// Concat: A = B + C (string concat)
    Concat,

    // Variables
    /// Get local variable at slot B into register A
    GetLocal,
    /// Set local variable at slot A from register B
    SetLocal,
    /// Get global variable (name in const pool[B]) into register A
    GetGlobal,
    /// Set global variable (name in const pool[A]) from register B
    SetGlobal,

    // Reactive Core
    /// Set reactive state (name in const pool[A]) from register B
    SetState,
    /// Init computed value (name in const pool[A]) from register B
    InitComputed,
    /// Update existing reactive state (name in const pool[A]) with register B
    UpdateState,
    /// Init reactive effect (dependencies array in A, closure in register B)
    InitEffect,

    // Object/Array
    /// Get member: A = B.const[C] (field name in constant pool)
    GetMember,
    /// Set member: A.const[B] = C
    SetMember,
    /// Get index: A = B[C]
    GetIndex,
    /// Set index: A[B] = C
    SetIndex,
    /// Make array with B elements starting from register A, result in A
    MakeArray,

    /// Make map with B key-value pairs from registers A..A+B*2
    MakeMap,
    /// Make range [B..C) -> A
    MakeRange,
    /// Get Iterator: A = iter(B)
    GetIter,
    /// Advance Iterator: A = next(B), jump to C if done
    IterNext,

    // Control flow
    /// Jump to instruction at offset A (absolute)
    Jump,
    /// Jump to offset A if register B is falsy
    JumpIfFalse,
    /// Jump to offset A if register B is truthy
    JumpIfTrue,

    // Functions
    /// Call function in register A with B arguments (args in A+1..A+B), result in A
    Call,
    /// Return value in register A
    Return,
    /// Return void
    ReturnVoid,
    /// Create closure: A = closure(const[B]) capturing C registers
    MakeClosure,
    /// Tail Call: Reuse current frame for recursive call
    TailCall,
    /// Load method: A = BoundMethod(object: B, method_name_idx: C)
    LoadMethod,

    // Built-in operations
    /// Print register A
    Print,

    // VM control
    /// Pop register A (discard value, mark as free)
    Pop,
    /// No operation
    Nop,
    /// Halt execution
    Halt,
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
    /// Build 35: Flag indicating if compiler optimization passes were applied
    #[serde(default)]
    pub is_optimized: bool,
}

impl CompiledProgram {
    pub fn new() -> Self {
        Self {
            main: CompiledFunction::new("<main>".to_string(), 0),
            functions: vec![],
            version: "0.1.0".to_string(), // will be updated by compiler
            reactive_graph: CompiledReactiveGraph::new(),
            vtable: std::collections::HashMap::new(),
            is_optimized: false,
        }
    }
}
