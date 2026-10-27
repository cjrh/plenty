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

    /// Read a value of type `ty` through `reference`, packed. A class with a
    /// record may be a flat field with no value word, so its value is the
    /// address of the record that follows the (possibly virtual) slot.
    pub(super) fn read_typed_reference(&mut self, reference: NativeValue, ty: &Ty) -> NativeValue {
        if ty.is_numeric() || *ty == Ty::Bool {
            return self.scalar_reference_word(reference, ty);
        }
        if ty.flat_field() {
            let (address, _) = self.reference_parts(reference);
            let record = self.referenced_record(address);
            return self.bcx.ins().uextend(types::I128, record);
        }
        self.read_reference(reference)
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
        if ty.flat_field() {
            // Every slot of such a class holds its record's address, so only
            // the record changes; a flat field has no word to store. An empty
            // value, left by a field move, clears the record: zero bytes own
            // nothing.
            let destination = self.referenced_record(address);
            self.write_record(value, ty, destination);
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

    /// Copy the class value `value` into the record storage at `destination`,
    /// or zero that storage when `value` is empty.
    pub(super) fn write_record(&mut self, value: NativeValue, ty: &Ty, destination: NativeValue) {
        let bytes = ty.inline_bytes() as u64;
        if known::known_zero(&self.bcx.func.dfg, value) {
            self.zero_bytes(destination, bytes);
            return;
        }
        let packed = self.pack(value, ty);
        let word = self.raw_word(packed);
        let copy = self.bcx.create_block();
        let clear = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.ins().brif(word, copy, &[], clear, &[]);
        self.bcx.switch_to_block(copy);
        self.bcx.seal_block(copy);
        self.copy_inline_to(value, ty, destination);
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(clear);
        self.bcx.seal_block(clear);
        self.zero_bytes(destination, bytes);
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
    }
}
