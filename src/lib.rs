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
mod generator;
mod lexer;
mod op;
mod ownership;
mod record;
mod sum;
mod toolchain;
mod value;

pub use codegen::{
    compile_file_to_executable, compile_file_to_executable_with_options, compile_file_to_object,
    compile_legacy_source_to_executable, compile_legacy_source_to_executable_with_options,
    compile_source_to_executable, compile_source_to_executable_with_options,
    compile_source_to_object,
};
pub use toolchain::{emit_runtime, CompileOptions, RuntimeArtifacts};

/// Parse and type-check a standalone modern Plenty binary without executing
/// it or generating native code. Requires a parameterless `main` returning
/// `()` or `i32`, just like [`compile_source_to_executable`].
pub fn check_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut heap = value::Heap::default();
    let program = frontend::compile(source, &mut heap)?;
    op::check(&program.ops)
}

/// Check a complete application, resolving imports from the supplied root or
/// the entry file's directory. No native code is emitted or executed.
pub fn check_file(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    check_file_mode(path, root, true)
}

/// Check a library module and its imports without requiring an application main.
pub fn check_module_file(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    check_file_mode(path, root, false)
}

fn check_file_mode(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
    require_main: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut heap = value::Heap::default();
    let program = frontend::compile_file(path, root, require_main, &mut heap)?;
    op::check(&program.ops)
}
