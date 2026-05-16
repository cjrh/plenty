//! AOT code generation via Cranelift (§11.1, §12.3 — phase c.1).
//!
//! Lowers a *top-level* Plenty `Op` stream — no function definitions,
//! no `match`, no strings — to a Cranelift module emitted as a native
//! object file. The object exports one symbol, `plenty_main`, which the
//! C runtime in `runtime/plenty_runtime.c` calls from its `main`.
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
//! What c.1 does *not* lower — function definitions, calls, `LoadLocal`,
//! `Match`, anything that touches a `Str` — is rejected with a clear
//! "not yet implemented in AOT" error. Those programs still run under
//! the tree-walking VM.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{types, AbiParam, Function, InstBuilder, UserFuncName};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::Configurable;
use cranelift_codegen::{settings, Context};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::lexer;
use crate::op::{self, Op, Ty};
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

/// Lower `ops` to a native object file at `output`. The object exports
/// `plenty_main` (`() -> i32`); link it with `runtime/plenty_runtime.c`
/// to produce an executable. The exit status of the final binary is 0
/// when the program runs to the end of `ops`.
pub fn compile_to_object(ops: &[Op], output: &Path) -> Result<()> {
    let isa = host_isa()?;
    let builder = ObjectBuilder::new(
        isa,
        "plenty",
        cranelift_module::default_libcall_names(),
    )?;
    let mut module = ObjectModule::new(builder);

    let runtime = declare_runtime(&mut module)?;

    // `plenty_main`: exported, no arguments, returns `i32`. The C
    // runtime's `int main(int, char**)` forwards into this and returns
    // its result as the process exit code.
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

        let mut lower = Lowerer { bcx: &mut bcx, module: &mut module, runtime: &runtime, stack: Vec::new() };
        for op in ops {
            lower.lower(op)?;
        }
        let zero = lower.bcx.ins().iconst(types::I32, 0);
        lower.bcx.ins().return_(&[zero]);
        bcx.finalize();
    }
    module.define_function(main_id, &mut ctx)?;

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
    stack: Vec<StackEntry>,
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
            Op::ListDir => return Err(unsupported(":listdir")),
            Op::PushStr(_) => return Err(unsupported("string literals")),
            Op::DefineFn(_, _) => return Err(unsupported("function definitions")),
            Op::Call(_) | Op::TailCall(_) => return Err(unsupported("function calls")),
            Op::LoadLocal(_) => return Err(unsupported("local loads")),
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
}

fn unsupported(what: &str) -> Box<dyn Error> {
    format!(
        "AOT compilation does not yet support {what} \
         (phase c.1 is integer-only top-level programs; \
         functions, `match`, and strings land in c.2-c.4)"
    )
    .into()
}
