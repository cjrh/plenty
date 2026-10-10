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
        // Allocate a returned instance's handle before calling Plenty, so an
        // allocation failure has no construction side effects.
        let returned_handle = if let Some((ok @ Ty::Class(_), _)) = result {
            let boxed = Ty::Box(std::rc::Rc::new(ok.clone()));
            let handle = collection(&mut b, module, runtime, 121, &[], Some(&boxed))?;
            let tags = b.ins().ushr_imm(handle, 64);
            let failed = b.ins().band_imm(tags, 1);
            let failed = b.ins().ireduce(types::I8, failed);
            let failure = b.create_block();
            let ready = b.create_block();
            b.ins().brif(failed, failure, &[], ready, &[]);
            b.switch_to_block(failure);
            b.seal_block(failure);
            // Consumed argument handles are released on every exit.
            let release = module.declare_func_in_func(runtime.release, b.func);
            for (argument, (_, ty)) in arguments.iter().zip(&export.signature.inputs) {
                if matches!(ty, Ty::Class(_)) {
                    b.ins().call(release, &[*argument]);
                }
            }
            let code = b.ins().ushr_imm(handle, 65);
            let code = b.ins().ireduce(types::I32, code);
            b.ins().store(
                cranelift_codegen::ir::MemFlags::new(),
                code,
                *outputs.last().unwrap(),
                0,
            );
            let status = b.ins().iconst(types::I32, 1);
            b.ins().return_(&[status]);
            b.switch_to_block(ready);
            b.seal_block(ready);
            Some(b.ins().ireduce(types::I64, handle))
        } else {
            None
        };
        let mut writebacks = Vec::new();
        for (argument, (_, ty)) in arguments.iter_mut().zip(&export.signature.inputs) {
            // A C handle is a box holding the instance. A borrow lends the
            // box's content slot; an owned argument moves the content out.
            if let Ty::Ref(inner, _) = ty {
                if matches!(inner.as_ref(), Ty::Class(_)) {
                    let slot = b.ins().iadd_imm(*argument, 32);
                    *argument = b.ins().uextend(types::I128, slot);
                    continue;
                }
            }
            if matches!(ty, Ty::Class(_)) {
                let storage = stack_storage(&mut b, ty.inline_bytes());
                let value = b.ins().load(
                    types::I128,
                    cranelift_codegen::ir::MemFlags::trusted(),
                    *argument,
                    32,
                );
                let value = collection(&mut b, module, runtime, 117, &[value, storage], Some(ty))?;
                collection(&mut b, module, runtime, 120, &[*argument], None)?;
                *argument = value;
                continue;
            }
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
                *argument = b.ins().uextend(types::I128, temporary);
            }
        }
        let callee = module.declare_func_in_func(functions[&export.function].id, b.func);
        let returned = export.signature.outputs.iter().map(Ty::inline_bytes).sum();
        if returned != 0 {
            let storage = stack_storage(&mut b, returned);
            arguments.push(storage);
        }
        let call = b.ins().call(callee, &arguments);
        let mut results = b.inst_results(call).to_vec();
        if let (Some(handle), Some((ok, _))) = (returned_handle, result) {
            // Move a returned instance into its handle; an error result frees
            // the still-empty handle.
            let value = results[0];
            let tags = b.ins().ushr_imm(value, 64);
            let failed = b.ins().band_imm(tags, 1);
            let failed = b.ins().ireduce(types::I8, failed);
            let filling = b.create_block();
            let empty = b.create_block();
            let done = b.create_block();
            b.append_block_param(done, types::I128);
            b.ins().brif(failed, empty, &[], filling, &[]);
            b.switch_to_block(empty);
            b.seal_block(empty);
            let release = module.declare_func_in_func(runtime.release, b.func);
            b.ins().call(release, &[handle]);
            b.ins().jump(done, &[value.into()]);
            b.switch_to_block(filling);
            b.seal_block(filling);
            let instance = b.ins().ushr_imm(value, 65);
            let instance = b.ins().ishl_imm(instance, 64);
            let word = b.ins().ireduce(types::I64, value);
            let word = b.ins().uextend(types::I128, word);
            let instance = b.ins().bor(word, instance);
            let slot = b.ins().iadd_imm(handle, 32);
            let storage = b.ins().iadd_imm(handle, 48);
            let instance =
                collection(&mut b, module, runtime, 117, &[instance, storage], Some(ok))?;
            b.ins().store(
                cranelift_codegen::ir::MemFlags::trusted(),
                instance,
                slot,
                0,
            );
            let handle = b.ins().uextend(types::I128, handle);
            b.ins().jump(done, &[handle.into()]);
            b.switch_to_block(done);
            b.seal_block(done);
            results[0] = b.block_params(done)[0];
        }
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
                    } else if matches!(ty, Ty::Class(_)) {
                        // The boxed handle.
                        PTR_TY
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
        emit_guard(&interface.contract_guard, module)?;
        for handle in &interface.handles {
            emit_destroy(handle, runtime, module)?;
        }
        emit_interface(interface, module)?;
    }
    Ok(())
}

fn stack_storage(b: &mut FunctionBuilder<'_>, bytes: usize) -> cranelift_codegen::ir::Value {
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        bytes as u32,
        4,
    ));
    b.ins().stack_addr(PTR_TY, slot, 0)
}

/// Call the runtime value helper from an export adapter, which has no lowerer.
fn collection(
    b: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
    runtime: &Runtime,
    opcode: i64,
    values: &[cranelift_codegen::ir::Value],
    ty: Option<&Ty>,
) -> Result<cranelift_codegen::ir::Value> {
    let descriptor = match ty {
        Some(ty) => {
            let id = metadata::declare(module, runtime, ty)?;
            let gv = module.declare_data_in_func(id, b.func);
            b.ins().global_value(PTR_TY, gv)
        }
        None => b.ins().iconst(PTR_TY, 0),
    };
    let args = stack_storage(b, 96);
    for i in 0..3 {
        let value = match values.get(i) {
            Some(value) if b.func.dfg.value_type(*value) == types::I128 => *value,
            Some(value) => b.ins().uextend(types::I128, *value),
            None => {
                let zero = b.ins().iconst(types::I64, 0);
                b.ins().uextend(types::I128, zero)
            }
        };
        b.ins().store(
            cranelift_codegen::ir::MemFlags::trusted(),
            value,
            args,
            (i * 16) as i32,
        );
    }
    let out = b.ins().iadd_imm(args, 48);
    let code = b.ins().iconst(types::I64, opcode);
    let f = module.declare_func_in_func(runtime.collection, b.func);
    b.ins().call(f, &[code, args, descriptor, out]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        out,
        0,
    ))
}

fn emit_guard(symbol: &str, module: &mut ObjectModule) -> Result<()> {
    let signature = module.make_signature();
    let id = module.declare_function(symbol, Linkage::Export, &signature)?;
    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
    let mut fc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fc);
    let block = b.create_block();
    b.switch_to_block(block);
    b.seal_block(block);
    b.ins().return_(&[]);
    b.finalize();
    module.define_function(id, &mut ctx)?;
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
