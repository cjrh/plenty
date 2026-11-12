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

/// Drop a check's final data, or leak it when `options` skip teardown.
pub(crate) fn teardown<T>(value: T, options: &CheckOptions) {
    if options.skip_teardown {
        std::mem::forget(value);
    }
}

mod atomic;
mod boxed;
mod bytes;
pub mod cache;
mod channel;
mod clist;
#[path = "../plenty-runtime/src/clist_layout.rs"]
mod clist_layout;
mod closure;
mod codegen;
mod collection;
#[path = "../plenty-runtime/src/collection_opcodes.rs"]
mod collection_opcodes;
#[cfg(test)]
mod collection_operand_tests;
mod compress;
mod control;
mod crypto;
mod diagnostic;
mod dimension;
mod effects;
mod executor;
mod exports;
mod filesystem;
mod foreign;
mod frontend;
mod generator;
mod hash;
mod hash_hex;
// Public for the integration test that runs compiled operators against
// `interval::exact`, the reference the interval rules are tested against.
#[doc(hidden)]
pub mod interval;
mod library;
mod library_metadata;
#[path = "../plenty-runtime/src/list_layout.rs"]
mod list_layout;
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
pub mod script;
mod source_path;
mod sqlite;
mod stream;
mod sum;
mod system;
mod tail_abi;
mod target;
mod text;
mod threading;
mod time;
mod tls;
mod toolchain;
mod type_facts;
mod unicode;
mod unloading;
mod value;

/// Editor queries over a checked file, for the language server (issue #155).
pub mod ide {
    pub use crate::frontend::ide::{
        Analysis, Diagnostic, File, InferredBinding, Location, Reference, Symbol, SymbolKind,
        analyze, index, relocate, rename, standard_modules,
    };
}

pub use codegen::{
    Dump, DumpOptions, DumpStage, compile_file_to_executable,
    compile_file_to_executable_with_options, compile_file_to_object,
    compile_file_to_object_with_options, compile_package_to_executable,
    compile_source_to_executable, compile_source_to_executable_with_options,
    compile_source_to_object, dump_file,
};
pub use diagnostic::{Lint, Warning};
pub use library::{
    LibraryArtifacts, LibraryKind, LibraryOptions, compile_file_to_library, emit_runtime_interface,
    runtime_interface_source,
};
pub use library_metadata::{
    LibraryInterface, extract_library_interface, read_library_interfaces, verify_library_interface,
};
pub use target::{native_cpu_baseline, native_target, validate_target};
pub use toolchain::{CompileOptions, RuntimeArtifacts, emit_runtime, find_libfuzzer};

/// Parse and type-check a standalone modern Plenty binary without executing
/// it or generating native code. Requires a parameterless `main` returning
/// `()`, `i32`, `Result[(), E]`, or `Result[i32, E]`, just like
/// [`compile_source_to_executable`].
#[cfg_attr(feature = "profile", hotpath::measure)]
pub fn check_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    check_source_with_options(source, &CheckOptions::default()).map(drop)
}

/// What a check verifies beyond what compilation checks, and how it ends.
/// Only the check entry points take these: compiling never needs them, and
/// the compile caches never see their results (issue #517).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// Check every method of every generic class and enum instance the
    /// program creates, as if each were called, rather than only the
    /// methods the program uses (`--check-all-methods`, issue #517).
    pub all_methods: bool,
    /// Leak, rather than free, the checked program (its operations and
    /// string pool). Freeing it only returns memory to a process that is
    /// about to exit: a command-line check that exits next sets this, saving
    /// about 2% of checking instructions (issue #321). A process that keeps
    /// compiling, such as the language server, must not: each check would
    /// keep its memory until the process exits.
    pub skip_teardown: bool,
}

/// [`check_source`] with the checks `options` adds. Returns the program's
/// [lint warnings](Warning), which never fail a check.
pub fn check_source_with_options(
    source: &str,
    options: &CheckOptions,
) -> Result<Vec<Warning>, Box<dyn std::error::Error>> {
    let mut heap = value::Heap::default();
    let mut program = frontend::check(source, &mut heap, *options)?;
    op::check(&program.ops)?;
    Ok(std::mem::take(&mut program.warnings))
}

/// Check a complete application, resolving imports from the supplied root or
/// the entry file's directory. No native code is emitted or executed.
pub fn check_file(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    check_file_with_options(path, root, true, &CheckOptions::default()).map(drop)
}

/// Check a library module and its imports without requiring an application main.
pub fn check_module_file(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    check_file_with_options(path, root, false, &CheckOptions::default()).map(drop)
}

/// [`check_file`] (`require_main`) or [`check_module_file`] with the checks
/// `options` adds. Returns the program's [lint warnings](Warning), which
/// never fail a check.
pub fn check_file_with_options(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
    require_main: bool,
    options: &CheckOptions,
) -> Result<Vec<Warning>, Box<dyn std::error::Error>> {
    let mut heap = value::Heap::default();
    let mut program = frontend::compile_file(path, root, require_main, &mut heap, *options)?;
    let checked = op::check(&program.ops);
    let warnings = std::mem::take(&mut program.warnings);
    teardown((program, heap), options);
    checked.map(|()| warnings)
}
