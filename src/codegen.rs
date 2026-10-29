//! AOT code generation via Cranelift (§11.1, §12.3 — phases c.1–c.4).
//!
//! Lowers a Plenty `Op` stream to a Cranelift module emitted as a native
//! object file. The object exports one symbol, `plenty_main`, which the
//! Rust runtime in `plenty-runtime` calls from its `main`. User
//! function definitions become locally-linked symbols inside the same
//! object, callable from each other and from `plenty_main`.
//!
//! The lowering threads a *compile-time stack* of `(cranelift::Value, Ty)`
//! pairs through the op stream. Each Plenty op becomes a small CLIF
//! sequence that pops its inputs from this stack and pushes its result:
//! operand values become SSA values, with no runtime operand stack.
//! The `Ty` tag travels alongside each SSA value so cast lowering,
//! signed/unsigned arithmetic dispatch, and `Display` formatting can pick
//! the right CLIF instruction without re-doing the type checker's work.
//!
//! Phase c.2 adds user functions: `Op::DefineFn` is hoisted out into one
//! Cranelift function per source-level definition (with `CallConv::Tail`
//! so `return_call` can implement Plenty's mandatory TCO); `Op::Call`
//! emits a regular call; `Op::TailCall` emits `return_call`, which
//! terminates the current block and reuses the caller's frame.
//! `Op::LoadLocal` reads the i-th function input via a CLIF `Variable`
//! defined once at function entry.
//!
//! Phase c.3 adds `Op::Match`: each arm becomes its own Cranelift block,
//! large contiguous integer/tag sets use a bounded jump table; other
//! patterns lower to a linear chain of `brif` compares (wildcards become
//! unconditional jumps and short-circuit the chain; a `bool` scrutinee
//! branches on itself and its second value is a jump), and a single join
//! block reunites the non-terminating arms with block params carrying
//! the agreed stack shape. Arms whose tail op is a `TailCall` skip the
//! join jump — `return_call` is already the block terminator.
//!
//! Phase c.4 adds strings. Every string literal referenced by the source
//! (whether by `Op::PushStr` or by a `Pattern::Str` inside a match) is
//! emitted as one static-data symbol per `StrId`, carrying the UTF-8
//! managed header, cached byte/scalar lengths, and exact UTF-8 payload. `Op::PushStr` lowers to `global_value` —
//! the data's address — and onto the compile-time stack tagged as
//! `Ty::Str` (CLIF `i64` for the host pointer width). `Op::Add` and
//! `Op::Eq` now dispatch on operand types: integer pairs take the
//! existing CLIF paths; `Str Str` calls `plenty_concat` / `plenty_str_eq`
//! in the Rust runtime. `Display` prints strings via `plenty_print_str`,
//! and `match` patterns of type `Str` become `plenty_str_eq` + `brif`.
//! A byte literal (`Op::PushBytes`) uses the same immortal layout and is
//! tagged `Ty::ByteStr`; it is never released and never reaches a text helper.
//!
//! The precompiled `plenty-runtime` archive is embedded in the compiler. AOT
//! builds write it beside the Cranelift object and invoke `cc` only to link.
//! Runtime compilation happens when building Plenty, never for individual programs.

use crate::hash::HashMap;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    AbiParam, Block, BlockArg, Function, InstBuilder, Signature, TrapCode, UserFuncName, types,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings;
use cranelift_codegen::settings::Configurable;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::ObjectModule;

use crate::cache::{Cache, Lookup, ProgramKey};
use crate::frontend::Sources;
use crate::lexer;
mod atomic;
mod casts;
mod channels;
mod checked;

mod clist_simd;
mod clists;
mod closures;
mod collections;
mod control;
mod coverage;
mod dump;
mod empty_slots;
mod enums;
mod executors;
mod exits;
mod exports;
mod foreign;
mod generators;
mod induction;
mod inline;
mod known;
mod last_use;
mod matching;
mod metadata;
mod mutex;
mod numeric;
mod output;
mod portable_simd;
mod references;
mod relax;
pub(crate) use output::describe_error;
pub(crate) use relax::relax_local_addresses;
mod inline_calls;
mod leaf_calls;
mod reachability;
mod scalar_captures;
mod sites;
mod small_calls;
mod sorting;
mod symbols;
#[cfg(test)]
mod tail_tests;
mod threading;
mod threads;
mod traps;
use crate::op::{self, ElifArm, FnSig, IntegerOp, MatchArm, Op, Pattern, Ty};
use crate::value::{Heap, StrId, Value};
use generators::GeneratorContext;
use output::{Output, Worker};
pub(crate) use traps::checks as trap_checks;

// ---- Cranelift API reference ----
//
// Cranelift's user-facing API is not indexed by context7; these notes
// record gotchas paid for during phases c.2–c.5.5 so the next agent does
// not pay for them again. Source under `~/.cargo/registry/src/.../cranelift-*-0.137.*/`.
//
// * **Crate split.** The `cranelift` umbrella crate's `module` and `object`
//   sub-crates are opt-in features absent from its default feature set
//   (`default = ["std", "frontend"]`). We depend on the five individual
//   crates (`cranelift-codegen`, `-frontend`, `-module`, `-object`,
//   `-native`) directly to avoid the feature trap.
//
// * **Tail calls.** `return_call` lives in the `cranelift-codegen-meta`
//   crate (instruction definition), called as `builder.ins().return_call(
//   func_ref, &args)`. It requires `CallConv::Tail` on **both** caller and
//   callee, and on x86_64 it also requires `preserve_frame_pointers =
//   "true"` in the ISA flags — otherwise emission panics at codegen time
//   ("frame pointers aren't fundamentally required for tail calls, but
//   the current implementation relies on them being present").
//
// * **Variables.** `Variable` is constructed by `bcx.declare_var(ty) ->
//   Variable`, not `Variable::new(i)`. The entity macro provides
//   `from_u32` / `from_bits` but no public `new`.
//
// * **`BlockArg`, not `Value`.** `jump` and `brif` take
//   `impl IntoIterator<Item = &BlockArg>`, not `&[Value]`. Convert with
//   `vals.iter().map(|v| BlockArg::Value(*v))`. `BlockArg` lives in
//   `cranelift_codegen::ir`.
//
// * **Static data.** Pattern is: `DataDescription::new()`,
//   `dd.define(bytes.into_boxed_slice())`, `module.define_data(id, &dd)`.
//   `ObjectModule::declare_data(name, Linkage, writable, tls) -> DataId`
//   (a single `DataId`, **not** a tuple — the inner `module.rs` returns
//   `(DataId, Linkage)` but the public trait wraps that). To use the data
//   inside a function body, call `module.declare_data_in_func(data_id,
//   func) -> GlobalValue` and then `builder.ins().symbol_value(ty, gv)`.
//
// * **Checked arithmetic results.** The `*_overflow` instructions
//   (`sadd_overflow`, `uadd_overflow`, `ssub_overflow`, `usub_overflow`,
//   `smul_overflow`, `umul_overflow`) return `(Value, Value)` (result,
//   overflow-flag) as a Rust tuple **directly** — they are not normal
//   multi-result instructions and `inst_results` does not apply.
//
// * **Block-filling rule.** A block must be fully terminated (via
//   `brif` / `jump` / `return` / `trap`) before calling
//   `switch_to_block` on a different block. You cannot fill a target
//   block's body while its predecessor is still open ("fill your block
//   before switching"). Consequence: trap blocks shared across an entire
//   function cannot be defined lazily; emit trap sequences inline at
//   each call site (see `trap_if`).

/// Compile a complete modern binary program to a native executable at `output`.
/// A parameterless `main` returning `()`, `i32`, or a Result wrapping either is required. The source is
/// lexed, lowered to typed operations, and checked; the
/// resulting op stream is lowered to a temp object file; the embedded
/// Rust runtime archive is written alongside it; `cc` links the pair into the
/// final executable and the temps are removed.
///
/// Uses the default `cc` driver. See [`compile_source_to_executable_with_options`]
/// to select another driver or pass native link arguments.
pub fn compile_source_to_executable(source: &str, output: &Path) -> Result<()> {
    compile_source_to_executable_with_options(source, output, &crate::CompileOptions::default())
}

/// Compile source with an explicitly configured native linker driver.
pub fn compile_source_to_executable_with_options(
    source: &str,
    output: &Path,
    options: &crate::CompileOptions,
) -> Result<()> {
    crate::validate_target(options.target.as_deref())?;
    compile_sources_to_executable(&Sources::Text(source), output, options, false)
}

/// Compile a file and its absolute imports. The source root defaults to the
/// entry file's directory; in-memory source compilation never reads imports.
pub fn compile_file_to_executable(path: &Path, output: &Path, root: Option<&Path>) -> Result<()> {
    compile_file_to_executable_with_options(path, output, root, &crate::CompileOptions::default())
}

/// Compile a file and imports with an explicitly configured linker driver.
pub fn compile_file_to_executable_with_options(
    path: &Path,
    output: &Path,
    root: Option<&Path>,
    options: &crate::CompileOptions,
) -> Result<()> {
    crate::validate_target(options.target.as_deref())?;
    compile_sources_to_executable(&Sources::file(path, root)?, output, options, false)
}

/// Compile the entry file of `graph.packages[0]`; each package sees only its
/// own modules, `std`, and its direct dependencies' aliases.
pub fn compile_package_to_executable(
    path: &Path,
    graph: &crate::project::SourceGraph,
    output: &Path,
    options: &crate::CompileOptions,
) -> Result<()> {
    compile_graph_to_executable(path, graph, output, options, false)
}

fn compile_graph_to_executable(
    path: &Path,
    graph: &crate::project::SourceGraph,
    output: &Path,
    options: &crate::CompileOptions,
    test_hooks: bool,
) -> Result<()> {
    crate::validate_target(options.target.as_deref())?;
    let sources = Sources::Graph {
        entry: path.to_path_buf(),
        graph: std::borrow::Cow::Borrowed(graph),
        appendix: None,
    };
    compile_sources_to_executable(&sources, output, options, test_hooks)
}

/// Compile a test file that defines `main` for `plenty test`, with the
/// failure-injection hooks of a test build.
pub(crate) fn compile_test_program_to_executable(
    path: &Path,
    graph: &crate::project::SourceGraph,
    output: &Path,
    options: &crate::CompileOptions,
) -> Result<()> {
    compile_graph_to_executable(path, graph, output, options, true)
}

/// Compile a file of test functions with the runner's generated `main` and
/// the failure-injection hooks of a test build.
pub(crate) fn compile_tests_to_executable(
    path: &Path,
    graph: &crate::project::SourceGraph,
    tests: &[(String, crate::frontend::TestReturn)],
    output: &Path,
    options: &crate::CompileOptions,
) -> Result<()> {
    crate::validate_target(options.target.as_deref())?;
    let sources = Sources::Graph {
        entry: path.to_path_buf(),
        graph: std::borrow::Cow::Borrowed(graph),
        appendix: Some(crate::frontend::test_appendix(tests)),
    };
    compile_sources_to_executable(&sources, output, options, true)
}

/// Historical stack syntax, retained for backend regression tests.
///
/// Only an unterminated string literal is reported with a `line:column`
/// location. Legacy tokens carry no positions, so every other syntax or
/// type error has none.
pub fn compile_legacy_source_to_executable(source: &str, output: &Path) -> Result<()> {
    compile_legacy_source_to_executable_with_options(
        source,
        output,
        &crate::CompileOptions::default(),
    )
}

/// Historical syntax with an explicitly configured linker driver.
pub fn compile_legacy_source_to_executable_with_options(
    source: &str,
    output: &Path,
    options: &crate::CompileOptions,
) -> Result<()> {
    crate::validate_target(options.target.as_deref())?;
    let toks = lexer::lex(source)?;
    let mut heap = Heap::default();
    let ops = op::compile(&toks, &mut heap)?;
    compile_ops_to_executable(ops, &heap, output, false, options, false)
}

/// With `test_hooks`, the executable is an instrumented build: it also links
/// [`TEST_HOOKS`], which wraps the C library's allocation and file calls, and
/// std's `__test_fault` reaches it. Every other executable is linked exactly
/// as without the hooks.
#[cfg_attr(feature = "profile", hotpath::measure)]
fn compile_ops_to_executable(
    ops: Vec<Op>,
    heap: &Heap,
    output: &Path,
    returns_status: bool,
    options: &crate::CompileOptions,
    test_hooks: bool,
) -> Result<()> {
    op::check(&ops)?;

    let test_hooks = test_hooks || options.test_hooks;
    let workspace = tempfile::tempdir()?;
    let obj_path = workspace.path().join("program.o");
    let build = Build::new(options, test_hooks);
    let cache = build.cache.clone();
    compile_to_object(ops, heap, &obj_path, returns_status, build)?;
    report_cache(options, cache.as_deref());
    link_object(&workspace, &obj_path, output, options, test_hooks)
}

/// [`compile_ops_to_executable`] for an application's sources, through the
/// program cache.
fn compile_sources_to_executable(
    sources: &Sources,
    output: &Path,
    options: &crate::CompileOptions,
    test_hooks: bool,
) -> Result<()> {
    let test_hooks = test_hooks || options.test_hooks;
    let workspace = tempfile::tempdir()?;
    let obj_path = workspace.path().join("program.o");
    let build = Build::new(options, test_hooks);
    let cache = build.cache.clone();
    program_object(sources, &obj_path, build)?;
    report_cache(options, cache.as_deref());
    link_object(&workspace, &obj_path, output, options, test_hooks)
}

/// Write the object for `sources` to `object`: the program cache's, when its
/// entry for them is current, or else a new build's, which then becomes the
/// entry (issue #317).
#[cfg_attr(feature = "profile", hotpath::measure)]
fn program_object(sources: &Sources, object: &Path, build: Build) -> Result<()> {
    let started = std::time::Instant::now();
    // Builds that check cache hits recompile every function; they are tests
    // of the function cache.
    let cached = build
        .cache
        .clone()
        .filter(|_| !build.check_cache_hits && !build.functions_only)
        .and_then(|cache| {
            let settings = format!(
                "optimize {} test hooks {} coverage {}",
                build.optimize, build.test_hooks, build.coverage
            );
            Some((cache, ProgramKey::new(&sources.identity()?, &settings)))
        });
    let mut seen = false;
    if let Some((cache, key)) = &cached {
        match cache.load_program(key, |inputs| sources.current(inputs), object)? {
            Lookup::Hit => return Ok(()),
            Lookup::Miss { seen: before } => seen = before,
        }
    }
    let mut heap = Heap::default();
    let program = sources.compile(&mut heap)?;
    op::check(&program.ops)?;
    compile_to_object(program.ops, &heap, object, program.returns_status, build)?;
    if let Some((cache, key)) = &cached {
        cache.store_program(key, &program.inputs, object, started.elapsed(), seen);
    }
    Ok(())
}

/// Link the application object at `obj_path` with the runtime, and the fuzz
/// driver or test hooks the options ask for, in `workspace`.
fn link_object(
    workspace: &tempfile::TempDir,
    obj_path: &Path,
    output: &Path,
    options: &crate::CompileOptions,
    test_hooks: bool,
) -> Result<()> {
    let rt_path = workspace.path().join("libplenty_runtime.a");
    std::fs::write(&rt_path, RUNTIME_ARCHIVE)?;
    // Fuzz and `plenty test` executables are run and debugged where they are
    // built, never shipped, so they keep their symbols.
    let unstripped;
    let options = if (options.fuzz.is_some() || test_hooks) && options.strip {
        unstripped = crate::CompileOptions {
            strip: false,
            ..options.clone()
        };
        &unstripped
    } else {
        options
    };
    if let Some(libfuzzer) = &options.fuzz {
        let driver = workspace.path().join("plenty_fuzz_driver.o");
        std::fs::write(&driver, FUZZ_DRIVER)?;
        let mut inputs = vec![
            driver.into_os_string(),
            "-Wl,--wrap=main".into(),
            libfuzzer.clone().into_os_string(),
        ];
        // libFuzzer's `memcmp` and `strcmp` interceptors report the strings
        // compared to it, so it can try them, when its build has them.
        let interceptors = libfuzzer.with_file_name(FUZZ_INTERCEPTORS);
        if interceptors.is_file() {
            inputs.push(interceptors.into_os_string());
        }
        inputs.push("-lstdc++".into());
        return crate::toolchain::link(
            obj_path,
            &rt_path,
            RUNTIME_LINK_ARGS,
            output,
            options,
            &inputs,
            &[],
        );
    }
    if !test_hooks {
        return crate::toolchain::link(
            obj_path,
            &rt_path,
            RUNTIME_LINK_ARGS,
            output,
            options,
            &[],
            &[],
        );
    }
    let hooks = workspace.path().join("plenty_test_hooks.o");
    std::fs::write(&hooks, TEST_HOOKS)?;
    crate::toolchain::link(
        obj_path,
        &rt_path,
        RUNTIME_LINK_ARGS,
        output,
        options,
        &[hooks.into_os_string(), TEST_HOOK_WRAPS.into()],
        &[],
    )
}

/// Code generation choices that do not change the program's meaning.
#[derive(Clone, Default)]
struct Build {
    /// Ask Cranelift to optimize for speed.
    optimize: bool,
    /// Lower std's `__test_fault` to the explicitly enabled hook object.
    test_hooks: bool,
    /// Count each block's runs for libFuzzer ([`coverage`]).
    coverage: bool,
    /// Reuse compiled functions from here, and add this build's to it.
    cache: Option<Arc<Cache>>,
    /// Never reuse or record a whole program's object.
    functions_only: bool,
    /// Compile every cached function again and assert the code matches
    /// (Cranelift's `enable_incremental_compilation_cache_checks`). Only
    /// tests set it: the flag is part of the cache key, so these builds use
    /// entries of their own.
    check_cache_hits: bool,
    /// Capture functions' text for `--dump`.
    dump: Option<DumpOptions>,
}

impl Build {
    fn new(options: &crate::CompileOptions, test_hooks: bool) -> Self {
        Self {
            optimize: options.optimize,
            test_hooks,
            coverage: options.fuzz.is_some(),
            cache: Cache::open(&options.cache),
            functions_only: !options.program_cache,
            check_cache_hits: false,
            dump: None,
        }
    }

    /// The default build with the cache the environment selects.
    fn from_environment() -> Self {
        Self {
            cache: Cache::open(&crate::cache::CacheMode::Environment),
            ..Self::default()
        }
    }
}

/// Print the cache counts when `--cache-stats` asks for them.
fn report_cache(options: &crate::CompileOptions, cache: Option<&Cache>) {
    if options.cache_stats {
        match cache {
            Some(cache) => eprintln!("{}", cache.stats()),
            None => eprintln!("compile cache: off"),
        }
    }
}

