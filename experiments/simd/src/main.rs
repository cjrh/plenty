//! Isolated backend feasibility probe, not a Plenty compiler pass.
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::path::Path;

fn emit(target: &str, lanes: u32, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed")?;
    let triple = target.parse().map_err(|e| format!("invalid target: {e}"))?;
    let isa = cranelift_codegen::isa::lookup(triple)?.finish(settings::Flags::new(flags))?;
    let mut module = ObjectModule::new(ObjectBuilder::new(
        isa,
        "simd-spike",
        cranelift_module::default_libcall_names(),
    )?);
    for floating in [false, true] {
        let name = format!("clif_{}_{}", if floating { "f32" } else { "u32" }, lanes);
        let mut ctx = module.make_context();
        let ptr = module.target_config().pointer_type();
        ctx.func.signature.params.extend([
            AbiParam::new(ptr),
            AbiParam::new(ptr),
            AbiParam::new(ptr),
        ]);
        let id = module.declare_function(&name, Linkage::Export, &ctx.func.signature)?;
        let scalar = if floating { types::F32 } else { types::I32 };
        let vector = scalar.by(lanes).ok_or("unsupported vector type")?;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = b.create_block();
            let test = b.create_block();
            let body = b.create_block();
            let done = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.append_block_param(test, ptr);
            b.switch_to_block(entry);
            let args = b.block_params(entry).to_vec();
            let zero = b.ins().iconst(ptr, 0);
            let three = if floating {
                b.ins().f32const(3.0)
            } else {
                b.ins().iconst(scalar, 3)
            };
            let seven = if floating {
                b.ins().f32const(7.0)
            } else {
                b.ins().iconst(scalar, 7)
            };
            let three = if lanes > 1 {
                b.ins().splat(vector, three)
            } else {
                three
            };
            let seven = if lanes > 1 {
                b.ins().splat(vector, seven)
            } else {
                seven
            };
            b.ins().jump(test, &[zero.into()]);
            b.switch_to_block(test);
            let i = b.block_params(test)[0];
            let more = b.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
                i,
                args[2],
            );
            b.ins().brif(more, body, &[], done, &[]);
            b.switch_to_block(body);
            let offset = b.ins().ishl_imm(i, 2);
            let src = b.ins().iadd(args[0], offset);
            let dst = b.ins().iadd(args[1], offset);
            let x = b.ins().load(vector, MemFlags::new(), src, 0);
            let product = if floating {
                b.ins().fmul(x, three)
            } else {
                b.ins().imul(x, three)
            };
            let y = if floating {
                b.ins().fadd(product, seven)
            } else {
                b.ins().iadd(product, seven)
            };
            b.ins().store(MemFlags::new(), y, dst, 0);
            let next = b.ins().iadd_imm(i, i64::from(lanes));
            b.ins().jump(test, &[next.into()]);
            b.switch_to_block(done);
            b.ins().return_(&[]);
            b.seal_all_blocks();
            b.finalize();
        }
        module.define_function(id, &mut ctx)?;
    }
    std::fs::write(output, module.finish().emit()?)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err(
            "usage: plenty-simd-spike TARGET LANES OUTPUT.o (length must be a lane multiple)"
                .into(),
        );
    }
    emit(&args[1], args[2].parse()?, Path::new(&args[3]))
}
