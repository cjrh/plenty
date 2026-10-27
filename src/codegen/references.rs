//! Internal references carry a slot address and an inline-sum tag offset.
//! A payload projection keeps the original storage alive through its loan;
//! it never manufactures a pointer to a temporary unpacked value.
//!
//! A reference may address a frame slot, a list row, a class field or a
//! payload inside another slot, so these accesses set no Cranelift alias
//! region: any partition would let a load move past a store to the same
//! bytes (issue #440, `tests/test_memory_order.rs`).
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

    /// Scalar references can address packed list storage as well as ordinary
    /// slots. Read only the declared width; the high slot word is not present
    /// in a packed list and is irrelevant to a scalar payload projection.
    pub(super) fn scalar_word(&mut self, address: NativeValue, ty: &Ty) -> NativeValue {
        let width = types::Type::int(clif_type(ty.clone()).bits() as u16).unwrap();
        let bits = self
            .bcx
            .ins()
            .load(width, MemFlagsData::trusted(), address, 0);
        if width == types::I64 {
            bits
        } else if is_signed(ty.clone()) {
            self.bcx.ins().sextend(types::I64, bits)
        } else {
            self.bcx.ins().uextend(types::I64, bits)
        }
    }

    pub(super) fn scalar_reference_word(&mut self, reference: NativeValue, ty: &Ty) -> NativeValue {
        let (address, _) = self.reference_parts(reference);
        self.scalar_word(address, ty)
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
        let (address, shift) = self.reference_parts(reference);
        if ty.is_numeric() || *ty == Ty::Bool {
            // A projected scalar keeps its ancestor tags untouched. Locals and
            // record fields likewise need only their declared payload bytes.
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), value, address, 0);
            return;
        }
        let old = shift.map(|_| {
            self.bcx
                .ins()
                .load(types::I128, MemFlagsData::trusted(), address, 0)
        });
        let value = if ty.has_inline_storage() {
            let destination = self.bcx.ins().iadd_imm_s(address, 16);
            self.copy_inline_to(value, ty, destination)
        } else {
            value
        };
        let packed = self.pack(value, ty);
        // A reference made from an address projects nothing and owns every
        // tag bit of its slot: the general path, with a zero shift, would
        // keep no old bits, so the slot takes the packed value as is.
        let (Some(shift), Some(old)) = (shift, old) else {
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), packed, address, 0);
            return;
        };
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
