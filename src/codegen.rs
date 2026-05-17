//! AOT code generation via Cranelift (§11.1, §12.3 — phases c.1–c.4).
//!
//! Lowers a Plenty `Op` stream to a Cranelift module emitted as a native
//! object file. The object exports one symbol, `plenty_main`, which the
//! C runtime in `runtime/plenty_runtime.c` calls from its `main`. User
//! function definitions become locally-linked symbols inside the same
//! object, callable from each other and from `plenty_main`.
//!
//! The lowering threads a *compile-time stack* of `(cranelift::Value, Ty)`
//! pairs through the op stream. Each Plenty op becomes a small CLIF
//! sequence that pops its inputs from this stack and pushes its result:
//! the runtime data stack the interpreter manages is virtualised into
//! SSA values, so the compiled code does no in-memory pushing or popping.
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
//! bytes plus a trailing nul. `Op::PushStr` lowers to `global_value` —
//! the data's address — and onto the compile-time stack tagged as
//! `Ty::Str` (CLIF `i64` for the host pointer width). `Op::Add` and
//! `Op::Eq` now dispatch on operand types: integer pairs take the
//! existing CLIF paths; `Str Str` calls `plenty_concat` / `plenty_str_eq`
//! in the C runtime. `Display` prints strings via `plenty_print_str`,
//! and `match` patterns of type `Str` become `plenty_str_eq` + `brif`.
//!
//! Phase c.5 packages the runtime. The contents of
//! `runtime/plenty_runtime.c` are embedded into the `plenty` binary at
//! build time via `include_bytes!`; [`compile_source_to_executable`]
//! writes the object and the runtime to a tempdir, invokes `cc` to link
//! them, and deletes the temps so the user's `-o OUT` is the only
//! artifact. Every Plenty op lowers, and the user no longer needs to
//! run `cc` by hand.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::rc::Rc;

use cranelift_codegen::ir::condcodes::IntCC;
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
use crate::op::{self, FnSig, MatchArm, Op, Pattern, Ty};
use crate::value::{Heap, StrId};

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

/// Read `source` and produce a native executable at `output` in one
/// step (DESIGN.md §11.1, §12.3 — phase c.5). The source is lexed,
/// compiled, and checked through the same pipeline the VM uses; the
/// resulting op stream is lowered to a temp object file; the embedded
/// C runtime is written alongside it; `cc` links the pair into the
/// final executable and the temps are removed.
///
/// `cc` is invoked by name from `PATH` (no override). When `cc` is
/// missing, the error message identifies the link step as the failure
/// site so users can install a C toolchain or wrap an alternative
/// compiler as `cc`.
pub fn compile_source_to_executable(source: &str, output: &Path) -> Result<()> {
    let toks = lexer::lex(source)?;
    let mut heap = Heap::default();
    let ops = op::compile(&toks, &mut heap)?;
    op::check(&ops, Vec::new(), &HashMap::new())?;

    // Tempfile names blend the process id and a nanosecond timestamp:
    // unique across concurrent `plenty --compile` invocations without
    // pulling in a tempfile crate. Both temps live in the same dir as
    // the system tempdir to inherit OS-level cleanup as a fallback.
    let tmp = std::env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let obj_path = tmp.join(format!("plenty-{pid}-{nonce}.o"));
    let rt_path = tmp.join(format!("plenty-{pid}-{nonce}-runtime.c"));

    let result = (|| -> Result<()> {
        compile_to_object(&ops, &heap, &obj_path)?;
        std::fs::write(&rt_path, RUNTIME_C)?;
        link_with_cc(&obj_path, &rt_path, output)
    })();

    // Best-effort cleanup; never override a primary error with a
    // missing-file error from `remove_file`.
    let _ = std::fs::remove_file(&obj_path);
    let _ = std::fs::remove_file(&rt_path);
    result
}

