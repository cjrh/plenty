use super::*;
use crate::sum::EnumOp;

/// Runtime slots carry 128 bits: a scalar/pointer payload and binary sum tags.
pub(super) fn pack_value(
    b: &mut FunctionBuilder<'_>,
    value: cranelift_codegen::ir::Value,
    ty: &Ty,
) -> cranelift_codegen::ir::Value {
    if ty.inline_sum() {
        return value;
    }
    let word = if ty.is_float() {
        let bits = b.ins().bitcast(
            if *ty == Ty::F32 {
                types::I32
            } else {
                types::I64
            },
            cranelift_codegen::ir::MemFlags::new(),
            value,
        );
        if *ty == Ty::F32 {
            b.ins().uextend(types::I64, bits)
        } else {
            bits
        }
    } else if clif_type(ty.clone()) == types::I64 {
        value
    } else if is_signed(ty.clone()) {
        b.ins().sextend(types::I64, value)
    } else {
        b.ins().uextend(types::I64, value)
    };
    b.ins().uextend(types::I128, word)
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_class(&mut self, op: &crate::record::ClassOp) -> Result<()> {
        use crate::record::ClassOp;
        let (inputs, output) = op.signature().ok_or("invalid class operation")?;
        let result = match op {
            ClassOp::TryNew(t) | ClassOp::ArmDrop(t) => {
                let callback = if let Some(name) = &t.destructor {
                    let id = self.user_fns[name]
                        .drop_callback
                        .ok_or("missing destructor adapter")?;
                    let reference = self.module.declare_func_in_func(id, self.bcx.func);
                    self.bcx.ins().func_addr(PTR_TY, reference)
                } else {
                    self.bcx.ins().iconst(PTR_TY, 0)
                };
                if matches!(op, ClassOp::ArmDrop(_)) {
                    let (address, _) = self.pop_typed(inputs[0].clone())?;
                    let owner = self.bcx.ins().load(
                        PTR_TY,
                        cranelift_codegen::ir::MemFlags::trusted(),
                        address,
                        0,
                    );
                    self.collection_call(109, &[owner, callback], None)?
                } else {
                    let disabled = self.bcx.ins().iconst(PTR_TY, 0);
                    self.collection_call(108, &[disabled], Some(&Ty::Class(t.clone())))?
                }
            }
            ClassOp::Field(_, i) => {
                let (owner, _) = self.pop_typed(inputs[0].clone())?;
                let index = self.bcx.ins().iconst(types::I64, *i as i64);
                let value = self.collection_call(31, &[owner, index], None)?;
                let value = self.unpack(value, &output);
                let value = self.snapshot_inline(value, &output);
                self.release(owner, &inputs[0]);
                self.pack(value, &output)
            }
            ClassOp::FieldRef(t, i, _) => {
                let (address, _) = self.pop_typed(inputs[0].clone())?;
                let owner = self.bcx.ins().load(
                    PTR_TY,
                    cranelift_codegen::ir::MemFlags::trusted(),
                    address,
                    0,
                );
                let offset: i64 = t.fields[..*i]
                    .iter()
                    .map(|(_, t)| t.slot_bytes() as i64)
                    .sum();
                self.bcx.ins().iadd_imm(owner, 32 + offset)
            }
        };
        let result = self.unpack(result, &output);
        self.stack.push((result, output));
        Ok(())
    }
    pub(super) fn lower_enum(&mut self, op: &EnumOp) -> Result<()> {
        let (inputs, output) = op.signature().ok_or("invalid enum operation")?;
        let t = match op {
            EnumOp::New(t, _)
            | EnumOp::Unwrap(t)
            | EnumOp::TryNew(t, _)
            | EnumOp::Tag(t)
            | EnumOp::Field(t, _, _)
            | EnumOp::Take(t, _, _) => t,
        };
        if t.inline() {
            let result = match op {
                EnumOp::Unwrap(t) => {
                    let (value, _) = self.pop_typed(inputs[0].clone())?;
                    let tag = self.sum_tag(value);
                    let failed =
                        self.bcx
                            .ins()
                            .icmp_imm(IntCC::NotEqual, tag, i64::from(t.is_option()));
                    self.bcx.ins().trapnz(failed, TrapCode::unwrap_user(4));
                    self.sum_payload(value)
                }
                EnumOp::New(_, tag) | EnumOp::TryNew(_, tag) => {
                    let payload = if let Some(ty) = inputs.first() {
                        let (v, _) = self.pop_typed(ty.clone())?;
                        self.pack(v, ty)
                    } else {
                        let zero = self.bcx.ins().iconst(types::I64, 0);
                        self.bcx.ins().uextend(types::I128, zero)
                    };
                    let value = self.wrap_sum(payload, *tag);
                    if matches!(op, EnumOp::TryNew(..)) {
                        self.wrap_sum(value, 0)
                    } else {
                        value
                    }
                }
                EnumOp::Tag(_) => {
                    let (value, _) = self.pop_typed(inputs[0].clone())?;
                    let tags = self.bcx.ins().ushr_imm(value, 64);
                    let tags = self.raw_word(tags);
                    let mask = (t.variants.len().next_power_of_two() - 1).max(1);
                    let tag = self.bcx.ins().band_imm(tags, mask as i64);
                    self.release(value, &inputs[0]);
                    tag
                }
                EnumOp::Field(..) | EnumOp::Take(..) => {
                    // Single-payload inline projection transfers this operand's
                    // ownership. Any remaining source binding owns its own retain.
                    let (value, _) = self.pop_typed(inputs[0].clone())?;
                    self.sum_payload(value)
                }
            };
            let result = self.unpack(result, &output);
            self.stack.push((result, output));
            return Ok(());
        }
        let mut values = Vec::new();
        for ty in inputs.iter().rev() {
            let (value, _) = self.pop_typed(ty.clone())?;
            values.push(self.pack(value, ty));
        }
        values.reverse();
        let result = match op {
            EnumOp::Unwrap(_) => unreachable!("standard sums are inline"),
            EnumOp::TryNew(t, tag) => {
                let tag = self.bcx.ins().iconst(types::I64, *tag as i64);
                let result = self.collection_call(108, &[tag], Some(&Ty::Enum(t.clone())))?;
                let success = self.bcx.create_block();
                let done = self.bcx.create_block();
                self.bcx.append_block_param(done, types::I128);
                let failed = self.sum_tag(result);
                self.bcx
                    .ins()
                    .brif(failed, done, &[result.into()], success, &[]);
                self.bcx.switch_to_block(success);
                self.bcx.seal_block(success);
                let record = self.sum_payload(result);
                for (i, value) in values.iter().enumerate() {
                    let field = self.bcx.ins().iconst(types::I64, i as i64);
                    self.collection_call(21, &[record, field, *value], None)?;
                }
                self.bcx.ins().jump(done, &[result.into()]);
                self.bcx.switch_to_block(done);
                self.bcx.seal_block(done);
                self.bcx.block_params(done)[0]
            }
            EnumOp::New(t, tag) => {
                let tag = self.bcx.ins().iconst(types::I64, *tag as i64);
                let result = self.collection_call(20, &[tag], Some(&Ty::Enum(t.clone())))?;
                for (i, value) in values.iter().enumerate() {
                    let field = self.bcx.ins().iconst(types::I64, i as i64);
                    self.collection_call(21, &[result, field, *value], None)?;
                }
                result
            }
            EnumOp::Tag(_) => self.collection_call(22, &values, None)?,
            EnumOp::Field(_, tag, field) | EnumOp::Take(_, tag, field) => {
                let tag = self.bcx.ins().iconst(types::I64, *tag as i64);
                let field = self.bcx.ins().iconst(types::I64, *field as i64);
                self.collection_call(
                    if matches!(op, EnumOp::Take(..)) {
                        25
                    } else {
                        23
                    },
                    &[values[0], field, tag],
                    None,
                )?
            }
        };
        let result = self.unpack(result, &output);
        let result = self.snapshot_inline(result, &output);
        for (value, ty) in values.iter().zip(&inputs) {
            self.release(*value, ty);
        }
        self.stack.push((result, output));
        Ok(())
    }

    pub(super) fn pack(
        &mut self,
        value: cranelift_codegen::ir::Value,
        ty: &Ty,
    ) -> cranelift_codegen::ir::Value {
        pack_value(self.bcx, value, ty)
    }
    pub(super) fn unpack(
        &mut self,
        value: cranelift_codegen::ir::Value,
        ty: &Ty,
    ) -> cranelift_codegen::ir::Value {
        let target = clif_type(ty.clone());
        if ty.inline_sum() {
            return if self.bcx.func.dfg.value_type(value) == types::I128 {
                value
            } else {
                self.bcx.ins().uextend(types::I128, value)
            };
        }
        let value = self.raw_word(value);
        if ty.is_float() {
            let bits = if *ty == Ty::F32 {
                self.bcx.ins().ireduce(types::I32, value)
            } else {
                value
            };
            return self
                .bcx
                .ins()
                .bitcast(target, cranelift_codegen::ir::MemFlags::new(), bits);
        }
        if target == types::I64 {
            value
        } else {
            self.bcx.ins().ireduce(target, value)
        }
    }

    pub(super) fn raw_word(
        &mut self,
        value: cranelift_codegen::ir::Value,
    ) -> cranelift_codegen::ir::Value {
        if self.bcx.func.dfg.value_type(value) == types::I128 {
            self.bcx.ins().ireduce(types::I64, value)
        } else {
            value
        }
    }
    pub(super) fn sum_tag(
        &mut self,
        value: cranelift_codegen::ir::Value,
    ) -> cranelift_codegen::ir::Value {
        let tags = self.bcx.ins().ushr_imm(value, 64);
        let tags = self.raw_word(tags);
        self.bcx.ins().band_imm(tags, 1)
    }
    fn sum_payload(&mut self, value: cranelift_codegen::ir::Value) -> cranelift_codegen::ir::Value {
        let word = self.raw_word(value);
        let word = self.bcx.ins().uextend(types::I128, word);
        let tags = self.bcx.ins().ushr_imm(value, 65);
        let tags = self.bcx.ins().ishl_imm(tags, 64);
        self.bcx.ins().bor(word, tags)
    }
    pub(super) fn wrap_sum(
        &mut self,
        value: cranelift_codegen::ir::Value,
        tag: usize,
    ) -> cranelift_codegen::ir::Value {
        let word = self.raw_word(value);
        let word = self.bcx.ins().uextend(types::I128, word);
        let tags = self.bcx.ins().ushr_imm(value, 64);
        let tags = self.bcx.ins().ishl_imm(tags, 1);
        let tags = self.bcx.ins().bor_imm(tags, tag as i64);
        let tags = self.bcx.ins().ishl_imm(tags, 64);
        self.bcx.ins().bor(word, tags)
    }
    pub(super) fn lower_try(
        &mut self,
        source: &std::rc::Rc<crate::sum::EnumType>,
        target: &std::rc::Rc<crate::sum::EnumType>,
        cleanup: &[Op],
    ) -> Result<()> {
        let (value, _) = self.pop_typed(Ty::Enum(source.clone()))?;
        let tag = self.sum_tag(value);
        let payload = self.sum_payload(value);
        let success_tag = usize::from(source.is_option());
        let success = self.bcx.create_block();
        let failure = self.bcx.create_block();
        let ready = self
            .bcx
            .ins()
            .icmp_imm(IntCC::Equal, tag, success_tag as i64);
        self.bcx.ins().brif(ready, success, &[], failure, &[]);
        self.bcx.switch_to_block(failure);
        self.bcx.seal_block(failure);
        let residual = if target.discards_error() {
            // Erasure consumes the original error, including its destructor,
            // before cleaning up earlier operands and the enclosing scope.
            self.release(payload, &source.variants[1].fields[0]);
            let zero = self.bcx.ins().iconst(types::I64, 0);
            let marker = self.bcx.ins().uextend(types::I128, zero);
            self.wrap_sum(marker, 1)
        } else {
            self.wrap_sum(payload, 1 - success_tag)
        };
        // The error has either transferred into residual or been discarded.
        // Earlier operands and locals still need normal early-return cleanup.
        let pending = self.stack.clone();
        self.release_stack();
        for op in cleanup {
            self.lower(op)?;
        }
        self.return_values(vec![(residual, Ty::Enum(target.clone()))]);
        self.stack = pending;
        self.bcx.switch_to_block(success);
        self.bcx.seal_block(success);
        let ty = source.variants[success_tag].fields[0].clone();
        let payload = self.unpack(payload, &ty);
        self.stack.push((payload, ty));
        debug_assert!(target.inline());
        Ok(())
    }
}
