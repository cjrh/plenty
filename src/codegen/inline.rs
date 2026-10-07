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
        let packed = self.pack(value, ty);
        let copied = self
            .collection_call(117, &[packed, destination], Some(ty))
            .expect("inline payload metadata");
        self.unpack(copied, ty)
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
        self.release_locals();
        self.bcx.ins().return_(&returned);
    }
}
