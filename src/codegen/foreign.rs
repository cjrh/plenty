//! C calls are ordinary target-ABI calls, never Plenty tail-convention calls.
use super::*;

pub(super) fn parameter(ty: &Ty) -> AbiParam {
    // A class crosses C as its boxed handle.
    let parameter = AbiParam::new(if matches!(ty, Ty::Ref(..) | Ty::Class(_)) {
        PTR_TY
    } else {
        clif_type(ty.clone())
    });
    match ty {
        Ty::I8 | Ty::I16 | Ty::I32 => parameter.sext(),
        Ty::Bool | Ty::U8 | Ty::U16 | Ty::U32 => parameter.uext(),
        _ => parameter,
    }
}

/// C booleans occupy one byte. Keep Plenty's boolean values canonical even
/// when trusted raw memory or a foreign implementation provides another byte.
pub(super) fn canonical_bool(
    b: &mut FunctionBuilder<'_>,
    value: cranelift_codegen::ir::Value,
    ty: &Ty,
) -> cranelift_codegen::ir::Value {
    if *ty == Ty::Bool {
        b.ins().icmp_imm_u(IntCC::NotEqual, value, 0)
    } else {
        value
    }
}

impl Lowerer<'_, '_> {
    fn lower_c_intrinsic(
        &mut self,
        intrinsic: &crate::foreign::Intrinsic,
        sig: &FnSig,
    ) -> Result<()> {
        use crate::foreign::Intrinsic;
        use cranelift_codegen::ir::MemFlagsData;
        let mut args = Vec::new();
        for (_, ty) in sig.inputs.iter().rev() {
            args.push(self.pop_typed(ty.clone())?.0);
        }
        args.reverse();
        let value = match intrinsic {
            Intrinsic::Allocator { opcode, descriptor } => {
                use crate::collection_opcodes::{
                    ALLOCATOR_BLOCK, ALLOCATOR_BLOCK_SPLIT, ALLOCATOR_BOX, ALLOCATOR_COPY,
                };
                for (argument, (_, ty)) in args.iter_mut().zip(&sig.inputs) {
                    if let Ty::Ref(inner, _) = ty {
                        *argument = self.read_typed_reference(*argument, inner);
                        if matches!(*opcode, ALLOCATOR_BOX | ALLOCATOR_COPY) {
                            // Reference reads leave narrow scalars unpacked; the
                            // runtime expects the ordinary packed value.
                            let value = self.unpack(*argument, inner);
                            *argument = self.pack(value, inner);
                        }
                    }
                }
                if *opcode == ALLOCATOR_COPY {
                    // The native walk writes any inline payload into private frame
                    // storage. Its packed Result points here, not into the source.
                    let bytes = descriptor.as_ref().expect("copy descriptor").inline_bytes();
                    if bytes != 0 {
                        args.push(self.inline_storage(bytes));
                    }
                }
                if matches!(*opcode, ALLOCATOR_BLOCK | ALLOCATOR_BLOCK_SPLIT) {
                    // Native results write into this frame, never a shared runtime
                    // temporary. Relocate the result below using normal inline moves.
                    args.push(self.inline_storage(sig.outputs[0].inline_bytes()));
                }
                let packed = self.collection_call(*opcode, &args, descriptor.as_ref())?;
                sig.outputs.first().map(|ty| {
                    let value = self.unpack(packed, ty);
                    if *opcode == ALLOCATOR_COPY {
                        value
                    } else {
                        self.snapshot_inline(value, ty)
                    }
                })
            }
            Intrinsic::AllocatorRegister(provider) => {
                let value = self.read_typed_reference(args[1], &provider.state);
                let value = self.unpack(value, &provider.state);
                args[1] = self.pack(value, &provider.state);
                for allocate in [true, false] {
                    let name = allocator_providers::name(provider, allocate);
                    let id = self
                        .module
                        .declared_function(&name)
                        .ok_or("missing allocator adapter")?;
                    let reference = self.module.declare_func_in_func(id, self.bcx.func);
                    args.push(self.bcx.ins().func_addr(PTR_TY, reference));
                }
                let descriptor = Ty::Box(Arc::new(provider.state.clone()));
                let value = self.collection_call(
                    crate::collection_opcodes::ALLOCATOR_REGISTER,
                    &args,
                    Some(&descriptor),
                )?;
                Some(self.unpack(value, &sig.outputs[0]))
            }
            Intrinsic::AllocatorCommit => None,
            Intrinsic::AllocatorBlockTake => {
                let Ty::Class(block) = &sig.inputs[0].1 else {
                    unreachable!("checked ByteBlock")
                };
                // Snapshot the private words, but neither retain nor release the
                // consumed owner. Zero is never exposed as a live ByteBlock.
                Some(self.class_field(args[0], block.get().field_offset(0) as i64, &sig.outputs[0]))
            }
            Intrinsic::SlotPut => {
                let address = self.bcx.ins().iadd(args[0], args[1]);
                self.store_slot(address, args[2], &sig.inputs[2].1);
                None
            }
            Intrinsic::SlotTake => {
                let ty = &sig.outputs[0];
                let address = self.bcx.ins().iadd(args[0], args[1]);
                let packed = self
                    .bcx
                    .ins()
                    .load(types::I128, MemFlagsData::trusted(), address, 0);
                let value = self.unpack(packed, ty);
                // Relocate inline payloads before the original storage can be
                // freed or reused. This is a move: no retain or destructor.
                let value = self.snapshot_inline(value, ty);
                self.zero_bytes(address, ty.slot_bytes() as u64);
                Some(value)
            }
            Intrinsic::Load => {
                let address = self.bcx.ins().iadd(args[0], args[1]);
                Some(self.bcx.ins().load(
                    clif_type(sig.outputs[0].clone()),
                    MemFlagsData::new(),
                    address,
                    0,
                ))
            }
            Intrinsic::Store => {
                let address = self.bcx.ins().iadd(args[0], args[1]);
                self.bcx
                    .ins()
                    .store(MemFlagsData::new(), args[2], address, 0);
                None
            }
            Intrinsic::Offset => Some(self.bcx.ins().iadd(args[0], args[1])),
            Intrinsic::Address(name) => {
                let Some(id) = self.module.declared_function(name) else {
                    return Err(format!(
                        "C address target `{name}` must name an export in this library"
                    )
                    .into());
                };
                let reference = self.module.declare_func_in_func(id, self.bcx.func);
                Some(self.bcx.ins().func_addr(PTR_TY, reference))
            }
            Intrinsic::Object(object) => {
                let id = self
                    .module
                    .declared_data(&object.symbol)
                    .ok_or("missing C object export")?;
                let reference = self.module.declare_data_in_func(id, self.bcx.func);
                Some(self.bcx.ins().symbol_value(PTR_TY, reference))
            }
            Intrinsic::Static {
                name,
                bytes,
                writable,
            } => {
                let id = self.module.raw_static(name, bytes, *writable);
                let reference = self.module.declare_data_in_func(id, self.bcx.func);
                Some(self.bcx.ins().symbol_value(PTR_TY, reference))
            }
        };
        if let Some(value) = value {
            let value = canonical_bool(self.bcx, value, &sig.outputs[0]);
            self.stack.push((value, sig.outputs[0].clone()));
        }
        Ok(())
    }

