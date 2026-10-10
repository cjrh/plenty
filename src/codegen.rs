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
//! patterns lower to a linear chain of `brif` compares (wildcards become
//! unconditional jumps and short-circuit the chain), and a single join
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
//!
//! The precompiled `plenty-runtime` archive is embedded in the compiler. AOT
//! builds write it beside the Cranelift object and invoke `cc` only to link.
//! Runtime compilation happens when building Plenty, never for individual programs.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::rc::Rc;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, AbiParam, Block, BlockArg, Function, InstBuilder, Signature, TrapCode, UserFuncName,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::Configurable;
use cranelift_codegen::{settings, Context};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::lexer;
mod channels;
mod closures;
mod collections;
mod control;
mod enums;
mod executors;
mod exports;
mod foreign;
mod generators;
mod inline;
mod metadata;
mod references;
mod threads;
use crate::op::{self, FnSig, MatchArm, Op, Pattern, Ty};
use crate::value::{Heap, StrId, Value};
use generators::GeneratorContext;

// ---- Cranelift API reference ----
//
// Cranelift's user-facing API is not indexed by context7; these notes
// record gotchas paid for during phases c.2–c.5.5 so the next agent does
// not pay for them again. Source under `~/.cargo/registry/src/.../cranelift-*-0.131.*/`.
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
//   func) -> GlobalValue` and then `builder.ins().global_value(ty, gv)`.
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
    let mut heap = Heap::default();
    let program = crate::frontend::compile(source, &mut heap)?;
    compile_ops_to_executable(&program.ops, &heap, output, program.returns_status, options)
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
    let mut heap = Heap::default();
    let program = crate::frontend::compile_file(path, root, true, &mut heap)?;
    compile_ops_to_executable(&program.ops, &heap, output, program.returns_status, options)
}

/// Historical stack syntax, retained for backend regression tests.
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
    compile_ops_to_executable(&ops, &heap, output, false, options)
}

fn compile_ops_to_executable(
    ops: &[Op],
    heap: &Heap,
    output: &Path,
    returns_status: bool,
    options: &crate::CompileOptions,
) -> Result<()> {
    op::check(ops)?;

    let workspace = tempfile::tempdir()?;
    let obj_path = workspace.path().join("program.o");
    let rt_path = workspace.path().join("libplenty_runtime.a");
    compile_to_object(ops, heap, &obj_path, returns_status)?;
    std::fs::write(&rt_path, RUNTIME_ARCHIVE)?;
    crate::toolchain::link(&obj_path, &rt_path, RUNTIME_LINK_ARGS, output, options)
}

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
    let mut heap = Heap::default();
    let program = crate::frontend::compile(source, &mut heap)?;
    op::check(&program.ops)?;
    compile_to_object(&program.ops, &heap, output, program.returns_status)
}

