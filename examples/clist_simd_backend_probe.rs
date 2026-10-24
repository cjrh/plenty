//! Compile-only width probe against the workspace's pinned Cranelift version.
//! Usage: cargo run --release --example clist_simd_backend_probe -- 4 native /tmp/probe.o
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: clist_simd_backend_probe LANES baseline|native OUTPUT.o".into());
    }
    let lanes: u32 = args[1].parse()?;
    let mut flags = settings::builder();
    flags.set("opt_level", "speed")?;
    flags.set("enable_verifier", "true")?;
    let builder = match args[2].as_str() {
        "baseline" => cranelift_codegen::isa::lookup(
            "x86_64-unknown-linux-gnu"
                .parse()
                .expect("valid target triple"),
        )?,
        "native" => cranelift_native::builder()?,
        _ => return Err("target mode must be baseline or native".into()),
    };
    let isa = builder.finish(settings::Flags::new(flags))?;
    let mut module = ObjectModule::new(ObjectBuilder::new(
        isa,
        "clist-width-probe",
        cranelift_module::default_libcall_names(),
    )?);
    let frontend = module.target_config();
    let mut context = module.make_context();
    context.func.signature.params.extend([
        AbiParam::new(frontend.pointer_type()),
        AbiParam::new(frontend.pointer_type()),
    ]);
    let id = module.declare_function("compare", Linkage::Export, &context.func.signature)?;
    let mut builder_context = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let input = b.block_params(entry)[0];
    let output = b.block_params(entry)[1];
    let vector = types::I32.by(lanes).ok_or("unsupported lane count")?;
    let values = b.ins().load(vector, MemFlagsData::new(), input, 0);
    let threshold = b.ins().iconst(types::I32, 5);
    let threshold = b.ins().splat(vector, threshold);
    let mask = b.ins().icmp(
        cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThan,
        values,
        threshold,
    );
    b.ins().store(MemFlagsData::new(), mask, output, 0);
    b.ins().return_(&[]);
    b.seal_all_blocks();
    b.finalize(frontend);
    module.define_function(id, &mut context)?;
    std::fs::write(&args[3], module.finish().emit()?)?;
    println!("compiled i32x{lanes} {}", args[2]);
    Ok(())
}
