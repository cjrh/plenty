//! Inline range ownership. Runtime operands stay scalar-sized addresses, while
//! every escaping value is copied into storage belonging to its new owner.
use super::*;
use cranelift_codegen::ir::{MemFlags, Value as NativeValue};

impl Lowerer<'_, '_> {
    pub(super) fn range_storage(&mut self, bytes: usize) -> NativeValue {
        let slot = self
            .bcx
            .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                bytes as u32,
                4,
            ));
        self.bcx.ins().stack_addr(PTR_TY, slot, 0)
    }

    pub(super) fn copy_range_to(
        &mut self,
        value: NativeValue,
        ty: &Ty,
        destination: NativeValue,
    ) -> NativeValue {
        if !ty.has_inline_range() {
            return value;
        }
        let packed = self.pack(value, ty);
        let copied = self
            .collection_call(117, &[packed, destination], Some(ty))
            .expect("inline range metadata");
        self.unpack(copied, ty)
    }

    pub(super) fn snapshot_range(&mut self, value: NativeValue, ty: &Ty) -> NativeValue {
        if !ty.has_inline_range() {
            return value;
        }
        let destination = self.range_storage(32);
        self.copy_range_to(value, ty, destination)
    }

    pub(super) fn store_slot(&mut self, slot: NativeValue, value: NativeValue, ty: &Ty) {
        let value = if ty.has_inline_range() {
            let destination = self.bcx.ins().iadd_imm(slot, 16);
            self.copy_range_to(value, ty, destination)
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
        let mut offset = 0;
        let mut returned = Vec::new();
        for (value, ty) in values {
            let value = if ty.has_inline_range() {
                let base = self
                    .return_range
                    .expect("caller-owned range return storage");
                let destination = self.bcx.ins().iadd_imm(base, offset);
                offset += 32;
                self.copy_range_to(value, &ty, destination)
            } else {
                value
            };
            returned.push(value);
        }
        self.release_locals();
        self.bcx.ins().return_(&returned);
    }
}
