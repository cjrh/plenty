//! Plenty — a statically typed, Python-shaped language with Cranelift AOT.
//!
//! Modern source is parsed and checked once before choosing an execution path:
//!
//! ```text
//!   source → frontend (AST, names, types) → Op → interpreter / Cranelift AOT
//! ```
//!
//! [`Vm::run`] evaluates modern source. The historical stack syntax has an
//! explicit compatibility entry point, [`Vm::run_legacy`].

mod codegen;
mod frontend;
mod lexer;
mod op;
mod value;
mod vm;

pub use codegen::{compile_legacy_source_to_executable, compile_source_to_executable};
pub use frontend::input_complete;
pub use op::{FnSig, Ty};
pub use value::{StrId, Value};
pub use vm::Vm;

/// Parse and type-check a standalone modern Plenty module without executing
/// it or generating native code. Useful for tooling and compile-time baselines.
pub fn check_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut heap = value::Heap::default();
    let ops = frontend::compile(source, &mut heap, &std::collections::HashMap::new())?;
    op::check(&ops, Vec::new(), &std::collections::HashMap::new())
}
