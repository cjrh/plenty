//! Plenty — a statically typed, Python-shaped language with Cranelift AOT.
//!
//! ```text
//! source → frontend (AST, names, types) → checked Op → Cranelift → object → executable
//! ```
//!
//! Compile with [`compile_source_to_executable`] or validate with [`check_source`].

// Keep scoped measurements expression-shaped: `?`, early returns and borrows
// retain their original behavior, and feature-disabled builds need no hotpath.
macro_rules! profile_block {
    ($label:literal, $body:expr_2021) => {{
        #[cfg(feature = "profile")]
        {
            hotpath::measure_block!($label, $body)
        }
        #[cfg(not(feature = "profile"))]
        {
            $body
        }
    }};
}
pub(crate) use profile_block;

/// Whether this process leaks a compile's final data rather than freeing it;
/// see [`skip_teardown`].
static SKIP_TEARDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Leak, rather than free, the checked program (its operations and string
/// pool) at the end of each later check. Freeing it only returns memory to a
/// process that is about to exit: a command-line check that exits next calls
/// this, saving about 2% of checking instructions (issue #321). Never call it
/// in a process that keeps compiling, such as the language server,
/// `plenty build` and `plenty test`, or a library caller: each check would
/// keep its memory until the process exits.
pub fn skip_teardown() {
    SKIP_TEARDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Drop `value`, or leak it after [`skip_teardown`].
pub(crate) fn teardown<T>(value: T) {
    if SKIP_TEARDOWN.load(std::sync::atomic::Ordering::Relaxed) {
        std::mem::forget(value);
    }
}

mod atomic;
mod boxed;
mod bytes;
pub mod cache;
mod channel;
mod closure;
mod codegen;
mod collection;
#[path = "../plenty-runtime/src/collection_opcodes.rs"]
mod collection_opcodes;
mod compress;
mod control;
mod crypto;
mod effects;
mod executor;
mod exports;
mod filesystem;
mod foreign;
mod frontend;
mod generator;
mod hash;
mod lexer;
mod library;
mod library_metadata;
pub mod lsp;
mod mutex;
mod net;
mod nominal;
mod numeric;
mod op;
mod ownership;
mod parallel;
mod process;
#[cfg(feature = "profile")]
pub mod profile;
pub mod project;
mod record;
mod regex;
// Experiment only: no stable compiler or CLI contract is implied.
#[cfg(feature = "rust-source-spike")]
#[doc(hidden)]
#[path = "../experiments/rust-source-backend/emitter/mod.rs"]
pub mod rust_source_spike;
mod sqlite;
mod stream;
mod sum;
mod tail_abi;
mod text;
mod threading;
mod time;
mod tls;
mod toolchain;
mod type_facts;
mod unicode;
mod value;

/// Editor queries over a checked file, for the language server (issue #155).
pub mod ide {
    pub use crate::frontend::ide::{
        Analysis, Diagnostic, File, Location, Reference, Symbol, SymbolKind, analyze,
        standard_modules,
    };
}

pub use codegen::{
    Dump, DumpOptions, DumpStage, compile_file_to_executable,
    compile_file_to_executable_with_options, compile_file_to_object,
    compile_file_to_object_with_options, compile_legacy_source_to_executable,
    compile_legacy_source_to_executable_with_options, compile_package_to_executable,
    compile_source_to_executable, compile_source_to_executable_with_options,
    compile_source_to_object, dump_file,
};
pub use library::{
    LibraryArtifacts, LibraryKind, LibraryOptions, compile_file_to_library, emit_runtime_interface,
    runtime_interface_source,
};
pub use library_metadata::{
    LibraryInterface, extract_library_interface, read_library_interfaces, verify_library_interface,
};
pub use toolchain::{
    CompileOptions, RuntimeArtifacts, emit_runtime, find_libfuzzer, native_target, validate_target,
};

/// Parse and type-check a standalone modern Plenty binary without executing
/// it or generating native code. Requires a parameterless `main` returning
/// `()`, `i32`, `Result[(), E]`, or `Result[i32, E]`, just like
/// [`compile_source_to_executable`].
#[cfg_attr(feature = "profile", hotpath::measure)]
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
    let checked = op::check(&program.ops);
    teardown((program, heap));
    checked
}