    pub(super) fn lower_foreign_call(
        &mut self,
        declaration: &crate::foreign::Declaration,
        sig: &FnSig,
    ) -> Result<()> {
        if let crate::foreign::Target::Intrinsic(intrinsic) = &declaration.target {
            return self.lower_c_intrinsic(intrinsic, sig);
        }
        use crate::foreign::Argument;
        use cranelift_codegen::ir::MemFlagsData;
        let mut signature = self.module.make_signature();
        let address_index = match declaration.target {
            crate::foreign::Target::Parameter(index) => Some(index),
            _ => None,
        };
        for (index, ((_, ty), mode)) in sig.inputs.iter().zip(&declaration.arguments).enumerate() {
            if address_index == Some(index) {
                continue;
            }
            match mode {
                Argument::Direct | Argument::ScopedBox => signature.params.push(parameter(ty)),
                Argument::Utf8 => signature
                    .params
                    .extend([AbiParam::new(PTR_TY), AbiParam::new(types::I64)]),
                Argument::CString | Argument::CStrings => {
                    signature.params.push(AbiParam::new(PTR_TY))
                }
                Argument::Errno => {}
                Argument::Strided => signature.params.extend([
                    AbiParam::new(PTR_TY),
                    AbiParam::new(types::I64),
                    AbiParam::new(types::I64),
                ]),
            }
        }
        let c_output = declaration.output(sig);
        signature.returns.extend(c_output.map(parameter));
        let mut arguments = Vec::new();
        for (_, ty) in sig.inputs.iter().rev() {
            arguments.push(self.pop_typed(ty.clone())?.0);
        }
        arguments.reverse();
        let address = address_index.map(|index| arguments[index]);
        let mut native_arguments = Vec::new();
        let mut buffers = Vec::new();
        let mut errno_output = None;
        let mut bool_outputs = Vec::new();
        for (index, (argument, mode)) in arguments
            .into_iter()
            .zip(&declaration.arguments)
            .enumerate()
        {
            if address_index == Some(index) {
                continue;
            }
            let argument = if matches!(sig.inputs[index].1, Ty::Ref(..)) {
                self.raw_word(argument)
            } else {
                argument
            };
            match mode {
                Argument::Direct => {
                    if matches!(&sig.inputs[index].1, Ty::Ref(inner, true) if **inner == Ty::Bool) {
                        bool_outputs.push(argument);
                    }
                    native_arguments.push(argument)
                }
                Argument::ScopedBox => {
                    // The exclusive reference points at the Box owner slot. Lend
                    // its allocation, not the slot or a pointer to its fields.
                    native_arguments.push(self.bcx.ins().load(
                        PTR_TY,
                        MemFlagsData::trusted(),
                        argument,
                        0,
                    ));
                }
                Argument::Errno => errno_output = Some(argument),
                Argument::Strided if matches!(sig.inputs[index].1, Ty::View(..)) => {
                    // Byte views already carry their borrowed address and count.
                    let (data, length) = self.bcx.ins().isplit(argument);
                    let stride = self.bcx.ins().iconst(types::I64, 1);
                    native_arguments.extend([data, length, stride]);
                }
                Argument::Strided => {
                    let owner = self
                        .bcx
                        .ins()
                        .load(PTR_TY, MemFlagsData::trusted(), argument, 0);
                    // Reuse the narrow list ABI; Collection/Vec layout stays
                    // private to the runtime. The source loan covers the call.
                    for helper in [
                        self.runtime.list_rows,
                        self.runtime.list_len,
                        self.runtime.list_row_bytes,
                    ] {
                        let callee = self.module.declare_func_in_func(helper, self.bcx.func);
                        let call = self.bcx.ins().call(callee, &[owner]);
                        native_arguments.push(self.bcx.inst_results(call)[0]);
                    }
                }
                Argument::Utf8 | Argument::CString | Argument::CStrings => {
                    // Both borrowed strings and lists hold their owner in the
                    // first word. CStrings returns a private text-header owner
                    // solely for release; its packed bytes are never text.
                    let text = self
                        .bcx
                        .ins()
                        .load(PTR_TY, MemFlagsData::trusted(), argument, 0);
                    if *mode == Argument::Utf8 {
                        // Inline strings keep their bytes after the length byte
                        // of the borrowed slot; heap texts after a 32-byte prefix.
                        let inline = self.bcx.ins().band_imm_s(text, 1);
                        let heap_len = self.bcx.ins().iadd_imm_s(text, 16);
                        let len_word = self.bcx.ins().select(inline, argument, heap_len);
                        let raw =
                            self.bcx
                                .ins()
                                .load(types::I64, MemFlagsData::trusted(), len_word, 0);
                        let low = self.bcx.ins().band_imm_s(raw, 0xff);
                        let inline_len = self.bcx.ins().ushr_imm_s(low, 1);
                        let inline_data = self.bcx.ins().iadd_imm_s(argument, 1);
                        let heap_data = self.bcx.ins().iadd_imm_s(text, 32);
                        native_arguments.push(self.bcx.ins().select(
                            inline,
                            inline_data,
                            heap_data,
                        ));
                        native_arguments.push(self.bcx.ins().select(inline, inline_len, raw));
                        continue;
                    }
                    let text = if matches!(mode, Argument::CString | Argument::CStrings) {
                        let opcode = if *mode == Argument::CStrings {
                            crate::collection_opcodes::C_STRINGS
                        } else {
                            118
                        };
                        let converted = self.collection_call(opcode, &[text], None)?;
                        let failed = self.sum_tag(converted);
                        let failure = self.bcx.create_block();
                        let ready = self.bcx.create_block();
                        self.bcx.ins().brif(failed, failure, &[], ready, &[]);
                        self.bcx.switch_to_block(failure);
                        self.bcx.seal_block(failure);
                        for &buffer in &buffers {
                            self.release(buffer, &Ty::Str);
                        }
                        // Err has the same inline representation for every supported C return.
                        // An inlined adapter must return to its caller, which
                        // can handle this error, rather than leave that caller.
                        self.leave(vec![(converted, sig.outputs[0].clone())], &Arc::from([]))?;
                        self.bcx.switch_to_block(ready);
                        self.bcx.seal_block(ready);
                        let buffer = self.raw_word(converted);
                        buffers.push(buffer);
                        buffer
                    } else {
                        text
                    };
                    native_arguments.push(self.bcx.ins().iadd_imm_s(text, 32));
                }
            }
        }
        let call = if let Some(address) = address {
            let signature = self.bcx.import_signature(signature);
            self.bcx
                .ins()
                .call_indirect(signature, address, &native_arguments)
        } else {
            let id = self
                .module
                .declare_foreign(declaration.symbol().unwrap(), &signature);
            let callee = self.module.declare_func_in_func(id, self.bcx.func);
            self.bcx.ins().call(callee, &native_arguments)
        };
        let result = self
            .bcx
            .inst_results(call)
            .first()
            .copied()
            .map(|value| canonical_bool(self.bcx, value, c_output.unwrap()));
        if let Some(output) = errno_output {
            // This must be the first call after native return: releases and
            // adapter cleanup may invoke native code and change errno.
            let callee = self
                .module
                .declare_func_in_func(self.runtime.ffi_errno, self.bcx.func);
            let capture = self.bcx.ins().call(callee, &[]);
            let error = self.bcx.inst_results(capture)[0];
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), error, output, 0);
        }
        for output in bool_outputs {
            // C may store any byte through a `bool *`; keep the borrowed
            // value one of Plenty's two states, as results and `load` are.
            let byte = self
                .bcx
                .ins()
                .load(types::I8, MemFlagsData::trusted(), output, 0);
            let value = canonical_bool(self.bcx, byte, &Ty::Bool);
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), value, output, 0);
        }
        for buffer in buffers {
            self.release(buffer, &Ty::Str);
        }
        if declaration.fallible() {
            let payload = match (result, c_output) {
                (Some(value), Some(ty)) => self.pack(value, ty),
                _ => {
                    let zero = self.bcx.ins().iconst(types::I64, 0);
                    self.pack(zero, &Ty::Unit)
                }
            };
            let value = self.wrap_sum(payload, 0);
            self.stack.push((value, sig.outputs[0].clone()));
        } else if let (Some(value), Some(ty)) = (result, c_output) {
            self.stack.push((value, ty.clone()));
        }
        Ok(())
    }
}