/// Plenty's C runtime, embedded at build time. Writing this to a tempfile
/// at link time lets `cc` do the runtime's compile-and-link in a single
/// invocation — the same path the test harness took before c.5, just
/// driven by the binary now.
const RUNTIME_C: &[u8] = include_bytes!("../runtime/plenty_runtime.c");

/// Invoke `cc` to link `obj` (the Cranelift-emitted object) with the
/// runtime source `runtime_src` into the executable at `output`. The
/// runtime is passed as a `.c` file rather than a precompiled archive
/// so the build pipeline stays one-step (no `build.rs`); the runtime
/// is small enough that the per-compile recompilation cost is invisible.
fn link_with_cc(obj: &Path, runtime_src: &Path, output: &Path) -> Result<()> {
    let out = std::process::Command::new("cc")
        .arg(obj)
        .arg(runtime_src)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|e| -> Box<dyn Error> {
            format!(
                "failed to invoke `cc` for the link step: {e}. \
                 Plenty's AOT mode shells out to a C compiler named `cc` \
                 on PATH to link the emitted object with the embedded runtime."
            )
            .into()
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("cc failed:\n{stderr}").into());
    }
    Ok(())
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Lower `ops` to a native object file at `output`.
///
/// The object exports `plenty_main` (`() -> i32`) and one locally-linked
/// symbol per user-defined Plenty function, plus one read-only data
/// symbol per source-level string literal whose bytes come from `heap`.
/// Link the object with `runtime/plenty_runtime.c` to produce an
/// executable; the exit status of the final binary is 0 when the
/// program runs to the end of `ops`.
fn compile_to_object(ops: &[Op], heap: &Heap, output: &Path) -> Result<()> {
    let isa = host_isa()?;
    let builder = ObjectBuilder::new(
        isa,
        "plenty",
        cranelift_module::default_libcall_names(),
    )?;
    let mut module = ObjectModule::new(builder);

    let runtime = declare_runtime(&mut module)?;

    // Pass 1: collect every user-defined function reachable from `ops`
    // (top-level, nested under another definition, or inside a match
    // arm), declare each as a Cranelift symbol with the tail-call
    // convention so its body can `return_call` other user functions.
    let mut user_fns: HashMap<String, UserFn> = HashMap::new();
    collect_user_fns(ops, &mut module, &mut user_fns)?;
    // AOT mode is closed-world: every `Call`/`TailCall` in `ops` must
    // resolve to a definition collected above (§11.1).
    check_calls_resolve(ops, &user_fns)?;

    // Pass 1b: emit one read-only data symbol per source string literal.
    // We walk the ops (recursing into bodies and match arms) collecting
    // every `StrId` referenced by `PushStr` or `Pattern::Str`, then
    // declare and define each one. The interpreter's `Heap` is the
    // source of truth for the literal bytes.
    let str_data = declare_str_data(ops, heap, &mut module)?;

    // One extra read-only data symbol holding a single `\0` byte — the
    // empty-string placeholder `Op::ReadLine` substitutes for `NULL`
    // on EOF so the value pushed as `Ty::Str` is always a valid C
    // string. Always declared (one byte of `.rodata`, negligible)
    // rather than conditionally so the Lowerer never has to track
    // whether the module uses `:readline`.
    let eof_empty_str = declare_eof_empty_str(&mut module)?;

    // Pass 2: emit each user function's body. Bodies can refer to each
    // other (forward references, mutual recursion) because every callee
    // is already declared.
    let names: Vec<String> = user_fns.keys().cloned().collect();
    for name in &names {
        emit_user_function(name, &user_fns, &str_data, eof_empty_str, &runtime, &mut module)?;
    }

    // Pass 3: emit `plenty_main`. Top-level `DefineFn`s are skipped
    // here — their bodies were emitted by Pass 2; at runtime a
    // definition is a no-op (it does not touch the data stack).
    emit_main(ops, &user_fns, &str_data, eof_empty_str, &runtime, &mut module)?;

    let product = module.finish();
    let bytes = product.emit()?;
    std::fs::write(output, bytes)?;
    Ok(())
}

