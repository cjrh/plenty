//! AOT code generation via Cranelift (§11.1, §12.3 — phases c.1, c.2).
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
//! What this still does *not* lower — `Op::Match`, anything that touches
//! a `Str`, `:listdir` — is rejected with a clear "not yet implemented in
//! AOT" error that names the next phase. Those programs still run under
//! the tree-walking VM.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::rc::Rc;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{types, AbiParam, Function, InstBuilder, Signature, UserFuncName};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::Configurable;
use cranelift_codegen::{settings, Context};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::lexer;
use crate::op::{self, FnSig, Op, Ty};
use crate::value::Heap;

/// Read `source`, run it through the same lex → compile → check pipeline
/// the VM uses, and lower the resulting op stream to a native object
/// file at `output`. The exposed surface for the binary's `--compile`
/// mode; everything else here is implementation detail.
pub fn compile_source_to_object(source: &str, output: &Path) -> Result<()> {
    let toks = lexer::lex(source)?;
    let mut heap = Heap::default();
    let ops = op::compile(&toks, &mut heap)?;
    op::check(&ops, Vec::new(), &HashMap::new())?;
    compile_to_object(&ops, output)
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Lower `ops` to a native object file at `output`.
///
/// The object exports `plenty_main` (`() -> i32`) and one locally-linked
/// symbol per user-defined Plenty function. Link the object with
/// `runtime/plenty_runtime.c` to produce an executable; the exit status
/// of the final binary is 0 when the program runs to the end of `ops`.
pub fn compile_to_object(ops: &[Op], output: &Path) -> Result<()> {
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

    // Pass 2: emit each user function's body. Bodies can refer to each
    // other (forward references, mutual recursion) because every callee
    // is already declared.
    let names: Vec<String> = user_fns.keys().cloned().collect();
    for name in &names {
        emit_user_function(name, &user_fns, &runtime, &mut module)?;
    }

    // Pass 3: emit `plenty_main`. Top-level `DefineFn`s are skipped
    // here — their bodies were emitted by Pass 2; at runtime a
    // definition is a no-op (it does not touch the data stack).
    emit_main(ops, &user_fns, &runtime, &mut module)?;

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
    print_open_bracket: FuncId,
    print_close_bracket: FuncId,
    print_space: FuncId,
}

fn declare_runtime(module: &mut ObjectModule) -> Result<Runtime> {
    fn one_arg(module: &mut ObjectModule, name: &str, arg: types::Type) -> Result<FuncId> {
        let mut sig = module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(arg));
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
        print_open_bracket: nullary(module, "plenty_print_open_bracket")?,
        print_close_bracket: nullary(module, "plenty_print_close_bracket")?,
        print_space: nullary(module, "plenty_print_space")?,
    })
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

/// The CLIF type backing each Plenty integer/bool width. Plenty's
/// signed/unsigned distinction lives in the `Ty` tag we carry alongside
/// the SSA value; Cranelift treats both with the same machine type, the
/// individual instruction (`sdiv` vs `udiv`, `icmp slt` vs `icmp ult`)
/// picks the interpretation.
fn clif_type(ty: Ty) -> types::Type {
    match ty {
        Ty::I8 | Ty::U8 | Ty::Bool => types::I8,
        Ty::I16 | Ty::U16 => types::I16,
        Ty::I32 | Ty::U32 => types::I32,
        Ty::I64 | Ty::U64 => types::I64,
        Ty::Str => unreachable!("Str values do not reach the AOT lowering yet"),
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

struct Lowerer<'a, 'b> {
    bcx: &'a mut FunctionBuilder<'b>,
    module: &'a mut ObjectModule,
    runtime: &'a Runtime,
    /// Every user function callable from anywhere in the source.
    /// Populated by Pass 1 before any body is emitted, so forward
    /// references and mutual recursion resolve cleanly.
    user_fns: &'a HashMap<String, UserFn>,
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
            Op::Add => self.int_binop(|bcx, a, b| bcx.ins().iadd(a, b))?,
            Op::Sub => self.int_binop(|bcx, a, b| bcx.ins().isub(a, b))?,
            Op::Mul => self.int_binop(|bcx, a, b| bcx.ins().imul(a, b))?,
            Op::Div => {
                let (a, b, ty) = self.pop_int_pair()?;
                let v = if is_signed(ty) {
                    self.bcx.ins().sdiv(a, b)
                } else {
                    self.bcx.ins().udiv(a, b)
                };
                self.stack.push((v, ty));
            }
            Op::Eq => {
                let (a, b, ty) = self.pop_pair()?;
                let v = self.bcx.ins().icmp(IntCC::Equal, a, b);
                // Cranelift `icmp` already produces an i8 (0/1).
                self.stack.push((v, Ty::Bool));
                let _ = ty; // suppress unused — kept for future Str dispatch
            }
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
            Op::ListDir => return Err(unsupported(":listdir")),
            Op::PushStr(_) => return Err(unsupported("string literals")),
            Op::Match(_) => return Err(unsupported("`match`")),
        }
        Ok(())
    }

    /// Lower an integer arithmetic op whose CLIF instruction is the same
    /// for signed and unsigned operands (add/sub/mul).
    fn int_binop(
        &mut self,
        emit: impl FnOnce(
            &mut FunctionBuilder,
            cranelift_codegen::ir::Value,
            cranelift_codegen::ir::Value,
        ) -> cranelift_codegen::ir::Value,
    ) -> Result<()> {
        let (a, b, ty) = self.pop_int_pair()?;
        let v = emit(self.bcx, a, b);
        self.stack.push((v, ty));
        Ok(())
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
            Ty::Str => unreachable!("Str does not reach AOT printing yet"),
        }
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
}

fn unsupported(what: &str) -> Box<dyn Error> {
    format!(
        "AOT compilation does not yet support {what} \
         (phase c.2 covers integer top-level programs plus user-defined \
         functions; `match`, strings, and `:listdir` land in c.3-c.4)"
    )
    .into()
}
