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
        self.bcx.ins().brif(in_range, fast, &[], slow, &[]);
        self.bcx.set_cold_block(slow);
        self.bcx.switch_to_block(slow);
        self.bcx.seal_block(slow);
        self.stack.extend(args);
        // The loop cannot resize the buffer, so the method sees the cached
        // length and traps on this index; nothing is live across the call.
        self.lower_call(&operation.bytes_view_method().unwrap())?;
        if value.is_none() {
            self.pop_typed(output.clone())?;
        }
        self.bcx.ins().trap(TrapCode::unwrap_user(1));
        self.bcx.switch_to_block(fast);
        self.bcx.seal_block(fast);
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

    fn unit_value(&mut self) -> NativeValue {
        self.bcx.ins().iconst(clif_type(Ty::Unit), 0)
    }
}