/// Build an `ISA` for the host target. Cranelift's `native` crate
/// inspects the running CPU's features so emitted code can take
/// advantage of what's available without us having to enumerate it.
fn host_isa() -> Result<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    let mut flags = settings::builder();
    // `is_pic` so the object can be linked into a position-independent
    // executable, which is what every modern Linux/macOS toolchain
    // produces by default.
    flags.set("is_pic", "true")?;
    // Frame pointers must be preserved for `return_call` emission on
    // x86_64: the backend hooks the tail-call stack-arg fixup off the
    // frame-pointer prologue/epilogue. Without this, lowering any
    // Plenty TailCall panics inside Cranelift with "the current
    // implementation relies on [frame pointers] being present".
    flags.set("preserve_frame_pointers", "true")?;
    let isa_builder = cranelift_native::builder().map_err(|e| -> Box<dyn Error> { e.into() })?;
    Ok(isa_builder.finish(settings::Flags::new(flags))?)
}

/// Handles for every runtime helper the lowerer can call. We declare
/// them all up-front so each call site is just `module.declare_func_in_func`
/// plus an `ins().call`.
struct Runtime {
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
    /// line from stdin, strip the trailing newline, return a malloc'd
    /// nul-terminated buffer. Returns NULL on EOF. Owned (never freed)
    /// to match the interpreter's append-only `Heap` (§12.1).
    readline: FuncId,
    /// `plenty_contains(*const u8 haystack, *const u8 needle) -> i8` —
    /// returns 1 if `needle` is a byte-substring of `haystack`, 0
    /// otherwise. Wraps `strstr`.
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

/// The CLIF type used for every Plenty `Str` value. Strings are passed
/// around as nul-terminated C-style pointers (see `runtime/plenty_runtime.c`),
/// and AOT mode only targets the host architecture today — every host
/// we care about is 64-bit, so the pointer width is `types::I64`. If we
/// ever cross-compile to a 32-bit target, this needs to come from
/// `module.target_config().pointer_type()` instead.
const PTR_TY: types::Type = types::I64;

/// Walk `ops` recursively and collect every `StrId` referenced by a
/// `PushStr` or `Pattern::Str`. For each unique `StrId`, declare a
/// read-only data symbol in `module` whose contents are the literal's
/// UTF-8 bytes plus a trailing nul (so C string helpers can scan with
/// `strlen` / `strcmp`).
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
        let mut bytes: Vec<u8> = Vec::with_capacity(s.len() + 1);
        bytes.extend_from_slice(s.as_bytes());
        bytes.push(0);
        let mut desc = DataDescription::new();
        desc.define(bytes.into_boxed_slice());
        module.define_data(data_id, &desc)?;
        out.insert(id, data_id);
    }
    Ok(out)
}

/// One read-only data symbol holding a single nul byte — i.e. the C
/// representation of `""`. [`Lowerer::lower_readline`] substitutes its
/// address for the `NULL` returned by `plenty_readline` on EOF, so the
/// value pushed onto the compile-time stack as `Ty::Str` is always a
/// valid C string. Always declared (one byte of `.rodata`) so the
/// Lowerer doesn't need to know whether the module uses `:readline`.
fn declare_eof_empty_str(module: &mut ObjectModule) -> Result<DataId> {
    let id = module.declare_data("plenty_readline_eof_empty", Linkage::Local, false, false)?;
    let mut desc = DataDescription::new();
    desc.define(vec![0u8].into_boxed_slice());
    module.define_data(id, &desc)?;
    Ok(id)
}

