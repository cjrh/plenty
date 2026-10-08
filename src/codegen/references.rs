//! Internal references carry a slot address and an inline-sum tag offset.
//! A payload projection keeps the original storage alive through its loan;
//! it never manufactures a pointer to a temporary unpacked value.
use super::*;
use cranelift_codegen::ir::{MemFlags, Value as NativeValue};

impl Lowerer<'_, '_> {
    pub(super) fn read_reference(&mut self, reference: NativeValue) -> NativeValue {
        let address = self.raw_word(reference);
        let shift = self.bcx.ins().ushr_imm(reference, 64);
        let packed = self
            .bcx
            .ins()
            .load(types::I128, MemFlags::trusted(), address, 0);
        let word = self.raw_word(packed);
        let word = self.bcx.ins().uextend(types::I128, word);
        let tags = self.bcx.ins().ushr_imm(packed, 64);
        // Shift in 128 bits so the deepest legal projection (64) produces zero.
        let tags = self.bcx.ins().ushr(tags, shift);
        let tags = self.bcx.ins().ishl_imm(tags, 64);
        self.bcx.ins().bor(word, tags)
    }

    pub(super) fn write_reference(&mut self, reference: NativeValue, value: NativeValue, ty: &Ty) {
        let address = self.raw_word(reference);
        let shift = self.bcx.ins().ushr_imm(reference, 64);
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
        let word = self.raw_word(packed);
        let word = self.bcx.ins().uextend(types::I128, word);
        let tags = self.bcx.ins().ushr_imm(packed, 64);
        let tags = self.bcx.ins().ishl(tags, shift);
        let tags = self.bcx.ins().ishl_imm(tags, 64);
        let one = self.bcx.ins().iconst(types::I64, 1);
        let one = self.bcx.ins().uextend(types::I128, one);
        let mask = self.bcx.ins().ishl(one, shift);
        let mask = self.bcx.ins().iadd_imm(mask, -1);
        let mask = self.bcx.ins().ishl_imm(mask, 64);
        let ancestors = self.bcx.ins().band(old, mask);
        let tags = self.bcx.ins().bor(tags, ancestors);
        let packed = self.bcx.ins().bor(word, tags);
        self.bcx
            .ins()
            .store(MemFlags::trusted(), packed, address, 0);
    }
}
