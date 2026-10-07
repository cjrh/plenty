//! C calls are ordinary target-ABI calls, never Plenty tail-convention calls.
use super::*;

pub(super) fn parameter(ty: &Ty) -> AbiParam {
    let parameter = AbiParam::new(clif_type(ty.clone()));
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
        use cranelift_codegen::ir::MemFlags;
        let mut signature = self.module.make_signature();
        for ((_, ty), mode) in sig.inputs.iter().zip(&declaration.arguments) {
            match mode {
                Argument::Direct => signature.params.push(parameter(ty)),
                Argument::Utf8 => signature
                    .params
                    .extend([AbiParam::new(PTR_TY), AbiParam::new(types::I64)]),
                Argument::CString => signature.params.push(AbiParam::new(PTR_TY)),
            }
        }
        let c_output = declaration.output(sig);
        signature.returns.extend(c_output.map(parameter));
        let id = self
            .module
            .declare_function(&declaration.symbol, Linkage::Import, &signature)?;
        let callee = self.module.declare_func_in_func(id, self.bcx.func);
        let mut arguments = Vec::new();
        for (_, ty) in sig.inputs.iter().rev() {
            arguments.push(self.pop_typed(ty.clone())?.0);
        }
        arguments.reverse();
        let mut native_arguments = Vec::new();
        let mut buffers = Vec::new();
        for (argument, mode) in arguments.into_iter().zip(&declaration.arguments) {
            match mode {
                Argument::Direct => native_arguments.push(argument),
                Argument::Utf8 | Argument::CString => {
                    let text = self
                        .bcx
                        .ins()
                        .load(PTR_TY, MemFlags::trusted(), argument, 0);
                    let text = if *mode == Argument::CString {
                        let converted = self.collection_call(118, &[text], None)?;
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
                        self.return_values(vec![(converted, sig.outputs[0].clone())]);
                        self.bcx.switch_to_block(ready);
                        self.bcx.seal_block(ready);
                        let buffer = self.raw_word(converted);
                        buffers.push(buffer);
                        buffer
                    } else {
                        text
                    };
                    native_arguments.push(self.bcx.ins().iadd_imm(text, 32));
                    if *mode == Argument::Utf8 {
                        native_arguments.push(self.bcx.ins().load(
                            types::I64,
                            MemFlags::trusted(),
                            text,
                            16,
                        ));
                    }
                }
            }
        }
        let call = self.bcx.ins().call(callee, &native_arguments);
        let result = self.bcx.inst_results(call).first().copied();
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
