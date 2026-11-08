use super::*;
use crate::collection::{CollectionOp, Query};

/// The trailing site argument a narrow runtime helper takes.
#[derive(Clone, Copy)]
enum Site {
    None,
    Trap,
    Null,
}

impl Lowerer<'_, '_> {
    /// The address of row `index` of the rows at `rows`, each `row_bytes`
    /// wide. A negative `index` counts from the end, as Python does, and an
    /// index out of `0..length` traps with the runtime's message and this
    /// site.
    ///
    /// Two choices keep the hot path free of register copies (issue #720).
    /// An index known nonnegative needs no normalization, so the in-range
    /// path uses it directly. Otherwise the two paths join on the row's
    /// address, not the normalized index: the index usually stays live after
    /// the access, and a joined copy of it would occupy a second register,
    /// while the hot address dies at the join.
    ///
    /// A check already done in this block or a dominating one, for the same
    /// rows, length and index values, is reused (issue #721): it cannot fail
    /// now if it passed then, so trap behavior is unchanged.
    pub(super) fn checked_row(
        &mut self,
        rows: cranelift_codegen::ir::Value,
        length: cranelift_codegen::ir::Value,
        index: cranelift_codegen::ir::Value,
        row_bytes: i64,
    ) -> Result<cranelift_codegen::ir::Value> {
        if self.assumed_index {
            return Ok(self.row_address(rows, index, row_bytes));
        }
        let dfg = &self.bcx.func.dfg;
        let known = super::known::known_signed(dfg, index);
        let key = super::induction::RowKey {
            rows: dfg.resolve_aliases(rows),
            length: dfg.resolve_aliases(length),
            index: known.ok_or_else(|| dfg.resolve_aliases(index)),
        };
        let block = self.bcx.current_block();
        if let Some(address) = self.counters.checked.find(block, key, row_bytes) {
            return Ok(address);
        }
        if (known.map_or_else(|| self.nonnegative_operand(index), |constant| constant >= 0)
            && self.counters.bounds.below(dfg, index, length))
            || self.versioned_index(index, length)
        {
            return Ok(self.row_address(rows, index, row_bytes));
        }
        let failure = self.bcx.create_block();
        let valid = self.bcx.create_block();
        let normalized = match known {
            Some(constant) if constant < 0 => Some(self.bcx.ins().iadd(index, length)),
            Some(_) => Some(index),
            None if self.nonnegative_operand(index) => Some(index),
            None => None,
        };
        let joined = match normalized {
            // A constant or nonnegative index needs only one test.
            Some(normalized) => {
                let in_range = self
                    .bcx
                    .ins()
                    .icmp(IntCC::UnsignedLessThan, normalized, length);
                self.bcx.ins().brif(in_range, valid, &[], failure, &[]);
                None
            }
            None => {
                let joined = self.bcx.append_block_param(valid, PTR_TY);
                let hot = self.row_address(rows, index, row_bytes);
                let negative = self.index_hot_test(index, length, valid, &[hot.into()]);
                let (adjusted, in_range) = self.index_adjust(index, length, negative);
                let cold = self.row_address(rows, adjusted, row_bytes);
                self.bcx
                    .ins()
                    .brif(in_range, valid, &[cold.into()], failure, &[]);
                Some(joined)
            }
        };
        self.index_failure(failure, index, length)?;
        self.bcx.switch_to_block(valid);
        self.bcx.seal_block(valid);
        // Computed in the block that uses it, a single-test address folds
        // into the access's addressing mode.
        let address = match joined {
            Some(joined) => joined,
            None => self.row_address(rows, normalized.unwrap_or(index), row_bytes),
        };
        self.counters
            .checked
            .record(block, valid, key, row_bytes, address);
        Ok(address)
    }

    /// Whether the versioned loop copy being lowered may skip the check of
    /// `index` against `length`, recording the test that copy then needs;
    /// see [`super::induction::Version`].
    pub(super) fn versioned_index(
        &mut self,
        index: cranelift_codegen::ir::Value,
        length: cranelift_codegen::ir::Value,
    ) -> bool {
        use super::induction::{Assumed, LengthOf};
        let dfg = &self.bcx.func.dfg;
        let bounds = &self.counters.bounds;
        let Some(version) = &bounds.version else {
            return false;
        };
        let canonical_length = bounds.canonical(dfg, length);
        let length = if version.entries.get(&canonical_length) == Some(&false) {
            LengthOf::Length(canonical_length)
        } else if let Some(&view) = bounds.lengths.get(&canonical_length)
            && version.entries.get(&view) == Some(&true)
        {
            LengthOf::View(view)
        } else {
            return false;
        };
        let (base, k) = super::induction::offset(dfg, index);
        let below_stop = bounds
            .below
            .get(&base)
            .is_some_and(|&bound| Some(bounds.canonical(dfg, bound)) == version.stop);
        let (assumed, k) = if (0..=super::induction::VERSION_OFFSET).contains(&k)
            && below_stop
            && self.nonnegative_operand(base)
        {
            (Assumed::Counter(length), k)
        } else if let Some(constant) = super::known::known_signed(dfg, index) {
            if constant < 0 {
                return false;
            }
            (Assumed::Constant(constant, length), 0)
        } else if let Some(max) = super::known::zero_extended_max(dfg, index)
            .filter(|&max| max <= super::induction::VERSION_EXTENT)
        {
            // A zero-extended byte or u16, as in a 256-entry table lookup
            // `table[i64(byte)]`: any index below the largest one it can
            // hold is below the length too (issue #760).
            (Assumed::Constant(max as i64, length), 0)
        } else {
            // The same value on every pass: the test before the loop is
            // this access's check.
            let index = bounds.canonical(dfg, index);
            if version.entries.get(&index) != Some(&false) {
                return false;
            }
            (Assumed::Index(index, length), 0)
        };
        let version = self.counters.bounds.version.as_mut().unwrap();
        match version.assumed.iter_mut().find(|(a, _)| *a == assumed) {
            Some((_, most)) => *most = (*most).max(k),
            None => version.assumed.push((assumed, k)),
        }
        true
    }

    /// `rows + index * row_bytes`. A shift lets x86-64 fold a power-of-two
    /// row size into the address.
    pub(super) fn row_address(
        &mut self,
        rows: cranelift_codegen::ir::Value,
        index: cranelift_codegen::ir::Value,
        row_bytes: i64,
    ) -> cranelift_codegen::ir::Value {
        let offset = if row_bytes.count_ones() == 1 {
            self.bcx.ins().ishl_imm_u(index, row_bytes.ilog2() as i64)
        } else {
            self.bcx.ins().imul_imm_s(index, row_bytes)
        };
        self.bcx.ins().iadd(rows, offset)
    }

    /// Reads (`store` is `None`) or writes the scalar row `index` of the
    /// rows at `rows`, checked against `length` by [`Self::checked_row`]. A
    /// read returns the unpacked element.
    fn scalar_row_access(
        &mut self,
        rows: cranelift_codegen::ir::Value,
        length: cranelift_codegen::ir::Value,
        index: cranelift_codegen::ir::Value,
        element: &Ty,
        store: Option<cranelift_codegen::ir::Value>,
    ) -> Result<Option<cranelift_codegen::ir::Value>> {
        let row_bytes = clif_type(element.clone()).bytes() as i64;
        let address = self.checked_row(rows, length, index, row_bytes)?;
        Ok(match store {
            Some(value) => {
                let flags = cranelift_codegen::ir::MemFlagsData::trusted();
                self.bcx.ins().store(flags, value, address, 0);
                None
            }
            None => Some(self.scalar_value(address, element, element)),
        })
    }

    /// Branches to `valid` with `args` when `index` is in 0..length, the hot
    /// path: one unsigned comparison, no normalization (issue #715). Returns
    /// the cold block taken otherwise.
    pub(super) fn index_hot_test(
        &mut self,
        index: cranelift_codegen::ir::Value,
        length: cranelift_codegen::ir::Value,
        valid: cranelift_codegen::ir::Block,
        args: &[cranelift_codegen::ir::BlockArg],
    ) -> cranelift_codegen::ir::Block {
        let in_range = self.bcx.ins().icmp(IntCC::UnsignedLessThan, index, length);
        let negative = self.bcx.create_block();
        self.bcx.ins().brif(in_range, valid, args, negative, &[]);
        negative
    }

