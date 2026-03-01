/// KiComp - Kinetix Compiler
/// Compiles AST into register-based bytecode for KiVM.

pub mod ir;
pub mod compiler;
#[cfg(feature = "llvm")]
pub mod llvm_codegen;
pub mod exn;
pub mod types;
pub mod symbol;
pub mod hir;
pub mod typeck;
pub mod mir;
pub mod borrowck;
pub mod trait_solver;
pub mod exhaustiveness;
pub mod type_normalize;
pub mod monomorphize;
pub mod ssa_validate;
pub mod mono_validate;
pub mod drop_verify;
pub mod benchmarks;
pub mod reactive;
pub mod ir_hash;
pub mod capability;
pub mod hir_validate;
pub mod vtable;
