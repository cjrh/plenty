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
        Ty::U8 | Ty::U16 | Ty::U32 => parameter.uext(),
        _ => parameter,
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_foreign_call(
        &mut self,
        declaration: &crate::foreign::Declaration,
        sig: &FnSig,
    ) -> Result<()> {
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
                Argument::Direct => signature.params.push(parameter(ty)),
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
                Argument::Direct => native_arguments.push(argument),
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
        let result = self.bcx.inst_results(call).first().copied();
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