/// Failure injection for `plenty test` and `--test-hooks` builds, compiled from
/// `plenty-runtime/native/test_hooks.c`. Linked only into instrumented executables.
const TEST_HOOKS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plenty_test_hooks.o"));
/// The libFuzzer entry of `--fuzz` executables, compiled from
/// `plenty-runtime/native/fuzz_driver.c`. Linked into no other executable.
const FUZZ_DRIVER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plenty_fuzz_driver.o"));
/// Found beside the `--libfuzzer` archive, as clang installs them.
const FUZZ_INTERCEPTORS: &str = "libclang_rt.fuzzer_interceptors.a";
/// The C library calls the hook object intercepts. Only references from the
/// program and runtime objects are redirected; the C library's own internal
/// calls are not.
const TEST_HOOK_WRAPS: &str = "-Wl,--wrap=malloc,--wrap=calloc,--wrap=realloc,--wrap=posix_memalign,--wrap=open,--wrap=open64,--wrap=openat,--wrap=read,--wrap=write,--wrap=pread64,--wrap=pwrite64,--wrap=writev,--wrap=fsync,--wrap=ftruncate64";

/// Prebuilt for the compiler's target and embedded so an installed or relocated
/// Plenty binary never needs runtime source files, Cargo, or rustc at run time.
pub(crate) const RUNTIME_ARCHIVE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/libplenty_runtime.a"));
pub(crate) const RUNTIME_LINK_ARGS: &str =
    include_str!(concat!(env!("OUT_DIR"), "/runtime-link-args.txt"));

/// Emit an application object without invoking a linker. It exports
/// `plenty_main`; link exactly one application object with the matching runtime.
pub fn compile_source_to_object(source: &str, output: &Path) -> Result<()> {
    crate::validate_target(None)?;
    program_object(&Sources::Text(source), output, Build::from_environment())
}

/// Emit an application object, resolving absolute Plenty imports.
pub fn compile_file_to_object(path: &Path, output: &Path, root: Option<&Path>) -> Result<()> {
    compile_file_to_object_with_options(path, output, root, &crate::CompileOptions::default())
}

/// Emit an application object with the code generation options of
/// `options`; its linker settings are unused.
pub fn compile_file_to_object_with_options(
    path: &Path,
    output: &Path,
    root: Option<&Path>,
    options: &crate::CompileOptions,
) -> Result<()> {
    crate::validate_target(None)?;
    let sources = Sources::file(path, root)?;
    let build = Build::new(options, false);
    let cache = build.cache.clone();
    program_object(&sources, output, build)?;
    report_cache(options, cache.as_deref());
    Ok(())
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub use dump::{Dump, DumpOptions, DumpStage};

/// Compile the program at `path` and return the text `dump` asks for,
/// without writing an object or linking. Both caches are bypassed, so every
/// function is compiled, and the text does not depend on the cache or the
/// worker count.
pub fn dump_file(
    path: &Path,
    root: Option<&Path>,
    options: &crate::CompileOptions,
    dump: &DumpOptions,
) -> Result<Dump> {
    crate::validate_target(None)?;
    let sources = Sources::file(path, root)?;
    let mut heap = Heap::default();
    let program = sources.compile(&mut heap)?;
    op::check(&program.ops)?;
    dump_program(
        program.ops,
        &heap,
        program.returns_status,
        options.optimize,
        crate::parallel::workers(),
        dump,
    )
}

fn dump_program(
    ops: Vec<Op>,
    heap: &Heap,
    returns_status: bool,
    optimize: bool,
    workers: usize,
    dump: &DumpOptions,
) -> Result<Dump> {
    let build = Build {
        optimize,
        functions_only: true,
        dump: Some(dump.clone()),
        ..Build::default()
    };
    let mut out = object_module(ops, heap, Some(returns_status), &[], None, workers, build)?;
    let entries = std::mem::take(&mut out.dumped);
    Ok(out.dump.as_ref().expect("dump plan").finish(entries))
}

/// Lower `ops` to a native object file at `output`.
///
/// The object exports `plenty_main` (`() -> i32`) and one locally-linked
/// symbol per user-defined Plenty function, plus one read-only data
/// symbol per source-level string literal whose bytes come from `heap`.
/// Link the object with the packaged `plenty-runtime` archive to produce an
/// executable. With `returns_status`, the final `i32` operand is the process
/// status; unit entrypoints and legacy programs return zero on completion.
fn compile_to_object(
    ops: Vec<Op>,
    heap: &Heap,
    output: &Path,
    returns_status: bool,
    build: Build,
) -> Result<()> {
    emit_object(ops, heap, output, Some(returns_status), &[], None, build)
}

pub(crate) fn emit_library_object(
    ops: Vec<Op>,
    exports: &[crate::exports::Export],
    heap: &Heap,
    output: &Path,
    interface: &crate::exports::Interface,
    options: &crate::CompileOptions,
) -> Result<()> {
    let build = Build::new(options, false);
    let cache = build.cache.clone();
    emit_object(ops, heap, output, None, exports, Some(interface), build)?;
    report_cache(options, cache.as_deref());
    Ok(())
}

#[cfg_attr(feature = "profile", hotpath::measure)]
fn emit_object(
    ops: Vec<Op>,
    heap: &Heap,
    output: &Path,
    entry: Option<bool>,
    exports: &[crate::exports::Export],
    interface: Option<&crate::exports::Interface>,
    build: Build,
) -> Result<()> {
    let product = object_module(
        ops,
        heap,
        entry,
        exports,
        interface,
        crate::parallel::workers(),
        build,
    )?
    .finish()?;
    // Written section by section: the file's bytes are never all in memory
    // beside the sections (issue #54).
    let file = std::io::BufWriter::new(std::fs::File::create(output)?);
    product.object.write_stream(file)
}

/// The object's bytes are the same for every `workers` count and for every
/// state of the cache.
#[cfg(test)]
fn object_bytes(
    ops: Vec<Op>,
    heap: &Heap,
    entry: Option<bool>,
    exports: &[crate::exports::Export],
    interface: Option<&crate::exports::Interface>,
    workers: usize,
    build: Build,
) -> Result<Vec<u8>> {
    let module = object_module(ops, heap, entry, exports, interface, workers, build)?;
    Ok(module.finish()?.emit()?)
}

/// Every definition of the object, not yet serialized. Takes the program to
/// release each function's operations once its code is built, rather than hold
/// them all until the last function is (issue #54).
#[cfg_attr(feature = "profile", hotpath::measure)]
fn object_module(
    mut ops: Vec<Op>,
    heap: &Heap,
    entry: Option<bool>,
    exports: &[crate::exports::Export],
    interface: Option<&crate::exports::Interface>,
    workers: usize,
    build: Build,
) -> Result<Output> {
    reachability::prune(&mut ops, exports);
    small_calls::substitute(&mut ops, build.optimize, workers);
    crate::tail_abi::prepare(&mut ops, workers);
    last_use::prepare(&mut ops, workers);
    let opt_level = if build.optimize { "speed" } else { "none" };
    let targets = output::Targets {
        ordinary: optimized_isa("backtracking", opt_level, build.check_cache_hits)?,
        single_pass: optimized_isa("single_pass", opt_level, build.check_cache_hits)?,
    };
    let mut out = Output::new(targets, build.cache.clone())?;
    out.coverage = build.coverage;

    let mut runtime = declare_runtime(&mut out.module, build.test_hooks)?;
    if entry.is_some() && !build.coverage {
        let table = out.declare_traps()?;
        let register = out
            .module
            .declare_function("plenty_traps_register", Linkage::Import, &{
                let mut sig = out.module.make_signature();
                sig.call_conv = CallConv::SystemV;
                sig.params.push(AbiParam::new(PTR_TY));
                sig
            })?;
        runtime.signal_traps = Some((register, table));
    }

    // Every symbol a body refers to by name is declared before any body is
    // built, so forward references and mutual recursion resolve, and the
    // threads building bodies only read the declarations.
    let mut user_fns: HashMap<String, UserFn> = HashMap::default();
    let mut bodies = Vec::new();
    collect_user_fns(&ops, &mut out.module, &mut user_fns, &mut bodies)?;
    if build.optimize {
        leaf_calls::prepare(&bodies, &mut user_fns);
        inline_calls::prepare(&bodies, &mut user_fns);
        clist_simd::prepare(&bodies, &mut user_fns);
    }
    sites::mark_context_preserving(&bodies, &mut user_fns);
    runtime.drop_hooks = user_fns
        .iter()
        .filter_map(|(name, f)| Some((name.clone(), f.drop_callback?)))
        .collect();
    for &hook in runtime.drop_hooks.values() {
        out.root_traps(hook);
    }
    runtime.standard_calls = user_fns.values().any(|f| f.standard_library);
    let threads =
        threads::declare_adapters(&ops, workers, &mut out.module, &mut runtime.thread_adapters)?;
    let executors =
        executors::declare_adapters(&ops, workers, &mut out.module, &mut runtime.thread_adapters)?;
    let sorts =
        sorting::declare_adapters(&ops, workers, &mut out.module, &mut runtime.sort_adapters)?;
    let runtime = runtime;
    // `bodies` is in declaration order, as `FuncId`s are. The map's own order
    // changes from run to run, which would make the object differ between
    // identical builds.
    for (name, _) in &bodies {
        let decl = user_fns.get_mut(name).unwrap();
        if decl.generator.is_some() {
            decl.frame_mask = Some(generators::declare_frame_mask(
                name,
                decl,
                &mut out.module,
                &runtime,
            )?);
        }
    }
    let main_id = match entry {
        Some(_) => Some(out.module.declare_function(
            "plenty_main",
            Linkage::Export,
            &main_signature(),
        )?),
        None => None,
    };

    // One read-only data symbol per source string literal, collected from
    // every `PushStr` or `Pattern::Str`. The compiler's string pool is the
    // source of truth for the literal bytes.
    let str_data = declare_str_data(&ops, heap, &mut out.module)?;
    let mut dump = build.dump.as_ref().map(|options| {
        dump::Plan::new(
            options,
            build.optimize,
            &ops,
            &user_fns,
            &out.module,
            heap,
            &str_data,
        )
    });

    // An immortal empty string substitutes for the legacy input helper's EOF.
    let eof_empty_str = declare_eof_empty_str(&mut out.module)?;

    // From here `bodies` alone holds the top-level functions' operations, so
    // each function's are released once its job is built, rather than held
    // until the last function is (issue #54). Top-level `DefineFn`s are
    // no-ops in `plenty_main`.
    ops.retain(|op| !matches!(op, Op::DefineFn(..)));
    if let (Some(plan), Some(id)) = (&mut dump, main_id) {
        plan.set_entry_ops(id, &ops, heap);
    }
    out.dump = dump.map(Arc::new);
    let mut jobs: Vec<Job> = threads
        .into_iter()
        .map(|(task, id)| Job::Thread(task, id))
        .chain(
            executors
                .into_iter()
                .map(|(job, id)| Job::Executor(job, id)),
        )
        .chain(sorts.into_iter().map(|(sort, id)| Job::Sort(sort, id)))
        .chain(
            bodies
                .into_iter()
                .map(|(name, body)| Job::Function(name, body)),
        )
        .collect();
    if let (Some(returns_status), Some(id)) = (entry, main_id) {
        jobs.push(Job::Main(ops, returns_status, id));
    }
    let (fns, str_data) = (&user_fns, &str_data);
    out.build(jobs, workers, &runtime, |job, worker| {
        let runtime = &runtime;
        match job {
            Job::Thread(task, id) => {
                threads::build_adapter(&task, id, fns, str_data, eof_empty_str, runtime, worker)
            }
            Job::Executor(job, id) => {
                executors::build_adapter(&job, id, fns, str_data, eof_empty_str, runtime, worker)
            }
            Job::Sort(sort, id) => {
                sorting::build_adapter(&sort, id, fns, str_data, eof_empty_str, runtime, worker)
            }
            Job::Function(name, body) => {
                build_user_function(&name, &body, fns, str_data, eof_empty_str, runtime, worker)
            }
            Job::Main(ops, returns_status, id) => build_main(
                &ops,
                returns_status,
                id,
                fns,
                str_data,
                eof_empty_str,
                runtime,
                worker,
            ),
        }
    })?;
    exports::emit(exports, interface, &user_fns, &runtime, &mut out)?;
    Ok(out)
}

/// Functions built together on one thread, in the order they are placed in
/// the object.
enum Job {
    Thread(Arc<crate::threading::Task>, FuncId),
    Executor(Arc<crate::executor::Job>, FuncId),
    Sort(Arc<crate::collection::KeySort>, FuncId),
    /// A user function, with its drop callback or, for a generator, its
    /// resume function.
    Function(String, Arc<[Op]>),
    Main(Vec<Op>, bool, FuncId),
}

/// The unoptimized baseline ISA used by backend unit tests.
#[cfg(test)]
fn host_isa() -> Result<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    target_isa("backtracking")
}

#[cfg(test)]
fn target_isa(
    regalloc_algorithm: &str,
) -> Result<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    optimized_isa(regalloc_algorithm, "none", false)
}

/// Build the target's baseline ISA without inspecting the running CPU.
/// On x86-64 this is the original SSE2 baseline, not host-specific extensions.
/// `opt_level` is `none` for debug builds, which favor compilation speed, and
/// `speed` for release builds. Language checks are part of the IR, so both
/// levels keep identical semantics.
fn optimized_isa(
    regalloc_algorithm: &str,
    opt_level: &str,
    check_cache_hits: bool,
) -> Result<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    crate::validate_target(None)?;
    let mut flags = settings::builder();
    flags.set("regalloc_algorithm", regalloc_algorithm)?;
    flags.set("opt_level", opt_level)?;
    if check_cache_hits {
        flags.set("enable_incremental_compilation_cache_checks", "true")?;
    }
    // `is_pic` so the object can be linked into a position-independent
    // executable, which is what every modern Linux/macOS toolchain
    // produces by default. Runtime calls and local data still avoid the GOT
    // (`symbols::runtime_colocated`, `relax`).
    flags.set("is_pic", "true")?;
    flags.set("enable_llvm_abi_extensions", "true")?;
    // Frame pointers must be preserved for `return_call` emission on
    // x86_64: the backend hooks the tail-call stack-arg fixup off the
    // frame-pointer prologue/epilogue. Without this, lowering any
    // Plenty TailCall panics inside Cranelift with "the current
    // implementation relies on [frame pointers] being present".
    flags.set("preserve_frame_pointers", "true")?;
    // A frame larger than a page touches each page it spans, so it cannot
    // step over a thread's one-page guard into the mapping below. The runtime
    // reports the resulting fault as a stack overflow.
    flags.set("enable_probestack", "true")?;
    flags.set("probestack_strategy", "inline")?;
    // The verifier checks the IR this compiler builds, not the user's program,
    // and costs about a fifth of emission (issue #55). Debug builds, which run
    // the tests, keep it.
    flags.set(
        "enable_verifier",
        if cfg!(debug_assertions) {
            "true"
        } else {
            "false"
        },
    )?;
    let isa_builder = cranelift_native::builder_with_options(false)
        .map_err(|e| -> Box<dyn Error> { e.into() })?;
    let isa = isa_builder.finish(settings::Flags::new(flags))?;
    if isa.triple().to_string() != crate::native_target() || isa.pointer_type() != PTR_TY {
        return Err(
            "native ISA does not match the packaged runtime's target and 64-bit pointer ABI".into(),
        );
    }
    Ok(isa)
}

/// Handles for every runtime helper the lowerer can call. We declare
/// them all up-front so each call site is just `module.declare_func_in_func`
/// plus an `ins().call`.
struct Runtime {
    standard_calls: bool,
    caller_enter: FuncId,
    caller_leave: FuncId,
    thread_start: FuncId,
    thread_join: FuncId,
    /// `plenty_mutex_lock_slow(*const u32)` and `plenty_mutex_wake(*const u32)`:
    /// the futex paths of a contended `Mutex`.
    mutex_lock_slow: FuncId,
    mutex_wake: FuncId,
    thread_adapters: HashMap<String, FuncId>,
    /// Type metadata declared so far. Only the thread placing code in the
    /// object declares symbols; the lock lets threads building IR share the
    /// rest of `Runtime`.
    type_data: std::sync::Mutex<HashMap<Ty, DataId>>,
    /// `__del__` adapters by method name, linked into class descriptors.
    drop_hooks: HashMap<String, FuncId>,
    list_len: FuncId,
    /// `plenty_list_rows(list) -> first row address` and
    /// `plenty_list_row_bytes(list) -> row size`, for borrowed iteration.
    list_rows: FuncId,
    list_row_bytes: FuncId,
    /// `plenty_list_row(list, index, site) -> row address`, for explicit
    /// element references (`&xs[i]`, `&mut xs[i]`).
    list_row: FuncId,
    list_scalar_get: FuncId,
    /// `plenty_list_sort_by(list, comparator, environment, descending)`.
    list_sort_by: FuncId,
    /// `plenty_order_compare(a_low, a_high, b_low, b_high, descriptor) -> i64`.
    order_compare: FuncId,
    /// Narrow byte and text queries on a borrowed owner (issue #364); see
    /// `CollectionCodegen::narrow_query`.
    bytes_len: FuncId,
    bytes_item: FuncId,
    bytes_set_item: FuncId,
    /// `plenty_bytes_push(owner, byte) -> high word of Result[(), AllocError]`.
    bytes_push: FuncId,
    text_len: FuncId,
    text_byte_len: FuncId,
    text_is_ascii: FuncId,
    text_at: FuncId,
    text_ord: FuncId,
    text_starts_with: FuncId,
    text_ends_with: FuncId,
    text_find: FuncId,
    text_rfind: FuncId,
    text_count: FuncId,
    byte_str_item: FuncId,
    text_at_byte: FuncId,
    text_next_byte: FuncId,
    text_prev_byte: FuncId,
    text_ascii_equal_folded: FuncId,
    text_find_in: FuncId,
    /// Generated key-sort comparators, by adapter name.
    sort_adapters: HashMap<String, FuncId>,
    channel: FuncId,
    control: FuncId,
    executor: FuncId,
    retain: FuncId,
    release: FuncId,
    generator_init: FuncId,
    generator_finish: FuncId,
    print_i8: FuncId,
    print_i16: FuncId,
    print_i32: FuncId,
    print_i64: FuncId,
    print_u8: FuncId,
    print_u16: FuncId,
    print_u32: FuncId,
    print_u64: FuncId,
    print_bool: FuncId,
    print_str: FuncId,
    print_open_bracket: FuncId,
    print_close_bracket: FuncId,
    print_space: FuncId,
    /// `plenty_concat(*const u8, *const u8) -> *const u8` — c.4.
    concat: FuncId,
    /// `plenty_str_eq(*const u8, *const u8) -> i8` — c.4.
    str_eq: FuncId,
    /// `plenty_trap_overflow(site) -> !` — prints `error: integer overflow`
    /// with the site's position to stderr and `exit(1)`s. The lowerer calls
    /// this from the overflow branch of every checked arithmetic op.
    trap_overflow: FuncId,
    /// `plenty_trap_shift(site) -> !` — prints `error: shift count outside
    /// integer width` with the site's position to stderr and `exit(1)`s.
    trap_shift: FuncId,
    /// `plenty_trap_div_zero(site) -> !` — prints `error: division by zero`
    /// with the site's position to stderr and `exit(1)`s. Called from the
    /// zero-check branch of the `Div` lowering.
    trap_div_zero: FuncId,
    /// `plenty_trap_unwrap(site, option) -> !` — prints `error: unwrap on
    /// Nothing` or `error: unwrap on Err` with the site's position to stderr
    /// and `exit(1)`s.
    trap_unwrap: FuncId,
    /// `plenty_trap_cast_range(site) -> !` — prints `error: integer cast out
    /// of range` with the site's position to stderr and `exit(1)`s. Called
    /// from the range-check branch of a bare numeric cast.
    trap_cast_range: FuncId,
    trap_clist_index: FuncId,
    /// `plenty_trap_list_index(index, length, site) -> !`: an inline scalar
    /// list row index out of bounds.
    trap_list_index: FuncId,
    /// In an executable whose failed checks trap ([`traps`]):
    /// `plenty_traps_register(table)` and the object's check table. `None`
    /// keeps the helper calls.
    signal_traps: Option<(FuncId, DataId)>,
    /// `plenty_readline() -> *const u8` — read one newline-terminated
    /// line from stdin, strip its newline, validate UTF-8, and return an owned
    /// counted string. NULL is internal EOF, never a language string.
    readline: FuncId,
    /// `plenty_contains(*const u8 haystack, *const u8 needle) -> i8` —
    /// returns 1 if `needle` is a byte-substring of `haystack`, 0
    /// otherwise. Both inputs are borrowed counted strings.
    contains: FuncId,
    /// `plenty_write_caller_location() -> i8`, behind std's
    /// `__caller_location`.
    caller_location: FuncId,
    /// `plenty_test_fault(kind, count, code) -> i8`, defined only by the hook
    /// object that `plenty test` links; absent from every other build, where
    /// std's `__test_fault` is the constant false.
    test_fault: Option<FuncId>,
    /// `plenty_actor_heap(op, argument) -> i64`, behind std's `__actor_heap`.
    actor_heap: FuncId,
    /// `plenty_println(*const u8) -> ()` — write the string raw to
    /// stdout, followed by a single `\n`. The bare-text output
    /// primitive; `plenty_print_str` (the `.` path) escapes and
    /// quotes, `plenty_println` does not.
    println: FuncId,
}

