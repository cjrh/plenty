//! Internal references carry a slot address and an inline-sum tag offset.
//! A payload projection keeps the original storage alive through its loan;
//! it never manufactures a pointer to a temporary unpacked value.
use super::*;
use cranelift_codegen::ir::{MemFlags, Value as NativeValue};

impl Lowerer<'_, '_> {
    pub(super) fn read_reference(&mut self, reference: NativeValue) -> NativeValue {
        let (address, shift) = self.bcx.ins().isplit(reference);
        let packed = self
            .bcx
            .ins()
            .load(types::I128, MemFlags::trusted(), address, 0);
        let (word, tags) = self.bcx.ins().isplit(packed);
        // The deepest legal projection (64) produces zero, which a 64-bit
        // shift, masked to 0, would not.
        let tags = self.bcx.ins().ushr(tags, shift);
        let projected = self.bcx.ins().icmp_imm(IntCC::UnsignedLessThan, shift, 64);
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let tags = self.bcx.ins().select(projected, tags, zero);
        self.bcx.ins().iconcat(word, tags)
    }

    pub(super) fn write_reference(&mut self, reference: NativeValue, value: NativeValue, ty: &Ty) {
        let (address, shift) = self.bcx.ins().isplit(reference);
        let old = self
            .bcx
            .ins()
            .load(types::I128, MemFlags::trusted(), address, 0);
        let value = if ty.has_inline_storage() {
            let destination = self.bcx.ins().iadd_imm(address, 16);
            self.copy_inline_to(value, ty, destination)
        } else {
            value
        };
        let packed = self.pack(value, ty);
        let (word, tags) = self.bcx.ins().isplit(packed);
        // At the deepest projection (64) every old tag bit is an ancestor's.
        let projected = self.bcx.ins().icmp_imm(IntCC::UnsignedLessThan, shift, 64);
        let tags = self.bcx.ins().ishl(tags, shift);
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let tags = self.bcx.ins().select(projected, tags, zero);
        let one = self.bcx.ins().iconst(types::I64, 1);
        let mask = self.bcx.ins().ishl(one, shift);
        let mask = self.bcx.ins().iadd_imm(mask, -1);
        let all = self.bcx.ins().iconst(types::I64, -1);
        let mask = self.bcx.ins().select(projected, mask, all);
        let (_, old_tags) = self.bcx.ins().isplit(old);
        let ancestors = self.bcx.ins().band(old_tags, mask);
        let tags = self.bcx.ins().bor(tags, ancestors);
        let packed = self.bcx.ins().iconcat(word, tags);
        self.bcx
            .ins()
            .store(MemFlags::trusted(), packed, address, 0);
    }
}