    /// In the cold block `negative`, the normalized index `index + length`
    /// and whether it is in range. Length is at most isize::MAX, so for a
    /// negative index the wrapped sum is below length, unsigned, exactly when
    /// index >= -length; for a nonnegative index >= length the sum cannot
    /// wrap and stays >= length.
    pub(super) fn index_adjust(
        &mut self,
        index: cranelift_codegen::ir::Value,
        length: cranelift_codegen::ir::Value,
        negative: cranelift_codegen::ir::Block,
    ) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        self.bcx.switch_to_block(negative);
        self.bcx.set_cold_block(negative);
        self.bcx.seal_block(negative);
        let adjusted = self.bcx.ins().iadd(index, length);
        let in_range = self
            .bcx
            .ins()
            .icmp(IntCC::UnsignedLessThan, adjusted, length);
        (adjusted, in_range)
    }

    /// Fills the cold block `failure`, every predecessor of which is now
    /// known: it reports `index` out of bounds for `length` at this site and
    /// traps.
    ///
    /// The block stores the index and length to the scratch slot and passes
    /// its address. Passed as arguments, they would need the first argument
    /// registers at the call, and the register allocator then keeps the hot
    /// path's values there too, adding moves to every iteration of a loop
    /// that indexes a list. A trap does not return, so the slot is free.
    fn index_failure(
        &mut self,
        failure: cranelift_codegen::ir::Block,
        index: cranelift_codegen::ir::Value,
        length: cranelift_codegen::ir::Value,
    ) -> Result<()> {
        self.bcx.switch_to_block(failure);
        self.bcx.set_cold_block(failure);
        self.bcx.seal_block(failure);
        let slot = self.scratch_slot(16);
        let record = self.bcx.ins().stack_addr(PTR_TY, slot, 0);
        let flags = cranelift_codegen::ir::MemFlagsData::trusted();
        self.bcx.ins().store(flags, index, record, 0);
        self.bcx.ins().store(flags, length, record, 8);
        let site = self.trap_site()?;
        let helper = self
            .module
            .declare_func_in_func(self.runtime.trap_list_index, self.bcx.func);
        self.bcx.ins().call(helper, &[record, site]);
        self.bcx.ins().trap(TrapCode::unwrap_user(3));
        Ok(())
    }

    /// The row count of the live scalar list `owner`: a scalar row uses its
    /// declared width.
    pub(super) fn scalar_list_len(
        &mut self,
        owner: cranelift_codegen::ir::Value,
        element: &Ty,
    ) -> cranelift_codegen::ir::Value {
        self.dense_list_len(owner, i64::from(clif_type(element.clone()).bytes()))
    }

    /// The row count of the live list `owner` whose rows are `row_bytes`
    /// apart, read through `list_layout` as `Entries::len` computes it for a
    /// dense list: a popped row stays stored until the next mutation. The
    /// divisor is a constant, so Cranelift turns the division into a shift or
    /// a multiplication.
    pub(super) fn dense_list_len(
        &mut self,
        owner: cranelift_codegen::ir::Value,
        row_bytes: i64,
    ) -> cranelift_codegen::ir::Value {
        use crate::list_layout::{BYTES, REMOVED};
        let flags = cranelift_codegen::ir::MemFlagsData::trusted();
        let bytes = self.bcx.ins().load(types::I64, flags, owner, BYTES);
        let removed = self.bcx.ins().uload8(types::I64, flags, owner, REMOVED);
        let stored = if row_bytes.count_ones() == 1 {
            self.bcx.ins().ushr_imm_u(bytes, row_bytes.ilog2() as i64)
        } else {
            let divisor = self.bcx.ins().iconst(types::I64, row_bytes);
            self.bcx.ins().udiv(bytes, divisor)
        };
        self.bcx.ins().isub(stored, removed)
    }

    /// Exchanges two scalar rows of a list or slice view in place. Both
    /// indices are checked before either row is written, so a trap leaves the
    /// rows unchanged. Rows move as raw bits of their declared width; equal
    /// indices store each value back.
    pub(super) fn swap_scalar_rows(
        &mut self,
        rows: cranelift_codegen::ir::Value,
        length: cranelift_codegen::ir::Value,
        indices: [cranelift_codegen::ir::Value; 2],
        row_bytes: i64,
        element: &Ty,
    ) -> Result<()> {
        let [first, second] = indices;
        let first = self.checked_row(rows, length, first, row_bytes)?;
        let second = self.checked_row(rows, length, second, row_bytes)?;
        let flags = cranelift_codegen::ir::MemFlagsData::trusted();
        let width = types::Type::int(clif_type(element.clone()).bits() as u16).unwrap();
        let a = self.bcx.ins().load(width, flags, first, 0);
        let b = self.bcx.ins().load(width, flags, second, 0);
        self.bcx.ins().store(flags, b, first, 0);
        self.bcx.ins().store(flags, a, second, 0);
        Ok(())
    }

    /// Pops the loop's cached row-0 address and length above a cached row
    /// access's other operands (`ListScalarGetView`, `ListScalarSetView`,
    /// `ElementRefView`).
    fn cached_rows(
        &mut self,
        operation: &CollectionOp,
    ) -> Result<Option<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value)>> {
        if !matches!(
            operation,
            CollectionOp::ListScalarGetView(_)
                | CollectionOp::ListScalarSetView(_)
                | CollectionOp::ElementRefView(..)
        ) {
            return Ok(None);
        }
        let (rows, _) = self.pop_typed(Ty::I64)?;
        let (length, _) = self.pop_typed(Ty::I64)?;
        Ok(Some((rows, length)))
    }

    /// The row-0 address and length of the list behind `receiver`: the
    /// loop's `cached` values, or read from its header. With a cache, the
    /// loop's loan or ownership holds the list, and the receiver is unused.
    fn dense_rows(
        &mut self,
        cached: Option<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value)>,
        receiver: cranelift_codegen::ir::Value,
        list: &Ty,
    ) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        if let Some(cached) = cached {
            return cached;
        }
        let owner = self.reference_word(receiver);
        let flags = cranelift_codegen::ir::MemFlagsData::trusted();
        let rows = self
            .bcx
            .ins()
            .load(PTR_TY, flags, owner, crate::list_layout::ROWS);
        let row_bytes = metadata::list_row_bytes(&list.element().unwrap()) as i64;
        (rows, self.dense_list_len(owner, row_bytes))
    }

    /// A runtime helper that answers `operation` directly: no argument block,
    /// descriptor, or caller frame, because it never calls back into Plenty
    /// (issue #364). It takes the receiver word (`true`: read through the shared
    /// reference the frontend keeps under its loan; `false`: owned and released
    /// here), the remaining inputs as words, and then the site, if any.
    fn narrow_helper(&self, operation: &CollectionOp) -> Option<(FuncId, bool, Site)> {
        use crate::bytes::BytesOp;
        let runtime = &self.runtime;
        Some(match operation {
            CollectionOp::Len(Ty::List(_)) => (runtime.list_len, false, Site::None),
            CollectionOp::Get(Ty::List(element)) | CollectionOp::IterGet(Ty::List(element))
                if element.is_numeric() || **element == Ty::Bool =>
            {
                let site = match operation {
                    CollectionOp::Get(_) => Site::Trap,
                    _ => Site::Null,
                };
                (runtime.list_scalar_get, false, site)
            }
            CollectionOp::Bytes(BytesOp::Item) => (runtime.bytes_item, true, Site::Trap),
            CollectionOp::Bytes(BytesOp::SetItem) => (runtime.bytes_set_item, true, Site::Trap),
            CollectionOp::Bytes(BytesOp::ValueItem) => (runtime.bytes_item, false, Site::Trap),
            CollectionOp::Bytes(BytesOp::Get) => (runtime.bytes_get, false, Site::None),
            CollectionOp::Bytes(BytesOp::Set) => (runtime.bytes_set, false, Site::None),
            CollectionOp::Bytes(BytesOp::Len) => (runtime.bytes_len, false, Site::None),
            CollectionOp::Len(Ty::Str) => (runtime.text_len, false, Site::None),
            CollectionOp::TextByteLen => (runtime.text_byte_len, false, Site::None),
            CollectionOp::TextIsAscii => (runtime.text_is_ascii, false, Site::None),
            CollectionOp::Get(Ty::Str) | CollectionOp::TextIndex => {
                (runtime.text_at, false, Site::Trap)
            }
            CollectionOp::IterGet(Ty::Str) => (runtime.text_at, false, Site::Null),
            // A `bytes` literal is immortal, so its release is a no-op.
            CollectionOp::Get(Ty::ByteStr) => (runtime.byte_str_item, false, Site::Trap),
            CollectionOp::IterGet(Ty::ByteStr) => (runtime.byte_str_item, false, Site::Null),
            CollectionOp::Len(Ty::ByteStr) => (runtime.text_byte_len, false, Site::None),
            CollectionOp::Text(crate::text::TextOp::FindIn) => {
                (runtime.text_find_in, true, Site::None)
            }
            CollectionOp::QueryRef(query) => {
                let helper = match query {
                    Query::BytesLen => runtime.bytes_len,
                    Query::BytesRows => runtime.byte_view_rows,
                    Query::TextLen => runtime.text_len,
                    Query::TextByteLen => runtime.text_byte_len,
                    Query::TextIsAscii => runtime.text_is_ascii,
                    Query::TextAsciiEqualFolded => runtime.text_ascii_equal_folded,
                    Query::TextAtByte => runtime.text_at_byte,
                    Query::TextNextByte => runtime.text_next_byte,
                    Query::TextPrevByte => runtime.text_prev_byte,
                    Query::TextAt => return Some((runtime.text_at, true, Site::Trap)),
                    Query::TextOrd => unreachable!("ord builds its Result inline"),
                };
                (helper, true, Site::None)
            }
            _ => return None,
        })
    }

    /// Whether `receiver` is the copy the previous op loaded from a local
    /// slot; if so, its retain is removed, and the caller reads it as
    /// borrowed and does not release it. Nothing ran between the load and
    /// this op, so the slot still holds the value through the helper call.
    fn forget_fresh_load(&mut self, receiver: cranelift_codegen::ir::Value) -> bool {
        match self.fresh_load.take() {
            Some(super::FreshLoad {
                value,
                retain: Some(retain),
                ..
            }) if value == receiver => {
                self.bcx.func.layout.remove_inst(retain);
                true
            }
            _ => false,
        }
    }

    /// Whether `value` is one of the [`slot_copies`](super::Lowerer::slot_copies);
    /// if so, its retain is removed, and the caller reads it as borrowed and
    /// does not release it. Each entry answers one operand.
    pub(super) fn forget_slot_copy(&mut self, value: cranelift_codegen::ir::Value) -> bool {
        let Some(i) = self
            .slot_copies
            .iter()
            .rposition(|(copy, _)| *copy == value)
        else {
            return false;
        };
        if let (_, Some(retain)) = self.slot_copies.remove(i) {
            self.bcx.func.layout.remove_inst(retain);
        }
        true
    }

    /// `startswith`, `endswith`, `find`, `rfind`, and `count` over two
    /// texts call one narrow helper each (issue #387). `find` and `rfind`
    /// return `u64::MAX` for no match, and the `Option` is built here with
    /// the dispatcher's encoding: the index in the low word, tag 1 for
    /// `Some` in the high word.
    fn lower_text_search(&mut self, operation: &CollectionOp) -> Result<()> {
        let helper = match operation {
            CollectionOp::TextStartsWith => self.runtime.text_starts_with,
            CollectionOp::TextEndsWith => self.runtime.text_ends_with,
            CollectionOp::TextFind => self.runtime.text_find,
            CollectionOp::TextRFind => self.runtime.text_rfind,
            CollectionOp::TextCount => self.runtime.text_count,
            _ => unreachable!("not a text search"),
        };
        let (_, output) = operation.signature();
        let (needle, _) = self.pop_typed(Ty::Str)?;
        let (text, _) = self.pop_typed(Ty::Str)?;
        let needle_held = self.forget_slot_copy(needle);
        let text_held = self.forget_slot_copy(text);
        let function = self.module.declare_func_in_func(helper, self.bcx.func);
        let call = self.bcx.ins().call(function, &[text, needle]);
        let found = self.bcx.inst_results(call)[0];
        if !text_held {
            self.release(text, &Ty::Str);
        }
        if !needle_held {
            self.release(needle, &Ty::Str);
        }
        let result = match operation {
            CollectionOp::TextFind | CollectionOp::TextRFind => {
                let b = &mut self.bcx;
                let missing = b.ins().icmp_imm_s(IntCC::Equal, found, -1);
                let zero = b.ins().iconst(types::I64, 0);
                let low = b.ins().select(missing, zero, found);
                let some = b.ins().icmp_imm_s(IntCC::NotEqual, found, -1);
                let high = b.ins().uextend(types::I64, some);
                let result = b.ins().iconcat(low, high);
                self.unpack(result, &output)
            }
            _ => self.unpack(found, &output),
        };
        self.stack.push((result, output));
        Ok(())
    }

    /// `ord(text)` (issue #379): the helper returns the code point, or
    /// `u64::MAX` when the text is not one scalar, and the `Result` is built
    /// here with the dispatcher's encoding: `Ok(code)` in the low word, or
    /// tag 1 over `ParseError` variant 0 in the high word.
    fn lower_text_ord(&mut self, operation: &CollectionOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let (receiver, ty) = self.pop_typed(inputs[0].clone())?;
        let reference = matches!(ty, Ty::Ref(..));
        let borrowed = reference || self.forget_fresh_load(receiver);
        let text = if reference {
            self.reference_word(receiver)
        } else {
            receiver
        };
        let function = self
            .module
            .declare_func_in_func(self.runtime.text_ord, self.bcx.func);
        let call = self.bcx.ins().call(function, &[text]);
        let code = self.bcx.inst_results(call)[0];
        if !borrowed {
            self.release(receiver, &ty);
        }
        let b = &mut self.bcx;
        let failed = b.ins().icmp_imm_s(IntCC::Equal, code, -1);
        let zero = b.ins().iconst(types::I64, 0);
        let low = b.ins().select(failed, zero, code);
        let high = b.ins().uextend(types::I64, failed);
        let result = b.ins().iconcat(low, high);
        let result = self.unpack(result, &output);
        self.stack.push((result, output));
        Ok(())
    }

    /// `Bytes.push` (issue #389): the helper appends through the exclusive
    /// field reference and returns the high word of the dispatcher's
    /// `Result[(), AllocError]`; the low word is always zero.
    fn lower_bytes_push(&mut self, operation: &CollectionOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let (value, _) = self.pop_typed(Ty::U8)?;
        let (receiver, _) = self.pop_typed(inputs[0].clone())?;
        let owner = self.reference_word(receiver);
        let value = self.bcx.ins().uextend(types::I64, value);
        let function = self
            .module
            .declare_func_in_func(self.runtime.bytes_push, self.bcx.func);
        let call = self.bcx.ins().call(function, &[owner, value]);
        let high = self.bcx.inst_results(call)[0];
        let low = self.bcx.ins().iconst(types::I64, 0);
        let result = self.bcx.ins().iconcat(low, high);
        let result = self.unpack(result, &output);
        self.stack.push((result, output));
        Ok(())
    }

    pub(super) fn collection_call(
        &mut self,
        opcode: i64,
        values: &[cranelift_codegen::ir::Value],
        ty: Option<&Ty>,
    ) -> Result<cranelift_codegen::ir::Value> {
        self.collection_call_at(opcode, values, ty, None)
    }

    /// The element count of `range(start, stop, step)`, computed inline for
    /// counted loops. The frontend passes a `NonZero[i64]` step's integer.
    /// An oversized length takes a cold call to the range constructor, so
    /// its failure keeps its message and site.
    pub(super) fn lower_range_length(&mut self, element: &Ty) -> Result<()> {
        let (step, _) = self.pop_typed(Ty::I64)?;
        let (stop, _) = self.pop_typed(element.clone())?;
        let (start, _) = self.pop_typed(element.clone())?;
        let signed = is_signed(element.clone());
        let narrow = clif_type(element.clone()) != types::I64;
        let b = &mut self.bcx;
        let widen = |b: &mut FunctionBuilder<'_>, value| match (narrow, signed) {
            (false, _) => value,
            (true, true) => b.ins().sextend(types::I64, value),
            (true, false) => b.ins().uextend(types::I64, value),
        };
        let first = widen(b, start);
        let bound = widen(b, stop);
        let forward = b.ins().icmp_imm_s(IntCC::SignedGreaterThan, step, 0);
        let low = b.ins().select(forward, first, bound);
        let high = b.ins().select(forward, bound, first);
        let order = if signed {
            IntCC::SignedLessThan
        } else {
            IntCC::UnsignedLessThan
        };
        let nonempty = b.ins().icmp(order, low, high);
        // Both bounds fit in 64 bits, so their distance is exact as u64, and
        // so is the magnitude of `i64::MIN`.
        let distance = b.ins().isub(high, low);
        let negated = b.ins().ineg(step);
        let magnitude = b.ins().select(forward, step, negated);
        let one = b.ins().iconst(types::I64, 1);
        // Cranelift keeps a division by a constant 1, and most loops use the
        // default step.
        let unit = match b.func.dfg.value_def(step) {
            cranelift_codegen::ir::ValueDef::Result(inst, _) => matches!(
                b.func.dfg.insts[inst],
                cranelift_codegen::ir::InstructionData::UnaryImm {
                    opcode: cranelift_codegen::ir::Opcode::Iconst,
                    imm,
                } if imm.bits() == 1
            ),
            _ => false,
        };
        let count = if unit {
            distance
        } else {
            let last = b.ins().isub(distance, one);
            let quotient = b.ins().udiv(last, magnitude);
            b.ins().iadd(quotient, one)
        };
        let zero = b.ins().iconst(types::I64, 0);
        let length = b.ins().select(nonempty, count, zero);
        if !narrow {
            self.counters.ranges.push((length, start, step, bound));
        }
        // A narrow range, or an i64 range stepping up from a nonnegative
        // start, is shorter than `i64::MAX`: fold its check (issue #721).
        let short = narrow
            || signed && {
                let dfg = &self.bcx.func.dfg;
                super::known::known_signed(dfg, step).is_some_and(|step| step > 0)
                    && self.nonnegative_operand(first)
            };
        if short {
            self.stack.push((length, Ty::I64));
            return Ok(());
        }
        let b = &mut self.bcx;
        let oversized = b.ins().icmp_imm_s(IntCC::SignedLessThan, length, 0);
        let cold = b.create_block();
        let after = b.create_block();
        b.ins().brif(oversized, cold, &[], after, &[]);
        b.switch_to_block(cold);
        b.seal_block(cold);
        b.set_cold_block(cold);
        let arguments = [
            self.pack(start, element),
            self.pack(stop, element),
            self.pack(step, &Ty::I64),
        ];
        let site = self.trap_site()?;
        let range = CollectionOp::Range(element.clone());
        let descriptor = Ty::Range(Arc::new(element.clone()));
        self.collection_call_at(range.opcode(), &arguments, Some(&descriptor), Some(site))?;
        self.bcx.ins().trap(TrapCode::unwrap_user(3));
        self.bcx.switch_to_block(after);
        self.bcx.seal_block(after);
        self.stack.push((length, Ty::I64));
        Ok(())
    }

    /// Read a range value's first element, step, and count. A range value
    /// addresses the runtime's `Range` payload (`plenty-runtime/src/ranges.rs`):
    /// `start`, `stop`, and `step` words followed by the validated length.
    /// `start` holds the element's bits, so truncating it restores a narrow
    /// element.
    pub(super) fn lower_range_parts(&mut self, element: &Ty) -> Result<()> {
        let (range, _) = self.pop_typed(Ty::Range(Arc::new(element.clone())))?;
        let flags = cranelift_codegen::ir::MemFlagsData::trusted();
        let b = &mut self.bcx;
        let start = b.ins().load(types::I64, flags, range, 0);
        let step = b.ins().load(types::I64, flags, range, 16);
        let length = b.ins().load(types::I64, flags, range, 24);
        let native = clif_type(element.clone());
        let start = if native == types::I64 {
            start
        } else {
            b.ins().ireduce(native, start)
        };
        self.stack.push((start, element.clone()));
        self.stack.push((step, Ty::I64));
        self.stack.push((length, Ty::I64));
        Ok(())
    }

    /// The function's scratch slot for runtime call operands, at least
    /// `bytes` long: created on first use, and grown for a larger use, so a
    /// function that only reports index failures keeps a small frame.
    fn scratch_slot(&mut self, bytes: u32) -> cranelift_codegen::ir::StackSlot {
        if let Some(slot) = self.collection_scratch {
            let data = &mut self.bcx.func.sized_stack_slots[slot];
            data.size = data.size.max(bytes);
            return slot;
        }
        let slot = self
            .bcx
            .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                bytes,
                4,
            ));
        self.collection_scratch = Some(slot);
        slot
    }

    /// Stores one two-word operand of a runtime operation. A one-word value
    /// gets a zero high word stored as a 64-bit constant, rather than being
    /// widened: a two-word zero is the empty-slot value, which would then be
    /// live across the call in two callee-saved registers (issue #757).
    fn store_scratch_operand(
        &mut self,
        value: cranelift_codegen::ir::Value,
        args: cranelift_codegen::ir::Value,
        offset: i32,
        zero: cranelift_codegen::ir::Value,
    ) {
        let flags = cranelift_codegen::ir::MemFlagsData::trusted();
        let ty = self.bcx.func.dfg.value_type(value);
        if ty == types::I128 {
            self.bcx.ins().store(flags, value, args, offset);
            return;
        }
        let word = if ty == types::I64 {
            value
        } else {
            self.bcx.ins().uextend(types::I64, value)
        };
        self.bcx.ins().store(flags, word, args, offset);
        self.bcx.ins().store(flags, zero, args, offset + 8);
    }

    /// Call runtime operation `opcode` through its own entry symbol,
    /// `plenty_op_<opcode>` (`plenty-runtime/src/aggregates.rs`), so the linker
    /// keeps only the operations a program calls. `site` is the trap site of
    /// an operation that can trap.
    fn collection_call_at(
        &mut self,
        opcode: i64,
        values: &[cranelift_codegen::ir::Value],
        ty: Option<&Ty>,
        site: Option<cranelift_codegen::ir::Value>,
    ) -> Result<cranelift_codegen::ir::Value> {
        // Reject an ABI disagreement while compiling, before a runtime entry
        // could read an uninitialized scratch word or lose an extra operand.
        assert_eq!(
            values.len().saturating_sub(3),
            crate::collection_opcodes::scratch_operand_count(opcode),
            "runtime opcode {opcode} scratch-operand count disagrees with codegen",
        );
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let descriptor = if let Some(ty) = ty {
            let id = self.module.metadata(ty);
            let gv = self.module.declare_data_in_func(id, self.bcx.func);
            self.bcx.ins().symbol_value(PTR_TY, gv)
        } else {
            zero
        };
        // Resume writes the yielded inline payload after the result word.
        // The usual six-word scratch area only leaves 32 bytes for it.
        let bytes = if opcode == 24 {
            64 + ty.map_or(0, Ty::inline_bytes)
        } else {
            96
        };
        let slot = self.scratch_slot(u32::try_from(bytes.max(96))?);
        let args = self.bcx.ins().stack_addr(PTR_TY, slot, 0);
        for i in 0..3 {
            let value = values.get(i).copied().unwrap_or(zero);
            self.store_scratch_operand(value, args, (i * 16) as i32, zero);
        }
        // `std.bytes` range copies take up to five operands; the two beyond
        // the fixed inputs travel in the scratch words after the result.
        for (i, value) in values.iter().enumerate().skip(3).take(2) {
            self.store_scratch_operand(*value, args, ((i + 1) * 16) as i32, zero);
        }
        let mut sig = self.module.make_signature();
        sig.call_conv = crate::target::native_call_conv();
        // Arguments, descriptor, trap site.
        sig.params.extend([AbiParam::new(PTR_TY); 3]);
        let callee =
            self.module
                .declare_function(&format!("plenty_op_{opcode}"), Linkage::Import, &sig)?;
        let f = self.module.declare_func_in_func(callee, self.bcx.func);
        // The runtime writes the result after the three inputs.
        // Advancing a generator or dropping a value can enter std without a
        // source-level call, so user code names itself as the caller. The
        // runtime restores the context after every callback into compiled
        // code, so std needs nothing here.
        // A release that runs no compiled code needs no site (issue #750).
        let tracked = match opcode {
            24 => true,
            27 => ty.is_none_or(Ty::release_runs_code),
            _ => false,
        };
        let saved = if !self.in_standard_library() && tracked {
            self.enter_call()?
        } else {
            None
        };
        self.bcx
            .ins()
            .call(f, &[args, descriptor, site.unwrap_or(zero)]);
        self.leave_call(saved);
        Ok(self.bcx.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            args,
            48,
        ))
    }

    /// Indexing a value consumes that value. Snapshot and retain a copied
    /// list row before releasing its owner; byte results need no ownership.
    pub(super) fn lower_assumed_index(&mut self, operation: &CollectionOp) -> Result<()> {
        use crate::bytes::BytesOp;
        match operation {
            CollectionOp::Get(ty @ Ty::List(element)) => {
                let (index, _) = self.pop_typed(Ty::I64)?;
                let (owner, _) = self.pop_typed(ty.clone())?;
                let flags = cranelift_codegen::ir::MemFlagsData::trusted();
                let rows = self
                    .bcx
                    .ins()
                    .load(PTR_TY, flags, owner, crate::list_layout::ROWS);
                let address =
                    self.row_address(rows, index, metadata::list_row_bytes(element) as i64);
                let reference = self.bcx.ins().uextend(types::I128, address);
                self.stack
                    .push((reference, Ty::Ref(element.clone(), false)));
                self.lower(&Op::ReadRef((**element).clone()))?;
                self.release(owner, ty);
                Ok(())
            }
            CollectionOp::Bytes(kind @ (BytesOp::Item | BytesOp::SetItem | BytesOp::ValueItem)) => {
                let (inputs, output) = operation.signature();
                let value = if *kind == BytesOp::SetItem {
                    Some(self.pop_typed(Ty::U8)?.0)
                } else {
                    None
                };
                let (index, _) = self.pop_typed(Ty::I64)?;
                let (receiver, _) = self.pop_typed(inputs[0].clone())?;
                let owned = *kind == BytesOp::ValueItem;
                let owner = if owned {
                    receiver
                } else {
                    self.reference_word(receiver)
                };
                // The existing view helper exposes the live buffer's rows,
                // without depending on Rust's private Vec layout.
                let helper = self
                    .module
                    .declare_func_in_func(self.runtime.byte_view_rows, self.bcx.func);
                let call = self.bcx.ins().call(helper, &[owner]);
                let rows = self.bcx.inst_results(call)[0];
                let address = self.row_address(rows, index, 1);
                let flags = cranelift_codegen::ir::MemFlagsData::trusted();
                let result = match value {
                    Some(value) => {
                        self.bcx.ins().store(flags, value, address, 0);
                        let unit = self.bcx.ins().iconst(types::I64, 0);
                        self.unpack(unit, &Ty::Unit)
                    }
                    None => self.bcx.ins().load(types::I8, flags, address, 0),
                };
                if owned {
                    self.release(owner, &inputs[0]);
                }
                self.stack.push((result, output));
                Ok(())
            }
            _ => self.lower_collection(operation),
        }
    }

    pub(super) fn lower_collection(&mut self, operation: &CollectionOp) -> Result<()> {
        if let CollectionOp::Clist(op) = operation {
            return self.lower_clist(op);
        }
        if let CollectionOp::BytesGetView(_) | CollectionOp::BytesSetView(_) = operation {
            return self.lower_bytes_view(operation);
        }
        if matches!(
            operation,
            CollectionOp::TextView(crate::text::TextViewOp::Bytes)
                | CollectionOp::Regex(crate::regex::RegexOp::TextBytes)
        ) {
            // Substrings already carry a byte address and byte count. Their
            // owner's loan (including inline-string storage) covers the call.
            let (view, _) = self.pop_typed(crate::frontend::text_view())?;
            self.stack.push((view, crate::bytes::byte_view()));
            return Ok(());
        }
        if self.lower_view(operation)? {
            return Ok(());
        }
        if let CollectionOp::ListRetag(types) = operation {
            let (from, to) = &**types;
            // The rows already have the target layout; only the header's
            // descriptor, which runtime formatting and cleanup read, changes.
            let (list, _) = self.pop_typed(from.clone())?;
            let id = self.module.metadata(to);
            let gv = self.module.declare_data_in_func(id, self.bcx.func);
            let descriptor = self.bcx.ins().symbol_value(PTR_TY, gv);
            self.bcx.ins().store(
                cranelift_codegen::ir::MemFlagsData::trusted(),
                descriptor,
                list,
                crate::list_layout::DESCRIPTOR,
            );
            self.stack.push((list, to.clone()));
            return Ok(());
        }
        if let CollectionOp::DictView(owner, _) = operation {
            let (reference, _) = self.pop_typed(Ty::Ref(Arc::new(owner.clone()), false))?;
            self.stack.push((reference, operation.signature().1));
            return Ok(());
        }
        if let CollectionOp::KeyView(owner) = operation {
            let (reference, _) = self.pop_typed(Ty::Ref(Arc::new(owner.clone()), false))?;
            let value = self.reference_word(reference);
            self.stack.push((value, operation.signature().1));
            return Ok(());
        }
        if let CollectionOp::CallerLocation = operation {
            let function = self
                .module
                .declare_func_in_func(self.runtime.caller_location, self.bcx.func);
            let call = self.bcx.ins().call(function, &[]);
            let written = self.bcx.inst_results(call)[0];
            self.stack.push((written, Ty::Bool));
            return Ok(());
        }
        if let CollectionOp::Trap = operation {
            let (message, _) = self.pop_typed(Ty::Str)?;
            let site = self.trap_site()?;
            let helper = self
                .module
                .declare_func_in_func(self.runtime.trap_message, self.bcx.func);
            self.bcx.ins().call(helper, &[message, site]);
            self.bcx.ins().trap(TrapCode::unwrap_user(3));
            self.terminated = true;
            return Ok(());
        }
        if let CollectionOp::ActorHeap = operation {
            let (argument, _) = self.pop_typed(Ty::I64)?;
            let (op, _) = self.pop_typed(Ty::I64)?;
            let function = self
                .module
                .declare_func_in_func(self.runtime.actor_heap, self.bcx.func);
            let call = self.bcx.ins().call(function, &[op, argument]);
            let result = self.bcx.inst_results(call)[0];
            self.stack.push((result, Ty::I64));
            return Ok(());
        }
        if let CollectionOp::TestFault = operation {
            // Failure injection is independent of codec stack checks.
            let (code, _) = self.pop_typed(Ty::I32)?;
            let (count, _) = self.pop_typed(Ty::I64)?;
            let (kind, _) = self.pop_typed(Ty::I64)?;
            let armed = match self.runtime.test_fault {
                Some(hook) => {
                    let function = self.module.declare_func_in_func(hook, self.bcx.func);
                    let call = self.bcx.ins().call(function, &[kind, count, code]);
                    self.bcx.inst_results(call)[0]
                }
                None => self.bcx.ins().iconst(types::I8, 0),
            };
            self.stack.push((armed, Ty::Bool));
            return Ok(());
        }
        if let CollectionOp::SerialStackReady { callee, failure } = operation {
            let budget = self.module.serial_stack_budget(
                self.user_fns[callee].id,
                self.user_fns[failure].id,
                self.bcx.func,
            );
            let address = self.bcx.ins().symbol_value(PTR_TY, budget);
            let required = self.bcx.ins().load(
                types::I64,
                cranelift_codegen::ir::MemFlagsData::trusted(),
                address,
                0,
            );
            let function = self
                .module
                .declare_func_in_func(self.runtime.serial_stack_ready, self.bcx.func);
            let call = self.bcx.ins().call(function, &[required]);
            self.stack.push((self.bcx.inst_results(call)[0], Ty::Bool));
            return Ok(());
        }
        if let CollectionOp::ListLenRef(ty) = operation {
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), false))?;
            let owner = self.reference_word(receiver);
            let row_bytes = metadata::list_row_bytes(&ty.element().unwrap()) as i64;
            let length = self.dense_list_len(owner, row_bytes);
            let bounds = &mut self.counters.bounds;
            let list = bounds.canonical(&self.bcx.func.dfg, receiver);
            if bounds.stable_lists.contains(&list) {
                bounds.lengths.insert(length, list);
            }
            self.stack.push((length, Ty::I64));
            return Ok(());
        }
        // A list's rows are dense, `list_row_bytes` apart, so its row-0
        // address and row size need no runtime call.
        if let CollectionOp::ListRowsRef(ty) | CollectionOp::ListRowBytesRef(ty) = operation {
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), false))?;
            let value = if matches!(operation, CollectionOp::ListRowsRef(_)) {
                let owner = self.reference_word(receiver);
                let flags = cranelift_codegen::ir::MemFlagsData::trusted();
                self.bcx
                    .ins()
                    .load(PTR_TY, flags, owner, crate::list_layout::ROWS)
            } else {
                let bytes = metadata::list_row_bytes(&ty.element().unwrap());
                self.bcx.ins().iconst(types::I64, bytes as i64)
            };
            self.stack.push((value, Ty::I64));
            return Ok(());
        }
        if let CollectionOp::ListScalarGetRef(ty) | CollectionOp::ListScalarGetView(ty) = operation
        {
            let cached = self.cached_rows(operation)?;
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), false))?;
            let (rows, length) = self.dense_rows(cached, receiver, ty);
            let element = ty.element().unwrap();
            let result = self
                .scalar_row_access(rows, length, index, &element, None)?
                .unwrap();
            self.stack.push((result, element));
            return Ok(());
        }
        if let CollectionOp::ListScalarSetRef(ty) | CollectionOp::ListScalarSetView(ty) = operation
        {
            let cached = self.cached_rows(operation)?;
            let element = ty.element().unwrap();
            let (value, _) = self.pop_typed(element.clone())?;
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), true))?;
            let (rows, length) = self.dense_rows(cached, receiver, ty);
            self.scalar_row_access(rows, length, index, &element, Some(value))?;
            let unit = self.bcx.ins().iconst(types::I64, 0);
            let unit = self.unpack(unit, &Ty::Unit);
            self.stack.push((unit, Ty::Unit));
            return Ok(());
        }
        if let CollectionOp::ListScalarSwapRef(ty) = operation {
            let element = ty.element().unwrap();
            let (second, _) = self.pop_typed(Ty::I64)?;
            let (first, _) = self.pop_typed(Ty::I64)?;
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), true))?;
            let owner = self.reference_word(receiver);
            // The header is read once for both rows.
            let flags = cranelift_codegen::ir::MemFlagsData::trusted();
            let rows = self
                .bcx
                .ins()
                .load(PTR_TY, flags, owner, crate::list_layout::ROWS);
            let length = self.scalar_list_len(owner, &element);
            let row_bytes = clif_type(element.clone()).bytes() as i64;
            self.swap_scalar_rows(rows, length, [first, second], row_bytes, &element)?;
            let unit = self.bcx.ins().iconst(types::I64, 0);
            let unit = self.unpack(unit, &Ty::Unit);
            self.stack.push((unit, Ty::Unit));
            return Ok(());
        }
        if let CollectionOp::ListSortBy(sort) = operation {
            let (descending, _) = self.pop_typed(Ty::Bool)?;
            let (callback, _) = self.pop_typed(sort.callback.clone())?;
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(sort.list.clone()), true))?;
            let owner = self.reference_word(receiver);
            let checked = sort
                .error
                .as_ref()
                .map(|error| crate::sum::result(Ty::Unit, error.clone()));
            let environment = if let Some(result) = &checked {
                // Callback word followed by a full caller-owned result slot;
                // inline errors must outlive each comparator invocation.
                let context = self.inline_storage(16 + result.slot_bytes());
                let flags = cranelift_codegen::ir::MemFlagsData::trusted();
                self.bcx.ins().store(flags, callback, context, 0);
                let zero = self.bcx.ins().iconst(types::I64, 0);
                let zero = self.bcx.ins().uextend(types::I128, zero);
                self.bcx.ins().store(flags, zero, context, 16);
                context
            } else {
                self.bcx.ins().ireduce(PTR_TY, callback)
            };
            let adapter = self.runtime.sort_adapters[&sort.adapter];
            let adapter = self.module.declare_func_in_func(adapter, self.bcx.func);
            let adapter = self.bcx.ins().func_addr(PTR_TY, adapter);
            let descending = self.bcx.ins().uextend(types::I64, descending);
            if sort.stable {
                let function = self
                    .module
                    .declare_func_in_func(self.runtime.list_sort_stable_by, self.bcx.func);
                let call = self
                    .bcx
                    .ins()
                    .call(function, &[owner, adapter, environment, descending]);
                let high = self.bcx.inst_results(call)[0];
                let low = self.bcx.ins().iconst(types::I64, 0);
                let reserved = self.bcx.ins().iconcat(low, high);
                let allocation = crate::sum::allocation_result();
                let reserved = self.unpack(reserved, &allocation);
                let output = crate::collection::key_sort_output(true, sort.error.as_ref()).unwrap();
                let Some(keyed) = checked else {
                    self.stack.push((reserved, output));
                    return Ok(());
                };
                // At most one result fails. The runtime reserves only after
                // its ordered-input check finds an out-of-order pair, and
                // after a key error the comparator reports every pair equal,
                // so a key error means no reservation was attempted. Widen
                // both and select the one that failed, if any.
                let key_result = self.bcx.ins().load(
                    types::I128,
                    cranelift_codegen::ir::MemFlagsData::trusted(),
                    environment,
                    16,
                );
                let reserved = self.widen_sum(reserved, &allocation, &output);
                let key_result = self.widen_sum(key_result, &keyed, &output);
                let failed = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, high, 0);
                let result = self.bcx.ins().select(failed, reserved, key_result);
                self.stack.push((result, output));
                return Ok(());
            }
            let function = self
                .module
                .declare_func_in_func(self.runtime.list_sort_by, self.bcx.func);
            self.bcx
                .ins()
                .call(function, &[owner, adapter, environment, descending]);
            if let Some(result) = checked {
                let value = self.bcx.ins().load(
                    types::I128,
                    cranelift_codegen::ir::MemFlagsData::trusted(),
                    environment,
                    16,
                );
                self.stack.push((value, result));
                return Ok(());
            }
            let unit = self.bcx.ins().iconst(types::I64, 0);
            let unit = self.unpack(unit, &Ty::Unit);
            self.stack.push((unit, Ty::Unit));
            return Ok(());
        }
        if let CollectionOp::OrderCompare(ty) = operation {
            let (b, _) = self.pop_typed(ty.clone())?;
            let (a, _) = self.pop_typed(ty.clone())?;
            let order = if ty.is_int() || ty.is_float() {
                self.scalar_order(a, b, ty)
            } else {
                let order = self.order_compare(a, b, ty);
                self.release(a, ty);
                self.release(b, ty);
                order
            };
            self.stack.push((order, Ty::I64));
            return Ok(());
        }
        if let CollectionOp::ListRowRef(ty, mutable) = operation {
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (row_bytes, _) = self.pop_typed(Ty::I64)?;
            let (rows, _) = self.pop_typed(Ty::I64)?;
            let offset = self.bcx.ins().imul(index, row_bytes);
            let address = self.bcx.ins().iadd(rows, offset);
            // A row is a top-level slot: no inline-sum tag is projected.
            let reference = self.bcx.ins().uextend(types::I128, address);
            let element = ty.element().unwrap();
            self.stack
                .push((reference, Ty::Ref(Arc::new(element), *mutable)));
            return Ok(());
        }
        if let CollectionOp::ListRowTake(ty) = operation {
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (row_bytes, _) = self.pop_typed(Ty::I64)?;
            let (rows, _) = self.pop_typed(Ty::I64)?;
            let offset = self.bcx.ins().imul(index, row_bytes);
            let address = self.bcx.ins().iadd(rows, offset);
            let element = ty.element().unwrap();
            let flags = cranelift_codegen::ir::MemFlagsData::trusted();
            let base = match element.newtype_base().into_owned() {
                Ty::Atomic(inner) => (*inner).clone(),
                base => base,
            };
            if base.is_numeric() || base == Ty::Bool {
                // Affine one-field classes and atomics can still have packed
                // scalar rows, as `list_row_bytes` lays them out. Taking one
                // must not read or clear its neighbor's storage.
                let value = self.scalar_value(address, &base, &element);
                let width = types::Type::int(clif_type(base).bits() as u16).unwrap();
                let zero = self.bcx.ins().iconst(width, 0);
                self.bcx.ins().store(flags, zero, address, 0);
                self.stack.push((value, element));
                return Ok(());
            }
            let value = self.bcx.ins().load(types::I128, flags, address, 0);
            let value = self.unpack(value, &element);
            // The row is emptied below: relocate an inline payload out of it
            // first, as `ReplaceRef` does, then leave value word zero so the
            // list's drop skips the row, as `IterTake` does.
            let value = self.snapshot_inline(value, &element);
            let zero = self.bcx.ins().iconst(types::I64, 0);
            let zero = self.bcx.ins().uextend(types::I128, zero);
            self.bcx.ins().store(flags, zero, address, 0);
            self.stack.push((value, element));
            return Ok(());
        }
        // Rows are dense and `list_row_bytes` apart, so a row's address
        // needs no runtime call (issue #739).
        if let CollectionOp::ElementRef(ty @ Ty::List(element), mutable)
        | CollectionOp::ElementRefView(ty @ Ty::List(element), mutable) = operation
        {
            let cached = self.cached_rows(operation)?;
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), *mutable))?;
            let (rows, length) = self.dense_rows(cached, receiver, ty);
            let row_bytes = metadata::list_row_bytes(element) as i64;
            let address = self.checked_row(rows, length, index, row_bytes)?;
            // As for `ListRowRef`: a row is a top-level slot with no tag.
            let reference = self.bcx.ins().uextend(types::I128, address);
            self.stack
                .push((reference, Ty::Ref(element.clone(), *mutable)));
            return Ok(());
        }
        if let CollectionOp::Bytes(crate::bytes::BytesOp::Push) = operation {
            return self.lower_bytes_push(operation);
        }
        if let CollectionOp::TextOrd | CollectionOp::QueryRef(Query::TextOrd) = operation {
            return self.lower_text_ord(operation);
        }
        if let CollectionOp::TextStartsWith
        | CollectionOp::TextEndsWith
        | CollectionOp::TextFind
        | CollectionOp::TextRFind
        | CollectionOp::TextCount = operation
        {
            return self.lower_text_search(operation);
        }
        if let Some((helper, borrowed, site)) = self.narrow_helper(operation) {
            let (inputs, output) = operation.signature();
            let mut trailing = Vec::new();
            for input in inputs[1..].iter().rev() {
                let (value, _) = self.pop_typed(input.clone())?;
                // Narrow helpers take later shared references as the value
                // word, a slice view as its address and length (pushed in
                // reverse, like the inputs), and integers as full words.
                if matches!(input, Ty::View(..)) {
                    let (address, len) = self.bcx.ins().isplit(value);
                    trailing.extend([len, address]);
                    continue;
                }
                let value = if matches!(input, Ty::Ref(..)) {
                    self.reference_word(value)
                } else if clif_type(input.clone()) == types::I64 {
                    value
                } else {
                    self.bcx.ins().uextend(types::I64, value)
                };
                trailing.push(value);
            }
            trailing.reverse();
            let (receiver, ty) = self.pop_typed(inputs[0].clone())?;
            let owner = if borrowed {
                self.reference_word(receiver)
            } else {
                receiver
            };
            // Byte method arguments can separate the receiver load from this
            // call. Its recorded slot/field copy remains borrowed while the
            // owner is held in place; these helpers cannot call back or grow it.
            let borrowed = borrowed
                || self.forget_fresh_load(receiver)
                || (matches!(
                    operation,
                    CollectionOp::Bytes(crate::bytes::BytesOp::Get | crate::bytes::BytesOp::Set)
                ) && self.forget_slot_copy(receiver));
            let mut args = vec![owner];
            args.extend(trailing);
            match site {
                Site::None => {}
                Site::Trap => args.push(self.trap_site()?),
                // A cursor stays inside the value, so it never reports a site.
                Site::Null => args.push(self.bcx.ins().iconst(PTR_TY, 0)),
            }
            let function = self.module.declare_func_in_func(helper, self.bcx.func);
            let call = self.bcx.ins().call(function, &args);
            let result = match self.bcx.inst_results(call).first() {
                Some(&result) => self.unpack(result, &output),
                None => {
                    let unit = self.bcx.ins().iconst(types::I64, 0);
                    self.unpack(unit, &Ty::Unit)
                }
            };
            if !borrowed {
                self.release(receiver, &ty);
            }
            self.stack.push((result, output));
            return Ok(());
        }
        let formatted = match operation {
            CollectionOp::Formatted(formatted) => Some(formatted.clone()),
            _ => None,
        };
        let operation = formatted.as_ref().map_or(operation, |f| &f.operation);
        // A method's error goes to this cell; see `Formatted`.
        let cell = formatted.as_ref().map(|_| {
            let slot = self
                .bcx
                .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    16,
                    4,
                ));
            let address = self.bcx.ins().stack_addr(PTR_TY, slot, 0);
            let zero = self.bcx.ins().iconst(types::I64, 0);
            let zero = self.bcx.ins().uextend(types::I128, zero);
            self.bcx.ins().store(
                cranelift_codegen::ir::MemFlagsData::trusted(),
                zero,
                address,
                0,
            );
            address
        });
        let (inputs, output) = operation.signature();
        // With only text and scalar operands, the dispatcher runs no Plenty
        // code and reads the texts without keeping them, so a text copy that
        // is still held by its source is read in place (issue #387).
        let reads_in_place = inputs.iter().all(|input| {
            *input == Ty::Str
                || input.is_numeric()
                || *input == Ty::Bool
                || *input == crate::frontend::text_view()
        });
        // A lookup or byte access reads every operand in place, such as the
        // borrowed key of `get_mut(&key)` and the buffer a `Bytes` method
        // reads through `self` (issue #744).
        let only_reads = operation.only_reads_operands();
        // A mutation through `&mut` changes the receiver in place, and the
        // exclusive loan keeps its owner alive, so the receiver's copy is read
        // in place too (issue #762).
        let receiver_in_place = operation.mutates_receiver_in_place();
        // A set update reads its `&set` argument in place (issue #825).
        let argument_in_place = operation.reads_argument_in_place();
        // Inserted operands are owned on the stack, moved or copied alike,
        // and the runtime adopts them: it stores their counts or drops them
        // once on a failed insert or a `get_or_insert` hit (issue #762). One
        // read in place would give the runtime a count it never had, so none
        // is, whatever the rules above say.
        let inserts = matches!(
            operation,
            CollectionOp::TryInsert(_)
                | CollectionOp::ListTryInsert(_)
                | CollectionOp::DictGetOrInsert(_)
        );
        let mut values = Vec::new();
        let mut held = Vec::new();
        for (i, input) in inputs.iter().enumerate().rev() {
            let (value, _) = self.pop_typed(input.clone())?;
            let adopted = inserts && i > 0;
            let in_place = !adopted
                && (only_reads
                    || reads_in_place && *input == Ty::Str
                    || receiver_in_place && i == 0
                    || argument_in_place && i == 1);
            held.push(in_place && self.forget_slot_copy(value));
            values.push(self.pack(value, input));
        }
        values.reverse();
        held.reverse();
        if let CollectionOp::TryPrintMany(types) = operation {
            let slot = self
                .bcx
                .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    (types.len().max(1) * 16) as u32,
                    4,
                ));
            let address = self.bcx.ins().stack_addr(PTR_TY, slot, 0);
            for (i, value) in values.iter().enumerate() {
                self.bcx.ins().store(
                    cranelift_codegen::ir::MemFlagsData::trusted(),
                    *value,
                    address,
                    (i * 16) as i32,
                );
            }
            let metadata = crate::sum::tuple(types.clone());
            let arguments: Vec<_> = std::iter::once(address).chain(cell).collect();
            let result = self.collection_call(operation.opcode(), &arguments, Some(&metadata))?;
            let result = self.unpack(result, &output);
            let (result, output) = match (&formatted, cell) {
                (Some(formatted), Some(cell)) => {
                    self.formatted_result(formatted, result, &output, cell)
                }
                _ => (result, output),
            };
            for ((value, ty), held) in values.iter().zip(&inputs).zip(held) {
                if !held {
                    self.release(*value, ty);
                }
            }
            self.stack.push((result, output));
            return Ok(());
        }
        let key_set = match operation {
            CollectionOp::KeySetAlgebra(op) => Some(Ty::Set(Arc::new(op.left.element().unwrap()))),
            CollectionOp::KeyViewToSet(t) => Some(Ty::Set(Arc::new(t.element().unwrap()))),
            _ => None,
        };
        let descriptor_operation = match operation {
            CollectionOp::TextView(crate::text::TextViewOp::Apply(original)) => &**original,
            original => original,
        };
        let descriptor = match descriptor_operation {
            CollectionOp::KeySetAlgebra(..) | CollectionOp::KeyViewToSet(_) => key_set.as_ref(),
            CollectionOp::Filesystem(_)
            | CollectionOp::Bytes(_)
            | CollectionOp::Crypto(_)
            | CollectionOp::Compress(_)
            | CollectionOp::Sqlite(_)
            | CollectionOp::Tls(_)
            | CollectionOp::Net(_)
            | CollectionOp::Process(_) => Some(&output),
            CollectionOp::Args
            | CollectionOp::ArgsBytes
            | CollectionOp::InputBytes
            | CollectionOp::EncodeUtf8
            | CollectionOp::ReadBytes
            | CollectionOp::OpenFile
            | CollectionOp::FileReadLines => Some(&output),
            CollectionOp::ParseNumber(ty)
            | CollectionOp::FormatScalar(ty)
            | CollectionOp::FormatValue(ty)
            | CollectionOp::TryPrint(ty) => Some(ty),
            // The types `print` rejects have no rendering; without metadata
            // the runtime writes its fixed message.
            CollectionOp::ReportEntryError(ty) => {
                (!ty.prints_generator_frame() && !ty.recursive_data()).then_some(ty)
            }
            CollectionOp::Next(_) => Some(&output),
            // The runtime chooses the traversal from the iterable's kind.
            CollectionOp::TextTryJoin(ty) => Some(ty),
            CollectionOp::Range(_) => Some(&output),
            CollectionOp::Len(t)
            | CollectionOp::Get(t)
            | CollectionOp::IterGet(t)
            | CollectionOp::Contains(t)
                if matches!(t, Ty::Range(_)) =>
            {
                Some(t)
            }
            CollectionOp::TryCopy(ty) | CollectionOp::OrderCompare(ty) => Some(ty),
            CollectionOp::TryNew(ty)
            | CollectionOp::ListTrySlice(ty)
            | CollectionOp::ListSlice(ty) => Some(ty),
            CollectionOp::TextTrySplit
            | CollectionOp::TextTrySplitWhitespace
            | CollectionOp::TextTrySplitLines
            | CollectionOp::TryKeys(_)
            | CollectionOp::TryValues(_) => Some(&output),
            CollectionOp::Len(Ty::Str)
            | CollectionOp::Get(Ty::Str)
            | CollectionOp::IterGet(Ty::Str)
            | CollectionOp::Contains(Ty::Str) => Some(&Ty::Str),
            CollectionOp::Len(Ty::ByteStr)
            | CollectionOp::Get(Ty::ByteStr)
            | CollectionOp::IterGet(Ty::ByteStr)
            | CollectionOp::Contains(Ty::ByteStr) => Some(&Ty::ByteStr),
            CollectionOp::TextByteLen => Some(&Ty::Str),
            _ => None,
        };
        let mut arguments = values.clone();
        // Indexed byte access passes its owner through the borrowed field, so
        // no reference count changes per access.
        if matches!(inputs.first(), Some(Ty::Ref(inner, _)) if **inner == Ty::ByteStorage) {
            arguments[0] = self.read_reference(values[0]);
        }
        if let CollectionOp::TryCopy(ty) = operation {
            // An inline copy needs its own storage before its children are copied.
            if ty.has_inline_storage() {
                arguments.push(self.inline_storage(ty.inline_bytes()));
            }
        }
        let site = if operation.traps() {
            Some(self.trap_site()?)
        } else {
            None
        };
        arguments.extend(cell);
        let result = self.collection_call_at(operation.opcode(), &arguments, descriptor, site)?;
        let result = if let CollectionOp::DictGetOrInsert(_) = operation {
            self.get_or_insert_hit(result, &values[1..], &inputs[1..])
        } else {
            result
        };
        let result = self.unpack(result, &output);
        let (result, output) = match (&formatted, cell) {
            (Some(formatted), Some(cell)) => {
                self.formatted_result(formatted, result, &output, cell)
            }
            _ => (result, output),
        };
        // An affine element read is a retained view into the borrowed
        // collection. Copying its storage would split the instance in two.
        let result = if matches!(operation, CollectionOp::Get(_)) && output.affine() {
            result
        } else {
            self.snapshot_inline(result, &output)
        };
        // The runtime owns the adopted operands now.
        for (i, (value, ty)) in values.iter().zip(&inputs).enumerate() {
            if !(held[i] || inserts && i > 0) {
                self.release(*value, ty);
            }
        }
        self.stack.push((result, output));
        Ok(())
    }

    /// The `dict.get_or_insert` result word without its hit flag. A hit
    /// leaves the copyable `operands` with the caller, so they are released
    /// here on that path only; a miss stored them (issue #762).
    fn get_or_insert_hit(
        &mut self,
        result: cranelift_codegen::ir::Value,
        operands: &[cranelift_codegen::ir::Value],
        operand_types: &[Ty],
    ) -> cranelift_codegen::ir::Value {
        let (low, high) = self.bcx.ins().isplit(result);
        let hit = self.bcx.ins().icmp_imm_u(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            high,
            crate::collection_opcodes::GET_OR_INSERT_HIT,
        );
        if operand_types.iter().any(|ty| !ty.affine() && ty.managed()) {
            let release = self.bcx.create_block();
            let done = self.bcx.create_block();
            self.bcx.ins().brif(hit, release, &[], done, &[]);
            self.bcx.switch_to_block(release);
            self.bcx.seal_block(release);
            for (value, ty) in operands.iter().zip(operand_types) {
                if !ty.affine() {
                    self.release(*value, ty);
                }
            }
            self.bcx.ins().jump(done, &[]);
            self.bcx.switch_to_block(done);
            self.bcx.seal_block(done);
        }
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let high = self.bcx.ins().select(hit, zero, high);
        self.bcx.ins().iconcat(low, high)
    }

    /// The `Result` of a formatting operation whose methods may fail: the
    /// method's error from `cell` if one failed, otherwise the runtime's
    /// `result` with its error widened. On failure the runtime's result owns
    /// nothing.
    fn formatted_result(
        &mut self,
        formatted: &crate::collection::Formatted,
        result: cranelift_codegen::ir::Value,
        output: &Ty,
        cell: cranelift_codegen::ir::Value,
    ) -> (cranelift_codegen::ir::Value, Ty) {
        let widened = formatted.widen(output);
        let Ty::Enum(sum) = output else {
            unreachable!("formatting returns a Result")
        };
        let success = sum.get().variants[0].fields[0].clone();
        let ok = self.widen_sum(result, output, &widened);
        let error = self.bcx.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            cell,
            0,
        );
        // The adapter's `Result[str, FormatError]`; an `Err` is laid out the
        // same whatever the success type.
        let method = crate::sum::result(success, formatted.error.clone());
        let failed = self.sum_tag(error);
        let error = self.widen_sum(error, &method, &widened);
        let value = self.bcx.ins().select(failed, error, ok);
        (value, widened)
    }

    fn lower_clist(&mut self, op: &crate::clist::ClistOp) -> Result<()> {
        use crate::clist::ClistOp;
        if let ClistOp::Load(ty) = op {
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (base, _) = self.pop_typed(Ty::I64)?;
            let offset = self
                .bcx
                .ins()
                .imul_imm_s(index, crate::clist::width(ty) as i64);
            let address = self.bcx.ins().iadd(base, offset);
            let value = self.bcx.ins().load(
                clif_type(ty.clone()),
                cranelift_codegen::ir::MemFlagsData::trusted(),
                address,
                0,
            );
            self.stack.push((value, ty.clone()));
            return Ok(());
        }
        if let ClistOp::New(ty) = op {
            let value = self.bcx.ins().iconst(PTR_TY, 0);
            self.stack.push((value, ty.clone()));
            return Ok(());
        }
        if matches!(
            op,
            ClistOp::Get(..) | ClistOp::Set(..) | ClistOp::CopyRow(_)
        ) {
            return self.lower_clist_access(op);
        }
        if matches!(
            op,
            ClistOp::Len(_) | ClistOp::Column(..) | ClistOp::GetView(..) | ClistOp::SetView(..)
        ) {
            return self.lower_clist_inline(op);
        }
        let (inputs, output) = op.signature();
        let mut args = Vec::new();
        for ty in inputs.iter().rev() {
            let (value, _) = self.pop_typed(ty.clone())?;
            args.push(self.pack(value, ty));
        }
        args.reverse();
        if let ClistOp::Column(_, field) = op {
            args.push(self.bcx.ins().iconst(types::I64, *field as i64));
        }
        if let ClistOp::Remove(_) = op {
            // The runtime gathers the removed primitive row into caller-owned
            // storage before shifting columns. No heap allocation is needed.
            args.push(self.inline_storage(output.inline_bytes()));
        }
        let result = self.collection_call(op.opcode(), &args, Some(op.owner()))?;
        let result = self.unpack(result, &output);
        let result = self.snapshot_inline(result, &output);
        // Row inputs have no managed fields or destructor. On failed append,
        // snapshot_inline preserves the intact row before its local expires.
        self.stack.push((result, output));
        Ok(())
    }

    /// The runtime's total order of two borrowed values as an `i64` of -1, 0,
    /// or 1. Operands travel in registers, not through the dispatcher's
    /// argument block.
    pub(super) fn order_compare(
        &mut self,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
        ty: &Ty,
    ) -> cranelift_codegen::ir::Value {
        let a = self.pack(a, ty);
        let b = self.pack(b, ty);
        let (a_low, a_high) = self.bcx.ins().isplit(a);
        let (b_low, b_high) = self.bcx.ins().isplit(b);
        let id = self.module.metadata(ty);
        let gv = self.module.declare_data_in_func(id, self.bcx.func);
        let descriptor = self.bcx.ins().symbol_value(PTR_TY, gv);
        let function = self
            .module
            .declare_func_in_func(self.runtime.order_compare, self.bcx.func);
        let call = self
            .bcx
            .ins()
            .call(function, &[a_low, a_high, b_low, b_high, descriptor]);
        self.bcx.inst_results(call)[0]
    }

    /// The runtime `compare` order of two scalars as -1, 0, or 1: integers by
    /// their signedness, floats by `total_cmp` except that every NaN equals
    /// every other NaN and follows all numbers.
    fn scalar_order(
        &mut self,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
        ty: &Ty,
    ) -> cranelift_codegen::ir::Value {
        let (lt, gt, nan) = if ty.is_int() {
            let (lt, gt) = if is_signed(ty.clone()) {
                (IntCC::SignedLessThan, IntCC::SignedGreaterThan)
            } else {
                (IntCC::UnsignedLessThan, IntCC::UnsignedGreaterThan)
            };
            (
                self.bcx.ins().icmp(lt, a, b),
                self.bcx.ins().icmp(gt, a, b),
                None,
            )
        } else {
            // Widening keeps every f32 order, sign, and NaN.
            let (a, b) = if *ty == Ty::F32 {
                (
                    self.bcx.ins().fpromote(types::F64, a),
                    self.bcx.ins().fpromote(types::F64, b),
                )
            } else {
                (a, b)
            };
            let a_nan = self.bcx.ins().fcmp(FloatCC::Unordered, a, a);
            let b_nan = self.bcx.ins().fcmp(FloatCC::Unordered, b, b);
            // `total_cmp`: flip the magnitude bits of negative values so
            // signed integer order matches.
            let mut total = |value| {
                let ins = self.bcx.ins();
                let bits = ins.bitcast(
                    types::I64,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    value,
                );
                let sign = self.bcx.ins().sshr_imm_s(bits, 63);
                let mask = self.bcx.ins().ushr_imm_s(sign, 1);
                self.bcx.ins().bxor(bits, mask)
            };
            let (a, b) = (total(a), total(b));
            (
                self.bcx.ins().icmp(IntCC::SignedLessThan, a, b),
                self.bcx.ins().icmp(IntCC::SignedGreaterThan, a, b),
                Some((a_nan, b_nan)),
            )
        };
        let lt = self.bcx.ins().uextend(types::I64, lt);
        let gt = self.bcx.ins().uextend(types::I64, gt);
        let order = self.bcx.ins().isub(gt, lt);
        let Some((a_nan, b_nan)) = nan else {
            return order;
        };
        let any = self.bcx.ins().bor(a_nan, b_nan);
        let a_nan = self.bcx.ins().uextend(types::I64, a_nan);
        let b_nan = self.bcx.ins().uextend(types::I64, b_nan);
        let nan_order = self.bcx.ins().isub(a_nan, b_nan);
        self.bcx.ins().select(any, nan_order, order)
    }

    /// Makes the check before a loop that [`super::induction::Relation`]
    /// describes, and records its facts for the body's first operations.
    /// Returns how many operations they cover and the length of
    /// `below_locals` to restore after them.
    fn enter_relation(
        &mut self,
        condition: &[Op],
        body: &[Op],
        nonnegative: Option<u8>,
        types: &[Ty],
        invariant: usize,
    ) -> Result<Option<(usize, usize)>> {
        if self.gathering() {
            return Ok(None);
        }
        let Some(relation) = super::induction::analyze_relation(condition, body) else {
            return Ok(None);
        };
        let (low, high) = (relation.low, relation.high);
        let view = self.counters.bounds.invariant_locals[invariant..]
            .iter()
            .find(|(local, _)| *local == relation.view)
            .map(|&(_, entry)| entry);
        let usable = nonnegative == Some(low)
            && types[low as usize] == Ty::I64
            && types[high as usize] == Ty::I64
            && types[relation.view as usize] == relation.ty
            && self.local_address(high).is_none();
        let Some(view) = view.filter(|_| usable) else {
            return Ok(None);
        };
        let element = super::views::view_element(&relation.ty);
        let row_bytes = metadata::list_row_bytes(&element) as i64;
        let (low_entry, high_entry) = (self.read_local(low), self.read_local(high));
        let runs = self
            .bcx
            .ins()
            .icmp(IntCC::SignedLessThan, low_entry, high_entry);
        let check = self.bcx.create_block();
        let join = self.bcx.create_block();
        self.bcx.ins().brif(runs, check, &[], join, &[]);
        self.bcx.switch_to_block(check);
        self.bcx.seal_block(check);
        // The first pass's checks, at its access's site.
        let position = self.position.clone();
        if let Some(site) = &relation.site {
            if self.module.lines() {
                self.bcx
                    .set_srcloc(cranelift_codegen::ir::SourceLoc::new(site.line));
            }
            self.position.set(site);
        }
        let (rows, count) = self.split_view(view);
        self.note_view_length(view, count);
        for &index in &relation.indices {
            let value = if index == low { low_entry } else { high_entry };
            self.checked_row(rows, count, value, row_bytes)?;
        }
        self.position = position;
        self.bcx.ins().jump(join, &[]);
        self.bcx.switch_to_block(join);
        self.bcx.seal_block(join);
        // `count` does not dominate the body and is only compared, through
        // the view it was read from, with the body's lengths.
        let bounds = &mut self.counters.bounds;
        let below = bounds.below_locals.len();
        bounds.below_locals.push((high, count));
        bounds.below_locals.push((low, count));
        self.counters.nonnegative_locals.push(high);
        Ok(Some((relation.prefix, below)))
    }

    /// Drops the facts [`Self::enter_relation`] recorded: a write to `low`
    /// or `high` follows.
    fn leave_relation(&mut self, below: usize) {
        self.counters.bounds.below_locals.truncate(below);
        self.counters.nonnegative_locals.pop();
    }

    /// Records the stable shared list references, views and lengths that `condition` and
    /// `body` read but never write, with their values on entry; see
    /// [`super::induction::Bounds`].
    /// `stop` is the loop's range stop, if any: a local holding it, such as a
    /// row cache's length taken from `range(len(xs))`, bounds the counter.
    /// A `versioned` loop records every `i64` local, any of which may hold
    /// a length its test compares; see [`super::induction::Version`].
    fn enter_invariant_locals(
        &mut self,
        condition: &[Op],
        body: &[Op],
        stop: Option<cranelift_codegen::ir::Value>,
        versioned: bool,
    ) {
        let mut read = crate::hash::HashSet::default();
        let mut written = crate::hash::HashSet::default();
        super::induction::local_uses(condition, &mut read, &mut written);
        super::induction::local_uses(body, &mut read, &mut written);
        for local in read.difference(&written).copied() {
            if self.local_address(local).is_some() {
                continue;
            }
            let ty = &self.locals[local as usize].1;
            let shared_list = matches!(ty, Ty::Ref(inner, false) if matches!(**inner, Ty::List(_)));
            let view = matches!(ty, Ty::View(..));
            if !shared_list && !view && *ty != Ty::I64 {
                continue;
            }
            let entry = self.read_local(local);
            // Inside an enclosing loop the read is a fresh header value;
            // map it to that loop's entry value, as a body read would be.
            self.note_bounds(local, entry);
            let bounds = &mut self.counters.bounds;
            let entry = bounds.canonical(&self.bcx.func.dfg, entry);
            let stop = stop.map(|stop| bounds.canonical(&self.bcx.func.dfg, stop));
            let stable = if shared_list {
                bounds.stable_lists.contains(&entry)
            } else {
                view || versioned || bounds.lengths.contains_key(&entry) || stop == Some(entry)
            };
            if !stable {
                continue;
            }
            bounds.invariant_locals.push((local, entry));
        }
    }

    pub(super) fn lower_loop(&mut self, condition: &[Op], body: &[Op]) -> Result<()> {
        self.lower_clist_simd_prefix(condition, body);
        let initial = self.stack.clone();
        // This loop's index facts start here; they cover body reads only.
        let facts = self.counters.bounds.below_locals.len();
        // An unrotated loop's body reads the header's values, which also
        // reach the exit, so its index facts would hold after a loop that
        // never ran (issue #816). Only a rotated loop records them.
        let rotated = self.rotatable(condition);
        let types: Vec<Ty> = self.locals.iter().map(|(_, ty)| ty.clone()).collect();
        let counter = super::induction::analyze(condition, body, &types)
            .filter(|counter| self.local_address(counter.local).is_none());
        let proven_step = counter.as_ref().and_then(|counter| counter.proven_step);
        let bound_step = counter
            .as_ref()
            .and_then(|counter| counter.bound_step)
            .filter(|&(local, _)| self.local_address(local).is_none())
            .map(|(_, index)| index);
        let nonnegative = counter.filter(|counter| {
            counter.only_grows && super::is_signed(types[counter.local as usize].clone()) && {
                let entry = self.read_local(counter.local);
                self.counters.nonnegative.contains(&entry)
                    || super::induction::nonnegative_constant(&self.bcx.func.dfg, entry)
            }
        });
        if let Some(counter) = &nonnegative {
            self.counters.nonnegative_locals.push(counter.local);
        }
        let mut stop = None;
        let range = super::induction::analyze_range(condition, body).filter(|range| {
            let slots = [range.target, range.remaining, range.advance];
            slots.iter().all(|&slot| self.local_address(slot).is_none()) && {
                let entry = (
                    self.read_local(range.remaining),
                    self.read_local(range.target),
                    self.read_local(range.advance),
                );
                stop = self
                    .counters
                    .ranges
                    .iter()
                    .find(|&&(length, start, step, _)| (length, start, step) == entry)
                    .map(|&(.., stop)| stop);
                stop.is_some() && super::known::known_signed(&self.bcx.func.dfg, entry.2) == Some(1)
            }
        });
        let unit_steps = range
            .as_ref()
            .map_or(&[][..], |range| &range.unit_steps[..]);
        let range_nonnegative = range.as_ref().filter(|range| {
            super::is_signed(types[range.target as usize].clone()) && {
                let first = self.read_local(range.target);
                self.nonnegative_operand(first)
            }
        });
        if let Some(range) = range_nonnegative {
            self.counters.nonnegative_locals.push(range.target);
        }
        // A cursor's length local is never written, so its entry value bounds
        // every body read of the index.
        let cursor = super::induction::analyze_cursor(condition, body).filter(|cursor| {
            [cursor.index, cursor.length]
                .iter()
                .all(|&slot| self.local_address(slot).is_none())
                && {
                    let first = self.read_local(cursor.index);
                    self.nonnegative_operand(first)
                }
        });
        let cursor_below = cursor
            .as_ref()
            .filter(|_| rotated)
            .map(|cursor| (cursor.index, self.read_local(cursor.length)));
        if let Some(below) = cursor_below {
            self.counters.nonnegative_locals.push(below.0);
            self.counters.bounds.below_locals.push(below);
        }
        // A range loop's counter, or any loop's unchanged indices; see
        // `induction::Version`.
        let versioned = (range.is_none() || (range_nonnegative.is_some() && stop.is_some()))
            && !self.gathering()
            && super::induction::versionable(condition, body);
        let invariant = self.counters.bounds.invariant_locals.len();
        self.enter_invariant_locals(condition, body, stop, versioned);
        // Each body value of the counter is an element of the range, so it
        // is below the range's stop.
        let below = range
            .as_ref()
            .zip(stop)
            .filter(|_| rotated)
            .map(|(range, stop)| (range.target, stop));
        if let Some(below) = below {
            self.counters.bounds.below_locals.push(below);
        }
        let low = nonnegative.as_ref().map(|counter| counter.local);
        let mut relation = if rotated {
            self.enter_relation(condition, body, low, &types, invariant)?
        } else {
            None
        };
        let proven = |index| {
            Some(index) == proven_step || Some(index) == bound_step || unit_steps.contains(&index)
        };
        let exit = self.bcx.create_block();
        let version = (versioned && relation.is_none()).then_some(stop);
        if let Some(stop) = version {
            // See `induction::Version`: lower an unchecked copy, then the test
            // that selects it, then, if the copy skipped any check, a checked
            // copy.
            let guard = self.bcx.create_block();
            self.bcx.ins().jump(guard, &[]);
            self.bcx.seal_block(guard);
            let bounds = &self.counters.bounds;
            let version = super::induction::Version {
                stop: stop.map(|stop| bounds.canonical(&self.bcx.func.dfg, stop)),
                entries: bounds
                    .invariant_locals
                    .iter()
                    .map(|&(local, entry)| {
                        (entry, matches!(self.locals[local as usize].1, Ty::View(..)))
                    })
                    .collect(),
                assumed: Vec::new(),
            };
            let outer = self.counters.bounds.version.replace(version);
            let fast = self.bcx.create_block();
            self.lower_loop_copy(
                condition, body, fast, exit, &proven, &mut None, facts, rotated, false,
            )?;
            let version = std::mem::replace(&mut self.counters.bounds.version, outer).unwrap();
            self.stack = initial.clone();
            self.terminated = false;
            self.bcx.switch_to_block(guard);
            if version.assumed.is_empty() {
                self.bcx.ins().jump(fast, &[]);
                self.bcx.seal_block(fast);
            } else {
                let mut holds = None;
                for (assumed, k) in version.assumed {
                    use super::induction::{Assumed, LengthOf};
                    let (Assumed::Counter(length)
                    | Assumed::Index(_, length)
                    | Assumed::Constant(_, length)) = assumed;
                    let length = match length {
                        LengthOf::Length(length) => length,
                        LengthOf::View(view) => self.bcx.ins().isplit(view).1,
                    };
                    let fits = match assumed {
                        Assumed::Index(index, _) => {
                            self.bcx.ins().icmp(IntCC::UnsignedLessThan, index, length)
                        }
                        Assumed::Constant(index, _) => {
                            self.bcx
                                .ins()
                                .icmp_imm_s(IntCC::SignedGreaterThan, length, index)
                        }
                        Assumed::Counter(_) => {
                            let stop = stop.expect("a counter assumption has a range stop");
                            // A length is nonnegative, so this cannot wrap.
                            let limit = self.bcx.ins().iadd_imm_s(length, -k);
                            self.bcx
                                .ins()
                                .icmp(IntCC::SignedLessThanOrEqual, stop, limit)
                        }
                    };
                    holds = Some(match holds {
                        Some(all) => self.bcx.ins().band(all, fits),
                        None => fits,
                    });
                }
                let checked = self.bcx.create_block();
                self.bcx.ins().brif(holds.unwrap(), fast, &[], checked, &[]);
                self.bcx.seal_block(fast);
                if !self.lower_loop_copy(
                    condition, body, checked, exit, &proven, &mut None, facts, rotated, true,
                )? {
                    self.bcx.seal_block(checked);
                }
            }
        } else {
            let header = self.bcx.create_block();
            self.bcx.ins().jump(header, &[]);
            // A rotated header has no back edge, so it is sealed early.
            if !self.lower_loop_copy(
                condition,
                body,
                header,
                exit,
                &proven,
                &mut relation,
                facts,
                rotated,
                true,
            )? {
                self.bcx.seal_block(header);
            }
        }
        if below.is_some() {
            self.counters.bounds.below_locals.pop();
        }
        self.counters.bounds.invariant_locals.truncate(invariant);
        if cursor_below.is_some() {
            self.counters.bounds.below_locals.pop();
            self.counters.nonnegative_locals.pop();
        }
        if range_nonnegative.is_some() {
            self.counters.nonnegative_locals.pop();
        }
        if nonnegative.is_some() {
            self.counters.nonnegative_locals.pop();
        }
        self.bcx.switch_to_block(exit);
        self.bcx.seal_block(exit);
        self.stack = initial;
        self.terminated = false;
        Ok(())
    }

    /// The complement of `test` when it is a compare, as a new compare of the
    /// same operands; the original becomes dead. Other conditions, such as a
    /// short-circuit join's parameter, have no free complement: inverting
    /// one costs a compare and a branch on its result.
    fn complement_compare(
        &mut self,
        test: cranelift_codegen::ir::Value,
    ) -> Option<cranelift_codegen::ir::Value> {
        use cranelift_codegen::ir::condcodes::CondCode;
        use cranelift_codegen::ir::{InstructionData, ValueDef};
        let dfg = &self.bcx.func.dfg;
        let ValueDef::Result(inst, 0) = dfg.value_def(dfg.resolve_aliases(test)) else {
            return None;
        };
        match dfg.insts[inst] {
            InstructionData::IntCompare {
                args: [a, b], cond, ..
            } => Some(self.bcx.ins().icmp(cond.complement(), a, b)),
            InstructionData::FloatCompare {
                args: [a, b], cond, ..
            } => Some(self.bcx.ins().fcmp(cond.complement(), a, b)),
            _ => None,
        }
    }

    /// Lowers one copy of a loop from its `header`, which the caller enters
    /// and seals, to `exit`. `facts` is the length of `below_locals` before
    /// this loop pushed its body facts. `rotated` is the caller's
    /// [`Self::rotatable`] result. `seal` says that `header` has all its
    /// predecessors; a rotated copy then seals it before the body and returns
    /// true. Otherwise the body's reads of variables set before the loop
    /// resolve to block parameters, and drops in debug builds cannot see
    /// that a local is empty.
    ///
    /// A [rotated](Self::rotatable) loop tests its condition in `header`
    /// once, as a guard, and again in a latch after the body, which branches
    /// back to the body: a pass runs one conditional branch and no `jmp`
    /// (issue #726). Both copies run the same ops at the same positions, and
    /// the condition still runs once before each pass and once at the end.
    #[allow(clippy::too_many_arguments)]
    fn lower_loop_copy(
        &mut self,
        condition: &[Op],
        body: &[Op],
        header: cranelift_codegen::ir::Block,
        exit: cranelift_codegen::ir::Block,
        proven: &dyn Fn(usize) -> bool,
        relation: &mut Option<(usize, usize)>,
        facts: usize,
        rotated: bool,
        seal: bool,
    ) -> Result<bool> {
        let iteration = self.bcx.create_block();
        let latch = if rotated {
            self.bcx.create_block()
        } else {
            header
        };
        let entry = (self.stack.clone(), self.position.clone(), self.bcx.srcloc());
        self.bcx.switch_to_block(header);
        if rotated && seal {
            self.bcx.seal_block(header);
        }
        let test = if rotated {
            // The relation's facts, like `facts`, hold in the body only.
            let high = relation
                .is_some()
                .then(|| self.counters.nonnegative_locals.pop())
                .flatten();
            let test = self.lower_rotated_condition(condition, facts);
            self.counters.nonnegative_locals.extend(high);
            test?
        } else {
            for op in condition {
                self.lower(op)?;
            }
            self.pop_typed(Ty::Bool)?.0
        };
        // Branch to the exit on the complemented test where one is free, so
        // the exit is the first successor. Cranelift lowers blocks in the
        // reverse postorder of a DFS that visits the first successor first,
        // so this places the body right after its header instead of after
        // everything the exit reaches. regalloc2 splits live ranges in that
        // linear order: with the body last, a value used after the loop but
        // live across it can keep a register through the body while the
        // header holds it on the stack, a reload each pass (issue #767).
        match self.complement_compare(test) {
            Some(done) => self.bcx.ins().brif(done, exit, &[], iteration, &[]),
            None => self.bcx.ins().brif(test, iteration, &[], exit, &[]),
        };
        self.bcx.switch_to_block(iteration);
        if !rotated {
            self.bcx.seal_block(iteration);
        }
        self.loop_targets.push((latch, exit));
        self.enter_loop_exits(latch, exit);
        for (index, op) in body.iter().enumerate() {
            if let Some((_, below)) = relation.take_if(|&mut (prefix, _)| prefix == index) {
                self.leave_relation(below);
            }
            if self.terminated {
                break;
            }
            if proven(index) && !self.gathering() {
                self.lower_proven_step(op)?;
            } else {
                self.lower(op)?;
            }
        }
        if let Some((_, below)) = relation.take() {
            self.leave_relation(below);
        }
        if !self.terminated {
            self.bcx.ins().jump(latch, &[]);
        }
        // The shared `break` blocks are still inside the iteration.
        self.finish_loop_exits()?;
        self.loop_targets.pop();
        if rotated {
            // The latch's condition runs where the header's did; the code
            // after the loop continues from the body's state, as without
            // rotation.
            let after = (self.position.clone(), self.bcx.srcloc());
            self.bcx.seal_block(latch);
            self.bcx.switch_to_block(latch);
            let (stack, position, srcloc) = entry;
            self.stack = stack;
            self.position = position;
            self.bcx.set_srcloc(srcloc);
            self.terminated = false;
            // A body that never falls through and has no `continue` leaves
            // the latch unreachable; an edge from it would turn the body's
            // reads into block parameters.
            if !self.bcx.is_unreachable() {
                let test = self.lower_rotated_condition(condition, facts)?;
                self.bcx.ins().brif(test, iteration, &[], exit, &[]);
            }
            self.bcx.seal_block(iteration);
            self.position = after.0;
            self.bcx.set_srcloc(after.1);
            self.fresh_load = None;
            self.tags.loaded = None;
        }
        Ok(rotated && seal)
    }

    /// Lowers one copy of a rotated loop's condition and returns its test.
    /// The loop's own `below_locals`, from `facts` on, describe body reads,
    /// so the condition's reads do not record them: the guard's values reach
    /// the exit when the loop does not run.
    fn lower_rotated_condition(
        &mut self,
        condition: &[Op],
        facts: usize,
    ) -> Result<cranelift_codegen::ir::Value> {
        let own = self.counters.bounds.below_locals.split_off(facts);
        let lowered = condition.iter().try_for_each(|op| self.lower(op));
        self.counters.bounds.below_locals.extend(own);
        lowered?;
        Ok(self.pop_typed(Ty::Bool)?.0)
    }

    /// Whether a loop with `condition` is rotated: its condition only reads
    /// scalar and view locals and computes with them, so lowering it twice
    /// runs no extra code, and no value it reads is cached across the body.
    /// Calls, collection methods other than a view's length and rows, and
    /// managed values keep the condition at the header.
    fn rotatable(&self, condition: &[Op]) -> bool {
        let plain = |ty: &Ty| ty.is_numeric() || *ty == Ty::Bool;
        condition.iter().all(|op| match op {
            Op::Site(_)
            | Op::Access(..)
            | Op::Loan(_)
            | Op::UseLoan(_)
            | Op::EndLoan(_)
            | Op::PushInt(_)
            | Op::PushBool(_)
            | Op::PushFloat { .. }
            | Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::FloorDiv(_)
            | Op::Modulo(_)
            | Op::Integer(_)
            | Op::FloatNeg
            | Op::Eq
            | Op::Ne
            | Op::Lt
            | Op::Le
            | Op::Gt
            | Op::Ge
            | Op::Not
            | Op::Dup
            | Op::Swap
            | Op::Collection(CollectionOp::ViewLen(_) | CollectionOp::ViewRow(..)) => true,
            Op::LoadLocal(i) => {
                let ty = &self.locals[*i as usize].1;
                plain(ty) || matches!(ty, Ty::View(..))
            }
            Op::Reborrow(ty) => matches!(ty, Ty::View(..)),
            Op::ReadRef(ty) | Op::Cast(ty) | Op::CheckedCast(ty) => plain(ty),
            // A short-circuit `and` or `or`.
            Op::Match(arms) => arms.iter().all(|arm| {
                matches!(arm.pattern, crate::op::Pattern::Bool(_)) && self.rotatable(&arm.body)
            }),
            _ => false,
        })
    }
}