fn declare_runtime(module: &mut ObjectModule, test_hooks: bool) -> Result<Runtime> {
    fn one_arg(module: &mut ObjectModule, name: &str, arg: types::Type) -> Result<FuncId> {
        let mut sig = module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(arg));
        Ok(module.declare_function(name, Linkage::Import, &sig)?)
    }
    fn two_args_one_return(
        module: &mut ObjectModule,
        name: &str,
        a: types::Type,
        b: types::Type,
        ret: types::Type,
    ) -> Result<FuncId> {
        let mut sig = module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(a));
        sig.params.push(AbiParam::new(b));
        sig.returns.push(AbiParam::new(ret));
        Ok(module.declare_function(name, Linkage::Import, &sig)?)
    }
    fn nullary(module: &mut ObjectModule, name: &str) -> Result<FuncId> {
        let mut sig = module.make_signature();
        sig.call_conv = CallConv::SystemV;
        Ok(module.declare_function(name, Linkage::Import, &sig)?)
    }
    fn helper(
        module: &mut ObjectModule,
        name: &str,
        params: &[types::Type],
        ret: Option<types::Type>,
    ) -> Result<FuncId> {
        let mut sig = module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params
            .extend(params.iter().map(|ty| AbiParam::new(*ty)));
        sig.returns.extend(ret.map(AbiParam::new));
        Ok(module.declare_function(name, Linkage::Import, &sig)?)
    }
    Ok(Runtime {
        standard_calls: false,
        caller_enter: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.push(AbiParam::new(PTR_TY));
            sig.returns.push(AbiParam::new(PTR_TY));
            module.declare_function("plenty_caller_enter", Linkage::Import, &sig)?
        },
        caller_leave: one_arg(module, "plenty_caller_leave", PTR_TY)?,
        control: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([
                AbiParam::new(types::I64),
                AbiParam::new(PTR_TY),
                AbiParam::new(PTR_TY),
            ]);
            module.declare_function("plenty_control", Linkage::Import, &sig)?
        },
        executor: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([
                AbiParam::new(types::I64),
                AbiParam::new(PTR_TY),
                AbiParam::new(PTR_TY),
                AbiParam::new(PTR_TY),
            ]);
            module.declare_function("plenty_executor", Linkage::Import, &sig)?
        },
        channel: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([
                AbiParam::new(types::I64),
                AbiParam::new(PTR_TY),
                AbiParam::new(PTR_TY),
                AbiParam::new(PTR_TY),
            ]);
            module.declare_function("plenty_channel", Linkage::Import, &sig)?
        },
        thread_start: two_args_one_return(
            module,
            "plenty_thread_start",
            PTR_TY,
            PTR_TY,
            types::I32,
        )?,
        thread_join: one_arg(module, "plenty_thread_join", PTR_TY)?,
        mutex_lock_slow: one_arg(module, "plenty_mutex_lock_slow", PTR_TY)?,
        mutex_wake: one_arg(module, "plenty_mutex_wake", PTR_TY)?,
        thread_adapters: Default::default(),
        generator_init: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([AbiParam::new(types::I64); 6]);
            module.declare_function("plenty_generator_init", Linkage::Import, &sig)?
        },
        generator_finish: one_arg(module, "plenty_generator_finish", PTR_TY)?,
        type_data: Default::default(),
        drop_hooks: HashMap::default(),
        retain: one_arg(module, "plenty_retain", PTR_TY)?,
        release: one_arg(module, "plenty_release", PTR_TY)?,
        list_len: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.push(AbiParam::new(PTR_TY));
            sig.returns.push(AbiParam::new(types::I64));
            module.declare_function("plenty_list_len", Linkage::Import, &sig)?
        },
        list_rows: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.push(AbiParam::new(PTR_TY));
            sig.returns.push(AbiParam::new(PTR_TY));
            module.declare_function("plenty_list_rows", Linkage::Import, &sig)?
        },
        list_row_bytes: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.push(AbiParam::new(PTR_TY));
            sig.returns.push(AbiParam::new(types::I64));
            module.declare_function("plenty_list_row_bytes", Linkage::Import, &sig)?
        },
        list_scalar_get: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            // List, index, trap site.
            sig.params.extend([
                AbiParam::new(PTR_TY),
                AbiParam::new(types::I64),
                AbiParam::new(PTR_TY),
            ]);
            sig.returns.push(AbiParam::new(types::I64));
            module.declare_function("plenty_list_scalar_get", Linkage::Import, &sig)?
        },
        list_row: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            // List, index, trap site.
            sig.params.extend([
                AbiParam::new(PTR_TY),
                AbiParam::new(types::I64),
                AbiParam::new(PTR_TY),
            ]);
            sig.returns.push(AbiParam::new(PTR_TY));
            module.declare_function("plenty_list_row", Linkage::Import, &sig)?
        },
        list_sort_by: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([
                AbiParam::new(PTR_TY),
                AbiParam::new(PTR_TY),
                AbiParam::new(PTR_TY),
                AbiParam::new(types::I64),
            ]);
            module.declare_function("plenty_list_sort_by", Linkage::Import, &sig)?
        },
        order_compare: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([AbiParam::new(types::I64); 4]);
            sig.params.push(AbiParam::new(PTR_TY));
            sig.returns.push(AbiParam::new(types::I64));
            module.declare_function("plenty_order_compare", Linkage::Import, &sig)?
        },
        // Owner or text word, then index and trap site where the query has them.
        bytes_len: helper(module, "plenty_bytes_len", &[PTR_TY], Some(types::I64))?,
        bytes_item: helper(
            module,
            "plenty_bytes_item",
            &[PTR_TY, types::I64, PTR_TY],
            Some(types::I64),
        )?,
        bytes_set_item: helper(
            module,
            "plenty_bytes_set_item",
            &[PTR_TY, types::I64, types::I64, PTR_TY],
            None,
        )?,
        bytes_push: helper(
            module,
            "plenty_bytes_push",
            &[PTR_TY, types::I64],
            Some(types::I64),
        )?,
        text_len: helper(module, "plenty_text_len", &[PTR_TY], Some(types::I64))?,
        text_byte_len: helper(module, "plenty_text_byte_len", &[PTR_TY], Some(types::I64))?,
        text_is_ascii: helper(module, "plenty_text_is_ascii", &[PTR_TY], Some(types::I64))?,
        text_at: helper(
            module,
            "plenty_text_at",
            &[PTR_TY, types::I64, PTR_TY],
            Some(PTR_TY),
        )?,
        text_ord: helper(module, "plenty_text_ord", &[PTR_TY], Some(types::I64))?,
        text_starts_with: helper(
            module,
            "plenty_text_starts_with",
            &[PTR_TY, PTR_TY],
            Some(types::I64),
        )?,
        text_ends_with: helper(
            module,
            "plenty_text_ends_with",
            &[PTR_TY, PTR_TY],
            Some(types::I64),
        )?,
        text_find: helper(
            module,
            "plenty_text_find",
            &[PTR_TY, PTR_TY],
            Some(types::I64),
        )?,
        text_rfind: helper(
            module,
            "plenty_text_rfind",
            &[PTR_TY, PTR_TY],
            Some(types::I64),
        )?,
        text_count: helper(
            module,
            "plenty_text_count",
            &[PTR_TY, PTR_TY],
            Some(types::I64),
        )?,
        byte_str_item: helper(
            module,
            "plenty_byte_str_item",
            &[PTR_TY, types::I64, PTR_TY],
            Some(types::I64),
        )?,
        text_at_byte: helper(
            module,
            "plenty_text_at_byte",
            &[PTR_TY, types::I64],
            Some(PTR_TY),
        )?,
        text_prev_byte: helper(
            module,
            "plenty_text_prev_byte",
            &[PTR_TY, types::I64],
            Some(types::I64),
        )?,
        text_ascii_equal_folded: helper(
            module,
            "plenty_text_ascii_equal_folded",
            &[PTR_TY, PTR_TY],
            Some(types::I64),
        )?,
        text_find_in: helper(
            module,
            "plenty_text_find_in",
            &[PTR_TY, PTR_TY, types::I64],
            Some(types::I64),
        )?,
        text_next_byte: helper(
            module,
            "plenty_text_next_byte",
            &[PTR_TY, types::I64],
            Some(types::I64),
        )?,
        sort_adapters: Default::default(),
        print_i8: one_arg(module, "plenty_print_i8", types::I8)?,
        print_i16: one_arg(module, "plenty_print_i16", types::I16)?,
        print_i32: one_arg(module, "plenty_print_i32", types::I32)?,
        print_i64: one_arg(module, "plenty_print_i64", types::I64)?,
        print_u8: one_arg(module, "plenty_print_u8", types::I8)?,
        print_u16: one_arg(module, "plenty_print_u16", types::I16)?,
        print_u32: one_arg(module, "plenty_print_u32", types::I32)?,
        print_u64: one_arg(module, "plenty_print_u64", types::I64)?,
        print_bool: one_arg(module, "plenty_print_bool", types::I8)?,
        print_str: one_arg(module, "plenty_print_str", PTR_TY)?,
        print_open_bracket: nullary(module, "plenty_print_open_bracket")?,
        print_close_bracket: nullary(module, "plenty_print_close_bracket")?,
        print_space: nullary(module, "plenty_print_space")?,
        concat: two_args_one_return(module, "plenty_concat", PTR_TY, PTR_TY, PTR_TY)?,
        str_eq: two_args_one_return(module, "plenty_str_eq", PTR_TY, PTR_TY, types::I8)?,
        trap_overflow: one_arg(module, "plenty_trap_overflow", PTR_TY)?,
        trap_shift: one_arg(module, "plenty_trap_shift", PTR_TY)?,
        trap_div_zero: one_arg(module, "plenty_trap_div_zero", PTR_TY)?,
        trap_unwrap: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params
                .extend([AbiParam::new(PTR_TY), AbiParam::new(types::I64)]);
            module.declare_function("plenty_trap_unwrap", Linkage::Import, &sig)?
        },
        trap_cast_range: one_arg(module, "plenty_trap_cast_range", PTR_TY)?,
        trap_clist_index: one_arg(module, "plenty_trap_clist_index", PTR_TY)?,
        trap_list_index: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            // Index as written, length, trap site.
            sig.params.extend([
                AbiParam::new(types::I64),
                AbiParam::new(types::I64),
                AbiParam::new(PTR_TY),
            ]);
            module.declare_function("plenty_trap_list_index", Linkage::Import, &sig)?
        },
        signal_traps: None,
        readline: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.returns.push(AbiParam::new(PTR_TY));
            module.declare_function("plenty_readline", Linkage::Import, &sig)?
        },
        contains: two_args_one_return(module, "plenty_contains", PTR_TY, PTR_TY, types::I8)?,
        println: one_arg(module, "plenty_println", PTR_TY)?,
        caller_location: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.returns.push(AbiParam::new(types::I8));
            module.declare_function("plenty_write_caller_location", Linkage::Import, &sig)?
        },
        test_fault: if test_hooks {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([
                AbiParam::new(types::I64),
                AbiParam::new(types::I64),
                AbiParam::new(types::I32),
            ]);
            sig.returns.push(AbiParam::new(types::I8));
            Some(module.declare_function("plenty_test_fault", Linkage::Import, &sig)?)
        } else {
            None
        },
        actor_heap: two_args_one_return(
            module,
            "plenty_actor_heap",
            types::I64,
            types::I64,
            types::I64,
        )?,
    })
}

/// Managed values use one pointer on the current native 64-bit host target.
const PTR_TY: types::Type = types::I64;

/// `FunctionBuilder::finalize` reads only the pointer type, to spill values
/// declared for stack maps; Plenty declares none.
pub(crate) const FRONTEND: cranelift_codegen::isa::TargetFrontendConfig =
    cranelift_codegen::isa::TargetFrontendConfig {
        default_call_conv: CallConv::SystemV,
        pointer_width: target_lexicon::PointerWidth::U64,
        page_size_align_log2: 12,
    };

/// Serialize the runtime's 32-byte immortal string prefix and exact UTF-8 bytes.
/// AOT targets the host, so serialization uses native byte order.
fn string_literal_bytes((data, scalars): (&[u8], usize)) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32 + data.len());
    for word in [u64::MAX, 0, data.len() as u64, scalars as u64] {
        bytes.extend_from_slice(&word.to_ne_bytes());
    }
    bytes.extend_from_slice(data);
    bytes
}

#[cfg_attr(feature = "profile", hotpath::measure)]
fn declare_str_data(
    ops: &[Op],
    heap: &Heap,
    module: &mut ObjectModule,
) -> Result<HashMap<StrId, DataId>> {
    let mut ids: Vec<StrId> = Vec::new();
    let mut seen: HashMap<StrId, ()> = HashMap::default();
    collect_str_ids(ops, &mut ids, &mut seen);

    let mut out: HashMap<StrId, DataId> = HashMap::default();
    for (i, id) in ids.into_iter().enumerate() {
        // The name only has to be unique within the module; the linker
        // never sees it externally (Linkage::Local). A stable index
        // keeps the symbol names predictable when reading disassembly.
        let name = format!("plenty_str_{i}");
        let data_id = module.declare_data(&name, Linkage::Local, false, false)?;
        let bytes = string_literal_bytes(heap.data(id));
        let mut desc = DataDescription::new();
        desc.define(bytes.into_boxed_slice());
        desc.set_align(8);
        module.define_data(data_id, &desc)?;
        out.insert(id, data_id);
    }
    Ok(out)
}

/// An aligned, immortal, counted empty string for the legacy EOF fallback.
fn declare_eof_empty_str(module: &mut ObjectModule) -> Result<DataId> {
    let id = module.declare_data("plenty_readline_eof_empty", Linkage::Local, false, false)?;
    let mut desc = DataDescription::new();
    desc.define(string_literal_bytes((b"", 0)).into_boxed_slice());
    desc.set_align(8);
    module.define_data(id, &desc)?;
    Ok(id)
}

/// Recursive helper for [`declare_str_data`]: emits `StrId`s in
/// first-seen order, skipping duplicates so the same literal appearing
/// in multiple places shares one data symbol.
fn collect_str_ids(ops: &[Op], out: &mut Vec<StrId>, seen: &mut HashMap<StrId, ()>) {
    for op in ops {
        match op {
            Op::Loop { condition, body } => {
                collect_str_ids(condition, out, seen);
                collect_str_ids(body, out, seen);
            }
            Op::CheckedBlock { body, .. } | Op::CheckedArith { cleanup: body, .. } => {
                collect_str_ids(body, out, seen);
            }
            Op::PushStr(id) | Op::PushBytes(id) if seen.insert(*id, ()).is_none() => out.push(*id),
            Op::Match(arms) => {
                for arm in arms.iter() {
                    if let Pattern::Str(id) = arm.pattern
                        && seen.insert(id, ()).is_none()
                    {
                        out.push(id);
                    }
                    collect_str_ids(&arm.body, out, seen);
                }
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                collect_str_ids(then, out, seen);
                for arm in elifs.iter() {
                    collect_str_ids(&arm.condition, out, seen);
                    collect_str_ids(&arm.body, out, seen);
                }
                collect_str_ids(otherwise, out, seen);
            }
            Op::DefineFn(_, f) => collect_str_ids(&f.body, out, seen),
            _ => {}
        }
    }
}

/// Declaration for a single user-defined Plenty function. Pass 1
/// allocates one of these per `DefineFn` reachable from the source set;
/// Pass 2 reads it back when emitting bodies and resolving calls.
struct UserFn {
    /// Small scalar leaf body, retained only for optimized call substitution.
    inline: Option<Arc<leaf_calls::Leaf>>,
    /// Small constructor body lowered at each call site in optimized builds;
    /// see [`inline_calls`].
    inline_body: Option<Arc<[Op]>>,
    column_predicate: Option<Arc<clist_simd::Predicate>>,
    standard_library: bool,
    drop_callback: Option<FuncId>,
    generator: Option<Ty>,
    resume: Option<FuncId>,
    /// A generator's frame slot types, defined by
    /// [`generators::declare_frame_mask`].
    frame_mask: Option<DataId>,
    id: FuncId,
    sig: Arc<FnSig>,
    locals: Arc<[Ty]>,
    /// File that defines the body, relative to the source root, for trap
    /// positions.
    source: Option<Arc<str>>,
    /// Every return leaves the caller context as the call found it, so a
    /// standard-library caller need not save and restore it; see
    /// [`sites::mark_context_preserving`].
    preserves_caller: bool,
}

/// Build a Cranelift `Signature` from a Plenty `FnSig`. User functions
/// always use `CallConv::Tail`: that is the only call convention in
/// Cranelift 0.137 that supports `return_call`, which is how we lower
/// Plenty's tail-call op. Tail-convention functions can still be called
/// non-tail (the verifier only requires matching conventions on
/// `return_call`), so `plenty_main` — which has SystemV convention,
/// because it is called from C — invokes user functions with a regular
/// `call` instruction.
fn user_fn_signature(sig: &FnSig) -> Signature {
    let mut cl = Signature::new(CallConv::Tail);
    let layout = crate::tail_abi::layout(sig);
    cl.params
        .extend(layout.inputs.into_iter().map(AbiParam::new));
    cl.returns
        .extend(layout.outputs.into_iter().map(AbiParam::new));
    cl
}