/// Recursive helper for [`declare_str_data`]: emits `StrId`s in
/// first-seen order, skipping duplicates so the same literal appearing
/// in multiple places shares one data symbol.
fn collect_str_ids(
    ops: &[Op],
    out: &mut Vec<StrId>,
    seen: &mut HashMap<StrId, ()>,
) {
    for op in ops {
        match op {
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
    id: FuncId,
    sig: Rc<FnSig>,
    body: Rc<[Op]>,
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
        cl.params.push(AbiParam::new(clif_type(*ty)));
    }
    for ty in &sig.outputs {
        cl.returns.push(AbiParam::new(clif_type(*ty)));
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
                    return Err(format!(
                        "AOT compilation does not allow redefining `{name}` \
                         (the REPL allows it; compiled programs do not)"
                    )
                    .into());
                }
                let cl_sig = user_fn_signature(module, &f.sig);
                let id = module.declare_function(name, Linkage::Local, &cl_sig)?;
                out.insert(
                    name.clone(),
                    UserFn { id, sig: Rc::clone(&f.sig), body: Rc::clone(&f.body) },
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

/// Reject any `Call`/`TailCall` in `ops` whose target was not collected
/// by [`collect_user_fns`]. The type checker already catches this for
/// most programs; the AOT-specific check exists because the checker
/// also accepts calls into the VM's pre-existing dictionary, which is
/// not available in compiled code (§11.1, closed-world).
fn check_calls_resolve(ops: &[Op], fns: &HashMap<String, UserFn>) -> Result<()> {
    for op in ops {
        match op {
            Op::Call(name) | Op::TailCall(name) if !fns.contains_key(name) => {
                return Err(format!(
                    "AOT compilation cannot resolve call to `{name}` \
                     (compiled programs are closed-world; every called \
                     function must be defined in the same source)"
                )
                .into());
            }
            Op::DefineFn(_, f) => check_calls_resolve(&f.body, fns)?,
            Op::Match(arms) => {
                for arm in arms.iter() {
                    check_calls_resolve(&arm.body, fns)?;
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
fn emit_user_function(
    name: &str,
    fns: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    let decl = &fns[name];
    let cl_sig = user_fn_signature(module, &decl.sig);

    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(
        UserFuncName::user(0, decl.id.as_u32()),
        cl_sig,
    );
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
            let var = bcx.declare_var(clif_type(*ty));
            let param = bcx.block_params(entry)[i];
            bcx.def_var(var, param);
            locals.push((var, *ty));
        }

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
        };
        for op in decl.body.iter() {
            if lower.terminated {
                // A `TailCall` already terminated this block; any
                // trailing op is dead. `op::compile` only emits
                // `TailCall` at the very end of a body (or a match-arm
                // tail), so this branch is defensive.
                break;
            }
            lower.lower(op).map_err(|e| -> Box<dyn Error> {
                format!("in `{name}`: {e}").into()
            })?;
        }
        if !lower.terminated {
            let returns: Vec<cranelift_codegen::ir::Value> =
                lower.stack.iter().map(|(v, _)| *v).collect();
            lower.bcx.ins().return_(&returns);
        }
        bcx.finalize();
    }
    module.define_function(decl.id, &mut ctx)?;
    Ok(())
}

/// Emit `plenty_main` — the entry point the C runtime forwards to.
/// Top-level `DefineFn` ops are skipped (their bodies are emitted
/// separately by [`emit_user_function`]); everything else lowers
/// against an initially-empty compile-time stack, with no locals
/// in scope.
fn emit_main(
    ops: &[Op],
    fns: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    // `plenty_main`: exported, no arguments, returns `i32`. The C
    // runtime's `int main(int, char**)` forwards into this and returns
    // its result as the process exit code. SystemV convention because
    // the caller (the C runtime) speaks the host's C ABI; user
    // functions use `CallConv::Tail` and can still be invoked from here
    // via a regular `call`.
    let mut main_sig = module.make_signature();
    main_sig.returns.push(AbiParam::new(types::I32));
    let main_id = module.declare_function("plenty_main", Linkage::Export, &main_sig)?;

    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(
        UserFuncName::user(0, main_id.as_u32()),
        main_sig,
    );
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
        };
        for op in ops {
            lower.lower(op)?;
        }
        // `plenty_main` never tail-calls (its convention doesn't
        // support it), so `lower.terminated` is always false here.
        let zero = lower.bcx.ins().iconst(types::I32, 0);
        lower.bcx.ins().return_(&[zero]);
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
/// a nul-terminated byte sequence in either the module's data section
/// (literals) or the runtime heap (results of `plenty_concat`).
fn clif_type(ty: Ty) -> types::Type {
    match ty {
        Ty::I8 | Ty::U8 | Ty::Bool => types::I8,
        Ty::I16 | Ty::U16 => types::I16,
        Ty::I32 | Ty::U32 => types::I32,
        Ty::I64 | Ty::U64 => types::I64,
        Ty::Str => PTR_TY,
    }
}

/// Width of an integer type in bits. Used to drive cast lowering.
fn width_bits(ty: Ty) -> u8 {
    match ty {
        Ty::I8 | Ty::U8 => 8,
        Ty::I16 | Ty::U16 => 16,
        Ty::I32 | Ty::U32 => 32,
        Ty::I64 | Ty::U64 => 64,
        Ty::Bool | Ty::Str => panic!("non-integer in width_bits"),
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
/// possible interpreter error messages.
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
    /// Read-only data symbol holding a single `\0` byte — i.e. `""`.
    /// `Op::ReadLine` substitutes its address for `NULL` on EOF so
    /// the value pushed onto the compile-time stack as `Ty::Str` is
    /// always a valid C string. Declared once per module by
    /// [`declare_eof_empty_str`].
    eof_empty_str: DataId,
    /// The active function's input variables, indexed by the local
    /// slot `Op::LoadLocal` was emitted with. Empty when lowering
    /// `plenty_main` (top-level has no locals).
    locals: &'a [(Variable, Ty)],
    stack: Vec<StackEntry>,
    /// Set after a `TailCall` lowers to `return_call`, which is a
    /// block terminator. Once set, the outer loop in
    /// [`emit_user_function`] stops feeding ops to this lowerer.
    terminated: bool,
}

impl Lowerer<'_, '_> {
    fn lower(&mut self, op: &Op) -> Result<()> {
        match op {
            Op::PushInt(n) => {
                let v = self.bcx.ins().iconst(types::I64, *n);
                self.stack.push((v, Ty::I64));
            }
            Op::PushBool(b) => {
                let v = self.bcx.ins().iconst(types::I8, if *b { 1 } else { 0 });
                self.stack.push((v, Ty::Bool));
            }
            Op::PushStr(id) => self.lower_push_str(*id)?,
            Op::Add => self.lower_add()?,
            Op::Sub => self.lower_checked_arith(ArithKind::Sub)?,
            Op::Mul => self.lower_checked_arith(ArithKind::Mul)?,
            Op::Div => self.lower_div()?,
            Op::Eq => self.lower_eq()?,
            Op::Lt => self.int_cmp(IntCC::SignedLessThan, IntCC::UnsignedLessThan)?,
            Op::Gt => self.int_cmp(IntCC::SignedGreaterThan, IntCC::UnsignedGreaterThan)?,
            Op::Not => {
                let (v, ty) = self.pop_typed(Ty::Bool)?;
                let one = self.bcx.ins().iconst(types::I8, 1);
                let neg = self.bcx.ins().bxor(v, one);
                self.stack.push((neg, ty));
            }
            Op::Cast(target) => {
                let (v, src) = self.stack.pop().ok_or("AOT: stack underflow on cast")?;
                let cast = self.cast(v, src, *target);
                self.stack.push((cast, *target));
            }
            Op::Display => self.lower_display()?,
            Op::Clear => self.stack.clear(),
            Op::LoadLocal(i) => self.lower_load_local(*i)?,
            Op::Call(name) => self.lower_call(name)?,
            Op::TailCall(name) => self.lower_tail_call(name)?,
            // `DefineFn` is hoisted into a top-level Cranelift function by
            // Pass 1 + Pass 2; at the point this lowerer sees one, the
            // body is already being emitted elsewhere and the definition
            // itself has no runtime effect.
            Op::DefineFn(_, _) => {}
            Op::Match(arms) => self.lower_match(arms)?,
            Op::ReadLine => self.lower_readline()?,
            Op::Contains => self.lower_contains()?,
            Op::PrintLn => self.lower_println()?,
        }
        Ok(())
    }

    /// Lower `Op::ReadLine`: call `plenty_readline`, which returns a
    /// malloc'd nul-terminated buffer or `NULL` on EOF. We turn `NULL`
    /// into the address of `plenty_readline_eof_empty` (the `""` data
    /// symbol) so the `Ty::Str` we push is always dereferenceable; the
    /// "got a line?" Bool is `ptr != 0`. The user discriminates via
    /// `match` on the Bool — see DESIGN.md §11.8 for the surface.
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
        // The EOF empty-string fallback: a 1-byte `\0` data symbol
        // emitted unconditionally per module. Substituting it for
        // `NULL` keeps the pushed `Ty::Str` always pointing at a
        // valid C string.
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
    /// `plenty_contains` (a thin wrapper over `strstr`), push the
    /// returned `i8` as Plenty `Bool`.
    fn lower_contains(&mut self) -> Result<()> {
        let needle = self.stack.pop().ok_or("AOT: stack underflow on :contains")?;
        let hay = self.stack.pop().ok_or("AOT: stack underflow on :contains")?;
        debug_assert_eq!(hay.1, Ty::Str);
        debug_assert_eq!(needle.1, Ty::Str);
        let contains = self
            .module
            .declare_func_in_func(self.runtime.contains, self.bcx.func);
        let inst = self.bcx.ins().call(contains, &[hay.0, needle.0]);
        let v = self.bcx.inst_results(inst)[0];
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
        Ok(())
    }

    /// Lower a signed-or-unsigned checked arithmetic op (add/sub/mul).
    /// Emits the matching Cranelift `*_overflow` instruction, branches
    /// on the overflow flag to the shared overflow-trap block, and
    /// switches to a fresh successor block with the result on the
    /// compile-time stack. The interpreter calls `checked_*` and
    /// errors with `"integer overflow"` on `None`; we match by exit
    /// status (1) and stderr line (`"error: integer overflow"`) via
    /// the runtime helper `plenty_trap_overflow`.
    fn lower_checked_arith(&mut self, kind: ArithKind) -> Result<()> {
        let (a, b, ty) = self.pop_int_pair()?;
        let signed = is_signed(ty);
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

    /// Lower `Op::Div`: explicit divisor-zero check (interpreter
    /// distinguishes `"division by zero"` from `"integer overflow"`),
    /// then for signed types an explicit INT_MIN/-1 check (the only
    /// non-zero divisor for which `sdiv` traps inside Cranelift —
    /// catching it ourselves lets us emit the same `"integer overflow"`
    /// message the interpreter does), then the bare `sdiv`/`udiv`.
    fn lower_div(&mut self) -> Result<()> {
        let (a, b, ty) = self.pop_int_pair()?;
        let cty = clif_type(ty);

        let zero = self.bcx.ins().iconst(cty, 0);
        let b_is_zero = self.bcx.ins().icmp(IntCC::Equal, b, zero);
        self.trap_if(b_is_zero, TrapKind::DivZero);

        if is_signed(ty) {
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

        let v = if is_signed(ty) {
            self.bcx.ins().sdiv(a, b)
        } else {
            self.bcx.ins().udiv(a, b)
        };
        self.stack.push((v, ty));
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

    /// Lower `<` / `>` with a signedness-aware `icmp` condition code.
    fn int_cmp(&mut self, signed: IntCC, unsigned: IntCC) -> Result<()> {
        let (a, b, ty) = self.pop_int_pair()?;
        let cc = if is_signed(ty) { signed } else { unsigned };
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
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value, Ty)> {
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
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value, Ty)> {
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
        let from_bits = width_bits(from);
        let to_bits = width_bits(to);
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
    /// AOT analogue of `Vm::stack_repr` plus `println!`. The print
    /// helpers all have fixed signatures, so we can resolve each
    /// `FuncId` to a local `FuncRef` once at the top and reuse it.
    fn lower_display(&mut self) -> Result<()> {
        // Snapshot the stack so we don't iterate-and-mutate; printing
        // leaves the stack untouched (`.` doesn't pop in Plenty).
        let entries: Vec<StackEntry> = self.stack.clone();
        let open = self.module.declare_func_in_func(self.runtime.print_open_bracket, self.bcx.func);
        let close = self.module.declare_func_in_func(self.runtime.print_close_bracket, self.bcx.func);
        let space = self.module.declare_func_in_func(self.runtime.print_space, self.bcx.func);
        self.bcx.ins().call(open, &[]);
        for (i, (v, ty)) in entries.iter().enumerate() {
            if i > 0 {
                self.bcx.ins().call(space, &[]);
            }
            let printer = self.printer_for(*ty);
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
        let gv = self
            .module
            .declare_data_in_func(data_id, self.bcx.func);
        let addr = self.bcx.ins().global_value(PTR_TY, gv);
        self.stack.push((addr, Ty::Str));
        Ok(())
    }

    /// Lower `Op::Add`: integers go through the checked-overflow
    /// arithmetic path; the `Str Str` case calls into the runtime's
    /// `plenty_concat`, which allocates a fresh nul-terminated buffer
    /// and returns its address. The polymorphic `+` is the only op
    /// that mixes these two backends — every other arithmetic op
    /// stays integer-only (`check::arith` rejects `Str Str` for `-`,
    /// `*`, `/`).
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
            self.stack.push((v, Ty::Str));
            return Ok(());
        }
        self.lower_checked_arith(ArithKind::Add)
    }

    /// Lower `Op::Eq`: same dispatch shape as [`lower_add`]. The
    /// integer/Bool case uses `icmp eq` (already produces an `i8`);
    /// the `Str Str` case calls `plenty_str_eq`, which returns an `i8`
    /// 0/1 directly suitable as a Plenty Bool.
    fn lower_eq(&mut self) -> Result<()> {
        let len = self.stack.len();
        if len >= 2 && self.stack[len - 1].1 == Ty::Str && self.stack[len - 2].1 == Ty::Str {
            let b = self.stack.pop().expect("len >= 2").0;
            let a = self.stack.pop().expect("len >= 2").0;
            let str_eq = self
                .module
                .declare_func_in_func(self.runtime.str_eq, self.bcx.func);
            let inst = self.bcx.ins().call(str_eq, &[a, b]);
            let v = self.bcx.inst_results(inst)[0];
            self.stack.push((v, Ty::Bool));
            return Ok(());
        }
        let (a, b, _) = self.pop_pair()?;
        let v = self.bcx.ins().icmp(IntCC::Equal, a, b);
        self.stack.push((v, Ty::Bool));
        Ok(())
    }

    /// Lower `LoadLocal(i)`: read the i-th input variable and push the
    /// resulting SSA value onto the compile-time stack. The compiler
    /// only emits `LoadLocal` inside a function body, so `self.locals`
    /// is always populated when we get here.
    fn lower_load_local(&mut self, i: u8) -> Result<()> {
        let (var, ty) = self
            .locals
            .get(i as usize)
            .copied()
            .ok_or_else(|| -> Box<dyn Error> {
                format!("AOT: LoadLocal({i}) has no matching input").into()
            })?;
        let v = self.bcx.use_var(var);
        self.stack.push((v, ty));
        Ok(())
    }

    /// Pop the inputs for a call to `name` from the compile-time stack,
    /// returning them in call order (deepest = position 0) along with
    /// the callee's declaration.
    fn pop_call_args(&mut self, name: &str) -> Result<(&UserFn, Vec<cranelift_codegen::ir::Value>)> {
        let decl = self
            .user_fns
            .get(name)
            .ok_or_else(|| -> Box<dyn Error> {
                // Should have been caught by `check_calls_resolve`; this
                // is the defensive arm for direct-construction paths.
                format!("AOT: undefined function `{name}`").into()
            })?;
        let n = decl.sig.inputs.len();
        if self.stack.len() < n {
            return Err(format!("AOT: stack underflow calling `{name}`").into());
        }
        // Drain in stack order: the deepest popped value is `inputs[0]`,
        // matching `Vm::do_call`'s drain orientation.
        let split = self.stack.len() - n;
        let args: Vec<_> = self.stack.drain(split..).map(|(v, _)| v).collect();
        Ok((decl, args))
    }

    /// Lower `Op::Call`: emit a regular call and push each return value
    /// onto the compile-time stack with its declared `Ty`.
    fn lower_call(&mut self, name: &str) -> Result<()> {
        let (decl, args) = self.pop_call_args(name)?;
        let outputs = decl.sig.outputs.clone();
        let func_id = decl.id;
        let funcref = self.module.declare_func_in_func(func_id, self.bcx.func);
        let inst = self.bcx.ins().call(funcref, &args);
        let results: Vec<cranelift_codegen::ir::Value> =
            self.bcx.inst_results(inst).to_vec();
        debug_assert_eq!(results.len(), outputs.len());
        for (v, ty) in results.into_iter().zip(outputs) {
            self.stack.push((v, ty));
        }
        Ok(())
    }

    /// Lower `Op::TailCall`: emit `return_call`, which transfers control
    /// to the callee without growing the call stack — the iteration
    /// primitive for Plenty's recursive control flow (§11.8). The
    /// instruction is a block terminator, so we set `self.terminated`
    /// and the outer loop stops feeding ops to this lowerer.
    fn lower_tail_call(&mut self, name: &str) -> Result<()> {
        let (decl, args) = self.pop_call_args(name)?;
        let func_id = decl.id;
        let funcref = self.module.declare_func_in_func(func_id, self.bcx.func);
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
    /// An arm whose tail op is a `TailCall` does *not* jump to the
    /// join block: `return_call` is itself a block terminator and the
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
        let arm_blocks: Vec<Block> =
            arms.iter().map(|_| self.bcx.create_block()).collect();
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
                Pattern::Int(n) => {
                    // Pattern literals are parsed as i64; `iconst` of a
                    // smaller CLIF type truncates the upper bits — same
                    // narrowing the interpreter's `pattern_matches` does
                    // via `n as i8` / `as u8` / etc. The checker has
                    // already rejected literals outside the scrutinee's
                    // declared range, so truncation never changes the
                    // user-intended value.
                    let pat = self.bcx.ins().iconst(clif_type(scrut_ty), n);
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
                    let data_id =
                        *self.str_data.get(&id).ok_or_else(|| -> Box<dyn Error> {
                            format!("AOT: Pattern::Str({id:?}) without declared data").into()
                        })?;
                    let gv = self.module.declare_data_in_func(data_id, self.bcx.func);
                    let pat_addr = self.bcx.ins().global_value(PTR_TY, gv);
                    let str_eq =
                        self.module.declare_func_in_func(self.runtime.str_eq, self.bcx.func);
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
            // direct-VM-construction bugs surface loudly instead of
            // walking off the end of the function.
            self.bcx
                .ins()
                .trap(TrapCode::unwrap_user(1));
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
                let types: Vec<Ty> = self.stack.iter().map(|(_, t)| *t).collect();
                for ty in &types {
                    self.bcx
                        .append_block_param(join_block, clif_type(*ty));
                }
                join_param_types = Some(types);
            }
            // `jump` takes `&[BlockArg]`; every Plenty stack value is
            // an SSA `Value`, which converts via `BlockArg::Value(_)`.
            let args: Vec<BlockArg> =
                self.stack.iter().map(|(v, _)| BlockArg::Value(*v)).collect();
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
            // Every arm tail-called; the join is unreachable. Emit a
            // trap to give the block a terminator and signal upward
            // that the surrounding context is also dead.
            self.bcx
                .ins()
                .trap(TrapCode::unwrap_user(2));
            self.terminated = true;
        }
        Ok(())
    }
}