/// Emit an application object, resolving absolute Plenty imports.
pub fn compile_file_to_object(path: &Path, output: &Path, root: Option<&Path>) -> Result<()> {
    crate::validate_target(None)?;
    let mut heap = Heap::default();
    let program = crate::frontend::compile_file(path, root, true, &mut heap)?;
    op::check(&program.ops)?;
    compile_to_object(&program.ops, &heap, output, program.returns_status)
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Lower `ops` to a native object file at `output`.
///
/// The object exports `plenty_main` (`() -> i32`) and one locally-linked
/// symbol per user-defined Plenty function, plus one read-only data
/// symbol per source-level string literal whose bytes come from `heap`.
/// Link the object with the packaged `plenty-runtime` archive to produce an
/// executable. With `returns_status`, the final `i32` operand is the process
/// status; unit entrypoints and legacy programs return zero on completion.
fn compile_to_object(ops: &[Op], heap: &Heap, output: &Path, returns_status: bool) -> Result<()> {
    emit_object(ops, heap, output, Some(returns_status), &[], None)
}

pub(crate) fn emit_library_object(
    program: &crate::frontend::Program,
    heap: &Heap,
    output: &Path,
    interface: &crate::exports::Interface,
) -> Result<()> {
    emit_object(
        &program.ops,
        heap,
        output,
        None,
        &program.exports,
        Some(interface),
    )
}

fn emit_object(
    ops: &[Op],
    heap: &Heap,
    output: &Path,
    entry: Option<bool>,
    exports: &[crate::exports::Export],
    interface: Option<&crate::exports::Interface>,
) -> Result<()> {
    let isa = host_isa()?;
    let builder = ObjectBuilder::new(isa, "plenty", cranelift_module::default_libcall_names())?;
    let mut module = ObjectModule::new(builder);

    let mut runtime = declare_runtime(&mut module)?;

    // Pass 1: collect every user-defined function reachable from `ops`
    // (top-level, nested under another definition, or inside a match
    // arm), declare each as a Cranelift symbol with the tail-call
    // convention so its body can `return_call` other user functions.
    let mut user_fns: HashMap<String, UserFn> = HashMap::new();
    collect_user_fns(ops, &mut module, &mut user_fns)?;
    runtime.drop_hooks = user_fns
        .iter()
        .filter_map(|(name, f)| Some((name.clone(), f.drop_callback?)))
        .collect();
    let runtime = runtime;

    // Pass 1b: emit one read-only data symbol per source string literal.
    // We walk the ops (recursing into bodies and match arms) collecting
    // every `StrId` referenced by `PushStr` or `Pattern::Str`, then
    // declare and define each one. The compiler's string pool is the
    // source of truth for the literal bytes.
    let str_data = declare_str_data(ops, heap, &mut module)?;

    // An immortal empty string substitutes for the legacy input helper's EOF.
    let eof_empty_str = declare_eof_empty_str(&mut module)?;
    threads::emit_adapters(
        ops,
        &user_fns,
        &str_data,
        eof_empty_str,
        &runtime,
        &mut module,
    )?;
    executors::emit_adapters(
        ops,
        &user_fns,
        &str_data,
        eof_empty_str,
        &runtime,
        &mut module,
    )?;

    // Pass 2: emit each user function's body. Bodies can refer to each
    // other (forward references, mutual recursion) because every callee
    // is already declared.
    let names: Vec<String> = user_fns.keys().cloned().collect();
    for name in &names {
        emit_user_function(
            name,
            &user_fns,
            &str_data,
            eof_empty_str,
            &runtime,
            &mut module,
        )?;
    }

    // Pass 3: emit `plenty_main`. Top-level `DefineFn`s are skipped
    // here — their bodies were emitted by Pass 2; at runtime a
    // definition is a no-op (it does not touch the data stack).
    if let Some(returns_status) = entry {
        emit_main(
            ops,
            returns_status,
            &user_fns,
            &str_data,
            eof_empty_str,
            &runtime,
            &mut module,
        )?;
    }
    exports::emit(exports, interface, &user_fns, &runtime, &mut module)?;

    let product = module.finish();
    let bytes = product.emit()?;
    std::fs::write(output, bytes)?;
    Ok(())
}

/// Build an `ISA` for the host target. Cranelift's `native` crate
/// inspects the running CPU's features so emitted code can take
/// advantage of what's available without us having to enumerate it.
fn host_isa() -> Result<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    crate::validate_target(None)?;
    let mut flags = settings::builder();
    // `is_pic` so the object can be linked into a position-independent
    // executable, which is what every modern Linux/macOS toolchain
    // produces by default.
    flags.set("is_pic", "true")?;
    flags.set("enable_llvm_abi_extensions", "true")?;
    // Frame pointers must be preserved for `return_call` emission on
    // x86_64: the backend hooks the tail-call stack-arg fixup off the
    // frame-pointer prologue/epilogue. Without this, lowering any
    // Plenty TailCall panics inside Cranelift with "the current
    // implementation relies on [frame pointers] being present".
    flags.set("preserve_frame_pointers", "true")?;
    let isa_builder = cranelift_native::builder().map_err(|e| -> Box<dyn Error> { e.into() })?;
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
    thread_start: FuncId,
    thread_join: FuncId,
    thread_adapters: std::cell::RefCell<HashMap<String, FuncId>>,
    type_data: std::cell::RefCell<HashMap<Ty, DataId>>,
    /// `__del__` adapters by method name, linked into class descriptors.
    drop_hooks: HashMap<String, FuncId>,
    collection: FuncId,
    list_len: FuncId,
    list_scalar_get: FuncId,
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
    /// `plenty_trap_overflow() -> !` — prints `error: integer overflow`
    /// to stderr and `exit(1)`s. The lowerer calls this from the
    /// overflow branch of every checked arithmetic op.
    trap_overflow: FuncId,
    /// `plenty_trap_div_zero() -> !` — prints `error: division by zero`
    /// to stderr and `exit(1)`s. Called from the zero-check branch of
    /// the `Div` lowering.
    trap_div_zero: FuncId,
    /// `plenty_readline() -> *const u8` — read one newline-terminated
    /// line from stdin, strip its newline, validate UTF-8, and return an owned
    /// counted string. NULL is internal EOF, never a language string.
    readline: FuncId,
    /// `plenty_contains(*const u8 haystack, *const u8 needle) -> i8` —
    /// returns 1 if `needle` is a byte-substring of `haystack`, 0
    /// otherwise. Both inputs are borrowed counted strings.
    contains: FuncId,
    /// `plenty_println(*const u8) -> ()` — write the string raw to
    /// stdout, followed by a single `\n`. The bare-text output
    /// primitive; `plenty_print_str` (the `.` path) escapes and
    /// quotes, `plenty_println` does not.
    println: FuncId,
}

fn declare_runtime(module: &mut ObjectModule) -> Result<Runtime> {
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
    Ok(Runtime {
        control: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([
                AbiParam::new(types::I64),
                AbiParam::new(PTR_TY),
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
        thread_adapters: Default::default(),
        generator_init: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([AbiParam::new(types::I64); 6]);
            module.declare_function("plenty_generator_init", Linkage::Import, &sig)?
        },
        generator_finish: one_arg(module, "plenty_generator_finish", PTR_TY)?,
        type_data: Default::default(),
        drop_hooks: HashMap::new(),
        retain: one_arg(module, "plenty_retain", PTR_TY)?,
        release: one_arg(module, "plenty_release", PTR_TY)?,
        collection: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.extend([AbiParam::new(PTR_TY); 4]);
            module.declare_function("plenty_collection", Linkage::Import, &sig)?
        },
        list_len: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params.push(AbiParam::new(PTR_TY));
            sig.returns.push(AbiParam::new(types::I64));
            module.declare_function("plenty_list_len", Linkage::Import, &sig)?
        },
        list_scalar_get: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.params
                .extend([AbiParam::new(PTR_TY), AbiParam::new(types::I64)]);
            sig.returns.push(AbiParam::new(types::I64));
            module.declare_function("plenty_list_scalar_get", Linkage::Import, &sig)?
        },
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
        trap_overflow: nullary(module, "plenty_trap_overflow")?,
        trap_div_zero: nullary(module, "plenty_trap_div_zero")?,
        readline: {
            let mut sig = module.make_signature();
            sig.call_conv = CallConv::SystemV;
            sig.returns.push(AbiParam::new(PTR_TY));
            module.declare_function("plenty_readline", Linkage::Import, &sig)?
        },
        contains: two_args_one_return(module, "plenty_contains", PTR_TY, PTR_TY, types::I8)?,
        println: one_arg(module, "plenty_println", PTR_TY)?,
    })
}

