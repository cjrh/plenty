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
            cranelift_codegen::ir::MemFlagsData::new(),
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

    /// Byte offset of a field inside a multi-field variant's record.
    pub(super) fn record_offset<'t>(fields: impl Iterator<Item = &'t Ty>) -> i64 {
        fields.map(|t| t.field_bytes() as i64).sum()
    }

    /// Fresh stack storage for a record. Zeroed slots own nothing, so a
    /// partially initialized class can be released safely.
    fn record_storage(&mut self, bytes: usize) -> cranelift_codegen::ir::Value {
        if bytes == 0 {
            return self.bcx.ins().iconst(PTR_TY, 0);
        }
        let storage = self.inline_storage(bytes);
        self.zero_bytes(storage, bytes as u64);
        storage
    }

    /// Load the field at `offset` of a class or multi-field variant record,
    /// copying its own inline storage out of the record so it survives the
    /// record's release. Also returns the field's place, its slot or, for a
    /// flat field, its record, whose address is the field's value.
    pub(super) fn record_field(
        &mut self,
        record: cranelift_codegen::ir::Value,
        offset: i64,
        ty: &Ty,
    ) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        let storage = self.raw_word(record);
        let place = self.bcx.ins().iadd_imm_s(storage, offset);
        let value = if ty.flat_field() {
            self.bcx.ins().uextend(types::I128, place)
        } else {
            self.bcx.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlagsData::trusted(),
                place,
                0,
            )
        };
        let value = self.unpack(value, ty);
        (self.snapshot_inline(value, ty), place)
    }

    /// Leave the field `record_field` returned the place of empty: a zero
    /// slot word, or a flat record of zero bytes, which owns nothing.
    fn clear_field(&mut self, place: cranelift_codegen::ir::Value, ty: &Ty) {
        if ty.flat_field() {
            self.zero_bytes(place, ty.inline_bytes() as u64);
            return;
        }
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let zero = self.bcx.ins().uextend(types::I128, zero);
        self.bcx.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            zero,
            place,
            0,
        );
    }

    /// The record of the class instance a reference addresses. A record
    /// always follows its slot, and a flat field's reference addresses the
    /// virtual slot before its record, so this needs no load.
    pub(super) fn referenced_record(
        &mut self,
        address: cranelift_codegen::ir::Value,
    ) -> cranelift_codegen::ir::Value {
        self.bcx.ins().iadd_imm_s(address, 16)
    }

    /// Load the field at `offset` of a class value, copying its own inline
    /// storage out of the record so it survives the record's release. A flat
    /// field's value is its record's address.
    pub(super) fn class_field(
        &mut self,
        owner: cranelift_codegen::ir::Value,
        offset: i64,
        ty: &Ty,
    ) -> cranelift_codegen::ir::Value {
        self.record_field(owner, offset, ty).0
    }

    pub(super) fn lower_class(&mut self, op: &crate::record::ClassOp) -> Result<()> {
        use crate::record::ClassOp;
        let (inputs, output) = op.signature().ok_or("invalid class operation")?;
        let newtype = match op {
            ClassOp::New(t)
            | ClassOp::ArmDrop(t)
            | ClassOp::Field(t, _)
            | ClassOp::FieldRef(t, ..) => t.get().newtype_field().is_some(),
        };
        // A newtype is its field: nothing to allocate, arm, project or
        // release. The `unpack` below converts between the class's 128-bit
        // slot and the field's native type.
        let result = match op {
            // A newtype around a class record is filled through a reference
            // like that record, so it needs the record's storage: a class
            // slot's word always addresses the record after it.
            ClassOp::New(t) if newtype && Ty::Class(t.clone()).flat_field() => {
                let storage = self.record_storage(Ty::Class(t.clone()).inline_bytes());
                self.bcx.ins().uextend(types::I128, storage)
            }
            ClassOp::New(_) if newtype => {
                let zero = self.bcx.ins().iconst(types::I64, 0);
                self.bcx.ins().uextend(types::I128, zero)
            }
            ClassOp::ArmDrop(_) if newtype => {
                self.pop_typed(inputs[0].clone())?;
                self.bcx.ins().iconst(types::I64, 0)
            }
            ClassOp::Field(..) | ClassOp::FieldRef(..) if newtype => {
                self.pop_typed(inputs[0].clone())?.0
            }
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
                    let storage = self.referenced_record(address);
                    let one = self.bcx.ins().iconst(types::I64, 1);
                    self.bcx.ins().store(
                        cranelift_codegen::ir::MemFlagsData::trusted(),
                        one,
                        storage,
                        0,
                    );
                }
                self.bcx.ins().iconst(types::I64, 0)
            }
            ClassOp::Field(t, i) => {
                let (owner, _) = self.pop_typed(inputs[0].clone())?;
                let offset = t.get().field_offset(*i) as i64;
                let value = self.class_field(owner, offset, &output);
                self.retain(value, &output);
                self.release(owner, &inputs[0]);
                self.pack(value, &output)
            }
            ClassOp::FieldRef(t, i, _) => {
                let (address, _) = self.pop_typed(inputs[0].clone())?;
                let address = self.raw_word(address);
                let owner = self.referenced_record(address);
                let mut offset = t.get().field_offset(*i) as i64;
                // A flat field has no slot: its reference addresses the
                // virtual slot just before its record.
                if t.get().fields[*i].1.flat_field() {
                    offset -= 16;
                }
                self.bcx.ins().iadd_imm_s(owner, offset)
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
            offset += field.field_bytes() as i64;
            self.stack.push((value, field));
        }
        Ok(())
    }

    /// Move every field out of a class record. The record is inline storage
    /// and the class has no `__del__`, so nothing is released.
    pub(super) fn lower_split_class(
        &mut self,
        t: &crate::nominal::Nominal<crate::record::ClassType>,
    ) -> Result<()> {
        let (record, _) = self.pop_typed(Ty::Class(t.clone()))?;
        let definition = t.get();
        for (index, (_, field)) in definition.fields.iter().enumerate() {
            let offset = definition.field_offset(index) as i64;
            let (value, _) = self.record_field(record, offset, field);
            self.stack.push((value, field.clone()));
        }
        Ok(())
    }

    /// Whether `value` is the copy the previous op loaded from a local slot;
    /// if so, the instructions that counted it are removed, and the caller
    /// reads it in place and does not release it. A `match` reads its owned
    /// scrutinee's tag this way before an arm moves the value (issue #388).
    fn forget_counted_copy(&mut self, value: cranelift_codegen::ir::Value) -> bool {
        match self.fresh_load.take() {
            Some(super::FreshLoad {
                value: fresh,
                counting,
                ..
            }) if fresh == value && !counting.is_empty() => {
                for inst in counting {
                    self.bcx.func.layout.remove_inst(inst);
                }
                true
            }
            _ => false,
        }
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
                    self.bcx.ins().band_imm_s(tags, mask as i64)
                }
                EnumOp::FieldRef(_, tag, field, _) => {
                    let fields = &t.get().variants[*tag].fields;
                    if fields.len() == 1 {
                        let (address, shift) = self.bcx.ins().isplit(reference);
                        let shift = self.bcx.ins().iadd_imm_s(shift, t.tag_bits() as i64);
                        self.bcx.ins().iconcat(address, shift)
                    } else {
                        // The payload word in the slot addresses the record.
                        let address = self.raw_word(reference);
                        let storage = self.bcx.ins().load(
                            PTR_TY,
                            cranelift_codegen::ir::MemFlagsData::trusted(),
                            address,
                            0,
                        );
                        let mut offset = Self::record_offset(fields[..*field].iter());
                        // A flat field has no slot: its reference addresses
                        // the virtual slot just before its record.
                        if fields[*field].flat_field() {
                            offset -= 16;
                        }
                        let slot = self.bcx.ins().iadd_imm_s(storage, offset);
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
                        .icmp_imm_s(IntCC::NotEqual, tag, i64::from(t.is_option()));
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
                        let bytes = Self::record_offset(fields.iter()) as usize;
                        let storage = self.inline_storage(bytes);
                        let mut offset = 0;
                        for (value, ty) in values.into_iter().zip(fields) {
                            let place = self.bcx.ins().iadd_imm_s(storage, offset);
                            if ty.flat_field() {
                                self.write_record(value, ty, place);
                            } else {
                                self.store_slot(place, value, ty);
                            }
                            offset += ty.field_bytes() as i64;
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
                let tag = self.bcx.ins().band_imm_s(tags, mask as i64);
                if !self.forget_counted_copy(value) {
                    self.release(value, &inputs[0]);
                }
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
                    let (field_value, place) = self.record_field(payload, offset, &output);
                    if matches!(op, EnumOp::Take(..)) {
                        self.clear_field(place, &output);
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
            if let Ty::Enum(from) = source
                && from.anonymous()
            {
                let tags = self.high_word(value);
                let mask = (from.get().variants.len().next_power_of_two() - 1).max(1);
                let old_tag = self.bcx.ins().band_imm_s(tags, mask as i64);
                let mut tag = self.bcx.ins().iconst(types::I64, 0);
                for (index, member) in crate::sum::members(source).iter().enumerate() {
                    let destination = targets.iter().position(|t| t == member).unwrap();
                    let selected = self
                        .bcx
                        .ins()
                        .icmp_imm_s(IntCC::Equal, old_tag, index as i64);
                    let new_tag = self.bcx.ins().iconst(types::I64, destination as i64);
                    tag = self.bcx.ins().select(selected, new_tag, tag);
                }
                let payload = self.tagged_payload(value, from.tag_bits());
                let wrapped = self.wrap_tagged(payload, 0, to.tag_bits());
                let (word, tags) = self.bcx.ins().isplit(wrapped);
                let tags = self.bcx.ins().bor(tags, tag);
                return self.bcx.ins().iconcat(word, tags);
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
        let ok = self.bcx.ins().icmp_imm_s(IntCC::Equal, tag, 0);
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
            return self.bcx.ins().bitcast(
                target,
                cranelift_codegen::ir::MemFlagsData::new(),
                bits,
            );
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
        self.bcx.ins().band_imm_s(tags, 1)
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
        let tags = self.bcx.ins().ushr_imm_s(tags, bits as i64);
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
        let tags = self.bcx.ins().ishl_imm_s(tags, bits as i64);
        let tags = self.bcx.ins().bor_imm_s(tags, tag as i64);
        self.bcx.ins().iconcat(word, tags)
    }
    pub(super) fn lower_try(
        &mut self,
        source: &crate::nominal::Nominal<crate::sum::EnumType>,
        target: &crate::nominal::Nominal<crate::sum::EnumType>,
        cleanup: &Arc<[Op]>,
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
            .icmp_imm_s(IntCC::Equal, tag, success_tag as i64);
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
        self.leave(vec![(residual, Ty::Enum(target.clone()))], cleanup)?;
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
            BoxOp::New(t) | BoxOp::Share(t) => {
                // The runtime moves the value into the box, or drops it on failure.
                let value = self.pack(operand, t);
                let boxed = Ty::Box(std::sync::Arc::new(t.clone()));
                self.collection_call(119, &[value], Some(&boxed))?
            }
            BoxOp::Take(t) => {
                let slot = self.bcx.ins().iadd_imm_s(operand, 32);
                let value = self.bcx.ins().load(
                    types::I128,
                    cranelift_codegen::ir::MemFlagsData::trusted(),
                    slot,
                    0,
                );
                let value = self.unpack(value, t);
                let value = self.snapshot_inline(value, t);
                self.collection_call(120, &[operand], None)?;
                self.pack(value, t)
            }
            BoxOp::Ref(..) | BoxOp::SharedRef(_) => {
                // A reference to the box's slot becomes one to its content slot.
                let address = self.raw_word(operand);
                let boxed = self.bcx.ins().load(
                    PTR_TY,
                    cranelift_codegen::ir::MemFlagsData::trusted(),
                    address,
                    0,
                );
                let slot = self.bcx.ins().iadd_imm_s(boxed, 32);
                self.bcx.ins().uextend(types::I128, slot)
            }
        };
        let result = self.unpack(result, &output);
        self.stack.push((result, output));
        Ok(())
    }
}
