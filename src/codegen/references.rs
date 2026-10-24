//! Internal references carry a slot address and an inline-sum tag offset.
//! A payload projection keeps the original storage alive through its loan;
//! it never manufactures a pointer to a temporary unpacked value.
use super::*;
use cranelift_codegen::ir::{
    InstructionData, MemFlagsData, Opcode, Value as NativeValue, ValueDef,
};

impl Lowerer<'_, '_> {
    /// The slot address, and the tag shift unless the reference was just
    /// made from an address (a local or a list row), which projects nothing.
    /// Cranelift does not see through `isplit` of a `uextend`, so without
    /// this every read would keep a dead shift-and-select.
    fn reference_parts(&mut self, reference: NativeValue) -> (NativeValue, Option<NativeValue>) {
        if let ValueDef::Result(inst, _) = self.bcx.func.dfg.value_def(reference)
            && let InstructionData::Unary {
                opcode: Opcode::Uextend,
                arg,
            } = self.bcx.func.dfg.insts[inst]
        {
            return (arg, None);
        }
        let (address, shift) = self.bcx.ins().isplit(reference);
        (address, Some(shift))
    }

    /// The low payload word of a referenced slot: the owner of a referenced
    /// collection, for helpers that take only that. Targets are
    /// little-endian, so it is the slot's first eight bytes.
    pub(super) fn reference_word(&mut self, reference: NativeValue) -> NativeValue {
        let (address, _) = self.reference_parts(reference);
        self.bcx
            .ins()
            .load(types::I64, MemFlagsData::trusted(), address, 0)
    }

    pub(super) fn read_reference(&mut self, reference: NativeValue) -> NativeValue {
        let (address, shift) = self.reference_parts(reference);
        let packed = self
            .bcx
            .ins()
            .load(types::I128, MemFlagsData::trusted(), address, 0);
        let Some(shift) = shift else {
            return packed;
        };
        let (word, tags) = self.bcx.ins().isplit(packed);
        // The deepest legal projection (64) produces zero, which a 64-bit
        // shift, masked to 0, would not.
        let tags = self.bcx.ins().ushr(tags, shift);
        let projected = self
            .bcx
            .ins()
            .icmp_imm_s(IntCC::UnsignedLessThan, shift, 64);
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let tags = self.bcx.ins().select(projected, tags, zero);
        self.bcx.ins().iconcat(word, tags)
    }

    pub(super) fn write_reference(&mut self, reference: NativeValue, value: NativeValue, ty: &Ty) {
        let (address, shift) = self.bcx.ins().isplit(reference);
        let old = self
            .bcx
            .ins()
            .load(types::I128, MemFlagsData::trusted(), address, 0);
        let value = if ty.has_inline_storage() {
            let destination = self.bcx.ins().iadd_imm_s(address, 16);
            self.copy_inline_to(value, ty, destination)
        } else {
            value
        };
        let packed = self.pack(value, ty);
        let (word, tags) = self.bcx.ins().isplit(packed);
        // At the deepest projection (64) every old tag bit is an ancestor's.
        let projected = self
            .bcx
            .ins()
            .icmp_imm_s(IntCC::UnsignedLessThan, shift, 64);
        let tags = self.bcx.ins().ishl(tags, shift);
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let tags = self.bcx.ins().select(projected, tags, zero);
        let one = self.bcx.ins().iconst(types::I64, 1);
        let mask = self.bcx.ins().ishl(one, shift);
        let mask = self.bcx.ins().iadd_imm_s(mask, -1);
        let all = self.bcx.ins().iconst(types::I64, -1);
        let mask = self.bcx.ins().select(projected, mask, all);
        let (_, old_tags) = self.bcx.ins().isplit(old);
        let ancestors = self.bcx.ins().band(old_tags, mask);
        let tags = self.bcx.ins().bor(tags, ancestors);
        let packed = self.bcx.ins().iconcat(word, tags);
        self.bcx
            .ins()
            .store(MemFlagsData::trusted(), packed, address, 0);
    }
}
