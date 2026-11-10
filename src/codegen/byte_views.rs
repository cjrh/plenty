//! `Bytes.get` and `Bytes.set` in a loop that reads the buffer's length and
//! first-byte address once (issue #843). The frontend allows this only for
//! loops that cannot resize or replace the buffer; see `IndexedLists`.
use super::*;
use crate::collection::CollectionOp;
use cranelift_codegen::ir::{MemFlagsData, Value as NativeValue};

impl Lowerer<'_, '_> {
    /// An index the loop proves in range reads or writes the byte directly.
    /// Otherwise one unsigned comparison with the cached length guards that
    /// access, and a failing index calls the method itself, which traps with
    /// its own message, site and caller note.
    pub(super) fn lower_bytes_view(&mut self, operation: &CollectionOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let mut args: Vec<(NativeValue, Ty)> = Vec::with_capacity(inputs.len());
        for ty in inputs.iter().rev() {
            args.push(self.pop_typed(ty.clone())?);
        }
        args.reverse();
        let (rows, _) = args.pop().unwrap();
        let (length, _) = args.pop().unwrap();
        let index = args[1].0;
        let value = args.get(2).map(|(value, _)| *value);
        let proven = (self.nonnegative_operand(index)
            && self
                .counters
                .bounds
                .below(&self.bcx.func.dfg, index, length))
            || self.versioned_index(index, length);
        if proven {
            let result = self.byte_view_access(rows, index, value);
            self.stack.push((result, output));
            return Ok(());
        }
        let fast = self.bcx.create_block();
        let slow = self.bcx.create_block();
        // Unsigned, so a negative index also fails.
        let in_range = self.bcx.ins().icmp(IntCC::UnsignedLessThan, index, length);
        let before = self.bcx.current_block();
        self.bcx.ins().brif(in_range, fast, &[], slow, &[]);
        // Only `before` reaches `fast`, so its row checks still hold there;
        // the failing path ends in a trap and must not disturb them.
        let checked = std::mem::take(&mut self.counters.checked);
        self.bcx.set_cold_block(slow);
        self.bcx.switch_to_block(slow);
        self.bcx.seal_block(slow);
        self.stack.extend(args);
        // The loop cannot resize the buffer, so the method sees the cached
        // length and traps on this index; nothing is live across the call.
        // Kept a call, so this block has no successor and the allocator
        // places its argument moves here instead of in the loop.
        self.lower_call_out_of_line(&operation.bytes_view_method().unwrap())?;
        if value.is_none() {
            self.pop_typed(output.clone())?;
        }
        self.bcx.ins().trap(TrapCode::unwrap_user(1));
        self.bcx.switch_to_block(fast);
        self.bcx.seal_block(fast);
        self.counters.checked = checked;
        self.counters.checked.continue_to(before, fast);
        let result = self.byte_view_access(rows, index, value);
        self.stack.push((result, output));
        Ok(())
    }

    /// The byte at `index` from `rows`, or the unit result of storing
    /// `value` there.
    fn byte_view_access(
        &mut self,
        rows: NativeValue,
        index: NativeValue,
        value: Option<NativeValue>,
    ) -> NativeValue {
        let address = self.bcx.ins().iadd(rows, index);
        let flags = MemFlagsData::trusted();
        match value {
            Some(value) => {
                self.bcx.ins().store(flags, value, address, 0);
                self.unit_value()
            }
            None => self.bcx.ins().load(types::I8, flags, address, 0),
        }
    }

    /// `len` and indexing of a `bytes` value without a runtime call. Every
    /// `bytes` value is the address of a literal in the immortal text layout
    /// (`string_literal_bytes`): the byte count at offset 16, the bytes from
    /// offset 32. Indexing follows the list index rules and reports a
    /// failure as the runtime's `plenty_byte_str_item` does.
    pub(super) fn lower_byte_literal(&mut self, operation: &CollectionOp) -> Result<()> {
        let index = match operation {
            CollectionOp::Len(_) => None,
            _ => Some(self.pop_typed(Ty::I64)?.0),
        };
        let (literal, _) = self.pop_typed(Ty::ByteStr)?;
        // The data is never written, so repeated reads share one load.
        let flags = MemFlagsData::trusted().with_readonly();
        let rows = |lower: &mut Self| lower.bcx.ins().iadd_imm_s(literal, 32);
        // Iteration's index runs from zero to below the literal's length.
        if let (CollectionOp::IterGet(_), Some(index)) = (operation, index) {
            let rows = rows(self);
            let address = self.bcx.ins().iadd(rows, index);
            let byte = self.bcx.ins().load(types::I8, flags, address, 0);
            self.stack.push((byte, Ty::U8));
            return Ok(());
        }
        let known = self.bcx.func.dfg.resolve_aliases(literal);
        let length = match self.counters.byte_literals.get(&known) {
            Some(&length) => self.bcx.ins().iconst(types::I64, length),
            None => self.bcx.ins().load(types::I64, flags, literal, 16),
        };
        let Some(index) = index else {
            self.stack.push((length, Ty::I64));
            return Ok(());
        };
        let rows = rows(self);
        let address = self.checked_row(rows, length, index, 1)?;
        let byte = self.bcx.ins().load(types::I8, flags, address, 0);
        self.stack.push((byte, Ty::U8));
        Ok(())
    }

    fn unit_value(&mut self) -> NativeValue {
        self.bcx.ins().iconst(clif_type(Ty::Unit), 0)
    }
}
