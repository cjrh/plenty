use super::*;
use crate::sum::EnumOp;

/// Runtime slots carry 128 bits: a scalar/pointer payload and binary sum tags.
pub(super) fn pack_value(
    b: &mut FunctionBuilder<'_>,
    value: cranelift_codegen::ir::Value,
    ty: &Ty,
) -> cranelift_codegen::ir::Value {
    if ty.wide() || matches!(ty, Ty::Ref(..)) {
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

/// The tag word of a 128-bit slot. Slot words are split and joined with
/// `isplit`/`iconcat`, which only name the two registers: x64 lowers every
/// 128-bit shift, even by a constant 64, as a dozen-instruction
/// variable-shift sequence that also dominated register allocation (issue #301).
pub(super) fn high_word(
    b: &mut FunctionBuilder<'_>,
    value: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    b.ins().isplit(value).1
}

impl Lowerer<'_, '_> {
    pub(super) fn high_word(
        &mut self,
        value: cranelift_codegen::ir::Value,
    ) -> cranelift_codegen::ir::Value {
        high_word(self.bcx, value)
    }

    /// Byte offset of a field slot inside its owner's inline record storage.
    fn record_offset<'t>(fields: impl Iterator<Item = &'t Ty>) -> i64 {
        fields.map(|t| t.slot_bytes() as i64).sum()
    }

    /// Fresh stack storage for a record. Zeroed slots own nothing, so a
    /// partially initialized class can be released safely.
    fn record_storage(&mut self, bytes: usize) -> cranelift_codegen::ir::Value {
        if bytes == 0 {
            return self.bcx.ins().iconst(PTR_TY, 0);
        }
        let storage = self.inline_storage(bytes);
        let config = self.module.target_config();
        self.bcx.emit_small_memset(
            config,
            storage,
            0,
            bytes as u64,
            8,
            cranelift_codegen::ir::MemFlags::trusted(),
        );
        storage
    }

    /// Load field `index` of a record value, copying its own inline storage
    /// out of the record so it survives the record's release.
    fn record_field(
        &mut self,
        record: cranelift_codegen::ir::Value,
        offset: i64,
        ty: &Ty,
    ) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        let storage = self.raw_word(record);
        let slot = self.bcx.ins().iadd_imm(storage, offset);
        let value = self.bcx.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlags::trusted(),
            slot,
            0,
        );
        let value = self.unpack(value, ty);
        (self.snapshot_inline(value, ty), slot)
    }

    pub(super) fn lower_class(&mut self, op: &crate::record::ClassOp) -> Result<()> {
        use crate::record::ClassOp;
        let (inputs, output) = op.signature().ok_or("invalid class operation")?;
        let result = match op {
            ClassOp::New(t) => {
                let storage = self.record_storage(Ty::Class(t.clone()).inline_bytes());
                self.bcx.ins().uextend(types::I128, storage)
            }
            ClassOp::ArmDrop(t) => {
                // Mark the state word: initialization completed, so release
                // runs `__del__`. A class without one has nothing to record.
                let (reference, _) = self.pop_typed(inputs[0].clone())?;
                if t.get().state_bytes() != 0 {
                    let address = self.raw_word(reference);
                    let storage = self.bcx.ins().load(
                        PTR_TY,
                        cranelift_codegen::ir::MemFlags::trusted(),
                        address,
                        0,
                    );
                    let one = self.bcx.ins().iconst(types::I64, 1);
                    self.bcx.ins().store(
                        cranelift_codegen::ir::MemFlags::trusted(),
                        one,
                        storage,
                        0,
                    );
                }
                self.bcx.ins().iconst(types::I64, 0)
            }
            ClassOp::Field(t, i) => {
                let (owner, _) = self.pop_typed(inputs[0].clone())?;
                let offset = t.get().state_bytes() as i64
                    + Self::record_offset(t.get().fields[..*i].iter().map(|(_, t)| t));
                let (value, _) = self.record_field(owner, offset, &output);
                self.retain(value, &output);
                self.release(owner, &inputs[0]);
                self.pack(value, &output)
            }
            ClassOp::FieldRef(t, i, _) => {
                let (address, _) = self.pop_typed(inputs[0].clone())?;
                let address = self.raw_word(address);
                let owner = self.bcx.ins().load(
                    PTR_TY,
                    cranelift_codegen::ir::MemFlags::trusted(),
                    address,
                    0,
                );
                let offset = t.get().state_bytes() as i64
                    + Self::record_offset(t.get().fields[..*i].iter().map(|(_, t)| t));
                self.bcx.ins().iadd_imm(owner, offset)
            }
        };
        let result = self.unpack(result, &output);
        self.stack.push((result, output));
        Ok(())
    }

    /// Move every field out of a multi-field variant or tuple.
    pub(super) fn lower_split(
        &mut self,
        t: &crate::nominal::Nominal<crate::sum::EnumType>,
        tag: usize,
    ) -> Result<()> {
        let ty = Ty::Enum(t.clone());
        let (value, _) = self.pop_typed(ty)?;
        let payload = self.tagged_payload(value, t.tag_bits());
        let fields = t.get().variants[tag].fields.clone();
        let mut offset = 0;
        for field in fields {
            let (value, _) = self.record_field(payload, offset, &field);
            offset += field.slot_bytes() as i64;
            self.stack.push((value, field));
        }
        Ok(())
    }

    pub(super) fn lower_enum(&mut self, op: &EnumOp) -> Result<()> {
        let (inputs, output) = op.signature().ok_or("invalid enum operation")?;
        if let EnumOp::Widen(source, target) = op {
            let (value, _) = self.pop_typed((**source).clone())?;
            let packed = self.pack(value, source);
            let result = self.widen_sum(packed, source, target);
            let result = self.unpack(result, target);
            self.stack.push((result, (**target).clone()));
            return Ok(());
        }
        if let EnumOp::TagRef(t, _) | EnumOp::FieldRef(t, ..) = op {
            let (reference, _) = self.pop_typed(inputs[0].clone())?;
            let result = match op {
                EnumOp::TagRef(..) => {
                    let packed = self.read_reference(reference);
                    let tags = self.high_word(packed);
                    let mask = (t.get().variants.len().next_power_of_two() - 1).max(1);
                    self.bcx.ins().band_imm(tags, mask as i64)
                }
                EnumOp::FieldRef(_, tag, field, _) => {
                    let fields = &t.get().variants[*tag].fields;
                    if fields.len() == 1 {
                        let (address, shift) = self.bcx.ins().isplit(reference);
                        let shift = self.bcx.ins().iadd_imm(shift, t.tag_bits() as i64);
                        self.bcx.ins().iconcat(address, shift)
                    } else {
                        // The payload word in the slot addresses the record.
                        let address = self.raw_word(reference);
                        let storage = self.bcx.ins().load(
                            PTR_TY,
                            cranelift_codegen::ir::MemFlags::trusted(),
                            address,
                            0,
                        );
                        let offset = Self::record_offset(fields[..*field].iter());
                        let slot = self.bcx.ins().iadd_imm(storage, offset);
                        self.bcx.ins().uextend(types::I128, slot)
                    }
                }
                _ => unreachable!(),
            };
            let result = self.unpack(result, &output);
            self.stack.push((result, output));
            return Ok(());
        }
        let t = match op {
            EnumOp::New(t, _)
            | EnumOp::Unwrap(t)
            | EnumOp::Tag(t)
            | EnumOp::Field(t, _, _)
            | EnumOp::Take(t, _, _) => t,
            EnumOp::TagRef(..) | EnumOp::FieldRef(..) | EnumOp::Widen(..) => unreachable!(),
        };
        let result = match op {
            EnumOp::Unwrap(t) => {
                let (value, _) = self.pop_typed(inputs[0].clone())?;
                let tag = self.sum_tag(value);
                let failed =
                    self.bcx
                        .ins()
                        .icmp_imm(IntCC::NotEqual, tag, i64::from(t.is_option()));
                self.trap_if(
                    failed,
                    TrapKind::Unwrap {
                        option: t.is_option(),
                    },
                )?;
                self.sum_payload(value)
            }
            EnumOp::New(_, tag) => {
                let payload = match inputs.as_slice() {
                    [] => {
                        let zero = self.bcx.ins().iconst(types::I64, 0);
                        self.bcx.ins().uextend(types::I128, zero)
                    }
                    [ty] => {
                        let (v, _) = self.pop_typed(ty.clone())?;
                        self.pack(v, ty)
                    }
                    fields => {
                        let mut values = Vec::new();
                        for ty in fields.iter().rev() {
                            let (value, _) = self.pop_typed(ty.clone())?;
                            values.push(value);
                        }
                        values.reverse();
                        let bytes = fields.iter().map(Ty::slot_bytes).sum();
                        let storage = self.inline_storage(bytes);
                        let mut offset = 0;
                        for (value, ty) in values.into_iter().zip(fields) {
                            let slot = self.bcx.ins().iadd_imm(storage, offset);
                            self.store_slot(slot, value, ty);
                            offset += ty.slot_bytes() as i64;
                        }
                        self.bcx.ins().uextend(types::I128, storage)
                    }
                };
                self.wrap_tagged(payload, *tag, t.tag_bits())
            }
            EnumOp::Tag(_) => {
                let (value, _) = self.pop_typed(inputs[0].clone())?;
                let tags = self.high_word(value);
                let mask = (t.get().variants.len().next_power_of_two() - 1).max(1);
                let tag = self.bcx.ins().band_imm(tags, mask as i64);
                self.release(value, &inputs[0]);
                tag
            }
            EnumOp::Field(_, tag, field) | EnumOp::Take(_, tag, field) => {
                let (value, _) = self.pop_typed(inputs[0].clone())?;
                let fields = t.get().variants[*tag].fields.clone();
                if fields.len() == 1 {
                    // Single-payload projection transfers this operand's
                    // ownership. Any remaining source binding owns its own retain.
                    self.tagged_payload(value, t.tag_bits())
                } else {
                    let payload = self.tagged_payload(value, t.tag_bits());
                    let offset = Self::record_offset(fields[..*field].iter());
                    let (field_value, slot) = self.record_field(payload, offset, &output);
                    if matches!(op, EnumOp::Take(..)) {
                        let zero = self.bcx.ins().iconst(types::I64, 0);
                        let zero = self.bcx.ins().uextend(types::I128, zero);
                        self.bcx.ins().store(
                            cranelift_codegen::ir::MemFlags::trusted(),
                            zero,
                            slot,
                            0,
                        );
                    } else {
                        self.retain(field_value, &output);
                    }
                    self.release(value, &inputs[0]);
                    self.pack(field_value, &output)
                }
            }
            EnumOp::TagRef(..) | EnumOp::FieldRef(..) | EnumOp::Widen(..) => unreachable!(),
        };
        let result = self.unpack(result, &output);
        self.stack.push((result, output));
        Ok(())
    }

    /// Retag packed values without retaining, releasing, or allocating payloads.
    fn widen_sum(
        &mut self,
        value: cranelift_codegen::ir::Value,
        source: &Ty,
        target: &Ty,
    ) -> cranelift_codegen::ir::Value {
        if source == target {
            return value;
        }
        let Ty::Enum(to) = target else { unreachable!() };
        if to.anonymous() {
            let targets = crate::sum::members(target);
            if let Ty::Enum(from) = source {
                if from.anonymous() {
                    let tags = self.high_word(value);
                    let mask = (from.get().variants.len().next_power_of_two() - 1).max(1);
                    let old_tag = self.bcx.ins().band_imm(tags, mask as i64);
                    let mut tag = self.bcx.ins().iconst(types::I64, 0);
                    for (index, member) in crate::sum::members(source).iter().enumerate() {
                        let destination = targets.iter().position(|t| t == member).unwrap();
                        let selected = self.bcx.ins().icmp_imm(IntCC::Equal, old_tag, index as i64);
                        let new_tag = self.bcx.ins().iconst(types::I64, destination as i64);
                        tag = self.bcx.ins().select(selected, new_tag, tag);
                    }
                    let payload = self.tagged_payload(value, from.tag_bits());
                    let wrapped = self.wrap_tagged(payload, 0, to.tag_bits());
                    let (word, tags) = self.bcx.ins().isplit(wrapped);
                    let tags = self.bcx.ins().bor(tags, tag);
                    return self.bcx.ins().iconcat(word, tags);
                }
            }
            return self.wrap_tagged(
                value,
                targets.iter().position(|t| t == source).unwrap(),
                to.tag_bits(),
            );
        }
        let Ty::Enum(from) = source else {
            unreachable!()
        };
        debug_assert!(from.is_result() && to.is_result());
        let success = self.bcx.create_block();
        let failure = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, types::I128);
        let tag = self.sum_tag(value);
        let ok = self.bcx.ins().icmp_imm(IntCC::Equal, tag, 0);
        self.bcx.ins().brif(ok, success, &[], failure, &[]);
        self.bcx.switch_to_block(success);
        self.bcx.seal_block(success);
        self.bcx.ins().jump(done, &[value.into()]);
        self.bcx.switch_to_block(failure);
        self.bcx.seal_block(failure);
        let payload = self.sum_payload(value);
        let widened = self.widen_sum(
            payload,
            &from.get().variants[1].fields[0],
            &to.get().variants[1].fields[0],
        );
        let result = self.wrap_sum(widened, 1);
        self.bcx.ins().jump(done, &[result.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        self.bcx.block_params(done)[0]
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
        if ty.wide() || matches!(ty, Ty::Ref(..)) {
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
        let tags = self.high_word(value);
        self.bcx.ins().band_imm(tags, 1)
    }
    fn sum_payload(&mut self, value: cranelift_codegen::ir::Value) -> cranelift_codegen::ir::Value {
        self.tagged_payload(value, 1)
    }
    fn tagged_payload(
        &mut self,
        value: cranelift_codegen::ir::Value,
        bits: u32,
    ) -> cranelift_codegen::ir::Value {
        let (word, tags) = self.bcx.ins().isplit(value);
        let tags = self.bcx.ins().ushr_imm(tags, bits as i64);
        self.bcx.ins().iconcat(word, tags)
    }
    pub(super) fn wrap_sum(
        &mut self,
        value: cranelift_codegen::ir::Value,
        tag: usize,
    ) -> cranelift_codegen::ir::Value {
        self.wrap_tagged(value, tag, 1)
    }
    fn wrap_tagged(
        &mut self,
        value: cranelift_codegen::ir::Value,
        tag: usize,
        bits: u32,
    ) -> cranelift_codegen::ir::Value {
        let (word, tags) = self.bcx.ins().isplit(value);
        let tags = self.bcx.ins().ishl_imm(tags, bits as i64);
        let tags = self.bcx.ins().bor_imm(tags, tag as i64);
        self.bcx.ins().iconcat(word, tags)
    }
    pub(super) fn lower_try(
        &mut self,
        source: &crate::nominal::Nominal<crate::sum::EnumType>,
        target: &crate::nominal::Nominal<crate::sum::EnumType>,
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
            self.release(payload, &source.get().variants[1].fields[0]);
            let zero = self.bcx.ins().iconst(types::I64, 0);
            let marker = self.bcx.ins().uextend(types::I128, zero);
            self.wrap_sum(marker, 1)
        } else {
            let payload = if source.is_result() {
                self.widen_sum(
                    payload,
                    &source.get().variants[1].fields[0],
                    &target.get().variants[1].fields[0],
                )
            } else {
                payload
            };
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
        let ty = source.get().variants[success_tag].fields[0].clone();
        let payload = self.unpack(payload, &ty);
        self.stack.push((payload, ty));
        Ok(())
    }

    /// Both outcomes are built from registers and one is selected, so a
    /// checked conversion neither branches nor calls the runtime.
    pub(super) fn lower_try_cast(&mut self, target: &Ty) -> Result<()> {
        let (value, source) = self
            .stack
            .pop()
            .ok_or("AOT: stack underflow on checked cast")?;
        let (Some((source_min, source_end)), Some((target_min, target_end))) =
            (source.int_range(), target.int_range())
        else {
            return Err("AOT: checked cast requires integer types".into());
        };
        let Ty::Enum(error) = crate::sum::range_error() else {
            unreachable!()
        };
        let Ty::Enum(result) = crate::sum::result(target.clone(), Ty::Enum(error.clone())) else {
            unreachable!()
        };
        // A bound the source type cannot cross needs no comparison. One that
        // it can cross lies inside the source range, so it is representable
        // as a constant of the source type.
        let width = clif_type(source.clone());
        let signed = is_signed(source.clone());
        let mut fits = None;
        if source_min < target_min {
            let min = self.bcx.ins().iconst(width, target_min as i64);
            fits = Some(
                self.bcx
                    .ins()
                    .icmp(IntCC::SignedGreaterThanOrEqual, value, min),
            );
        }
        if source_end > target_end {
            let max = self.bcx.ins().iconst(width, (target_end - 1) as i64);
            let below = self.bcx.ins().icmp(
                if signed {
                    IntCC::SignedLessThanOrEqual
                } else {
                    IntCC::UnsignedLessThanOrEqual
                },
                value,
                max,
            );
            fits = Some(match fits {
                Some(above) => self.bcx.ins().band(above, below),
                None => below,
            });
        }
        let converted = self.cast(value, source, target.clone());
        self.stack.push((converted, target.clone()));
        self.lower_enum(&EnumOp::New(result.clone(), 0))?;
        let Some(fits) = fits else {
            return Ok(());
        };
        let (ok, output) = self.stack.pop().ok_or("AOT: missing checked cast result")?;
        let out_of_range = error
            .get()
            .variants
            .iter()
            .position(|variant| variant.name == "OutOfRange")
            .ok_or("AOT: RangeError has no OutOfRange variant")?;
        self.lower_enum(&EnumOp::New(error, out_of_range))?;
        self.lower_enum(&EnumOp::New(result, 1))?;
        let (failed, _) = self.stack.pop().ok_or("AOT: missing checked cast error")?;
        let chosen = self.bcx.ins().select(fits, ok, failed);
        self.stack.push((chosen, output));
        Ok(())
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_box(&mut self, op: &crate::boxed::BoxOp) -> Result<()> {
        use crate::boxed::BoxOp;
        let (inputs, output) = op.signature();
        let (operand, _) = self.pop_typed(inputs[0].clone())?;
        let result = match op {
            BoxOp::New(t) => {
                // The runtime moves the value into the box, or drops it on failure.
                let value = self.pack(operand, t);
                let boxed = Ty::Box(std::sync::Arc::new(t.clone()));
                self.collection_call(119, &[value], Some(&boxed))?
            }
            BoxOp::Take(t) => {
                let slot = self.bcx.ins().iadd_imm(operand, 32);
                let value = self.bcx.ins().load(
                    types::I128,
                    cranelift_codegen::ir::MemFlags::trusted(),
                    slot,
                    0,
                );
                let value = self.unpack(value, t);
                let value = self.snapshot_inline(value, t);
                self.collection_call(120, &[operand], None)?;
                self.pack(value, t)
            }
            BoxOp::Ref(..) => {
                // A reference to the box's slot becomes one to its content slot.
                let address = self.raw_word(operand);
                let boxed = self.bcx.ins().load(
                    PTR_TY,
                    cranelift_codegen::ir::MemFlags::trusted(),
                    address,
                    0,
                );
                let slot = self.bcx.ins().iadd_imm(boxed, 32);
                self.bcx.ins().uextend(types::I128, slot)
            }
        };
        let result = self.unpack(result, &output);
        self.stack.push((result, output));
        Ok(())
    }
}