/// Walk `ops` recursively, declaring every `DefineFn` we encounter — at
/// the top level, nested inside another definition's body, or inside a
/// match arm. Each definition becomes a Cranelift symbol with linkage
/// `Local` (visible only within this object). Redefinition is rejected
/// here, before any codegen, per the AOT closed-world rule (§11.1).
fn collect_user_fns(
    ops: &[Op],
    module: &mut ObjectModule,
    out: &mut HashMap<String, UserFn>,
    bodies: &mut Vec<(String, Arc<[Op]>)>,
) -> Result<()> {
    for op in ops {
        match op {
            Op::DefineFn(name, f) => {
                if out.contains_key(name) {
                    return Err(
                        format!("AOT compilation does not allow redefining `{name}`").into(),
                    );
                }
                let cl_sig = user_fn_signature(&f.sig);
                // Source names must never alias runtime helpers or the C entry
                // point (a user may legitimately define `plenty_main`).
                let symbol = format!("__plenty_fn_{name}");
                let id = module.declare_function(&symbol, Linkage::Local, &cl_sig)?;
                out.insert(
                    name.clone(),
                    UserFn {
                        inline: None,
                        inline_body: None,
                        column_predicate: None,
                        drop_callback: if name.starts_with("__plenty_class_")
                            && name.ends_with(".__del__")
                        {
                            let mut signature = module.make_signature();
                            signature.params.push(AbiParam::new(PTR_TY));
                            Some(module.declare_function(
                                &format!("__plenty_drop_{name}"),
                                Linkage::Local,
                                &signature,
                            )?)
                        } else {
                            None
                        },
                        id,
                        generator: f.generator.clone(),
                        resume: if f.generator.is_some() {
                            Some(module.declare_function(
                                &format!("__plenty_resume_{name}"),
                                Linkage::Local,
                                &generators::resume_signature(),
                            )?)
                        } else {
                            None
                        },
                        frame_mask: None,
                        sig: Arc::clone(&f.sig),
                        locals: Arc::clone(&f.locals),
                        source: f.origins.trap_file.clone(),
                        standard_library: f.origins.standard_library,
                        preserves_caller: false,
                    },
                );
                bodies.push((name.clone(), Arc::clone(&f.body)));
                collect_user_fns(&f.body, module, out, bodies)?;
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    collect_user_fns(&arm.body, module, out, bodies)?;
                }
            }
            Op::CheckedBlock { body, .. } => collect_user_fns(body, module, out, bodies)?,

            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                collect_user_fns(then, module, out, bodies)?;
                for arm in elifs.iter() {
                    collect_user_fns(&arm.condition, module, out, bodies)?;
                    collect_user_fns(&arm.body, module, out, bodies)?;
                }
                collect_user_fns(otherwise, module, out, bodies)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Which locals need stack storage: those with inline storage and those whose
/// address the body takes with `BorrowLocal`. The rest stay in CLIF variables,
/// so taking one local's address does not move unrelated locals into memory
/// (issue #361). Thread operations add none: task storage is a separate stack
/// slot, the task token local is only read and written, and a worker reaches
/// parent locals only through borrowed arguments (issue #373).
fn addressed_locals(ops: &[Op], locals: &[(Variable, Ty)]) -> Vec<bool> {
    let mut addressed: Vec<bool> = locals.iter().map(|(_, t)| t.has_inline_storage()).collect();
    mark_borrowed_locals(ops, &mut addressed);
    addressed
}

/// Mark each local borrowed anywhere in `ops`, including nested bodies.
fn mark_borrowed_locals(ops: &[Op], addressed: &mut [bool]) {
    for op in ops {
        match op {
            Op::BorrowLocal(i, _) => addressed[*i as usize] = true,
            Op::Match(arms) => arms
                .iter()
                .for_each(|arm| mark_borrowed_locals(&arm.body, addressed)),
            Op::Loop { condition, body } => {
                mark_borrowed_locals(condition, addressed);
                mark_borrowed_locals(body, addressed);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                mark_borrowed_locals(then, addressed);
                for arm in elifs.iter() {
                    mark_borrowed_locals(&arm.condition, addressed);
                    mark_borrowed_locals(&arm.body, addressed);
                }
                mark_borrowed_locals(otherwise, addressed);
            }
            Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => {
                mark_borrowed_locals(cleanup, addressed)
            }
            Op::CheckedBlock { body, .. } => mark_borrowed_locals(body, addressed),

            _ => {}
        }
    }
}

/// Stack storage for the locals [`addressed_locals`] selects.
struct LocalFrame {
    base: cranelift_codegen::ir::Value,
    /// Byte offset from `base` of each local kept in memory.
    offsets: Vec<Option<i32>>,
}

/// Keep the `addressed` locals in one stack frame, storing their entry
/// values: the first `inputs` locals are parameters.
fn local_frame(
    bcx: &mut FunctionBuilder<'_>,
    locals: &[(Variable, Ty)],
    addressed: &[bool],
    inputs: usize,
) -> Option<LocalFrame> {
    addressed.contains(&true).then(|| {
        let mut size = 0;
        let offsets: Vec<Option<i32>> = locals
            .iter()
            .zip(addressed)
            .map(|((_, ty), &addressed)| {
                addressed.then(|| {
                    let offset = size;
                    size += ty.slot_bytes() as i32;
                    offset
                })
            })
            .collect();
        let slot = bcx.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            size as u32,
            4,
        ));
        let base = bcx.ins().stack_addr(PTR_TY, slot, 0);
        for (i, ((var, ty), offset)) in locals.iter().zip(&offsets).enumerate() {
            let Some(offset) = offset else { continue };
            let packed = if i < inputs || ty.managed() {
                let value = bcx.use_var(*var);
                enums::pack_value(bcx, value, ty)
            } else {
                let zero = bcx.ins().iconst(types::I64, 0);
                bcx.ins().uextend(types::I128, zero)
            };
            bcx.ins().store(
                cranelift_codegen::ir::MemFlagsData::trusted(),
                packed,
                base,
                *offset,
            );
        }
        LocalFrame { base, offsets }
    })
}

/// Emit the body of one user function. Each input becomes a CLIF
/// `Variable` defined once at entry from the matching block parameter;
/// `Op::LoadLocal(i)` later reads that variable. If the body falls
/// through without a tail call, emit a `return` carrying the values
/// remaining on the compile-time stack (the type checker has already
/// ensured those values match the declared outputs).
#[cfg_attr(feature = "profile", hotpath::measure)]
fn build_user_function(
    name: &str,
    body: &[Op],
    fns: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    out: &mut Worker,
) -> Result<()> {
    let decl = &fns[name];
    if decl.generator.is_some() {
        return generators::build_generator(name, body, fns, str_data, eof_empty_str, runtime, out);
    }
    let mut module = out.symbols();
    let cl_sig = user_fn_signature(&decl.sig);

    let mut func = out.take_function(decl.id, cl_sig);
    {
        let mut bcx = FunctionBuilder::new(&mut func, &mut out.builder);
        let entry = bcx.create_block();
        bcx.append_block_params_for_function_params(entry);
        bcx.switch_to_block(entry);
        bcx.seal_block(entry);

        // One `Variable` per input. We bind each from its block param
        // right at entry. Using `Variable` rather than the raw SSA
        // block-param value makes c.3 cleaner — match arms become new
        // blocks, and a `Variable` is visible across blocks where a
        // raw block-param value would have to be threaded explicitly.
        let empty_word = bcx.ins().iconst(types::I64, 0);
        let empty_double = bcx.ins().uextend(types::I128, empty_word);
        let mut locals: Vec<(Variable, Ty)> = Vec::with_capacity(decl.sig.inputs.len());
        for (i, (_, ty)) in decl.sig.inputs.iter().enumerate() {
            let var = bcx.declare_var(clif_type(ty.clone()));
            let param = bcx.block_params(entry)[i];
            bcx.def_var(var, param);
            locals.push((var, ty.clone()));
        }
        for ty in decl.locals.iter() {
            let var = bcx.declare_var(clif_type(ty.clone()));
            // Null denotes an uninitialized ownership slot, never a source value.
            if ty.managed() {
                bcx.def_var(var, if ty.wide() { empty_double } else { empty_word });
            }
            locals.push((var, ty.clone()));
        }

        let portable_simd = portable_simd::State::new(
            body,
            &locals,
            decl.sig.inputs.len(),
            &mut bcx,
            module.optimizes(),
        );
        let mut addressed = addressed_locals(body, &locals);
        for (index, addressed) in addressed.iter_mut().enumerate() {
            if portable_simd.promoted(index as u8) {
                *addressed = false;
            }
        }
        let local_frame = local_frame(&mut bcx, &locals, &addressed, decl.sig.inputs.len());
        let return_storage = decl
            .sig
            .outputs
            .iter()
            .any(Ty::has_inline_storage)
            .then(|| bcx.block_params(entry)[decl.sig.inputs.len()]);
        let scalar_captures =
            scalar_captures::Captures::new(body, &locals, decl.sig.inputs.len(), fns, &mut bcx);
        let mut lower = Lowerer {
            bcx: &mut bcx,
            module: &mut module,
            runtime,
            user_fns: fns,
            str_data,
            eof_empty_str,
            locals: &locals,
            stack: Vec::new(),
            terminated: false,
            loop_targets: Vec::new(),
            checked_targets: Vec::new(),
            generator: None,
            local_frame,
            return_storage,
            return_types: &decl.sig.outputs,
            collection_scratch: None,
            fresh_load: None,
            scalar_captures,
            portable_simd,
            slot_copies: Vec::new(),
            empty_slot: Some((empty_word, empty_double)),
            exits: Some(Default::default()),
            counters: Default::default(),
            position: sites::Position::in_file(decl.source.clone(), decl.standard_library),
            trap_blocks: HashMap::default(),
            moves: inline::Moves::find(body),
        };
        // Argument addresses belong to the caller. Snapshot inline values into
        // this function's locals before any mutation or nested call can occur.
        for (i, (_, ty)) in decl.sig.inputs.iter().enumerate() {
            if ty.has_inline_storage() {
                let value = lower.read_local(i as u8);
                lower.write_local(i as u8, value);
            }
        }
        let simd = portable_simd::operation(name, decl, lower.module.optimizes());
        if let Some(simd) = &simd {
            let args = (0..decl.sig.inputs.len())
                .map(|index| lower.read_local(index as u8))
                .collect::<Vec<_>>();
            lower.lower_portable_simd(simd, &args);
        }
        for op in if simd.is_some() { &[] } else { body } {
            if lower.terminated {
                // A `TailCall` already terminated this block; any
                // trailing op is dead. `op::compile` only emits
                // `TailCall` at the very end of a body (or a match-arm
                // tail), so this branch is defensive.
                break;
            }
            lower
                .lower(op)
                .map_err(|e| -> Box<dyn Error> { format!("in `{name}`: {e}").into() })?;
        }
        if !lower.terminated {
            lower
                .leave(lower.stack.clone(), &Arc::from([]))
                .map_err(|e| -> Box<dyn Error> { format!("in `{name}`: {e}").into() })?;
        }
        lower
            .finish_exits()
            .map_err(|e| -> Box<dyn Error> { format!("in `{name}`: {e}").into() })?;
        lower.seal_trap_blocks();
        lower.finish_portable_simd();
        bcx.finalize(crate::codegen::FRONTEND);
    }
    out.define(decl.id, func, module);
    if let Some(id) = decl.drop_callback {
        let mut module = out.symbols();
        let mut signature = module.make_signature();
        signature.params.push(AbiParam::new(PTR_TY));
        let mut func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
        let mut b = FunctionBuilder::new(&mut func, &mut out.builder);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        b.seal_block(block);
        let receiver = b.block_params(block)[0];
        let receiver = b.ins().uextend(types::I128, receiver);
        let callee = module.declare_func_in_func(decl.id, b.func);
        b.ins().call(callee, &[receiver]);
        b.ins().return_(&[]);
        b.finalize(crate::codegen::FRONTEND);
        out.define(id, func, module);
    }
    Ok(())
}

/// `plenty_main`: exported, no arguments, returns `i32`. The Rust
/// runtime's native `main` forwards into this and returns its result as the
/// process exit code. SystemV convention because the caller (the Rust
/// runtime) speaks the host's C ABI; user functions use `CallConv::Tail` and
/// can still be invoked from here via a regular `call`.
fn main_signature() -> Signature {
    let mut signature = Signature::new(CallConv::SystemV);
    signature.returns.push(AbiParam::new(types::I32));
    signature
}

/// Build `plenty_main` — the entry point the Rust runtime forwards to.
/// Top-level `DefineFn` ops are already removed (their bodies are built
/// separately by [`build_user_function`]); everything else lowers
/// against an initially-empty compile-time stack, with no locals
/// in scope.
#[allow(clippy::too_many_arguments)]
fn build_main(
    ops: &[Op],
    returns_status: bool,
    main_id: FuncId,
    fns: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    out: &mut Worker,
) -> Result<()> {
    let mut module = out.symbols();
    let main_sig = main_signature();

    let mut func = Function::with_name_signature(UserFuncName::user(0, main_id.as_u32()), main_sig);
    {
        let mut bcx = FunctionBuilder::new(&mut func, &mut out.builder);
        let entry = bcx.create_block();
        bcx.append_block_params_for_function_params(entry);
        bcx.switch_to_block(entry);
        bcx.seal_block(entry);

        let mut lower = Lowerer {
            bcx: &mut bcx,
            module: &mut module,
            runtime,
            user_fns: fns,
            str_data,
            eof_empty_str,
            locals: &[],
            stack: Vec::new(),
            terminated: false,
            loop_targets: Vec::new(),
            checked_targets: Vec::new(),
            generator: None,
            local_frame: None,
            return_storage: None,
            return_types: &[],
            collection_scratch: None,
            fresh_load: None,
            scalar_captures: Default::default(),
            portable_simd: Default::default(),
            slot_copies: Vec::new(),
            empty_slot: None,
            exits: None,
            counters: Default::default(),
            position: Default::default(),
            trap_blocks: HashMap::default(),
            moves: Default::default(),
        };
        if let Some((register, table)) = runtime.signal_traps {
            let table = lower.module.declare_data_in_func(table, lower.bcx.func);
            let table = lower.bcx.ins().symbol_value(PTR_TY, table);
            let register = lower.module.declare_func_in_func(register, lower.bcx.func);
            lower.bcx.ins().call(register, &[table]);
        }
        for op in ops {
            lower.lower(op)?;
        }
        // `plenty_main` never tail-calls (its convention doesn't
        // support it), so `lower.terminated` is always false here.
        let status = if returns_status {
            lower.pop_typed(Ty::I32)?.0
        } else {
            lower.bcx.ins().iconst(types::I32, 0)
        };
        lower.release_stack();
        lower.bcx.ins().return_(&[status]);
        lower.seal_trap_blocks();
        bcx.finalize(crate::codegen::FRONTEND);
    }
    out.define(main_id, func, module);
    Ok(())
}

/// The CLIF type backing each Plenty value. Plenty's signed/unsigned
/// distinction lives in the `Ty` tag we carry alongside the SSA value;
/// Cranelift treats both with the same machine type, the individual
/// instruction (`sdiv` vs `udiv`, `icmp slt` vs `icmp ult`) picks the
/// interpretation. `Str` is a host pointer (`PTR_TY`), the address of
/// a counted immutable object in read-only data or the managed runtime heap.
fn clif_type(ty: Ty) -> types::Type {
    crate::tail_abi::native_type(&ty)
}

/// Reinterpret an integer `Value` as the signed host integer Cranelift's
/// `iconst` accepts. The destination CLIF type preserves the low bits, so this
/// represents unsigned maxima such as `18446744073709551615u64` exactly.
fn int_value_bits(value: Value) -> i64 {
    match value {
        Value::I8(n) => i64::from(n),
        Value::I16(n) => i64::from(n),
        Value::I32(n) => i64::from(n),
        Value::I64(n) => n,
        Value::U8(n) => i64::from(n),
        Value::U16(n) => i64::from(n),
        Value::U32(n) => i64::from(n),
        Value::U64(n) => n as i64,
    }
}

/// Width of an integer type in bits. Used to drive cast lowering.
fn width_bits(ty: Ty) -> u8 {
    match ty {
        Ty::I8 | Ty::U8 => 8,
        Ty::I16 | Ty::U16 => 16,
        Ty::I32 | Ty::U32 => 32,
        Ty::I64 | Ty::U64 => 64,
        _ => panic!("non-integer in width_bits"),
    }
}

fn is_signed(ty: Ty) -> bool {
    matches!(ty, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64)
}

/// One CLIF stack slot, paired with the Plenty `Ty` that produced it.
type StackEntry = (cranelift_codegen::ir::Value, Ty);

/// Which checked-overflow CLIF instruction family to emit. The signed
/// variants are picked by the `Ty` tag at the call site, so this enum
/// only distinguishes the three ops, not their signedness.
#[derive(Clone, Copy, PartialEq)]
enum ArithKind {
    Add,
    Sub,
    Mul,
}

/// Which trap block to branch into on a failed check. The kinds map
/// one-to-one to the runtime trap helpers and their diagnostics.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum TrapKind {
    Overflow,
    DivZero,
    Shift,
    CastRange,
    /// `unwrap()` on `Nothing` when `option`, otherwise on `Err`.
    Unwrap {
        option: bool,
    },
}

/// Whether `op` neither runs code nor writes a slot or referent, so the copies
/// in [`Lowerer::slot_copies`] stay held by their sources across it.
fn keeps_slot_copies(op: &Op) -> bool {
    matches!(
        op,
        Op::LoadLocal(_)
            | Op::ReadRef(_)
            | Op::BorrowLocal(..)
            | Op::Reborrow(_)
            | Op::PushStr(_)
            | Op::PushInt(_)
            | Op::PushBool(_)
            | Op::Site(_)
            | Op::CallerSite(_)
            | Op::Leaving
            | Op::FieldMove(_)
            | Op::Access(..)
            | Op::Loan(_)
            | Op::UseLoan(_)
            | Op::EndLoan(_)
    )
}

/// A counted copy of a local slot that the op just pushed. The op lowered next
/// can read it in place and drop the counting: nothing has run since the
/// load, so the slot keeps the value alive.
struct FreshLoad {
    value: cranelift_codegen::ir::Value,
    /// The `plenty_retain` call of a one-word copy. A narrow query consuming
    /// the copy removes it and skips its release (issue #379).
    retain: Option<cranelift_codegen::ir::Inst>,
    /// For an enum, every instruction that counted the copy, which a tag
    /// read removes (issue #388).
    counting: Vec<cranelift_codegen::ir::Inst>,
}

