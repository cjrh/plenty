//! Plenty — a statically typed, Python-shaped language with Cranelift AOT.
//!
//! ```text
//! source → frontend (AST, names, types) → checked Op → Cranelift → object → executable
//! ```
//!
//! Compile with [`compile_source_to_executable`] or validate with [`check_source`].

mod codegen;
mod collection;
mod frontend;
mod lexer;
mod op;
mod value;

pub use codegen::{compile_legacy_source_to_executable, compile_source_to_executable};

/// Parse and type-check a standalone modern Plenty module without executing
/// it or generating native code. Useful for tooling and compile-time baselines.
pub fn check_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut heap = value::Heap::default();
    let ops = frontend::compile(source, &mut heap)?;
    op::check(&ops)
}
