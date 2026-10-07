//! C-convention entry points call private Tail-convention Plenty bodies.
use super::*;

pub(super) fn emit(
    exports: &[crate::exports::Export],
    interface: Option<&crate::exports::Interface>,
    functions: &HashMap<String, UserFn>,
    module: &mut ObjectModule,
) -> Result<()> {
    for export in exports {
        let mut signature = module.make_signature();
        signature.params.extend(
            export
                .signature
                .inputs
                .iter()
                .map(|(_, ty)| foreign::parameter(ty)),
        );
        signature
            .returns
            .extend(export.signature.outputs.iter().map(foreign::parameter));
        let id = module.declare_function(&export.symbol, Linkage::Export, &signature)?;
        let mut ctx = Context::new();
        ctx.func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
        let mut fc = FunctionBuilderContext::new();
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fc);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        b.seal_block(block);
        let arguments = b.block_params(block).to_vec();
        let callee = module.declare_func_in_func(functions[&export.function].id, b.func);
        let call = b.ins().call(callee, &arguments);
        let results = b.inst_results(call).to_vec();
        b.ins().return_(&results);
        b.finalize();
        module.define_function(id, &mut ctx)?;
    }
    if let Some(interface) = interface {
        emit_interface(interface, module)?;
    }
    Ok(())
}

fn emit_interface(interface: &crate::exports::Interface, module: &mut ObjectModule) -> Result<()> {
    let data = module.declare_data(
        &format!("{}_plenty_contract", interface.name),
        Linkage::Local,
        false,
        false,
    )?;
    let mut description = DataDescription::new();
    description.define(interface.source.as_bytes().to_vec().into_boxed_slice());
    description.set_segment_section("", &format!(".plenty.interface.{}", interface.name));
    description.set_used(true);
    module.define_data(data, &description)?;
    let mut signature = module.make_signature();
    signature.params.push(AbiParam::new(PTR_TY));
    signature.returns.push(AbiParam::new(PTR_TY));
    let id = module.declare_function(&interface.discovery, Linkage::Export, &signature)?;
    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
    let mut fc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fc);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    b.seal_block(block);
    let output = b.block_params(block)[0];
    let length = b.ins().iconst(types::I64, interface.source.len() as i64);
    b.ins()
        .store(cranelift_codegen::ir::MemFlags::new(), length, output, 0);
    let global = module.declare_data_in_func(data, b.func);
    let pointer = b.ins().global_value(PTR_TY, global);
    b.ins().return_(&[pointer]);
    b.finalize();
    module.define_function(id, &mut ctx)?;
    Ok(())
}
