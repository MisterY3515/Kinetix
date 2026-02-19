/// KiComp - Kinetix Compiler
/// Compiles AST into register-based bytecode for KiVM.

pub mod ir;
pub mod compiler;
#[cfg(feature = "llvm")]
pub mod llvm_codegen;
pub mod exn;
