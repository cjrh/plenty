//! Emit a tiny Cranelift caller, or run the real Plenty frontend on a source.
use cranelift_codegen::ir::{AbiParam, InstBuilder, TrapCode, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn emit(plan: &Value, path: &Path) -> Result<()> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed")?;
    flags.set("is_pic", "true")?;
    let isa = cranelift_native::builder()?.finish(settings::Flags::new(flags))?;
    let mut module = ObjectModule::new(ObjectBuilder::new(
        isa,
        "rust_crate_probe",
        cranelift_module::default_libcall_names(),
    )?);
    let mut context = module.make_context();
    context
        .func
        .signature
        .params
        .push(AbiParam::new(types::I64));
    context
        .func
        .signature
        .returns
        .push(AbiParam::new(types::I64));
    let function =
        module.declare_function("probe_run", Linkage::Export, &context.func.signature)?;
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let block = builder.create_block();
    builder.append_block_params_for_function_params(block);
    builder.switch_to_block(block);
    builder.seal_block(block);
    let mut values = HashMap::from([("value".to_owned(), builder.block_params(block)[0])]);
    for (index, operation) in plan["operations"]
        .as_array()
        .ok_or("operations")?
        .iter()
        .enumerate()
    {
        let mut args = Vec::new();
        for arg in operation["args"].as_array().ok_or("args")? {
            if let Some(text) = arg["text"].as_str() {
                let data = module.declare_data(
                    &format!("text_{index}_{}", args.len()),
                    Linkage::Local,
                    false,
                    false,
                )?;
                let mut desc = DataDescription::new();
                desc.define(text.as_bytes().to_vec().into_boxed_slice());
                module.define_data(data, &desc)?;
                let global = module.declare_data_in_func(data, builder.func);
                args.push(builder.ins().global_value(types::I64, global));
                args.push(builder.ins().iconst(types::I64, text.len() as i64));
            } else if let Some(name) = arg.as_str() {
                args.push(*values.get(name).ok_or("unknown variable")?);
            } else {
                args.push(
                    builder
                        .ins()
                        .iconst(types::I64, arg.as_i64().ok_or("integer")?),
                );
            }
        }
        let mut signature = module.make_signature();
        signature.params = args.iter().map(|_| AbiParam::new(types::I64)).collect();
        signature.returns.push(AbiParam::new(types::I64));
        // Deliberately constrained observed x86-64 scalar ABI, NOT a rustc FnAbi reader.
        let name = operation["call"].as_str().ok_or("call")?;
        let id = module.declare_function(&format!("bridge_{name}"), Linkage::Import, &signature)?;
        let reference = module.declare_func_in_func(id, builder.func);
        let call = builder.ins().call(reference, &args);
        values.insert(
            operation["bind"].as_str().ok_or("bind")?.to_owned(),
            builder.inst_results(call)[0],
        );
    }
    let mut sum = builder.ins().iconst(types::I64, 0);
    for name in plan["sum"].as_array().ok_or("sum")? {
        let (next, overflow) = builder
            .ins()
            .sadd_overflow(sum, values[name.as_str().ok_or("name")?]);
        builder.ins().trapnz(overflow, TrapCode::user(1).unwrap());
        sum = next;
    }
    builder.ins().return_(&[sum]);
    builder.finalize();
    module.define_function(function, &mut context)?;
    std::fs::write(path, module.finish().emit()?)?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("check") => plenty::check_source(&std::fs::read_to_string(&args[2])?),
        Some("emit") => emit(
            &serde_json::from_str(&std::fs::read_to_string(&args[2])?)?,
            Path::new(&args[3]),
        ),
        _ => Err("usage: probe check SOURCE | emit PLAN OBJECT".into()),
    }
}
