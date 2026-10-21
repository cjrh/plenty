//! Inline value storage. Runtime operands carry scalar-sized addresses, while
//! escaping ranges and generator frames live in storage belonging to their owner.
use super::*;
use cranelift_codegen::ir::{MemFlags, Value as NativeValue};

impl Lowerer<'_, '_> {
    pub(super) fn inline_storage(&mut self, bytes: usize) -> NativeValue {
        let slot = self
            .bcx
            .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                bytes as u32,
                4,
            ));
        self.bcx.ins().stack_addr(PTR_TY, slot, 0)
    }

    pub(super) fn copy_inline_to(
        &mut self,
        value: NativeValue,
        ty: &Ty,
        destination: NativeValue,
    ) -> NativeValue {
        if !ty.has_inline_storage() {
            return value;
        }
        if ty.flat_record() {
            return self.copy_flat_record(value, ty, destination);
        }
        if !ty.wide()
            && let Some(bytes) = plain_bytes(ty)
        {
            return self.copy_words(value, destination, bytes);
        }
        // A null value word owns no inline payload, and the runtime returns
        // such a value unchanged. Testing it here keeps calls out of the
        // common payload-free cases, such as `Ok(())` returned as a
        // `Result[(), E]` whose error holds a record (issue #121).
        let packed = self.pack(value, ty);
        let word = self.raw_word(packed);
        let copy = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, types::I128);
        self.bcx.ins().brif(word, copy, &[], done, &[packed.into()]);
        self.bcx.switch_to_block(copy);
        self.bcx.seal_block(copy);
        let copied = if let Ty::Enum(t) = ty
            && let Some(variants) = plain_variants(t)
        {
            self.copy_plain_variant(packed, destination, t.tag_bits(), &variants)
        } else {
            self.collection_call(117, &[packed, destination], Some(ty))
                .expect("inline payload metadata")
        };
        self.bcx.ins().jump(done, &[copied.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        let result = self.bcx.block_params(done)[0];
        self.unpack(result, ty)
    }

    /// Copy the payload of a sum value whose payload word is not null, given
    /// the bytes each variant with inline storage keeps (see
    /// [`plain_variants`]). Other variants own nothing inline and keep their
    /// value. The tags stay as they are, as in the runtime's `copy_payload`.
    fn copy_plain_variant(
        &mut self,
        packed: NativeValue,
        destination: NativeValue,
        tag_bits: u32,
        variants: &[(usize, u64)],
    ) -> NativeValue {
        let (word, tags) = self.bcx.ins().isplit(packed);
        let tag = self.bcx.ins().band_imm(tags, (1i64 << tag_bits) - 1);
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, types::I128);
        let config = self.module.target_config();
        for &(variant, bytes) in variants {
            let copy = self.bcx.create_block();
            let next = self.bcx.create_block();
            let matches = self.bcx.ins().icmp_imm(IntCC::Equal, tag, variant as i64);
            self.bcx.ins().brif(matches, copy, &[], next, &[]);
            self.bcx.switch_to_block(copy);
            self.bcx.seal_block(copy);
            self.bcx.emit_small_memory_copy(
                config,
                destination,
                word,
                bytes,
                8,
                8,
                false,
                MemFlags::trusted(),
            );
            let moved = self.bcx.ins().iconcat(destination, tags);
            self.bcx.ins().jump(done, &[moved.into()]);
            self.bcx.switch_to_block(next);
            self.bcx.seal_block(next);
        }
        self.bcx.ins().jump(done, &[packed.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        self.bcx.block_params(done)[0]
    }

    /// Copy a record whose fields keep no inline data of their own: its bytes
    /// need no relocation. A null value word owns nothing and is not copied.
    fn copy_flat_record(
        &mut self,
        value: NativeValue,
        ty: &Ty,
        destination: NativeValue,
    ) -> NativeValue {
        let source = self.raw_word(value);
        let copy = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, types::I128);
        self.bcx
            .ins()
            .brif(source, copy, &[], done, &[value.into()]);
        self.bcx.switch_to_block(copy);
        self.bcx.seal_block(copy);
        let config = self.module.target_config();
        self.bcx.emit_small_memory_copy(
            config,
            destination,
            source,
            ty.inline_bytes() as u64,
            8,
            8,
            false,
            MemFlags::trusted(),
        );
        let tags = self.high_word(value);
        let moved = self.bcx.ins().iconcat(destination, tags);
        self.bcx.ins().jump(done, &[moved.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        self.bcx.block_params(done)[0]
    }

    /// Copy a payload of `bytes` plain bytes with no internal addresses to
    /// relocate, such as a range's words: copying a stored range for a loop or
    /// an argument then needs no runtime call (issue #371). A null value word
    /// owns nothing and is not copied.
    fn copy_words(
        &mut self,
        value: NativeValue,
        destination: NativeValue,
        bytes: u64,
    ) -> NativeValue {
        let copy = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, PTR_TY);
        self.bcx.ins().brif(value, copy, &[], done, &[value.into()]);
        self.bcx.switch_to_block(copy);
        self.bcx.seal_block(copy);
        let config = self.module.target_config();
        self.bcx.emit_small_memory_copy(
            config,
            destination,
            value,
            bytes,
            8,
            8,
            false,
            MemFlags::trusted(),
        );
        self.bcx.ins().jump(done, &[destination.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        self.bcx.block_params(done)[0]
    }

    pub(super) fn snapshot_inline(&mut self, value: NativeValue, ty: &Ty) -> NativeValue {
        if !ty.has_inline_storage() {
            return value;
        }
        let destination = self.inline_storage(ty.inline_bytes());
        self.copy_inline_to(value, ty, destination)
    }

    pub(super) fn store_slot(&mut self, slot: NativeValue, value: NativeValue, ty: &Ty) {
        let value = if ty.has_inline_storage() {
            let destination = self.bcx.ins().iadd_imm(slot, 16);
            self.copy_inline_to(value, ty, destination)
        } else {
            value
        };
        let packed = self.pack(value, ty);
        self.bcx.ins().store(MemFlags::trusted(), packed, slot, 0);
    }

    pub(super) fn local_offset(&self, index: usize) -> i64 {
        self.locals[..index]
            .iter()
            .map(|(_, t)| t.slot_bytes() as i64)
            .sum()
    }

    pub(super) fn return_values(&mut self, values: Vec<StackEntry>) {
        let returned = self.stage_results(values);
        self.release_locals();
        self.bcx.ins().return_(&returned);
    }

    fn stage_results(&mut self, values: Vec<StackEntry>) -> Vec<NativeValue> {
        let mut offset = 0;
        let mut returned = Vec::new();
        for (value, ty) in values {
            let value = if ty.has_inline_storage() {
                let base = self
                    .return_storage
                    .expect("caller-owned inline return storage");
                let destination = self.bcx.ins().iadd_imm(base, offset);
                offset += ty.inline_bytes() as i64;
                self.copy_inline_to(value, &ty, destination)
            } else {
                value
            };
            returned.push(value);
        }
        returned
    }
}

/// The bytes of a payload that moves without relocation: no field keeps inline
/// storage, so no internal address points into it. `None` when copying needs
/// the runtime's descriptor walk.
fn plain_bytes(ty: &Ty) -> Option<u64> {
    match ty {
        Ty::Range(_) => Some(32),
        // An environment holds its descriptor and capture slots.
        Ty::Closure(t) if t.captures.iter().all(|(_, t)| !t.has_inline_storage()) => {
            Some(t.bytes() as u64)
        }
        Ty::Class(_) if ty.flat_record() => Some(ty.inline_bytes() as u64),
        _ => None,
    }
}

/// For a sum, each variant with inline storage and the bytes its payload keeps,
/// when every such payload is plain: a single field with [`plain_bytes`], or
/// several fields none of which keeps inline storage. A `Result[Bytes, E]`
/// then copies without a runtime call (issue #384).
fn plain_variants(ty: &crate::nominal::Nominal<crate::sum::EnumType>) -> Option<Vec<(usize, u64)>> {
    let mut variants = Vec::new();
    for (tag, variant) in ty.get().variants.iter().enumerate() {
        match variant.fields.as_slice() {
            [] => {}
            [field] if !field.has_inline_storage() => {}
            [field] => variants.push((tag, plain_bytes(field)?)),
            fields => {
                if fields.iter().any(Ty::has_inline_storage) {
                    return None;
                }
                variants.push((tag, 16 * fields.len() as u64));
            }
        }
    }
    Some(variants)
}
