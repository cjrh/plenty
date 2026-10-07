//! C-convention entry points call private Tail-convention Plenty bodies.
use super::*;

pub(super) fn emit(
    exports: &[crate::exports::Export],
    interface: Option<&crate::exports::Interface>,
    functions: &HashMap<String, UserFn>,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    for export in exports {
        let result = export
            .signature
            .outputs
            .first()
            .and_then(crate::exports::result_payloads);
        let mut signature = module.make_signature();
        signature.params.extend(
            export
                .signature
                .inputs
                .iter()
                .map(|(_, ty)| foreign::parameter(ty)),
        );
        if let Some((ok, _)) = result {
            if *ok != Ty::Unit {
                signature.params.push(AbiParam::new(PTR_TY));
            }
            signature.params.push(AbiParam::new(PTR_TY));
            signature.returns.push(foreign::parameter(&Ty::U32));
        } else {
            signature
                .returns
                .extend(export.signature.outputs.iter().map(foreign::parameter));
        }
        let id = module.declare_function(&export.symbol, Linkage::Export, &signature)?;
        let mut ctx = Context::new();
        ctx.func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
        let mut fc = FunctionBuilderContext::new();
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fc);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        b.seal_block(block);
        let mut arguments = b.block_params(block).to_vec();
        let outputs = arguments.split_off(export.signature.inputs.len());
        let mut writebacks = Vec::new();
        for (argument, (_, ty)) in arguments.iter_mut().zip(&export.signature.inputs) {
            if let Ty::Ref(inner, mutable) = ty {
                // C promises one exact-sized initialized scalar, never a Plenty
                // 16-byte slot. Marshal through a private, aligned stack slot.
                let scalar_type = clif_type(inner.as_ref().clone());
                let object = matches!(inner.as_ref(), Ty::Class(_));
                let value = if object {
                    *argument
                } else {
                    b.ins().load(
                        scalar_type,
                        cranelift_codegen::ir::MemFlags::new(),
                        *argument,
                        0,
                    )
                };
                let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    16,
                    4,
                ));
                let temporary = b.ins().stack_addr(PTR_TY, slot, 0);
                let packed = enums::pack_value(&mut b, value, inner);
                b.ins().store(
                    cranelift_codegen::ir::MemFlags::trusted(),
                    packed,
                    temporary,
                    0,
                );
                if *mutable && !object {
                    writebacks.push((*argument, temporary, scalar_type));
                }
                *argument = temporary;
            }
        }
        let callee = module.declare_func_in_func(functions[&export.function].id, b.func);
        let call = b.ins().call(callee, &arguments);
        let results = b.inst_results(call).to_vec();
        for (destination, temporary, scalar_type) in writebacks {
            let value = b.ins().load(
                scalar_type,
                cranelift_codegen::ir::MemFlags::new(),
                temporary,
                0,
            );
            b.ins().store(
                cranelift_codegen::ir::MemFlags::new(),
                value,
                destination,
                0,
            );
        }
        if let Some((ok, error)) = result {
            let value = results[0];
            let tags = b.ins().ushr_imm(value, 64);
            let tag = b.ins().ireduce(types::I32, tags);
            let tag = b.ins().band_imm(tag, 1);
            let success = b.create_block();
            let failure = b.create_block();
            b.ins().brif(tag, failure, &[], success, &[]);
            for (block, ty, destination) in [
                (success, ok, outputs.first().copied()),
                (failure, error, outputs.last().copied()),
            ] {
                b.switch_to_block(block);
                b.seal_block(block);
                if *ty != Ty::Unit {
                    let marker = crate::exports::error_variants(ty).is_some();
                    let value = if marker {
                        b.ins().ushr_imm(value, 65)
                    } else {
                        value
                    };
                    let target = if marker {
                        types::I32
                    } else {
                        clif_type(ty.clone())
                    };
                    let bits = b.ins().ireduce(
                        if ty.is_float() {
                            if *ty == Ty::F32 {
                                types::I32
                            } else {
                                types::I64
                            }
                        } else {
                            target
                        },
                        value,
                    );
                    let scalar = if ty.is_float() {
                        b.ins()
                            .bitcast(target, cranelift_codegen::ir::MemFlags::new(), bits)
                    } else {
                        bits
                    };
                    b.ins().store(
                        cranelift_codegen::ir::MemFlags::new(),
                        scalar,
                        destination.unwrap(),
                        0,
                    );
                }
                b.ins().return_(&[tag]);
            }
        } else {
            b.ins().return_(&results);
        }
        b.finalize();
        module.define_function(id, &mut ctx)?;
    }
    if let Some(interface) = interface {
        for handle in &interface.handles {
            emit_destroy(handle, runtime, module)?;
        }
        emit_interface(interface, module)?;
    }
    Ok(())
}

fn emit_destroy(
    handle: &crate::exports::Handle,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    let mut signature = module.make_signature();
    signature.params.push(AbiParam::new(PTR_TY));
    let id = module.declare_function(&handle.destroy, Linkage::Export, &signature)?;
    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
    let mut fc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fc);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    b.seal_block(block);
    let owner = b.block_params(block)[0];
    let release = module.declare_func_in_func(runtime.release, b.func);
    b.ins().call(release, &[owner]);
    b.ins().return_(&[]);
    b.finalize();
    module.define_function(id, &mut ctx)?;
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