/// Managed values use one pointer on the current native 64-bit host target.
const PTR_TY: types::Type = types::I64;

/// Serialize the runtime's 32-byte immortal string prefix and exact UTF-8 bytes.
/// AOT targets the host, so serialization uses native byte order.
fn string_literal_bytes(s: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32 + s.len());
    for word in [u64::MAX, 0, s.len() as u64, s.chars().count() as u64] {
        bytes.extend_from_slice(&word.to_ne_bytes());
    }
    bytes.extend_from_slice(s.as_bytes());
    bytes
}

fn declare_str_data(
    ops: &[Op],
    heap: &Heap,
    module: &mut ObjectModule,
) -> Result<HashMap<StrId, DataId>> {
    let mut ids: Vec<StrId> = Vec::new();
    let mut seen: HashMap<StrId, ()> = HashMap::new();
    collect_str_ids(ops, &mut ids, &mut seen);

    let mut out: HashMap<StrId, DataId> = HashMap::new();
    for (i, id) in ids.into_iter().enumerate() {
        // The name only has to be unique within the module; the linker
        // never sees it externally (Linkage::Local). A stable index
        // keeps the symbol names predictable when reading disassembly.
        let name = format!("plenty_str_{i}");
        let data_id = module.declare_data(&name, Linkage::Local, false, false)?;
        let s = heap.str(id);
        let bytes = string_literal_bytes(s);
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
    desc.define(string_literal_bytes("").into_boxed_slice());
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
            Op::PushStr(id) if seen.insert(*id, ()).is_none() => out.push(*id),
            Op::Match(arms) => {
                for arm in arms.iter() {
                    if let Pattern::Str(id) = arm.pattern {
                        if seen.insert(id, ()).is_none() {
                            out.push(id);
                        }
                    }
                    collect_str_ids(&arm.body, out, seen);
                }
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
    drop_callback: Option<FuncId>,
    generator: Option<Ty>,
    resume: Option<FuncId>,
    id: FuncId,
    sig: Rc<FnSig>,
    body: Rc<[Op]>,
    locals: Rc<[Ty]>,
}

/// Build a Cranelift `Signature` from a Plenty `FnSig`. User functions
/// always use `CallConv::Tail`: that is the only call convention in
/// Cranelift 0.131 that supports `return_call`, which is how we lower
/// Plenty's tail-call op. Tail-convention functions can still be called
/// non-tail (the verifier only requires matching conventions on
/// `return_call`), so `plenty_main` — which has SystemV convention,
/// because it is called from C — invokes user functions with a regular
/// `call` instruction.
fn user_fn_signature(module: &ObjectModule, sig: &FnSig) -> Signature {
    let mut cl = module.make_signature();
    cl.call_conv = CallConv::Tail;
    for (_, ty) in &sig.inputs {
        cl.params.push(AbiParam::new(clif_type(ty.clone())));
    }
    for ty in &sig.outputs {
        cl.returns.push(AbiParam::new(clif_type(ty.clone())));
    }
    if sig.outputs.iter().any(Ty::has_inline_storage) {
        cl.params.push(AbiParam::new(PTR_TY));
    }
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
) -> Result<()> {
    for op in ops {
        match op {
            Op::DefineFn(name, f) => {
                if out.contains_key(name) {
                    return Err(
                        format!("AOT compilation does not allow redefining `{name}`").into(),
                    );
                }
                let cl_sig = user_fn_signature(module, &f.sig);
                // Source names must never alias runtime helpers or the C entry
                // point (a user may legitimately define `plenty_main`).
                let symbol = format!("__plenty_fn_{name}");
                let id = module.declare_function(&symbol, Linkage::Local, &cl_sig)?;
                out.insert(
                    name.clone(),
                    UserFn {
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
                                &generators::resume_signature(module),
                            )?)
                        } else {
                            None
                        },
                        sig: Rc::clone(&f.sig),
                        body: Rc::clone(&f.body),
                        locals: Rc::clone(&f.locals),
                    },
                );
                collect_user_fns(&f.body, module, out)?;
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    collect_user_fns(&arm.body, module, out)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Emit the body of one user function. Each input becomes a CLIF
/// `Variable` defined once at entry from the matching block parameter;
/// `Op::LoadLocal(i)` later reads that variable. If the body falls
/// through without a tail call, emit a `return` carrying the values
/// remaining on the compile-time stack (the type checker has already
/// ensured those values match the declared outputs).
fn needs_local_addresses(ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::BorrowLocal(..) | Op::Thread(_) => true,
        Op::Match(arms) => arms.iter().any(|a| needs_local_addresses(&a.body)),
        Op::Loop { condition, body } => {
            needs_local_addresses(condition) || needs_local_addresses(body)
        }
        _ => false,
    })
}

fn emit_user_function(
    name: &str,
    fns: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    let decl = &fns[name];
    if decl.generator.is_some() {
        return generators::emit_generator(name, fns, str_data, eof_empty_str, runtime, module);
    }
    let cl_sig = user_fn_signature(module, &decl.sig);

    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, decl.id.as_u32()), cl_sig);
    let mut func_ctx = FunctionBuilderContext::new();
    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let entry = bcx.create_block();
        bcx.append_block_params_for_function_params(entry);
        bcx.switch_to_block(entry);
        bcx.seal_block(entry);

        // One `Variable` per input. We bind each from its block param
        // right at entry. Using `Variable` rather than the raw SSA
        // block-param value makes c.3 cleaner — match arms become new
        // blocks, and a `Variable` is visible across blocks where a
        // raw block-param value would have to be threaded explicitly.
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
                let zero = bcx.ins().iconst(types::I64, 0);
                let zero = if ty.wide() {
                    bcx.ins().uextend(types::I128, zero)
                } else {
                    zero
                };
                bcx.def_var(var, zero);
            }
            locals.push((var, ty.clone()));
        }

        let local_frame = if needs_local_addresses(&decl.body)
            || locals.iter().any(|(_, t)| t.has_inline_storage())
        {
            let slot = bcx.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                locals.iter().map(|(_, t)| t.slot_bytes() as u32).sum(),
                4,
            ));
            let frame = bcx.ins().stack_addr(PTR_TY, slot, 0);
            for (i, (var, ty)) in locals.iter().enumerate() {
                let packed = if i < decl.sig.inputs.len() || ty.managed() {
                    let value = bcx.use_var(*var);
                    enums::pack_value(&mut bcx, value, ty)
                } else {
                    let zero = bcx.ins().iconst(types::I64, 0);
                    bcx.ins().uextend(types::I128, zero)
                };
                bcx.ins().store(
                    cranelift_codegen::ir::MemFlags::trusted(),
                    packed,
                    frame,
                    locals[..i]
                        .iter()
                        .map(|(_, t)| t.slot_bytes() as i32)
                        .sum::<i32>(),
                );
            }
            Some(frame)
        } else {
            None
        };
        let return_storage = decl
            .sig
            .outputs
            .iter()
            .any(Ty::has_inline_storage)
            .then(|| bcx.block_params(entry)[decl.sig.inputs.len()]);
        let mut lower = Lowerer {
            bcx: &mut bcx,
            module,
            runtime,
            user_fns: fns,
            str_data,
            eof_empty_str,
            locals: &locals,
            stack: Vec::new(),
            terminated: false,
            loop_targets: Vec::new(),
            generator: None,
            local_frame,
            return_storage,
            collection_scratch: None,
        };
        // Argument addresses belong to the caller. Snapshot inline values into
        // this function's locals before any mutation or nested call can occur.
        for (i, (_, ty)) in decl.sig.inputs.iter().enumerate() {
            if ty.has_inline_storage() {
                let value = lower.read_local(i as u8);
                lower.write_local(i as u8, value);
            }
        }
        for op in decl.body.iter() {
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
            lower.return_values(lower.stack.clone());
        }
        bcx.finalize();
    }
    module
        .define_function(decl.id, &mut ctx)
        .map_err(|e| -> Box<dyn Error> { format!("in `{name}`: {e:?}").into() })?;
    if let Some(id) = decl.drop_callback {
        let mut signature = module.make_signature();
        signature.params.push(AbiParam::new(PTR_TY));
        let mut ctx = Context::new();
        ctx.func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
        let mut fc = FunctionBuilderContext::new();
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fc);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        b.seal_block(block);
        let receiver = b.block_params(block)[0];
        let receiver = b.ins().uextend(types::I128, receiver);
        let callee = module.declare_func_in_func(decl.id, b.func);
        b.ins().call(callee, &[receiver]);
        b.ins().return_(&[]);
        b.finalize();
        module.define_function(id, &mut ctx)?;
    }
    Ok(())
}