struct Lowerer<'a, 'b> {
    bcx: &'a mut FunctionBuilder<'b>,
    module: &'a mut symbols::Symbols,
    runtime: &'a Runtime,
    /// Every user function callable from anywhere in the source.
    /// Populated by Pass 1 before any body is emitted, so forward
    /// references and mutual recursion resolve cleanly.
    user_fns: &'a HashMap<String, UserFn>,
    /// Read-only data symbol per source string literal. `Op::PushStr`
    /// emits a `global_value` against the matching entry; pattern
    /// compares in `Op::Match` use the same map for the `Pattern::Str`
    /// case. Populated once per module by `declare_str_data`.
    str_data: &'a HashMap<StrId, DataId>,
    /// Immortal counted empty string used for legacy input EOF.
    eof_empty_str: DataId,
    /// The active function's input variables, indexed by the local
    /// slot `Op::LoadLocal` was emitted with. Empty when lowering
    /// `plenty_main` (top-level has no locals).
    locals: &'a [(Variable, Ty)],
    stack: Vec<StackEntry>,
    /// Set after a return, tail call, or branch whose paths all exit.
    /// Once set, the outer loop in
    /// [`build_user_function`] stops feeding ops to this lowerer.
    terminated: bool,
    /// The last entry is the innermost loop: (continue target, break target).
    loop_targets: Vec<(Block, Block)>,
    /// The last entry is the innermost `checked:` block: its exit, which
    /// takes the block's `Result`, and the stack depth at its start.
    checked_targets: Vec<checked::Target>,
    generator: Option<GeneratorContext>,
    local_frame: Option<LocalFrame>,
    return_storage: Option<cranelift_codegen::ir::Value>,
    return_types: &'a [Ty],
    /// Reused across non-overlapping runtime calls; callbacks have their own frame.
    collection_scratch: Option<cranelift_codegen::ir::StackSlot>,
    /// Loop counter range facts; see [`induction`].
    counters: induction::Facts,
    position: sites::Position,
    /// The copy the op just lowered with a `LoadLocal`, if it was counted.
    fresh_load: Option<FreshLoad>,
    scalar_captures: scalar_captures::Captures,
    portable_simd: portable_simd::State,
    /// Counted one-word copies pushed since the last op that could run code
    /// or write a slot, with the `plenty_retain` call that counted each, and
    /// string literals, which are immortal and need none. Their sources hold
    /// them until such an op runs. An op that only reads a text operand
    /// during its call, and releases it right after, reads such a copy in
    /// place: it removes the retain and skips the release (issue #387).
    slot_copies: Vec<(
        cranelift_codegen::ir::Value,
        Option<cranelift_codegen::ir::Inst>,
    )>,
    /// One-word and two-word zeros defined in a user function's entry block.
    /// Every write of an empty slot uses them, so where all paths into a join
    /// leave a slot empty, SSA construction resolves its value to the same
    /// constant and [`Lowerer::release`] emits nothing (issue #391).
    empty_slot: Option<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value)>,
    /// The trap block already emitted for each kind of check at each source
    /// position, by the position's record offset in the function's site
    /// table (`None` before the first marker). A block reads no SSA variable,
    /// so later checks of the same kind at the same position branch to it
    /// rather than emitting another; see [`Lowerer::trap_if`]. The blocks are
    /// sealed together by [`Lowerer::seal_trap_blocks`].
    trap_blocks: HashMap<(TrapKind, Option<i64>), Block>,
    /// Moves read in place; see [`inline::Moves`].
    moves: inline::Moves,
    /// Blocks shared by a user function's exits; `None` lowers each exit
    /// inline. See [`exits`].
    exits: Option<exits::Exits>,
}

