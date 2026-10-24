//! `std.atomic` lowering. An atomic's payload word is the low word of its
//! slot, so a reference's address is the operand address; the tag shift is
//! irrelevant to a scalar. Every access is a Cranelift atomic instruction of
//! the element's exact width: a plain load could be merged across another
//! thread's store, and `write_reference` rewrites the whole 128-bit slot.
//! Cranelift's atomics are all sequentially consistent, which strengthens
//! every requested `Ordering`.
use super::*;
use crate::atomic::{AtomicOp, Modify};
use cranelift_codegen::ir::{AtomicRmwOp, MemFlagsData, Value as NativeValue};

impl Lowerer<'_, '_> {
    pub(super) fn lower_atomic(&mut self, operation: &AtomicOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let mut values = Vec::new();
        for input in inputs.iter().rev() {
            values.push(self.pop_typed(input.clone())?.0);
        }
        values.reverse();
        let flags = MemFlagsData::trusted();
        let result = match operation {
            AtomicOp::New(_) | AtomicOp::IntoInner(_) => values[0],
            AtomicOp::Fence => {
                self.bcx.ins().fence();
                self.bcx.ins().iconst(types::I64, 0)
            }
            AtomicOp::Load(t) => {
                let address = self.raw_word(values[0]);
                self.bcx
                    .ins()
                    .atomic_load(clif_type(t.clone()), flags, address)
            }
            AtomicOp::Store(_) => {
                let address = self.raw_word(values[0]);
                self.bcx.ins().atomic_store(flags, values[1], address);
                self.bcx.ins().iconst(types::I64, 0)
            }
            AtomicOp::Modify(t, Modify::Nand) if *t == Ty::Bool => {
                let address = self.raw_word(values[0]);
                self.bool_nand(address, values[1])
            }
            AtomicOp::Modify(t, modify) => {
                let address = self.raw_word(values[0]);
                let signed = is_signed(t.clone());
                let op = match modify {
                    Modify::Swap => AtomicRmwOp::Xchg,
                    Modify::WrappingAdd => AtomicRmwOp::Add,
                    Modify::WrappingSub => AtomicRmwOp::Sub,
                    Modify::And => AtomicRmwOp::And,
                    Modify::Or => AtomicRmwOp::Or,
                    Modify::Xor => AtomicRmwOp::Xor,
                    Modify::Nand => AtomicRmwOp::Nand,
                    Modify::Max if signed => AtomicRmwOp::Smax,
                    Modify::Max => AtomicRmwOp::Umax,
                    Modify::Min if signed => AtomicRmwOp::Smin,
                    Modify::Min => AtomicRmwOp::Umin,
                };
                self.bcx
                    .ins()
                    .atomic_rmw(clif_type(t.clone()), flags, op, address, values[1])
            }
            AtomicOp::Checked(t, subtract) => {
                let address = self.raw_word(values[0]);
                self.checked_fetch(t, *subtract, address, values[1])?
            }
            AtomicOp::CompareExchange(_) => {
                let address = self.raw_word(values[0]);
                let old = self
                    .bcx
                    .ins()
                    .atomic_cas(flags, address, values[1], values[2]);
                let failed = self.bcx.ins().icmp(IntCC::NotEqual, old, values[1]);
                let word = self.pack(old, &inputs[1]);
                let word = self.raw_word(word);
                // Result's one tag bit: 0 is `Ok`, 1 is `Err`.
                let tag = self.bcx.ins().uextend(types::I64, failed);
                self.bcx.ins().iconcat(word, tag)
            }
        };
        let result = if matches!(output, Ty::Enum(_)) {
            self.unpack(result, &output)
        } else {
            result
        };
        self.stack.push((result, output));
        Ok(())
    }

    /// `fetch_add`/`fetch_sub` with the ordinary overflow trap: compute the
    /// checked result from the value last seen, then publish it only if the
    /// slot still holds that value.
    fn checked_fetch(
        &mut self,
        ty: &Ty,
        subtract: bool,
        address: NativeValue,
        operand: NativeValue,
    ) -> Result<NativeValue> {
        let width = clif_type(ty.clone());
        let flags = MemFlagsData::trusted();
        let first = self.bcx.ins().atomic_load(width, flags, address);
        let retry = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(retry, width);
        self.bcx.append_block_param(done, width);
        self.bcx.ins().jump(retry, &[first.into()]);
        self.bcx.switch_to_block(retry);
        let seen = self.bcx.block_params(retry)[0];
        let (next, overflow) = match (subtract, is_signed(ty.clone())) {
            (false, true) => self.bcx.ins().sadd_overflow(seen, operand),
            (false, false) => self.bcx.ins().uadd_overflow(seen, operand),
            (true, true) => self.bcx.ins().ssub_overflow(seen, operand),
            (true, false) => self.bcx.ins().usub_overflow(seen, operand),
        };
        self.trap_if(overflow, TrapKind::Overflow)?;
        let actual = self.bcx.ins().atomic_cas(flags, address, seen, next);
        let stored = self.bcx.ins().icmp(IntCC::Equal, actual, seen);
        self.bcx
            .ins()
            .brif(stored, done, &[seen.into()], retry, &[actual.into()]);
        self.bcx.seal_block(retry);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        Ok(self.bcx.block_params(done)[0])
    }

    /// Bitwise nand would set a bool's seven high bits. `!(old & true)` is
    /// `!old`, and `!(old & false)` is `true`.
    fn bool_nand(&mut self, address: NativeValue, operand: NativeValue) -> NativeValue {
        let flags = MemFlagsData::trusted();
        let invert = self.bcx.create_block();
        let set = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, types::I8);
        self.bcx.ins().brif(operand, invert, &[], set, &[]);
        let one = |lowerer: &mut Self| lowerer.bcx.ins().iconst(types::I8, 1);
        for (block, op) in [(invert, AtomicRmwOp::Xor), (set, AtomicRmwOp::Xchg)] {
            self.bcx.switch_to_block(block);
            self.bcx.seal_block(block);
            let value = one(self);
            let old = self
                .bcx
                .ins()
                .atomic_rmw(types::I8, flags, op, address, value);
            self.bcx.ins().jump(done, &[old.into()]);
        }
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        self.bcx.block_params(done)[0]
    }
}