/// Emit `plenty_main` — the entry point the Rust runtime forwards to.
/// Top-level `DefineFn` ops are skipped (their bodies are emitted
/// separately by [`emit_user_function`]); everything else lowers
/// against an initially-empty compile-time stack, with no locals
/// in scope.
fn emit_main(
    ops: &[Op],
    returns_status: bool,
    fns: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    // `plenty_main`: exported, no arguments, returns `i32`. The Rust
    // runtime's native `main` forwards into this and returns
    // its result as the process exit code. SystemV convention because
    // the caller (the Rust runtime) speaks the host's C ABI; user
    // functions use `CallConv::Tail` and can still be invoked from here
    // via a regular `call`.
    let mut main_sig = module.make_signature();
    main_sig.returns.push(AbiParam::new(types::I32));
    let main_id = module.declare_function("plenty_main", Linkage::Export, &main_sig)?;

    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, main_id.as_u32()), main_sig);
    let mut func_ctx = FunctionBuilderContext::new();
    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let entry = bcx.create_block();
        bcx.append_block_params_for_function_params(entry);
        bcx.switch_to_block(entry);
        bcx.seal_block(entry);

        let mut lower = Lowerer {
            bcx: &mut bcx,
            module,
            runtime,
            user_fns: fns,
            str_data,
            eof_empty_str,
            locals: &[],
            stack: Vec::new(),
            terminated: false,
            loop_targets: Vec::new(),
            generator: None,
            local_frame: None,
            return_storage: None,
            collection_scratch: None,
        };
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
        bcx.finalize();
    }
    module.define_function(main_id, &mut ctx)?;
    Ok(())
}

