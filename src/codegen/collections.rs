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
    /// A runtime helper that answers `operation` directly: no argument block,
    /// descriptor, or caller frame, because it never calls back into Plenty
    /// (issue #364). It takes the receiver word (`true`: read through the shared
    /// reference the frontend keeps under its loan; `false`: owned and released
    /// here), the remaining inputs as words, and then the site, if any.
    fn narrow_helper(&self, operation: &CollectionOp) -> Option<(FuncId, bool, Site)> {
        use crate::bytes::BytesOp;
        let runtime = &self.runtime;
        Some(match operation {
            CollectionOp::ListLenRef(_) => (runtime.list_len, true, Site::None),
            CollectionOp::ListRowsRef(_) => (runtime.list_rows, true, Site::None),
            CollectionOp::ListRowBytesRef(_) => (runtime.list_row_bytes, true, Site::None),
            CollectionOp::ListScalarGetRef(_) => (runtime.list_scalar_get, true, Site::Trap),
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
            CollectionOp::Text(
                crate::text::TextOp::FindIn | crate::text::TextOp::FindInLiteral,
            ) => (runtime.text_find_in, true, Site::None),
            CollectionOp::QueryRef(query) => {
                let helper = match query {
                    Query::BytesLen => runtime.bytes_len,
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
                let missing = b.ins().icmp_imm(IntCC::Equal, found, -1);
                let zero = b.ins().iconst(types::I64, 0);
                let low = b.ins().select(missing, zero, found);
                let some = b.ins().icmp_imm(IntCC::NotEqual, found, -1);
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
        let failed = b.ins().icmp_imm(IntCC::Equal, code, -1);
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
    /// counted loops. Invalid parameters take a cold call to the range
    /// constructor, so its zero-step and oversized-length failures keep
    /// their messages and site.
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
        let forward = b.ins().icmp_imm(IntCC::SignedGreaterThan, step, 0);
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
        let zero_step = b.ins().icmp_imm(IntCC::Equal, step, 0);
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
            let divisor = b.ins().select(zero_step, one, magnitude);
            let last = b.ins().isub(distance, one);
            let quotient = b.ins().udiv(last, divisor);
            b.ins().iadd(quotient, one)
        };
        let zero = b.ins().iconst(types::I64, 0);
        let length = b.ins().select(nonempty, count, zero);
        let oversized = b.ins().icmp_imm(IntCC::SignedLessThan, length, 0);
        let invalid = b.ins().bor(zero_step, oversized);
        let cold = b.create_block();
        let after = b.create_block();
        b.ins().brif(invalid, cold, &[], after, &[]);
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
        let flags = cranelift_codegen::ir::MemFlags::trusted();
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

    /// `site` is the trap site of an operation that can trap.
    fn collection_call_at(
        &mut self,
        opcode: i64,
        values: &[cranelift_codegen::ir::Value],
        ty: Option<&Ty>,
        site: Option<cranelift_codegen::ir::Value>,
    ) -> Result<cranelift_codegen::ir::Value> {
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let code = self.bcx.ins().iconst(types::I64, opcode);
        let descriptor = if let Some(ty) = ty {
            let id = self.module.metadata(ty);
            let gv = self.module.declare_data_in_func(id, self.bcx.func);
            self.bcx.ins().global_value(PTR_TY, gv)
        } else {
            zero
        };
        let slot = if let Some(slot) = self.collection_scratch {
            slot
        } else {
            let slot = self
                .bcx
                .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    96,
                    4,
                ));
            self.collection_scratch = Some(slot);
            slot
        };
        let args = self.bcx.ins().stack_addr(PTR_TY, slot, 0);
        for i in 0..3 {
            let value = values.get(i).copied().unwrap_or(zero);
            let value = if self.bcx.func.dfg.value_type(value) == types::I128 {
                value
            } else {
                self.bcx.ins().uextend(types::I128, value)
            };
            self.bcx.ins().store(
                cranelift_codegen::ir::MemFlags::trusted(),
                value,
                args,
                (i * 16) as i32,
            );
        }
        // `std.bytes` range copies take up to five operands; the two beyond
        // the fixed inputs travel in the scratch words after the result.
        for (i, value) in values.iter().enumerate().skip(3).take(2) {
            let value = if self.bcx.func.dfg.value_type(*value) == types::I128 {
                *value
            } else {
                self.bcx.ins().uextend(types::I128, *value)
            };
            self.bcx.ins().store(
                cranelift_codegen::ir::MemFlags::trusted(),
                value,
                args,
                ((i + 1) * 16) as i32,
            );
        }
        let callee = if (300..320).contains(&opcode) {
            self.runtime.crypto
        } else if (320..340).contains(&opcode) {
            self.runtime.compress
        } else if (440..470).contains(&opcode) {
            self.runtime.sqlite
        } else if (470..500).contains(&opcode) {
            self.runtime.tls
        } else {
            self.runtime.collection
        };
        let f = self.module.declare_func_in_func(callee, self.bcx.func);
        // The runtime writes the result after the three inputs, which leaves
        // the fourth argument for the trap site.
        // Advancing a generator or dropping a value can enter std without a
        // source-level call, so user code names itself as the caller. The
        // runtime restores the context after every callback into compiled
        // code, so std needs nothing here.
        let saved = if !self.in_standard_library() && matches!(opcode, 24 | 27) {
            self.enter_call()?
        } else {
            None
        };
        self.bcx
            .ins()
            .call(f, &[code, args, descriptor, site.unwrap_or(zero)]);
        self.leave_call(saved);
        Ok(self.bcx.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlags::trusted(),
            args,
            48,
        ))
    }

    pub(super) fn lower_collection(&mut self, operation: &CollectionOp) -> Result<()> {
        if let CollectionOp::CallerLocation = operation {
            let function = self
                .module
                .declare_func_in_func(self.runtime.caller_location, self.bcx.func);
            let call = self.bcx.ins().call(function, &[]);
            let written = self.bcx.inst_results(call)[0];
            self.stack.push((written, Ty::Bool));
            return Ok(());
        }
        if let CollectionOp::TestFault = operation {
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
        if let CollectionOp::ListScalarSetRef(ty) = operation {
            let element = ty.element().unwrap();
            let (value, _) = self.pop_typed(element.clone())?;
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), true))?;
            let owner = self.reference_word(receiver);
            let packed = self.pack(value, &element);
            let bits = self.raw_word(packed);
            let site = self.trap_site()?;
            let function = self
                .module
                .declare_func_in_func(self.runtime.list_scalar_set, self.bcx.func);
            self.bcx.ins().call(function, &[owner, index, bits, site]);
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
            let environment = self.bcx.ins().ireduce(PTR_TY, callback);
            let adapter = self.runtime.sort_adapters[&sort.adapter];
            let adapter = self.module.declare_func_in_func(adapter, self.bcx.func);
            let adapter = self.bcx.ins().func_addr(PTR_TY, adapter);
            let descending = self.bcx.ins().uextend(types::I64, descending);
            let function = self
                .module
                .declare_func_in_func(self.runtime.list_sort_by, self.bcx.func);
            self.bcx
                .ins()
                .call(function, &[owner, adapter, environment, descending]);
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
        if let CollectionOp::ElementRef(ty @ Ty::List(element), mutable) = operation {
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), *mutable))?;
            let owner = self.reference_word(receiver);
            let site = self.trap_site()?;
            let function = self
                .module
                .declare_func_in_func(self.runtime.list_row, self.bcx.func);
            let call = self.bcx.ins().call(function, &[owner, index, site]);
            let address = self.bcx.inst_results(call)[0];
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
                // word and integer arguments as full words.
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
            let borrowed = borrowed || self.forget_fresh_load(receiver);
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
        let (inputs, output) = operation.signature();
        // With only text and scalar operands, the dispatcher runs no Plenty
        // code and reads the texts without keeping them, so a text copy that
        // is still held by its source is read in place (issue #387).
        let reads_in_place = inputs
            .iter()
            .all(|input| *input == Ty::Str || input.is_numeric() || *input == Ty::Bool);
        let mut values = Vec::new();
        let mut held = Vec::new();
        for input in inputs.iter().rev() {
            let (value, _) = self.pop_typed(input.clone())?;
            held.push(reads_in_place && *input == Ty::Str && self.forget_slot_copy(value));
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
                    cranelift_codegen::ir::MemFlags::trusted(),
                    *value,
                    address,
                    (i * 16) as i32,
                );
            }
            let metadata = crate::sum::tuple(types.clone());
            let result = self.collection_call(operation.opcode(), &[address], Some(&metadata))?;
            let result = self.unpack(result, &output);
            for ((value, ty), held) in values.iter().zip(&inputs).zip(held) {
                if !held {
                    self.release(*value, ty);
                }
            }
            self.stack.push((result, output));
            return Ok(());
        }
        let descriptor = match operation {
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
            CollectionOp::TryNew(ty) | CollectionOp::ListTrySlice(ty) => Some(ty),
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
        let result = self.collection_call_at(operation.opcode(), &arguments, descriptor, site)?;
        let result = self.unpack(result, &output);
        // An affine element read is a retained view into the borrowed
        // collection. Copying its storage would split the instance in two.
        let result = if matches!(operation, CollectionOp::Get(_)) && output.affine() {
            result
        } else {
            self.snapshot_inline(result, &output)
        };
        // Inserted affine values move into the collection; the runtime
        // retains only the copyable operands it stores.
        let inserts = matches!(
            operation,
            CollectionOp::Insert(_) | CollectionOp::TryInsert(_)
        );
        for (i, (value, ty)) in values.iter().zip(&inputs).enumerate() {
            if !(held[i] || inserts && i > 0 && ty.affine()) {
                self.release(*value, ty);
            }
        }
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
        let descriptor = self.bcx.ins().global_value(PTR_TY, gv);
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
                let bits = ins.bitcast(types::I64, cranelift_codegen::ir::MemFlags::new(), value);
                let sign = self.bcx.ins().sshr_imm(bits, 63);
                let mask = self.bcx.ins().ushr_imm(sign, 1);
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

    pub(super) fn lower_loop(&mut self, condition: &[Op], body: &[Op]) -> Result<()> {
        let initial = self.stack.clone();
        let types: Vec<Ty> = self.locals.iter().map(|(_, ty)| ty.clone()).collect();
        let counter = super::induction::analyze(condition, body, &types)
            .filter(|counter| self.local_address(counter.local).is_none());
        let proven_step = counter.as_ref().and_then(|counter| counter.proven_step);
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
        let header = self.bcx.create_block();
        let iteration = self.bcx.create_block();
        let exit = self.bcx.create_block();
        self.bcx.ins().jump(header, &[]);
        self.bcx.switch_to_block(header);
        for op in condition {
            self.lower(op)?;
        }
        let (test, _) = self.pop_typed(Ty::Bool)?;
        self.bcx.ins().brif(test, iteration, &[], exit, &[]);
        self.bcx.switch_to_block(iteration);
        self.bcx.seal_block(iteration);
        self.loop_targets.push((header, exit));
        for (index, op) in body.iter().enumerate() {
            if self.terminated {
                break;
            }
            if Some(index) == proven_step {
                self.lower_proven_step(op)?;
            } else {
                self.lower(op)?;
            }
        }
        self.loop_targets.pop();
        if nonnegative.is_some() {
            self.counters.nonnegative_locals.pop();
        }
        if !self.terminated {
            self.bcx.ins().jump(header, &[]);
        }
        self.bcx.seal_block(header);
        self.bcx.switch_to_block(exit);
        self.bcx.seal_block(exit);
        self.stack = initial;
        self.terminated = false;
        Ok(())
    }
}