impl Lowerer<'_, '_> {
    /// Counts a new copy of `value`. Returns the `plenty_retain` call for a
    /// one-word value, which is the copy's only cost.
    fn retain(
        &mut self,
        value: cranelift_codegen::ir::Value,
        ty: &Ty,
    ) -> Option<cranelift_codegen::ir::Inst> {
        if let Some(field) = ty.newtype_field() {
            let value = self.unpack(value, &field);
            return self.retain(value, &field);
        }
        if ty.wide() || matches!(ty, Ty::Closure(_)) {
            if ty.managed() {
                self.collection_call(26, &[value], Some(ty))
                    .expect("sum metadata");
            }
            return None;
        }
        let value = self.raw_word(value);
        if !ty.managed() {
            return None;
        }
        let f = self
            .module
            .declare_func_in_func(self.runtime.retain, self.bcx.func);
        Some(self.bcx.ins().call(f, &[value]))
    }
    /// The value of an empty ownership slot of type `ty`.
    fn empty_slot(&mut self, ty: &Ty) -> cranelift_codegen::ir::Value {
        let Some((word, double)) = self.empty_slot else {
            let zero = self.bcx.ins().iconst(types::I64, 0);
            return self.unpack(zero, ty);
        };
        if clif_type(ty.clone()) == types::I128 {
            double
        } else {
            self.unpack(word, ty)
        }
    }
    fn release(&mut self, value: cranelift_codegen::ir::Value, ty: &Ty) {
        // A known zero is an empty slot, such as one a last-use move left
        // behind (issue #391); it owns nothing.
        if ty.managed() && known::known_zero(&self.bcx.func.dfg, value) {
            return;
        }
        if let Some(field) = ty.newtype_field() {
            let value = self.unpack(value, &field);
            self.release(value, &field);
            return;
        }
        if ty.managed() && inline::release_fields(ty).is_some() {
            self.release_record(value, ty);
            return;
        }
        if ty.wide() || matches!(ty, Ty::Generator(_) | Ty::Closure(_)) {
            if ty.managed() {
                // A zero value is an empty ownership slot, such as a local
                // moved out of or not yet assigned, and owns nothing. Testing
                // it here keeps the runtime call off those paths (issue #384).
                let word = if self.bcx.func.dfg.value_type(value) == types::I128 {
                    let (low, high) = self.bcx.ins().isplit(value);
                    self.bcx.ins().bor(low, high)
                } else {
                    value
                };
                let release = self.bcx.create_block();
                let done = self.bcx.create_block();
                self.bcx.ins().brif(word, release, &[], done, &[]);
                self.bcx.switch_to_block(release);
                self.bcx.seal_block(release);
                self.collection_call(27, &[value], Some(ty))
                    .expect("sum metadata");
                self.bcx.ins().jump(done, &[]);
                self.bcx.switch_to_block(done);
                self.bcx.seal_block(done);
            }
            return;
        }
        let value = self.raw_word(value);
        if ty.managed() {
            // User code names itself as the caller of std destructors. In
            // std the context is already right, and the runtime restores it
            // after destructors.
            let bookkeeping = !self.in_standard_library() && self.runtime.standard_calls;
            // An empty slot owns nothing, and the runtime returns at once.
            // Where the call has caller bookkeeping, testing here keeps both
            // off that path. A value read from the stack frame may be known
            // empty once the function is built; `remove_empty_releases` then
            // folds this test, or removes a bare call (issue #549).
            let done = bookkeeping.then(|| {
                let release = self.bcx.create_block();
                let done = self.bcx.create_block();
                self.bcx.ins().brif(value, release, &[], done, &[]);
                self.bcx.switch_to_block(release);
                self.bcx.seal_block(release);
                done
            });
            let saved = if bookkeeping {
                self.enter_call().expect("drop caller site")
            } else {
                None
            };
            let f = self
                .module
                .declare_func_in_func(self.runtime.release, self.bcx.func);
            self.bcx.ins().call(f, &[value]);
            self.leave_call(saved);
            if let Some(done) = done {
                self.bcx.ins().jump(done, &[]);
                self.bcx.switch_to_block(done);
                self.bcx.seal_block(done);
            }
        }
    }
    fn release_locals(&mut self) {
        for i in (0..self.locals.len()).rev() {
            self.drop_local(i as u8);
        }
    }
    /// Where local `i` lives in memory, or `None` if it is a CLIF variable.
    fn local_address(&self, i: u8) -> Option<(cranelift_codegen::ir::Value, i64)> {
        if let Some(g) = &self.generator {
            return Some((g.frame, 64 + self.local_offset(i as usize)));
        }
        let frame = self.local_frame.as_ref()?;
        frame.offsets[i as usize].map(|offset| (frame.base, offset as i64))
    }
    fn read_local(&mut self, i: u8) -> cranelift_codegen::ir::Value {
        if let Some(value) = self.read_simd_local(i) {
            return value;
        }
        let (var, ty) = self.locals[i as usize].clone();
        let value = if let Some((frame, offset)) = self.local_address(i) {
            let value = self.bcx.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlagsData::trusted(),
                frame,
                offset as i32,
            );
            self.unpack(value, &ty)
        } else {
            self.bcx.use_var(var)
        };
        self.read_capture_local(i, value);
        value
    }
    fn write_local(&mut self, i: u8, value: cranelift_codegen::ir::Value) {
        if self.write_simd_local(i, value) {
            return;
        }
        self.save_capture_local(i, value);
        let (var, ty) = self.locals[i as usize].clone();
        if let Some((frame, offset)) = self.local_address(i) {
            let slot = self.bcx.ins().iadd_imm_s(frame, offset);
            self.store_slot(slot, value, &ty);
        } else {
            self.bcx.def_var(var, value);
        }
    }
    fn drop_local(&mut self, i: u8) {
        let ty = self.locals[i as usize].1.clone();
        if ty.managed() {
            let value = self.read_local(i);
            self.release(value, &ty);
            let zero = self.empty_slot(&ty);
            self.write_local(i, zero);
        }
    }
    fn release_stack(&mut self) {
        while let Some((value, ty)) = self.stack.pop() {
            self.release(value, &ty);
        }
    }
    fn lower(&mut self, op: &Op) -> Result<()> {
        if !self.moves.pending.is_empty()
            && !inline::Moves::passes(op)
            && !matches!(op, Op::Call(_) | Op::Return)
        {
            self.settle_moves();
        }
        if self.gathering() && self.gather(op)? {
            return Ok(());
        }
        let fresh_load = self.fresh_load.take();
        // A copy is read in place only by an op in the same block as its
        // retain, never by one inside a nested body.
        if !keeps_slot_copies(op)
            && !matches!(
                op,
                Op::Collection(_)
                    | Op::Contains
                    | Op::Eq
                    | Op::Ne
                    | Op::Lt
                    | Op::Gt
                    | Op::Le
                    | Op::Ge
            )
        {
            self.slot_copies.clear();
        }
        match op {
            Op::Thread(operation) => self.lower_thread(operation)?,
            Op::Channel(operation) => self.lower_channel(operation)?,
            Op::Control(operation) => self.lower_control(operation)?,
            Op::Atomic(operation) => self.lower_atomic(operation)?,
            Op::Mutex(operation) => self.lower_mutex(operation)?,
            Op::Executor(operation) => self.lower_executor(operation)?,
            Op::Try {
                source,
                target,
                cleanup,
            } => self.lower_try(source, target, cleanup)?,
            Op::CheckedBlock { body, result } => self.lower_checked_block(body, result)?,
            Op::CheckedArith {
                operation,
                ty,
                cleanup,
            } => self.lower_block_arith(*operation, ty, cleanup)?,

            Op::Loan(_) | Op::UseLoan(_) | Op::EndLoan(_) | Op::Access(..) => {}
            Op::Site(site) => {
                if self.module.lines() {
                    self.bcx
                        .set_srcloc(cranelift_codegen::ir::SourceLoc::new(site.line));
                }
                self.position.set(site)
            }
            Op::CallerSite(site) => self.position.set_call(site),
            Op::Returning(..)
            | Op::Building(_)
            | Op::Passes(_)
            | Op::Forwards(_)
            | Op::Tokens(_)
            | Op::FieldMove(_) => {}
            Op::Leaving => self.start_leaving(),
            // Consumed by `tail_abi::prepare`, which decides whether the call
            // after it keeps this frame.
            Op::ForwardsReferences => {}
            Op::BorrowLocal(i, mutable) => {
                if let Some(reference) = self.borrow_simd_local(*i) {
                    debug_assert!(!mutable);
                    self.stack.push((
                        reference,
                        Ty::Ref(Arc::new(self.locals[*i as usize].1.clone()), false),
                    ));
                    return Ok(());
                }
                let (frame, offset) = self.local_address(*i).expect("addressable local");
                let ptr = self.bcx.ins().iadd_imm_s(frame, offset);
                let ptr = self.bcx.ins().uextend(types::I128, ptr);
                self.borrow_capture_local(*i, ptr);
                self.stack.push((
                    ptr,
                    Ty::Ref(
                        std::sync::Arc::new(self.locals[*i as usize].1.clone()),
                        *mutable,
                    ),
                ));
            }
            Op::ReadRef(ty) => {
                let (reference, _) = self.stack.pop().ok_or("reference stack underflow")?;
                // A scalar is entirely in its slot's first word.
                let packed = self.read_typed_reference(reference, ty);
                let value = self.unpack(packed, ty);
                let value = if ty.can_copy() {
                    self.snapshot_inline(value, ty)
                } else {
                    value
                };
                if let Some(retain) = self.retain(value, ty) {
                    self.slot_copies.push((value, Some(retain)));
                }
                self.stack.push((value, ty.clone()));
            }
            Op::Reborrow(ty) => {
                self.stack.last_mut().ok_or("reference stack underflow")?.1 = ty.clone();
            }
            Op::WriteRef(ty) | Op::FillRef(ty) => {
                let (reference, _) = self.stack.pop().ok_or("reference stack underflow")?;
                let (value, _) = self.pop_typed(ty.clone())?;
                // An unmanaged old value owns nothing to release, and filled
                // storage holds none.
                if ty.managed() && matches!(op, Op::WriteRef(_)) {
                    let old = self.read_typed_reference(reference, ty);
                    self.release(old, ty);
                }
                self.write_reference(reference, value, ty);
            }
            Op::ReplaceRef(ty) => {
                let (reference, _) = self.stack.pop().ok_or("reference stack underflow")?;
                let (value, _) = self.pop_typed(ty.clone())?;
                let old = self.read_typed_reference(reference, ty);
                let old = self.unpack(old, ty);
                // Inline field storage is overwritten below; relocate the old
                // owner without retaining it or running its destructor.
                let old = self.snapshot_inline(old, ty);
                self.write_reference(reference, value, ty);
                self.stack.push((old, ty.clone()));
            }
            Op::Collection(operation) => {
                self.fresh_load = fresh_load;
                let lowered = self.lower_collection(operation);
                self.fresh_load = None;
                lowered?
            }
            Op::ClosureNew(t) => self.lower_closure_new(t)?,
            Op::ClosureCall(t) => self.lower_closure_call(t)?,
            Op::BoxedCallableNew(source, adapter, sig) => {
                self.lower_boxed_callable_new(source, adapter, sig)?
            }
            Op::BoxedCallableCall(sig) => self.lower_boxed_callable_call(sig)?,
            Op::Class(operation) => self.lower_class(operation)?,
            Op::Enum(operation) => {
                self.fresh_load = fresh_load;
                let lowered = self.lower_enum(operation);
                self.fresh_load = None;
                lowered?
            }
            Op::Split(t, tag) => self.lower_split(t, *tag)?,
            Op::SplitClass(t) => self.lower_split_class(t)?,
            Op::Box(operation) => self.lower_box(operation)?,
            Op::Yield(ty) => self.lower_yield(ty)?,
            Op::Next(i) => self.lower_next(*i)?,
            Op::DropLocal(i) => self.drop_local(*i),
            Op::EmptyLocal(i) => {
                let ty = self.locals[*i as usize].1.clone();
                let zero = self.empty_slot(&ty);
                self.write_local(*i, zero);
            }
            Op::MoveLocal(i) => {
                let value = self.read_local(*i);
                let ty = self.locals[*i as usize].1.clone();
                let value = if self.moves.in_place(op, &ty) {
                    self.moves.pending.push(value);
                    value
                } else {
                    self.snapshot_inline(value, &ty)
                };
                let zero = self.empty_slot(&ty);
                self.write_local(*i, zero);
                self.stack.push((value, ty));
            }
            Op::Unreachable => {
                self.bcx.ins().trap(TrapCode::unwrap_user(1));
                self.terminated = true;
            }
            Op::Loop { condition, body } => self.lower_loop(condition, body)?,
            Op::PushInt(value) => {
                let ty = Ty::from(*value);
                let v = self
                    .bcx
                    .ins()
                    .iconst(clif_type(ty.clone()), int_value_bits(*value));
                self.stack.push((v, ty));
            }
            Op::PushFloat { bits, ty } => {
                let value =
                    if *ty == Ty::F32 {
                        self.bcx.ins().f32const(
                            cranelift_codegen::ir::immediates::Ieee32::with_bits(*bits as u32),
                        )
                    } else {
                        self.bcx
                            .ins()
                            .f64const(cranelift_codegen::ir::immediates::Ieee64::with_bits(*bits))
                    };
                self.stack.push((value, ty.clone()));
            }
            Op::PushUnit => {
                let value = self.bcx.ins().iconst(types::I64, 0);
                self.stack.push((value, Ty::Unit));
            }
            Op::PipelineToken(item, id) => {
                let value = self.bcx.ins().iconst(types::I64, 0);
                self.stack.push((value, Ty::Iter(item.clone(), *id)));
            }
            Op::FloatNeg => {
                let (value, ty) = self.stack.pop().ok_or("empty float negation")?;
                let value = self.float_neg(value);
                self.stack.push((value, ty));
            }
            Op::PushBool(b) => {
                let v = self.bcx.ins().iconst(types::I8, if *b { 1 } else { 0 });
                self.stack.push((v, Ty::Bool));
            }
            Op::PushStr(id) => self.lower_push_str(*id, Ty::Str)?,
            Op::PushBytes(id) => self.lower_push_str(*id, Ty::ByteStr)?,
            Op::Add => self.lower_add()?,
            Op::Sub => self.lower_checked_arith(ArithKind::Sub)?,
            Op::Mul => self.lower_checked_arith(ArithKind::Mul)?,
            Op::Div => self.lower_div(false)?,
            Op::FloorDiv => self.lower_div(true)?,
            Op::Modulo => self.lower_modulo()?,
            Op::Integer(kind) => self.lower_integer(*kind)?,
            Op::Numeric(operation) => self.lower_numeric(operation)?,
            Op::Eq => self.lower_eq()?,
            Op::Lt => self.int_cmp(IntCC::SignedLessThan, IntCC::UnsignedLessThan)?,
            Op::Gt => self.int_cmp(IntCC::SignedGreaterThan, IntCC::UnsignedGreaterThan)?,
            Op::Not => {
                let (v, ty) = self.pop_typed(Ty::Bool)?;
                let one = self.bcx.ins().iconst(types::I8, 1);
                let neg = self.bcx.ins().bxor(v, one);
                self.stack.push((neg, ty));
            }
            Op::Ne => self.lower_ne()?,
            Op::Le => self.int_cmp(IntCC::SignedLessThanOrEqual, IntCC::UnsignedLessThanOrEqual)?,
            Op::Ge => self.int_cmp(
                IntCC::SignedGreaterThanOrEqual,
                IntCC::UnsignedGreaterThanOrEqual,
            )?,
            Op::And => self.lower_bool_binop(true)?,
            Op::Or => self.lower_bool_binop(false)?,
            Op::Drop => {
                let (value, ty) = self.stack.pop().ok_or("AOT: stack underflow on `drop`")?;
                self.release(value, &ty);
            }
            Op::Dup => {
                let value = self
                    .stack
                    .last()
                    .ok_or("AOT: stack underflow on `dup`")?
                    .clone();
                self.retain(value.0, &value.1);
                self.stack.push(value);
            }
            Op::Swap => {
                if self.stack.len() < 2 {
                    return Err(format!(
                        "AOT: stack underflow on `swap` (need 2 values, have {})",
                        self.stack.len()
                    )
                    .into());
                }
                let len = self.stack.len();
                self.stack.swap(len - 1, len - 2);
            }
            Op::Cast(target) => {
                let (v, src) = self.stack.pop().ok_or("AOT: stack underflow on cast")?;
                let cast = self.cast(v, src, target.clone());
                self.stack.push((cast, target.clone()));
            }
            Op::CheckedCast(target) => self.lower_checked_cast(target)?,
            Op::TryCast(target) => self.lower_try_cast(target)?,
            Op::RangeLength(element) => self.lower_range_length(element)?,
            Op::RangeParts(element) => self.lower_range_parts(element)?,
            Op::Display => self.lower_display()?,
            Op::Clear => self.release_stack(),
            Op::LoadLocal(i) => {
                self.lower_load_local(*i)?;
                if let Some(FreshLoad {
                    value,
                    retain: Some(retain),
                    ..
                }) = self.fresh_load
                {
                    self.slot_copies.push((value, Some(retain)));
                }
            }
            Op::StoreLocal(i) => {
                let ty = self.locals[*i as usize].1.clone();
                let (value, _) = self.pop_typed(ty.clone())?;
                if ty.managed() {
                    let old = self.read_local(*i);
                    self.release(old, &ty);
                }
                self.write_local(*i, value);
            }
            Op::Call(name) => self.lower_call(name)?,
            Op::FunctionAddress(name, signature) => {
                let id = self
                    .user_fns
                    .get(name)
                    .ok_or("undefined function value")?
                    .id;
                let reference = self.module.declare_func_in_func(id, self.bcx.func);
                let value = self.bcx.ins().func_addr(PTR_TY, reference);
                self.stack.push((value, Ty::Callable(signature.clone())));
            }
            Op::CallIndirect(signature) => self.lower_indirect_call(signature, false)?,
            Op::TailCallIndirect(signature) => self.lower_indirect_call(signature, true)?,
            Op::ForeignCall { declaration, sig } => self.lower_foreign_call(declaration, sig)?,
            Op::ForeignNull(ty) => {
                let value = self.bcx.ins().iconst(PTR_TY, 0);
                self.stack.push((value, ty.clone()));
            }
            Op::TailCall(name) => self.lower_tail_call(name)?,
            Op::Break | Op::Continue => {
                let &(header, exit) = self
                    .loop_targets
                    .last()
                    .ok_or("loop control outside a loop")?;
                self.bcx.ins().jump(
                    if matches!(op, Op::Break) {
                        exit
                    } else {
                        header
                    },
                    &[],
                );
                self.terminated = true;
            }
            Op::Return => {
                // The results are copied out before the locals are released.
                self.moves.pending.clear();
                if self.generator.is_some() {
                    self.complete_generator();
                    return Ok(());
                }
                self.leave(self.stack.clone(), &Arc::from([]))?;
                self.terminated = true;
            }
            // `DefineFn` is hoisted into a top-level Cranelift function by
            // Pass 1 + Pass 2; at the point this lowerer sees one, the
            // body is already being emitted elsewhere and the definition
            // itself has no runtime effect.
            Op::DefineFn(_, _) => {}
            Op::Match(arms) => self.lower_match(arms)?,
            Op::If {
                then,
                elifs,
                otherwise,
            } => self.lower_if(then, elifs, otherwise)?,
            Op::ReadLine => self.lower_readline()?,
            Op::Contains => self.lower_contains()?,
            Op::PrintLn => self.lower_println()?,
            Op::Print => self.lower_print()?,
        }
        // A nested body may end with a load; the copy stays fresh only for
        // the op right after its own load.
        if !matches!(op, Op::LoadLocal(_)) {
            self.fresh_load = None;
        }
        if let Op::PushStr(_) = op {
            let (literal, _) = *self.stack.last().ok_or("empty stack after a literal")?;
            self.slot_copies.push((literal, None));
        } else if !keeps_slot_copies(op) {
            self.slot_copies.clear();
        }
        Ok(())
    }

    /// Lower `Op::ReadLine`: call `plenty_readline`, which returns a
    /// newly owned counted string or `NULL` on EOF. We turn `NULL`
    /// into the address of `plenty_readline_eof_empty` (the `""` data
    /// symbol) so the `Ty::Str` we push is always dereferenceable; the
    /// "got a line?" Bool is `ptr != 0`. The user discriminates via
    /// `match` on the Bool.
    fn lower_readline(&mut self) -> Result<()> {
        let readline = self
            .module
            .declare_func_in_func(self.runtime.readline, self.bcx.func);
        let inst = self.bcx.ins().call(readline, &[]);
        let ptr = self.bcx.inst_results(inst)[0];
        let zero = self.bcx.ins().iconst(PTR_TY, 0);
        // got_line = (ptr != 0). Cranelift's `icmp` over a non-Bool
        // operand still produces an `i1`-widened-to-`i8`, which is
        // Plenty's Bool ABI.
        let got_line = self.bcx.ins().icmp(IntCC::NotEqual, ptr, zero);
        // EOF still carries an ordinary empty string alongside a false flag.
        let eof_gv = self
            .module
            .declare_data_in_func(self.eof_empty_str, self.bcx.func);
        let eof_addr = self.bcx.ins().symbol_value(PTR_TY, eof_gv);
        let safe_ptr = self.bcx.ins().select(got_line, ptr, eof_addr);
        self.stack.push((safe_ptr, Ty::Str));
        self.stack.push((got_line, Ty::Bool));
        Ok(())
    }

    /// Lower `Op::Contains`: pop `haystack needle`, call
    /// the bounded `plenty_contains` helper, and push the
    /// returned `i8` as Plenty `Bool`.
    fn lower_contains(&mut self) -> Result<()> {
        let needle = self
            .stack
            .pop()
            .ok_or("AOT: stack underflow on :contains")?;
        let hay = self
            .stack
            .pop()
            .ok_or("AOT: stack underflow on :contains")?;
        debug_assert_eq!(hay.1, Ty::Str);
        debug_assert_eq!(needle.1, Ty::Str);
        let contains = self
            .module
            .declare_func_in_func(self.runtime.contains, self.bcx.func);
        let needle_held = self.forget_slot_copy(needle.0);
        let hay_held = self.forget_slot_copy(hay.0);
        let inst = self.bcx.ins().call(contains, &[hay.0, needle.0]);
        let v = self.bcx.inst_results(inst)[0];
        if !hay_held {
            self.release(hay.0, &hay.1);
        }
        if !needle_held {
            self.release(needle.0, &needle.1);
        }
        self.stack.push((v, Ty::Bool));
        Ok(())
    }

    /// Lower `Op::PrintLn`: pop one `Ty::Str` address and forward it
    /// to `plenty_println`, which writes the bytes verbatim plus a
    /// single `\n`.
    fn lower_println(&mut self) -> Result<()> {
        let (v, ty) = self.stack.pop().ok_or("AOT: stack underflow on :println")?;
        debug_assert_eq!(ty, Ty::Str);
        let println_fn = self
            .module
            .declare_func_in_func(self.runtime.println, self.bcx.func);
        self.bcx.ins().call(println_fn, &[v]);
        self.release(v, &ty);
        Ok(())
    }

    /// Lower `Op::Print`: pop and render one value with the same helper `.`
    /// uses, but do not add brackets or a newline.
    fn lower_print(&mut self) -> Result<()> {
        let (value, ty) = self.stack.pop().ok_or("AOT: stack underflow on :print")?;
        let printer = self.printer_for(ty.clone());
        let local = self.module.declare_func_in_func(printer, self.bcx.func);
        self.bcx.ins().call(local, &[value]);
        self.release(value, &ty);
        Ok(())
    }

    /// Lower add/sub/mul: IEEE float operations or checked integer arithmetic.
    /// Emits the matching Cranelift `*_overflow` instruction, branches
    /// on the overflow flag to the shared overflow-trap block, and
    /// switches to a fresh successor block with the result on the
    /// compile-time stack. Overflow exits with status 1 and a diagnostic
    /// through the runtime helper `plenty_trap_overflow`.
    fn lower_checked_arith(&mut self, kind: ArithKind) -> Result<()> {
        let (a, b, ty) = self.pop_pair()?;
        if ty.is_float() {
            let result = match kind {
                ArithKind::Sub => {
                    let a = self.float_left_operand(a, b);
                    self.bcx.ins().fsub(a, b)
                }
                ArithKind::Add | ArithKind::Mul => {
                    // Cranelift rematerializes a float constant in each block
                    // that uses it and, on x86-64, builds a left operand in a
                    // register with `movabs` + `movq` but reads a right one from
                    // the constant pool. Moving a non-NaN constant right is
                    // exact: only the choice between two NaN operands depends
                    // on order. `2.0 * x` becomes `x + x`, which rounds,
                    // overflows, and propagates signed zero, infinities and NaN
                    // identically (issue #370).
                    let dfg = &self.bcx.func.dfg;
                    let constant_left = known::known_float(dfg, a).is_some_and(|c| !c.is_nan())
                        && known::known_float(dfg, b).is_none();
                    let (x, y) = if constant_left { (b, a) } else { (a, b) };
                    match kind {
                        ArithKind::Mul if known::known_float(dfg, y) == Some(2.0) => {
                            self.bcx.ins().fadd(x, x)
                        }
                        ArithKind::Mul => self.bcx.ins().fmul(x, y),
                        _ => self.bcx.ins().fadd(x, y),
                    }
                }
            };
            self.stack.push((result, ty));
            return Ok(());
        }
        let signed = is_signed(ty.clone());
        let (result, of) = match self.compared_add_sub(kind, signed, a, b) {
            Some(checked) => checked,
            None => match (kind, signed) {
                (ArithKind::Add, true) => self.bcx.ins().sadd_overflow(a, b),
                (ArithKind::Add, false) => self.bcx.ins().uadd_overflow(a, b),
                (ArithKind::Sub, true) => self.bcx.ins().ssub_overflow(a, b),
                (ArithKind::Sub, false) => self.bcx.ins().usub_overflow(a, b),
                (ArithKind::Mul, true) => self.bcx.ins().smul_overflow(a, b),
                (ArithKind::Mul, false) => self.bcx.ins().umul_overflow(a, b),
            },
        };
        self.trap_if(of, TrapKind::Overflow)?;
        self.stack.push((result, ty));
        Ok(())
    }

    /// Checked add or subtract with an overflow flag that is an `icmp`,
    /// where one exists: unsigned subtraction, and signed operations with a
    /// constant right operand (or left, for addition). `None` leaves the
    /// operation to the `*_overflow` instruction.
    ///
    /// Cranelift 0.137's x86-64 backend fuses `brif` with the flag of
    /// `uadd_overflow` (`add; jb`), `umul_overflow` and `smul_overflow`
    /// (`jo`), but materializes the others (`seto; test; jne`, 13 bytes)
    /// where an `icmp` becomes `cmp; jcc` (9 bytes). In a large program
    /// every check's bytes stream through the instruction cache (issue #425).
    ///
    /// The optimizer rematerializes an add or subtract of a constant in each
    /// block that uses it, so a comparison of such a result would compute it
    /// a second time after the branch. Optimized code keeps the flag there.
    fn compared_add_sub(
        &mut self,
        kind: ArithKind,
        signed: bool,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
    ) -> Option<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value)> {
        let dfg = &self.bcx.func.dfg;
        let constant_operand =
            known::known_int(dfg, a).is_some() || known::known_int(dfg, b).is_some();
        let rematerialized = self.module.optimizes() && constant_operand;
        match (kind, signed) {
            // A borrow happens exactly when the subtrahend is larger. The
            // comparison reads only the operands.
            (ArithKind::Sub, false) => {
                let of = self.bcx.ins().icmp(IntCC::UnsignedLessThan, a, b);
                let difference = self.bcx.ins().isub(a, b);
                Some((difference, of))
            }
            // Adding a positive constant (or subtracting a negative one)
            // overflows exactly when the wrapped result falls below `x`;
            // the opposite sign, exactly when it rises above.
            (ArithKind::Add | ArithKind::Sub, true) if !rematerialized => {
                let (x, c, constant) = match known::known_signed(dfg, b) {
                    Some(constant) => (a, b, constant),
                    None if kind == ArithKind::Add => (b, a, known::known_signed(dfg, a)?),
                    None => return None,
                };
                let result = match kind {
                    ArithKind::Add => self.bcx.ins().iadd(x, c),
                    _ => self.bcx.ins().isub(x, c),
                };
                let of = match (constant.signum(), kind) {
                    (0, _) => self.bcx.ins().iconst(types::I8, 0),
                    (1, ArithKind::Add) | (-1, ArithKind::Sub) => {
                        self.bcx.ins().icmp(IntCC::SignedLessThan, result, x)
                    }
                    _ => self.bcx.ins().icmp(IntCC::SignedGreaterThan, result, x),
                };
                Some((result, of))
            }
            _ => None,
        }
    }

    /// Lower a loop counter step that [`induction`] proved cannot overflow.
    fn lower_proven_step(&mut self, op: &Op) -> Result<()> {
        let (a, b, ty) = self.pop_int_pair()?;
        let result = match op {
            Op::Add => self.bcx.ins().iadd(a, b),
            Op::Sub => self.bcx.ins().isub(a, b),
            _ => return Err("AOT: proven loop step is not an add or subtract".into()),
        };
        self.stack.push((result, ty));
        Ok(())
    }

    /// Whether floor division or modulo of `a` by `b` may skip its sign
    /// handling: a nonnegative dividend and a divisor that is nonnegative and,
    /// once the zero check passes, positive.
    fn nonnegative_operands(
        &self,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
    ) -> bool {
        let known = |v| {
            self.counters.nonnegative.contains(&v)
                || induction::nonnegative_constant(&self.bcx.func.dfg, v)
        };
        known(a) && known(b)
    }

    fn lower_integer(&mut self, kind: IntegerOp) -> Result<()> {
        if kind == IntegerOp::Not {
            let (value, ty) = self.stack.pop().ok_or("empty bitwise complement")?;
            let result = self.bcx.ins().bnot(value);
            self.stack.push((result, ty));
            return Ok(());
        }
        let (a, b, ty) = self.pop_pair()?;
        if matches!(kind, IntegerOp::ShiftLeft | IntegerOp::ShiftRight) {
            // Unsigned comparison rejects negative signed counts as well.
            let width = clif_type(ty.clone()).bits();
            let invalid =
                self.bcx
                    .ins()
                    .icmp_imm_s(IntCC::UnsignedGreaterThanOrEqual, b, i64::from(width));
            self.trap_if(invalid, TrapKind::Shift)?;
        }
        let result = match kind {
            IntegerOp::And => self.bcx.ins().band(a, b),
            IntegerOp::Or => self.bcx.ins().bor(a, b),
            IntegerOp::Xor => self.bcx.ins().bxor(a, b),
            IntegerOp::ShiftLeft => self.bcx.ins().ishl(a, b),
            IntegerOp::ShiftRight if is_signed(ty.clone()) => self.bcx.ins().sshr(a, b),
            IntegerOp::ShiftRight => self.bcx.ins().ushr(a, b),
            IntegerOp::WrappingAdd => self.bcx.ins().iadd(a, b),
            IntegerOp::WrappingSub => self.bcx.ins().isub(a, b),
            IntegerOp::WrappingMul => self.bcx.ins().imul(a, b),
            // Cranelift masks the count to the operand width.
            IntegerOp::RotateLeft => self.bcx.ins().rotl(a, b),
            IntegerOp::RotateRight => self.bcx.ins().rotr(a, b),
            IntegerOp::Not => unreachable!(),
        };
        self.stack.push((result, ty));
        Ok(())
    }

    /// Check divisor zero and signed INT_MIN/-1 before emitting division,
    /// so both failures produce runtime diagnostics instead of hardware traps.
    fn lower_div(&mut self, floor: bool) -> Result<()> {
        let (a, b, ty) = self.pop_pair()?;
        if ty.is_float() {
            let a = self.float_left_operand(a, b);
            let value = self.bcx.ins().fdiv(a, b);
            self.stack.push((value, ty));
            return Ok(());
        }
        let cty = clif_type(ty.clone());

        let zero = self.bcx.ins().iconst(cty, 0);
        let b_is_zero = self.bcx.ins().icmp(IntCC::Equal, b, zero);
        self.trap_if(b_is_zero, TrapKind::DivZero)?;

        let signed = is_signed(ty.clone()) && !self.nonnegative_operands(a, b);
        if signed {
            // Only one signed-division overflow case exists: INT_MIN / -1.
            // (Result `-INT_MIN` is not representable at the same width.)
            let int_min = match ty {
                Ty::I8 => i64::from(i8::MIN),
                Ty::I16 => i64::from(i16::MIN),
                Ty::I32 => i64::from(i32::MIN),
                Ty::I64 => i64::MIN,
                _ => unreachable!("signed integer type"),
            };
            let int_min_v = self.bcx.ins().iconst(cty, int_min);
            let neg_one_v = self.bcx.ins().iconst(cty, -1);
            let a_is_min = self.bcx.ins().icmp(IntCC::Equal, a, int_min_v);
            let b_is_neg_one = self.bcx.ins().icmp(IntCC::Equal, b, neg_one_v);
            let overflow = self.bcx.ins().band(a_is_min, b_is_neg_one);
            self.trap_if(overflow, TrapKind::Overflow)?;
        }

        let mut v = if signed {
            self.bcx.ins().sdiv(a, b)
        } else {
            self.bcx.ins().udiv(a, b)
        };
        if floor && signed {
            let remainder = self.bcx.ins().srem(a, b);
            let nonzero = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, remainder, 0);
            let signs = self.bcx.ins().bxor(a, b);
            let different = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
            let round_down = self.bcx.ins().band(nonzero, different);
            let below = self.bcx.ins().iadd_imm_s(v, -1);
            v = self.bcx.ins().select(round_down, below, v);
        }
        self.stack.push((v, ty));
        Ok(())
    }

    /// Python-style modulo has the divisor's sign, including negative divisors.
    fn lower_modulo(&mut self) -> Result<()> {
        let (a, b, ty) = self.pop_int_pair()?;
        let zero = self.bcx.ins().iconst(clif_type(ty.clone()), 0);
        let is_zero = self.bcx.ins().icmp(IntCC::Equal, b, zero);
        self.trap_if(is_zero, TrapKind::DivZero)?;
        let result = if is_signed(ty.clone()) && !self.nonnegative_operands(a, b) {
            // INT_MIN % -1 is zero, so avoid a hardware division overflow.
            let minus_one = self.bcx.ins().icmp_imm_s(IntCC::Equal, b, -1);
            let one = self.bcx.ins().iconst(clif_type(ty.clone()), 1);
            let divisor = self.bcx.ins().select(minus_one, one, b);
            let rem = self.bcx.ins().srem(a, divisor);
            let nonzero = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, rem, 0);
            let signs = self.bcx.ins().bxor(rem, b);
            let different = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
            let adjust = self.bcx.ins().band(nonzero, different);
            let corrected = self.bcx.ins().iadd(rem, b);
            self.bcx.ins().select(adjust, corrected, rem)
        } else {
            self.bcx.ins().urem(a, b)
        };
        self.stack.push((result, ty));
        Ok(())
    }

    /// Branch to a fresh trap block when `flag` is non-zero (Plenty
    /// Bool true); otherwise fall through into a sealed successor
    /// block which becomes the new current block. The trap block
    /// calls the runtime helper for `kind` (which `_Noreturn`s) and
    /// ends with a CLIF `trap` to satisfy the verifier. In an executable,
    /// a block whose site is fixed holds only a `trap` with a code of its
    /// own, which the runtime's handler maps to the same helper ([`traps`]).
    ///
    /// Checks of one kind at one source position share a trap block: it
    /// reads no SSA variable, only the position's record address, so every
    /// such check branches to the same code and the function carries one
    /// block, one helper call and one relocation per distinct trap rather
    /// than per operation. A shared block gains predecessors until the body
    /// ends, so it is sealed by [`Self::seal_trap_blocks`], not here.
    ///
    /// The trap block computes the site's address itself, so the passing
    /// path gains nothing, and is cold, so it is laid out after the body.
    ///
    /// A guard whose condition is computed from constants and is known to be
    /// false emits nothing, not even a trap-site record; see `known.rs`.
    fn trap_if(&mut self, flag: cranelift_codegen::ir::Value, kind: TrapKind) -> Result<()> {
        if known::known_int(&self.bcx.func.dfg, flag) == Some(0) {
            return Ok(());
        }
        // A site read from a shared exit's variable differs by predecessor,
        // so its trap block is not shared.
        let key = self.trap_site_offset().ok().map(|offset| (kind, offset));
        let after = self.bcx.create_block();
        if let Some(&trap_block) = key.as_ref().and_then(|key| self.trap_blocks.get(key)) {
            self.bcx.ins().brif(flag, trap_block, &[], after, &[]);
        } else {
            let trap_block = self.bcx.create_block();
            self.bcx.ins().brif(flag, trap_block, &[], after, &[]);

            // Fill the trap block. The `brif` above terminated the
            // previous block, so this switch is legal.
            self.bcx.switch_to_block(trap_block);
            self.bcx.set_cold_block(trap_block);
            let code = match key {
                Some((_, site)) if self.runtime.signal_traps.is_some() => {
                    self.module.check_code(traps::kind(kind), site)
                }
                _ => None,
            };
            match code {
                Some(code) => {
                    self.bcx.ins().trap(code);
                }
                None => self.call_trap_helper(kind)?,
            }
            match key {
                Some(key) => {
                    self.trap_blocks.insert(key, trap_block);
                }
                None => self.bcx.seal_block(trap_block),
            }
        }

        // Continue lowering into `after`.
        self.bcx.switch_to_block(after);
        self.bcx.seal_block(after);
        Ok(())
    }

    /// Seals the shared trap blocks once the body has emitted its last
    /// branch into them. Call before the builder is finalized.
    fn seal_trap_blocks(&mut self) {
        for block in std::mem::take(&mut self.trap_blocks).into_values() {
            self.bcx.seal_block(block);
        }
    }

    /// End the current block with a call to the runtime helper for `kind`.
    fn call_trap_helper(&mut self, kind: TrapKind) -> Result<()> {
        let site = self.trap_site()?;
        let (helper, arguments) = match kind {
            TrapKind::Overflow => (self.runtime.trap_overflow, vec![site]),
            TrapKind::DivZero => (self.runtime.trap_div_zero, vec![site]),
            TrapKind::Shift => (self.runtime.trap_shift, vec![site]),
            TrapKind::CastRange => (self.runtime.trap_cast_range, vec![site]),
            TrapKind::Unwrap { option } => {
                let option = self.bcx.ins().iconst(types::I64, i64::from(option));
                (self.runtime.trap_unwrap, vec![site, option])
            }
        };
        let local = self.module.declare_func_in_func(helper, self.bcx.func);
        self.bcx.ins().call(local, &arguments);
        self.bcx.ins().trap(TrapCode::unwrap_user(3));
        Ok(())
    }

    /// Tuples share sorting's lexicographic total order. Scalar floats retain
    /// IEEE comparisons, and scalar integers use their declared signedness.
    fn int_cmp(&mut self, signed: IntCC, unsigned: IntCC) -> Result<()> {
        let (a, b, ty) = self.pop_pair()?;
        if ty == Ty::Str || matches!(&ty, Ty::Enum(t) if t.tuple()) {
            let (operation, descriptor) = if ty == Ty::Str {
                (crate::collection::CollectionOp::TextCompare, None)
            } else {
                (
                    crate::collection::CollectionOp::OrderCompare(ty.clone()),
                    Some(&ty),
                )
            };
            let order = match descriptor {
                Some(ty) => self.order_compare(a, b, ty),
                None => {
                    let order = self.collection_call(operation.opcode(), &[a, b], None)?;
                    self.bcx.ins().ireduce(types::I64, order)
                }
            };
            let value = self.bcx.ins().icmp_imm_s(signed, order, 0);
            for operand in [a, b] {
                if !(ty == Ty::Str && self.forget_slot_copy(operand)) {
                    self.release(operand, &ty);
                }
            }
            self.stack.push((value, Ty::Bool));
            return Ok(());
        }
        if ty.is_float() {
            let cc = match signed {
                IntCC::SignedLessThan => FloatCC::LessThan,
                IntCC::SignedLessThanOrEqual => FloatCC::LessThanOrEqual,
                IntCC::SignedGreaterThan => FloatCC::GreaterThan,
                IntCC::SignedGreaterThanOrEqual => FloatCC::GreaterThanOrEqual,
                _ => unreachable!(),
            };
            let value = self.bcx.ins().fcmp(cc, a, b);
            self.stack.push((value, Ty::Bool));
            return Ok(());
        }
        let cc = if is_signed(ty.clone()) {
            signed
        } else {
            unsigned
        };
        let v = self.bcx.ins().icmp(cc, a, b);
        self.stack.push((v, Ty::Bool));
        Ok(())
    }

    /// Pop the top two values, requiring them to share the same integer
    /// type. The checker has already enforced this; the defensive arm is
    /// a panic so a future Op-stream constructed without the checker
    /// surfaces the bug loudly.
    fn pop_int_pair(
        &mut self,
    ) -> Result<(
        cranelift_codegen::ir::Value,
        cranelift_codegen::ir::Value,
        Ty,
    )> {
        let (b, b_ty) = self.stack.pop().ok_or("AOT: stack underflow")?;
        let (a, a_ty) = self.stack.pop().ok_or("AOT: stack underflow")?;
        if a_ty != b_ty || !a_ty.is_int() {
            panic!("AOT lowering reached an arithmetic op with mismatched or non-int operands");
        }
        Ok((a, b, a_ty))
    }

    /// Pop the top two values; their types must match but may be any.
    fn pop_pair(
        &mut self,
    ) -> Result<(
        cranelift_codegen::ir::Value,
        cranelift_codegen::ir::Value,
        Ty,
    )> {
        let (b, b_ty) = self.stack.pop().ok_or("AOT: stack underflow")?;
        let (a, a_ty) = self.stack.pop().ok_or("AOT: stack underflow")?;
        debug_assert_eq!(a_ty, b_ty);
        Ok((a, b, a_ty))
    }

    /// Pop one value, requiring it to have the given type.
    fn pop_typed(&mut self, expected: Ty) -> Result<(cranelift_codegen::ir::Value, Ty)> {
        let (v, ty) = self.stack.pop().ok_or("AOT: stack underflow")?;
        debug_assert_eq!(ty, expected);
        Ok((v, ty))
    }

    /// Emit the cast: widen with sign- or zero-extend (depending on the
    /// source's signedness), narrow with `ireduce`, leave bit-equal-width
    /// pairs untouched (Cranelift doesn't model signedness in the type).
    fn cast(
        &mut self,
        v: cranelift_codegen::ir::Value,
        from: Ty,
        to: Ty,
    ) -> cranelift_codegen::ir::Value {
        if from == to {
            return v;
        }
        if let Some(constant) = self.constant_cast(v, &from, &to) {
            return constant;
        }
        let target = clif_type(to.clone());
        if from.is_float() && to.is_float() {
            return if to == Ty::F64 {
                self.bcx.ins().fpromote(target, v)
            } else {
                self.bcx.ins().fdemote(target, v)
            };
        }
        if to.is_float() {
            return if is_signed(from) {
                self.bcx.ins().fcvt_from_sint(target, v)
            } else {
                self.bcx.ins().fcvt_from_uint(target, v)
            };
        }
        if from.is_float() {
            let wide = if target.bits() < 32 {
                types::I32
            } else {
                target
            };
            let mut value = if is_signed(to.clone()) {
                self.bcx.ins().fcvt_to_sint_sat(wide, v)
            } else {
                self.bcx.ins().fcvt_to_uint_sat(wide, v)
            };
            if target != wide {
                let (min, end) = to.int_range().unwrap();
                let max = self.bcx.ins().iconst(wide, (end - 1) as i64);
                value = if is_signed(to) {
                    let min = self.bcx.ins().iconst(wide, min as i64);
                    let above = self.bcx.ins().smax(value, min);
                    self.bcx.ins().smin(above, max)
                } else {
                    self.bcx.ins().umin(value, max)
                };
                value = self.bcx.ins().ireduce(target, value);
            }
            return value;
        }
        let from_bits = width_bits(from.clone());
        let to_bits = width_bits(to.clone());
        if from_bits == to_bits {
            return v;
        }
        let to_clif = clif_type(to);
        if to_bits > from_bits {
            if is_signed(from) {
                self.bcx.ins().sextend(to_clif, v)
            } else {
                self.bcx.ins().uextend(to_clif, v)
            }
        } else {
            self.bcx.ins().ireduce(to_clif, v)
        }
    }

    /// Render the legacy operand stack (`.`, `--legacy` syntax only).
    /// Each print helper's `FuncId` is resolved to a local `FuncRef`
    /// once at the top and reused.
    fn lower_display(&mut self) -> Result<()> {
        // Snapshot the stack so we don't iterate-and-mutate; printing
        // leaves the stack untouched (`.` doesn't pop in Plenty).
        let entries: Vec<StackEntry> = self.stack.clone();
        let open = self
            .module
            .declare_func_in_func(self.runtime.print_open_bracket, self.bcx.func);
        let close = self
            .module
            .declare_func_in_func(self.runtime.print_close_bracket, self.bcx.func);
        let space = self
            .module
            .declare_func_in_func(self.runtime.print_space, self.bcx.func);
        self.bcx.ins().call(open, &[]);
        for (i, (v, ty)) in entries.iter().enumerate() {
            if i > 0 {
                self.bcx.ins().call(space, &[]);
            }
            let printer = self.printer_for(ty.clone());
            let local = self.module.declare_func_in_func(printer, self.bcx.func);
            self.bcx.ins().call(local, &[*v]);
        }
        self.bcx.ins().call(close, &[]);
        Ok(())
    }

    /// The runtime-helper `FuncId` that prints one value of `ty`.
    fn printer_for(&self, ty: Ty) -> FuncId {
        match ty {
            Ty::I8 => self.runtime.print_i8,
            Ty::I16 => self.runtime.print_i16,
            Ty::I32 => self.runtime.print_i32,
            Ty::I64 => self.runtime.print_i64,
            Ty::U8 => self.runtime.print_u8,
            Ty::U16 => self.runtime.print_u16,
            Ty::U32 => self.runtime.print_u32,
            Ty::U64 => self.runtime.print_u64,
            Ty::Bool => self.runtime.print_bool,
            Ty::Str => self.runtime.print_str,
            _ => unreachable!("collection printing uses its own runtime helper"),
        }
    }

    /// Lower `Op::PushStr` or `Op::PushBytes`: emit `global_value` for the
    /// data symbol that holds this literal's bytes, push the address (typed
    /// as `ty`) onto the compile-time stack.
    fn lower_push_str(&mut self, id: StrId, ty: Ty) -> Result<()> {
        let data_id = *self.str_data.get(&id).ok_or_else(|| -> Box<dyn Error> {
            // `declare_str_data` is supposed to register every StrId
            // reachable through ops; missing here means the collection
            // walk missed an op variant.
            format!("AOT: PushStr({id:?}) without a declared data symbol").into()
        })?;
        let gv = self.module.declare_data_in_func(data_id, self.bcx.func);
        let addr = self.bcx.ins().symbol_value(PTR_TY, gv);
        self.stack.push((addr, ty));
        Ok(())
    }

    /// Add numbers with checked integer or IEEE float arithmetic; concatenate
    /// strings through the counted-string runtime.
    fn lower_add(&mut self) -> Result<()> {
        let len = self.stack.len();
        if len >= 2 && self.stack[len - 1].1 == Ty::Str && self.stack[len - 2].1 == Ty::Str {
            let b = self.stack.pop().expect("len >= 2").0;
            let a = self.stack.pop().expect("len >= 2").0;
            let concat = self
                .module
                .declare_func_in_func(self.runtime.concat, self.bcx.func);
            let inst = self.bcx.ins().call(concat, &[a, b]);
            let v = self.bcx.inst_results(inst)[0];
            self.release(a, &Ty::Str);
            self.release(b, &Ty::Str);
            self.stack.push((v, Ty::Str));
            return Ok(());
        }
        self.lower_checked_arith(ArithKind::Add)
    }

    /// Lower equality and inequality. Strings use the runtime content
    /// comparison; integers and Bools use CLIF's fixed-width `icmp`.
    fn lower_eq(&mut self) -> Result<()> {
        self.lower_equality(IntCC::Equal, false)
    }

    fn lower_ne(&mut self) -> Result<()> {
        self.lower_equality(IntCC::NotEqual, true)
    }

    fn lower_equality(&mut self, cc: IntCC, negate_string_result: bool) -> Result<()> {
        if self
            .stack
            .last()
            .is_some_and(|(_, ty)| ty.uses_value_runtime() || *ty == Ty::ByteStr)
        {
            let (a, b, ty) = self.pop_pair()?;
            let eq = self.collection_call(8, &[a, b], Some(&ty))?;
            self.release(a, &ty);
            self.release(b, &ty);
            let mut value = self.bcx.ins().ireduce(types::I8, eq);
            if negate_string_result {
                value = self.bcx.ins().bxor_imm_s(value, 1);
            }
            self.stack.push((value, Ty::Bool));
            return Ok(());
        }
        let len = self.stack.len();
        if len >= 2 && self.stack[len - 1].1 == Ty::Str && self.stack[len - 2].1 == Ty::Str {
            let b = self.stack.pop().expect("len >= 2").0;
            let a = self.stack.pop().expect("len >= 2").0;
            let str_eq = self
                .module
                .declare_func_in_func(self.runtime.str_eq, self.bcx.func);
            let inst = self.bcx.ins().call(str_eq, &[a, b]);
            let eq = self.bcx.inst_results(inst)[0];
            for operand in [a, b] {
                if !self.forget_slot_copy(operand) {
                    self.release(operand, &Ty::Str);
                }
            }
            let v = if negate_string_result {
                let one = self.bcx.ins().iconst(types::I8, 1);
                self.bcx.ins().bxor(eq, one)
            } else {
                eq
            };
            self.stack.push((v, Ty::Bool));
            return Ok(());
        }
        let (a, b, ty) = self.pop_pair()?;
        let v = if ty.is_float() {
            self.bcx.ins().fcmp(
                if negate_string_result {
                    FloatCC::NotEqual
                } else {
                    FloatCC::Equal
                },
                a,
                b,
            )
        } else {
            self.bcx.ins().icmp(cc, a, b)
        };
        self.stack.push((v, Ty::Bool));
        Ok(())
    }

    /// Lower strict Boolean `and` / `or`. Both values have already been
    /// evaluated by the stack language, so these are bit operations rather
    /// than short-circuit control flow.
    fn lower_bool_binop(&mut self, and: bool) -> Result<()> {
        let (b, _) = self.pop_typed(Ty::Bool)?;
        let (a, _) = self.pop_typed(Ty::Bool)?;
        let v = if and {
            self.bcx.ins().band(a, b)
        } else {
            self.bcx.ins().bor(a, b)
        };
        self.stack.push((v, Ty::Bool));
        Ok(())
    }

    /// Lower `LoadLocal(i)`: read the i-th input variable and push the
    /// resulting SSA value onto the compile-time stack. The compiler
    /// only emits `LoadLocal` inside a function body, so `self.locals`
    /// is always populated when we get here.
    fn lower_load_local(&mut self, i: u8) -> Result<()> {
        let (_, ty) = self
            .locals
            .get(i as usize)
            .cloned()
            .ok_or_else(|| -> Box<dyn Error> {
                format!("AOT: LoadLocal({i}) has no matching input").into()
            })?;
        let v = self.read_local(i);
        let v = if ty.can_copy() {
            self.snapshot_inline(v, &ty)
        } else {
            v
        };
        if self.counters.nonnegative_locals.contains(&i) {
            self.counters.nonnegative.insert(v);
        }
        let block = self.bcx.current_block().ok_or("load outside a block")?;
        let before = self.bcx.func.layout.last_inst(block);
        let retain = self.retain(v, &ty);
        let counting = if matches!(ty, Ty::Enum(_)) && ty.managed() {
            let layout = &self.bcx.func.layout;
            let mut next = before.map_or(layout.first_inst(block), |inst| layout.next_inst(inst));
            std::iter::from_fn(|| {
                let inst = next?;
                next = layout.next_inst(inst);
                Some(inst)
            })
            .collect()
        } else {
            Vec::new()
        };
        self.fresh_load = (retain.is_some() || !counting.is_empty()).then_some(FreshLoad {
            value: v,
            retain,
            counting,
        });
        self.stack.push((v, ty));
        Ok(())
    }

    /// Pop the inputs for a call to `name` from the compile-time stack,
    /// returning them in call order (deepest = position 0) along with
    /// the callee's declaration.
    fn pop_call_args(
        &mut self,
        name: &str,
    ) -> Result<(&UserFn, Vec<cranelift_codegen::ir::Value>)> {
        let decl = self.user_fns.get(name).ok_or_else(|| -> Box<dyn Error> {
            // Should have been caught by the operation checker; this
            // is the defensive arm for direct-construction paths.
            format!("AOT: undefined function `{name}`").into()
        })?;
        let n = decl.sig.inputs.len();
        if self.stack.len() < n {
            return Err(format!("AOT: stack underflow calling `{name}`").into());
        }
        // Drain in stack order: the deepest popped value is `inputs[0]`,
        // in function parameter order.
        let split = self.stack.len() - n;
        let args: Vec<_> = self.stack.drain(split..).map(|(v, _)| v).collect();
        Ok((decl, args))
    }

    /// Lower `Op::Call`: emit a regular call and push each return value
    /// onto the compile-time stack with its declared `Ty`.
    fn lower_call(&mut self, name: &str) -> Result<()> {
        self.lower_call_captured(name, &[])
    }

    fn lower_call_captured(
        &mut self,
        name: &str,
        captures: &[cranelift_codegen::ir::Value],
    ) -> Result<()> {
        let caller_standard = self.in_standard_library();
        let optimize = self.module.optimizes();
        let in_generator = self.generator.is_some();
        let user_fns = self.user_fns;
        let (_, mut args) = self.pop_call_args(name)?;
        let decl: &UserFn = &user_fns[name];
        // A native call or an inlined constructor copies what it keeps of its
        // arguments before the caller runs again; a leaf may return one in
        // place. A move this call does not take stays pending: no callee can
        // write the caller's locals. See [`inline::Moves`].
        let keeps_address = decl.inline.is_some()
            || portable_simd::operation(name, decl, optimize).is_some()
            || decl.sig.outputs.iter().any(Ty::contains_generator_frame);
        for moved in std::mem::take(&mut self.moves.pending) {
            match args.iter().rposition(|value| *value == moved) {
                Some(index) if keeps_address => {
                    let ty = decl.sig.inputs[index].1.clone();
                    args[index] = self.snapshot_inline(moved, &ty);
                }
                Some(_) => {}
                None => self.moves.pending.push(moved),
            }
        }
        if let Some(simd) = portable_simd::operation(name, decl, optimize) {
            self.lower_portable_simd(&simd, &args);
            return Ok(());
        }
        // User code names itself as the caller of std. Std restores the
        // enclosing user site after a callee that may change it, such as one
        // that tail-calls through user code before it returns.
        let track_caller = if caller_standard {
            !decl.preserves_caller
        } else {
            decl.standard_library || decl.source.is_none()
        };
        if let Some(inline) = decl.inline.clone() {
            let saved = if track_caller {
                self.enter_call()?
            } else {
                None
            };
            self.lower_leaf_call(&inline, &args, captures)?;
            self.leave_call(saved);
            return Ok(());
        }
        if let Some(body) = decl.inline_body.clone()
            && captures.is_empty()
            && !in_generator
        {
            return self.lower_inlined_call(name, &body, args, track_caller);
        }
        let outputs = decl.sig.outputs.clone();
        let func_id = decl.id;
        let bytes = outputs.iter().map(Ty::inline_bytes).sum();
        if bytes != 0 {
            args.push(self.inline_storage(bytes));
        }
        let funcref = self.module.declare_func_in_func(func_id, self.bcx.func);
        let saved = if track_caller {
            self.enter_call()?
        } else {
            None
        };
        let inst = self.bcx.ins().call(funcref, &args);
        let results: Vec<cranelift_codegen::ir::Value> = self.bcx.inst_results(inst).to_vec();
        self.leave_call(saved);
        debug_assert_eq!(results.len(), outputs.len());
        for (v, ty) in results.into_iter().zip(outputs) {
            self.stack.push((v, ty));
        }
        Ok(())
    }

    fn lower_indirect_call(
        &mut self,
        signature: &crate::op::CallableSig,
        tail: bool,
    ) -> Result<()> {
        let split = self
            .stack
            .len()
            .checked_sub(signature.inputs.len())
            .ok_or("indirect call stack underflow")?;
        let mut args: Vec<_> = self.stack.drain(split..).map(|(v, _)| v).collect();
        let (callee, _) = self.stack.pop().ok_or("missing indirect callee")?;
        let sig = signature.function();
        if tail {
            self.tail_arguments(&sig, &mut args)?;
            let native = user_fn_signature(&sig);
            let reference = self.bcx.import_signature(native);
            // In std the site would be null, which changes nothing.
            let track_caller = !self.in_standard_library();
            self.terminated = true;
            if self.shared_tail_call_indirect(reference, callee, &args, track_caller)? {
                return Ok(());
            }
            self.release_locals();
            if track_caller {
                self.enter_call()?;
            }
            self.bcx
                .ins()
                .return_call_indirect(reference, callee, &args);
            self.terminated = true;
            return Ok(());
        }
        let bytes = sig.outputs.iter().map(Ty::inline_bytes).sum();
        if bytes != 0 {
            args.push(self.inline_storage(bytes));
        }
        let native = user_fn_signature(&sig);
        let reference = self.bcx.import_signature(native);
        let saved = self.enter_call()?;
        let call = self.bcx.ins().call_indirect(reference, callee, &args);
        let results = self.bcx.inst_results(call).to_vec();
        self.leave_call(saved);
        for (value, ty) in results.into_iter().zip(sig.outputs) {
            self.stack.push((value, ty));
        }
        Ok(())
    }

    /// Lower `Op::TailCall`: emit `return_call`, which transfers control
    /// to the callee without growing the call stack — the iteration
    /// primitive for Plenty's recursive control flow (§11.8). The
    /// instruction is a block terminator, so we set `self.terminated`
    /// and the outer loop stops feeding ops to this lowerer.
    ///
    /// Eligibility was checked before lowering. A rejected plan here is an
    /// internal invariant violation, never an ordinary-call substitution.
    fn lower_tail_call(&mut self, name: &str) -> Result<()> {
        let caller_standard = self.in_standard_library();
        let cleanup_free = !self.locals.iter().any(|(_, ty)| ty.managed());
        let in_generator = self.generator.is_some();
        let (decl, mut args) = self.pop_call_args(name)?;
        // Only user code sets a site; std would pass null, which changes
        // nothing.
        let track_caller = !caller_standard && (decl.standard_library || decl.source.is_none());
        // A real tail call destroys owned locals before entering its callee.
        // Keep that ordering when the leaf may trap: only substitute callers
        // whose final local cleanup has no observable work.
        if let Some(inline) = decl.inline.clone()
            && cleanup_free
        {
            let saved = if track_caller {
                self.enter_call()?
            } else {
                None
            };
            self.lower_leaf_call(&inline, &args, &[])?;
            self.leave_call(saved);
            return self.lower(&Op::Return);
        }
        // A constructor lowered in its callers keeps a real tail call's order:
        // the caller's locals are released before the callee's body runs, and
        // the return's shared exit then finds only empty slots.
        if let Some(body) = decl.inline_body.clone()
            && !in_generator
        {
            self.release_locals();
            self.lower_inlined_call(name, &body, args, track_caller)?;
            return self.lower(&Op::Return);
        }
        let func_id = decl.id;
        let sig = decl.sig.clone();
        self.tail_arguments(&sig, &mut args)?;
        let funcref = self.module.declare_func_in_func(func_id, self.bcx.func);
        self.terminated = true;
        if self.shared_tail_call(func_id, funcref, track_caller, &args)? {
            return Ok(());
        }
        self.release_locals();
        if track_caller {
            self.enter_call()?;
        }
        self.bcx.ins().return_call(funcref, &args);
        self.terminated = true;
        Ok(())
    }

    fn tail_arguments(
        &mut self,
        callee: &FnSig,
        args: &mut Vec<cranelift_codegen::ir::Value>,
    ) -> Result<()> {
        let caller = FnSig {
            inputs: Vec::new(),
            outputs: self.return_types.to_vec(),
        };
        let plan = crate::tail_abi::classify(&caller, callee)
            .map_err(|reason| format!("internal tail-call ABI invariant: {reason}"))?;
        if self.bcx.func.signature.call_conv != CallConv::Tail {
            return Err(
                "internal tail-call ABI invariant: caller does not use Tail convention".into(),
            );
        }
        if self
            .bcx
            .func
            .signature
            .returns
            .iter()
            .map(|param| param.value_type)
            .ne(plan.layout.outputs.iter().copied())
        {
            return Err("internal tail-call ABI invariant: native result layout mismatch".into());
        }
        if plan.layout.result_area.is_some() {
            // This address belongs to an older frame. It cannot alias outgoing
            // borrows: source code has no access to its hidden result area.
            args.push(
                self.return_storage
                    .ok_or("internal tail-call ABI invariant: missing caller result area")?,
            );
        }
        if args.len() != plan.layout.inputs.len()
            || args
                .iter()
                .zip(&plan.layout.inputs)
                .any(|(value, ty)| self.bcx.func.dfg.value_type(*value) != *ty)
        {
            return Err("internal tail-call ABI invariant: native argument layout mismatch".into());
        }
        Ok(())
    }

    /// Lower `Op::If`: a block per body, each later condition evaluated in the
    /// block the previous `brif` falls into, and one join block as for
    /// `Op::Match`. A loop over the arms, so a long `elif` chain costs no
    /// native stack (issue #70).
    fn lower_if(&mut self, then: &[Op], elifs: &[ElifArm], otherwise: &[Op]) -> Result<()> {
        let (mut test, _) = self.stack.pop().ok_or("AOT: stack underflow on if")?;
        // Every body and every later condition starts from this stack.
        let entry_stack = self.stack.clone();
        let join_block = self.bcx.create_block();
        let mut join_param_types: Option<Vec<Ty>> = None;
        let conditions = elifs.iter().map(|arm| Some(&arm.condition)).chain([None]);
        let bodies = std::iter::once(then).chain(elifs.iter().map(|arm| &*arm.body));
        for (body, next_condition) in bodies.zip(conditions) {
            let (taken, fall) = (self.bcx.create_block(), self.bcx.create_block());
            self.bcx.ins().brif(test, taken, &[], fall, &[]);
            self.bcx.switch_to_block(taken);
            self.bcx.seal_block(taken);
            self.lower_branch(body, &entry_stack, join_block, &mut join_param_types)?;
            self.bcx.switch_to_block(fall);
            self.bcx.seal_block(fall);
            self.stack = entry_stack.clone();
            self.terminated = false;
            let Some(condition) = next_condition else {
                break;
            };
            for op in condition.iter() {
                self.lower(op)?;
            }
            test = self.stack.pop().ok_or("AOT: stack underflow on elif")?.0;
        }
        self.lower_branch(otherwise, &entry_stack, join_block, &mut join_param_types)?;

        self.bcx.switch_to_block(join_block);
        self.bcx.seal_block(join_block);
        match join_param_types {
            Some(types) => {
                let params = self.bcx.block_params(join_block).to_vec();
                self.stack = params.into_iter().zip(types).collect();
                self.terminated = false;
            }
            None => {
                // Every branch exited the function; the join is unreachable.
                self.bcx.ins().trap(TrapCode::unwrap_user(2));
                self.terminated = true;
            }
        }
        Ok(())
    }

    /// Lower one body of an `if` in the current block. A body that continues
    /// jumps to `join` carrying its whole stack; the first such body fixes the
    /// join's parameter types, which the checker made every other one match.
    fn lower_branch(
        &mut self,
        body: &[Op],
        entry_stack: &[StackEntry],
        join: Block,
        join_param_types: &mut Option<Vec<Ty>>,
    ) -> Result<()> {
        self.stack = entry_stack.to_vec();
        self.terminated = false;
        for op in body {
            if self.terminated {
                return Ok(());
            }
            self.lower(op)?;
        }
        if self.terminated {
            return Ok(());
        }
        if join_param_types.is_none() {
            let types: Vec<Ty> = self.stack.iter().map(|(_, t)| t.clone()).collect();
            for ty in &types {
                self.bcx.append_block_param(join, clif_type(ty.clone()));
            }
            *join_param_types = Some(types);
        }
        let args: Vec<BlockArg> = self
            .stack
            .iter()
            .map(|(v, _)| BlockArg::Value(*v))
            .collect();
        self.bcx.ins().jump(join, &args);
        Ok(())
    }

    /// Lower `Op::Match`: one CLIF block per arm, a bounded jump table or
    /// linear `brif` chain for dispatch, and a join block whose params carry the
    /// agreed stack shape every arm leaves (§11.8). The type checker
    /// has already enforced exhaustiveness and pointwise agreement, so
    /// the lowerer only has to mirror that structure — no runtime
    /// shape-checking is needed.
    ///
    /// An arm that returns or tail-calls does *not* jump to the
    /// join block: both instructions are block terminators and the
    /// arm leaves the function entirely. If *every* arm terminates,
    /// the whole match terminates the surrounding context and the join
    /// block is unreachable — we still need a terminator so Cranelift
    /// accepts the function, so we emit a defensive `trap` there.
    fn lower_match(&mut self, arms: &[MatchArm]) -> Result<()> {
        let (scrut, scrut_ty) = self.stack.pop().ok_or("AOT: stack underflow on match")?;
        // The state every arm starts from — the data stack at the
        // point `match` consumes its scrutinee.
        let entry_stack = self.stack.clone();

        // One block per arm body; seal only once every dispatch edge is known.
        let arm_blocks: Vec<Block> = arms.iter().map(|_| self.bcx.create_block()).collect();
        let join_block = self.bcx.create_block();

        // --- Dispatch chain --------------------------------------------------
        // We're currently in whatever block called `lower_match`. Each
        // non-wildcard pattern emits an `icmp eq` + `brif`; the false
        // branch falls into a fresh block we switch to for the next
        // compare. A wildcard short-circuits with an unconditional jump
        // and renders any trailing arms unreachable (the checker would
        // already have noticed if a useful arm came after `_`).
        let mut chain_terminated = self.lower_dense_dispatch(scrut, &scrut_ty, arms, &arm_blocks);
        // Indexed by `bool as usize`: the values earlier `Bool` arms tested.
        let mut bool_covered = [false; 2];
        for (i, arm) in arms.iter().enumerate() {
            if chain_terminated {
                break;
            }
            match arm.pattern {
                Pattern::Wildcard => {
                    self.bcx.ins().jump(arm_blocks[i], &[]);
                    chain_terminated = true;
                }
                Pattern::Bool(b) => {
                    // A bool holds 0 or 1, so the scrutinee is its own
                    // branch condition, and once one value has been tested
                    // the other is the only case left: that arm takes the
                    // fall-through edge unconditionally, with no
                    // impossible-case trap after it (issue #362).
                    if bool_covered[usize::from(b)] {
                        continue;
                    }
                    bool_covered[usize::from(b)] = true;
                    if bool_covered[usize::from(!b)] {
                        self.bcx.ins().jump(arm_blocks[i], &[]);
                        chain_terminated = true;
                        continue;
                    }
                    let next = self.bcx.create_block();
                    if b {
                        self.bcx.ins().brif(scrut, arm_blocks[i], &[], next, &[]);
                    } else {
                        self.bcx.ins().brif(scrut, next, &[], arm_blocks[i], &[]);
                    }
                    self.bcx.switch_to_block(next);
                    self.bcx.seal_block(next);
                }
                Pattern::Int { value, .. } => {
                    // The checker has already established that an untyped
                    // pattern fits the scrutinee or that a typed pattern has
                    // the same type. `iconst` therefore receives the exact
                    // bit pattern to compare at the scrutinee's width.
                    let pat = self
                        .bcx
                        .ins()
                        .iconst(clif_type(scrut_ty.clone()), int_value_bits(value));
                    let eq = self.bcx.ins().icmp(IntCC::Equal, scrut, pat);
                    let next = self.bcx.create_block();
                    self.bcx.ins().brif(eq, arm_blocks[i], &[], next, &[]);
                    self.bcx.switch_to_block(next);
                    self.bcx.seal_block(next);
                }
                Pattern::Str(id) => {
                    // String compares are runtime calls — `plenty_str_eq`
                    // does the byte-for-byte comparison and returns a
                    // Plenty Bool (`i8`). The data symbol for `id` was
                    // already declared by `declare_str_data`.
                    let data_id = *self.str_data.get(&id).ok_or_else(|| -> Box<dyn Error> {
                        format!("AOT: Pattern::Str({id:?}) without declared data").into()
                    })?;
                    let gv = self.module.declare_data_in_func(data_id, self.bcx.func);
                    let pat_addr = self.bcx.ins().symbol_value(PTR_TY, gv);
                    let str_eq = self
                        .module
                        .declare_func_in_func(self.runtime.str_eq, self.bcx.func);
                    let call = self.bcx.ins().call(str_eq, &[scrut, pat_addr]);
                    let eq = self.bcx.inst_results(call)[0];
                    let next = self.bcx.create_block();
                    self.bcx.ins().brif(eq, arm_blocks[i], &[], next, &[]);
                    self.bcx.switch_to_block(next);
                    self.bcx.seal_block(next);
                }
            }
        }
        if !chain_terminated {
            // No wildcard arm matched the chain's fall-through path.
            // The checker enforces exhaustiveness, so this is dead
            // code under any well-formed source — emit a trap so
            // compiler construction bugs surface loudly instead of
            // walking off the end of the function.
            self.bcx.ins().trap(TrapCode::unwrap_user(1));
        }

        // --- Arm bodies ------------------------------------------------------
        // The join block's param types are decided by the first
        // non-terminating arm; the checker has already guaranteed every
        // subsequent non-terminating arm leaves the same shape, so the
        // Cranelift verifier's "block param count must match jump arg
        // count" rule lines up automatically.
        let mut join_param_types: Option<Vec<Ty>> = None;
        let mut any_arm_falls_through = false;
        for (i, arm) in arms.iter().enumerate() {
            self.bcx.switch_to_block(arm_blocks[i]);
            self.bcx.seal_block(arm_blocks[i]);
            self.stack = entry_stack.clone();
            self.terminated = false;
            self.release(scrut, &scrut_ty);
            for op in arm.body.iter() {
                if self.terminated {
                    break;
                }
                self.lower(op)?;
            }
            if self.terminated {
                continue;
            }
            any_arm_falls_through = true;
            if join_param_types.is_none() {
                let types: Vec<Ty> = self.stack.iter().map(|(_, t)| t.clone()).collect();
                for ty in &types {
                    self.bcx
                        .append_block_param(join_block, clif_type(ty.clone()));
                }
                join_param_types = Some(types);
            }
            // `jump` takes `&[BlockArg]`; every Plenty stack value is
            // an SSA `Value`, which converts via `BlockArg::Value(_)`.
            let args: Vec<BlockArg> = self
                .stack
                .iter()
                .map(|(v, _)| BlockArg::Value(*v))
                .collect();
            self.bcx.ins().jump(join_block, &args);
        }

        // --- Join block ------------------------------------------------------
        self.bcx.switch_to_block(join_block);
        self.bcx.seal_block(join_block);
        if any_arm_falls_through {
            // Each arm started from a clone of `entry_stack` and the
            // join block's params carry the *whole* stack the arm ended
            // with — so the post-match stack is exactly those params,
            // not entry_stack with the params appended. The type
            // checker reflects the same shape (`*stack = joined` in
            // `check_match`).
            let types = join_param_types.expect("set when an arm falls through");
            let params = self.bcx.block_params(join_block).to_vec();
            self.stack = params.into_iter().zip(types).collect();
            self.terminated = false;
        } else {
            // Every arm exited the function; the join is unreachable. Emit a
            // trap to give the block a terminator and signal upward
            // that the surrounding context is also dead.
            self.bcx.ins().trap(TrapCode::unwrap_user(2));
            self.terminated = true;
        }
        Ok(())
    }
}