/// The CLIF type backing each Plenty value. Plenty's signed/unsigned
/// distinction lives in the `Ty` tag we carry alongside the SSA value;
/// Cranelift treats both with the same machine type, the individual
/// instruction (`sdiv` vs `udiv`, `icmp slt` vs `icmp ult`) picks the
/// interpretation. `Str` is a host pointer (`PTR_TY`), the address of
/// a counted immutable object in read-only data or the managed runtime heap.
fn clif_type(ty: Ty) -> types::Type {
    if ty.wide() {
        return types::I128;
    }
    match ty {
        Ty::I8 | Ty::U8 | Ty::Bool => types::I8,
        Ty::I16 | Ty::U16 => types::I16,
        Ty::I32 | Ty::U32 => types::I32,
        Ty::I64 | Ty::U64 | Ty::Unit => types::I64,
        Ty::F32 => types::F32,
        Ty::F64 => types::F64,
        Ty::Str
        | Ty::Task(_)
        | Ty::Channel(..)
        | Ty::CancellationToken
        | Ty::Executor
        | Ty::Future(_)
        | Ty::Callable(_)
        | Ty::Closure(_)
        | Ty::File
        | Ty::ForeignPtr(_)
        | Ty::List(_)
        | Ty::Set(_)
        | Ty::Dict(_, _)
        | Ty::Box(_)
        | Ty::Range(_)
        | Ty::Class(_)
        | Ty::Enum(_)
        | Ty::Generator(_) => PTR_TY,
        Ty::Ref(..) => types::I128,
    }
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
#[derive(Clone, Copy)]
enum ArithKind {
    Add,
    Sub,
    Mul,
}

/// Which shared trap block to branch into on a failed check. The two
/// kinds map one-to-one to the two runtime helpers and the two
/// runtime arithmetic diagnostics.
#[derive(Clone, Copy)]
enum TrapKind {
    Overflow,
    DivZero,
}

struct Lowerer<'a, 'b> {
    bcx: &'a mut FunctionBuilder<'b>,
    module: &'a mut ObjectModule,
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
    /// [`emit_user_function`] stops feeding ops to this lowerer.
    terminated: bool,
    /// The last entry is the innermost loop: (continue target, break target).
    loop_targets: Vec<(Block, Block)>,
    generator: Option<GeneratorContext>,
    local_frame: Option<cranelift_codegen::ir::Value>,
    return_storage: Option<cranelift_codegen::ir::Value>,
    /// Reused across non-overlapping runtime calls; callbacks have their own frame.
    collection_scratch: Option<cranelift_codegen::ir::StackSlot>,
}

impl Lowerer<'_, '_> {
    fn retain(&mut self, value: cranelift_codegen::ir::Value, ty: &Ty) {
        if ty.wide() || matches!(ty, Ty::Closure(_)) {
            if ty.managed() {
                self.collection_call(26, &[value], Some(ty))
                    .expect("sum metadata");
            }
            return;
        }
        let value = self.raw_word(value);
        if ty.managed() {
            let f = self
                .module
                .declare_func_in_func(self.runtime.retain, self.bcx.func);
            self.bcx.ins().call(f, &[value]);
        }
    }
    fn release(&mut self, value: cranelift_codegen::ir::Value, ty: &Ty) {
        if ty.wide() || matches!(ty, Ty::Generator(_) | Ty::Closure(_)) {
            if ty.managed() {
                self.collection_call(27, &[value], Some(ty))
                    .expect("sum metadata");
            }
            return;
        }
        let value = self.raw_word(value);
        if ty.managed() {
            let f = self
                .module
                .declare_func_in_func(self.runtime.release, self.bcx.func);
            self.bcx.ins().call(f, &[value]);
        }
    }
    fn release_locals(&mut self) {
        for i in (0..self.locals.len()).rev() {
            self.drop_local(i as u8);
        }
    }
    fn read_local(&mut self, i: u8) -> cranelift_codegen::ir::Value {
        let (var, ty) = self.locals[i as usize].clone();
        let frame = self
            .generator
            .as_ref()
            .map(|g| (g.frame, 64))
            .or(self.local_frame.map(|p| (p, 0)));
        let value = if let Some((frame, base)) = frame {
            let offset = base + self.local_offset(i as usize);
            let value = self.bcx.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlags::trusted(),
                frame,
                offset as i32,
            );
            self.unpack(value, &ty)
        } else {
            self.bcx.use_var(var)
        };
        value
    }
    fn write_local(&mut self, i: u8, value: cranelift_codegen::ir::Value) {
        let (var, ty) = self.locals[i as usize].clone();
        let frame = self
            .generator
            .as_ref()
            .map(|g| (g.frame, 64))
            .or(self.local_frame.map(|p| (p, 0)));
        if let Some((frame, base)) = frame {
            let offset = base + self.local_offset(i as usize);
            let slot = self.bcx.ins().iadd_imm(frame, offset);
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
            let zero = self.bcx.ins().iconst(types::I64, 0);
            let zero = self.unpack(zero, &ty);
            self.write_local(i, zero);
        }
    }
    fn release_stack(&mut self) {
        while let Some((value, ty)) = self.stack.pop() {
            self.release(value, &ty);
        }
    }
    fn lower(&mut self, op: &Op) -> Result<()> {
        match op {
            Op::Thread(operation) => self.lower_thread(operation)?,
            Op::Channel(operation) => self.lower_channel(operation)?,
            Op::Control(operation) => self.lower_control(operation)?,
            Op::Executor(operation) => self.lower_executor(operation)?,
            Op::Try {
                source,
                target,
                cleanup,
            } => self.lower_try(source, target, cleanup)?,
            Op::Loan(_) | Op::UseLoan(_) | Op::Access(..) | Op::Site(_) => {}
            Op::BorrowLocal(i, mutable) => {
                let (frame, offset) = if let Some(g) = &self.generator {
                    (g.frame, 64)
                } else {
                    (self.local_frame.expect("addressable locals"), 0)
                };
                let offset = offset + self.local_offset(*i as usize);
                let ptr = self.bcx.ins().iadd_imm(frame, offset);
                let ptr = self.bcx.ins().uextend(types::I128, ptr);
                self.stack.push((
                    ptr,
                    Ty::Ref(
                        std::rc::Rc::new(self.locals[*i as usize].1.clone()),
                        *mutable,
                    ),
                ));
            }
            Op::ReadRef(ty) => {
                let (reference, _) = self.stack.pop().ok_or("reference stack underflow")?;
                let packed = self.read_reference(reference);
                let value = self.unpack(packed, ty);
                let value = if ty.can_copy() {
                    self.snapshot_inline(value, ty)
                } else {
                    value
                };
                self.retain(value, ty);
                self.stack.push((value, ty.clone()));
            }
            Op::Reborrow(ty) => {
                self.stack.last_mut().ok_or("reference stack underflow")?.1 = ty.clone();
            }
            Op::WriteRef(ty) => {
                let (reference, _) = self.stack.pop().ok_or("reference stack underflow")?;
                let (value, _) = self.pop_typed(ty.clone())?;
                let old = self.read_reference(reference);
                self.release(old, ty);
                self.write_reference(reference, value, ty);
            }
            Op::Collection(operation) => self.lower_collection(operation)?,
            Op::ClosureNew(t) => self.lower_closure_new(t)?,
            Op::ClosureCall(t) => self.lower_closure_call(t)?,
            Op::Class(operation) => self.lower_class(operation)?,
            Op::Enum(operation) => self.lower_enum(operation)?,
            Op::Split(t, tag) => self.lower_split(t, *tag)?,
            Op::Box(operation) => self.lower_box(operation)?,
            Op::Yield(ty) => self.lower_yield(ty)?,
            Op::Next(i) => self.lower_next(*i)?,
            Op::DropLocal(i) => self.drop_local(*i),
            Op::MoveLocal(i) => {
                let value = self.read_local(*i);
                let ty = self.locals[*i as usize].1.clone();
                let value = self.snapshot_inline(value, &ty);
                let zero = self.bcx.ins().iconst(types::I64, 0);
                let zero = self.unpack(zero, &ty);
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
            Op::FloatNeg => {
                let (value, ty) = self.stack.pop().ok_or("empty float negation")?;
                let value = self.bcx.ins().fneg(value);
                self.stack.push((value, ty));
            }
            Op::PushBool(b) => {
                let v = self.bcx.ins().iconst(types::I8, if *b { 1 } else { 0 });
                self.stack.push((v, Ty::Bool));
            }
            Op::PushStr(id) => self.lower_push_str(*id)?,
            Op::Add => self.lower_add()?,
            Op::Sub => self.lower_checked_arith(ArithKind::Sub)?,
            Op::Mul => self.lower_checked_arith(ArithKind::Mul)?,
            Op::Div => self.lower_div(false)?,
            Op::FloorDiv => self.lower_div(true)?,
            Op::Modulo => self.lower_modulo()?,
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
            Op::Display => self.lower_display()?,
            Op::Clear => self.release_stack(),
            Op::LoadLocal(i) => self.lower_load_local(*i)?,
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
                if self.generator.is_some() {
                    self.complete_generator();
                    return Ok(());
                }
                self.return_values(self.stack.clone());
                self.terminated = true;
            }
            // `DefineFn` is hoisted into a top-level Cranelift function by
            // Pass 1 + Pass 2; at the point this lowerer sees one, the
            // body is already being emitted elsewhere and the definition
            // itself has no runtime effect.
            Op::DefineFn(_, _) => {}
            Op::Match(arms) => self.lower_match(arms)?,
            Op::ReadLine => self.lower_readline()?,
            Op::Contains => self.lower_contains()?,
            Op::PrintLn => self.lower_println()?,
            Op::Print => self.lower_print()?,
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
        let eof_addr = self.bcx.ins().global_value(PTR_TY, eof_gv);
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
        let inst = self.bcx.ins().call(contains, &[hay.0, needle.0]);
        let v = self.bcx.inst_results(inst)[0];
        self.release(hay.0, &hay.1);
        self.release(needle.0, &needle.1);
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
                ArithKind::Add => self.bcx.ins().fadd(a, b),
                ArithKind::Sub => self.bcx.ins().fsub(a, b),
                ArithKind::Mul => self.bcx.ins().fmul(a, b),
            };
            self.stack.push((result, ty));
            return Ok(());
        }
        let signed = is_signed(ty.clone());
        let (result, of) = match (kind, signed) {
            (ArithKind::Add, true) => self.bcx.ins().sadd_overflow(a, b),
            (ArithKind::Add, false) => self.bcx.ins().uadd_overflow(a, b),
            (ArithKind::Sub, true) => self.bcx.ins().ssub_overflow(a, b),
            (ArithKind::Sub, false) => self.bcx.ins().usub_overflow(a, b),
            (ArithKind::Mul, true) => self.bcx.ins().smul_overflow(a, b),
            (ArithKind::Mul, false) => self.bcx.ins().umul_overflow(a, b),
        };
        self.trap_if(of, TrapKind::Overflow);
        self.stack.push((result, ty));
        Ok(())
    }

    /// Check divisor zero and signed INT_MIN/-1 before emitting division,
    /// so both failures produce runtime diagnostics instead of hardware traps.
    fn lower_div(&mut self, floor: bool) -> Result<()> {
        let (a, b, ty) = self.pop_pair()?;
        if ty.is_float() {
            let value = self.bcx.ins().fdiv(a, b);
            self.stack.push((value, ty));
            return Ok(());
        }
        let cty = clif_type(ty.clone());

        let zero = self.bcx.ins().iconst(cty, 0);
        let b_is_zero = self.bcx.ins().icmp(IntCC::Equal, b, zero);
        self.trap_if(b_is_zero, TrapKind::DivZero);

        if is_signed(ty.clone()) {
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
            self.trap_if(overflow, TrapKind::Overflow);
        }

        let mut v = if is_signed(ty.clone()) {
            self.bcx.ins().sdiv(a, b)
        } else {
            self.bcx.ins().udiv(a, b)
        };
        if floor && is_signed(ty.clone()) {
            let remainder = self.bcx.ins().srem(a, b);
            let nonzero = self.bcx.ins().icmp_imm(IntCC::NotEqual, remainder, 0);
            let signs = self.bcx.ins().bxor(a, b);
            let different = self.bcx.ins().icmp_imm(IntCC::SignedLessThan, signs, 0);
            let round_down = self.bcx.ins().band(nonzero, different);
            let below = self.bcx.ins().iadd_imm(v, -1);
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
        self.trap_if(is_zero, TrapKind::DivZero);
        let result = if is_signed(ty.clone()) {
            // INT_MIN % -1 is zero, so avoid a hardware division overflow.
            let minus_one = self.bcx.ins().icmp_imm(IntCC::Equal, b, -1);
            let one = self.bcx.ins().iconst(clif_type(ty.clone()), 1);
            let divisor = self.bcx.ins().select(minus_one, one, b);
            let rem = self.bcx.ins().srem(a, divisor);
            let nonzero = self.bcx.ins().icmp_imm(IntCC::NotEqual, rem, 0);
            let signs = self.bcx.ins().bxor(rem, b);
            let different = self.bcx.ins().icmp_imm(IntCC::SignedLessThan, signs, 0);
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
    /// ends with a CLIF `trap` to satisfy the verifier.
    ///
    /// Each call emits its own trap block rather than sharing one
    /// per function: Cranelift's FunctionBuilder forbids switching
    /// away from an unterminated block, so a shared lazily-filled
    /// trap block would require either eager construction at entry
    /// or a post-pass. Inlining is straightforward and the IR cost
    /// is a handful of instructions per arithmetic op.
    fn trap_if(&mut self, flag: cranelift_codegen::ir::Value, kind: TrapKind) {
        let trap_block = self.bcx.create_block();
        let after = self.bcx.create_block();
        self.bcx.ins().brif(flag, trap_block, &[], after, &[]);

        // Fill the trap block. The `brif` above terminated the
        // previous block, so this switch is legal.
        self.bcx.switch_to_block(trap_block);
        self.bcx.seal_block(trap_block);
        let helper = match kind {
            TrapKind::Overflow => self.runtime.trap_overflow,
            TrapKind::DivZero => self.runtime.trap_div_zero,
        };
        let local = self.module.declare_func_in_func(helper, self.bcx.func);
        self.bcx.ins().call(local, &[]);
        self.bcx.ins().trap(TrapCode::unwrap_user(3));

        // Continue lowering into `after`.
        self.bcx.switch_to_block(after);
        self.bcx.seal_block(after);
    }

    /// Lower numeric ordering with IEEE float or signedness-aware integer comparisons.
    fn int_cmp(&mut self, signed: IntCC, unsigned: IntCC) -> Result<()> {
        let (a, b, ty) = self.pop_pair()?;
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

    /// Emit the calls that print the current compile-time stack — the
    /// Render the legacy operand stack. The print
    /// helpers all have fixed signatures, so we can resolve each
    /// `FuncId` to a local `FuncRef` once at the top and reuse it.
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

    /// Lower `Op::PushStr`: emit `global_value` for the data symbol
    /// that holds this literal's bytes, push the address (typed as
    /// `Ty::Str`) onto the compile-time stack.
    fn lower_push_str(&mut self, id: StrId) -> Result<()> {
        let data_id = *self.str_data.get(&id).ok_or_else(|| -> Box<dyn Error> {
            // `declare_str_data` is supposed to register every StrId
            // reachable through ops; missing here means the collection
            // walk missed an op variant.
            format!("AOT: PushStr({id:?}) without a declared data symbol").into()
        })?;
        let gv = self.module.declare_data_in_func(data_id, self.bcx.func);
        let addr = self.bcx.ins().global_value(PTR_TY, gv);
        self.stack.push((addr, Ty::Str));
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
            .is_some_and(|(_, ty)| ty.uses_value_runtime())
        {
            let (a, b, ty) = self.pop_pair()?;
            let eq = self.collection_call(8, &[a, b], Some(&ty))?;
            self.release(a, &ty);
            self.release(b, &ty);
            let mut value = self.bcx.ins().ireduce(types::I8, eq);
            if negate_string_result {
                value = self.bcx.ins().bxor_imm(value, 1);
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
            self.release(a, &Ty::Str);
            self.release(b, &Ty::Str);
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
        self.retain(v, &ty);
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
        let (decl, mut args) = self.pop_call_args(name)?;
        let outputs = decl.sig.outputs.clone();
        let func_id = decl.id;
        let bytes = outputs.iter().map(Ty::inline_bytes).sum();
        if bytes != 0 {
            args.push(self.inline_storage(bytes));
        }
        let funcref = self.module.declare_func_in_func(func_id, self.bcx.func);
        let inst = self.bcx.ins().call(funcref, &args);
        let results: Vec<cranelift_codegen::ir::Value> = self.bcx.inst_results(inst).to_vec();
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
        let bytes = sig.outputs.iter().map(Ty::inline_bytes).sum();
        if bytes != 0 {
            args.push(self.inline_storage(bytes));
        }
        let native = user_fn_signature(self.module, &sig);
        let reference = self.bcx.import_signature(native);
        if tail
            && !signature
                .inputs
                .iter()
                .chain(signature.output.iter())
                .any(|ty| {
                    ty.has_inline_storage()
                        || matches!(ty, Ty::Ref(inner, _) if inner.has_inline_storage())
                })
        {
            self.release_locals();
            self.bcx
                .ins()
                .return_call_indirect(reference, callee, &args);
            self.terminated = true;
            return Ok(());
        }
        let call = self.bcx.ins().call_indirect(reference, callee, &args);
        for (value, ty) in self.bcx.inst_results(call).iter().copied().zip(sig.outputs) {
            self.stack.push((value, ty));
        }
        if tail {
            self.return_values(self.stack.clone());
            self.terminated = true;
        }
        Ok(())
    }

    /// Lower `Op::TailCall`: emit `return_call`, which transfers control
    /// to the callee without growing the call stack — the iteration
    /// primitive for Plenty's recursive control flow (§11.8). The
    /// instruction is a block terminator, so we set `self.terminated`
    /// and the outer loop stops feeding ops to this lowerer.
    fn lower_tail_call(&mut self, name: &str) -> Result<()> {
        let signature = &self.user_fns[name].sig;
        if signature.inputs.iter().any(|(_, t)| {
            t.has_inline_storage() || matches!(t, Ty::Ref(inner, _) if inner.has_inline_storage())
        }) || signature.outputs.iter().any(Ty::has_inline_storage)
        {
            self.lower_call(name)?;
            self.return_values(self.stack.clone());
            self.terminated = true;
            return Ok(());
        }
        let (decl, args) = self.pop_call_args(name)?;
        let func_id = decl.id;
        let funcref = self.module.declare_func_in_func(func_id, self.bcx.func);
        self.release_locals();
        self.bcx.ins().return_call(funcref, &args);
        self.terminated = true;
        Ok(())
    }

    /// Lower `Op::Match`: one CLIF block per arm, a linear `brif` chain
    /// for dispatch, and a single join block whose params carry the
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

        // One block per arm body; arms are sealed once the dispatch
        // chain finishes emitting (each arm has exactly one predecessor,
        // the dispatch block that jumped to it).
        let arm_blocks: Vec<Block> = arms.iter().map(|_| self.bcx.create_block()).collect();
        let join_block = self.bcx.create_block();

        // --- Dispatch chain --------------------------------------------------
        // We're currently in whatever block called `lower_match`. Each
        // non-wildcard pattern emits an `icmp eq` + `brif`; the false
        // branch falls into a fresh block we switch to for the next
        // compare. A wildcard short-circuits with an unconditional jump
        // and renders any trailing arms unreachable (the checker would
        // already have noticed if a useful arm came after `_`).
        let mut chain_terminated = false;
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
                    let pat = self.bcx.ins().iconst(types::I8, i64::from(b as i8));
                    let eq = self.bcx.ins().icmp(IntCC::Equal, scrut, pat);
                    let next = self.bcx.create_block();
                    self.bcx.ins().brif(eq, arm_blocks[i], &[], next, &[]);
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
                    let pat_addr = self.bcx.ins().global_value(PTR_TY, gv);
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
