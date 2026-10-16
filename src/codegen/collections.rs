use super::*;
use crate::collection::CollectionOp;

impl Lowerer<'_, '_> {
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
        } else {
            self.runtime.collection
        };
        let f = self.module.declare_func_in_func(callee, self.bcx.func);
        // The runtime writes the result after the three inputs, which leaves
        // the fourth argument for the trap site.
        // Advancing a generator or dropping a value can enter std without a
        // source-level call. Within std, runtime callbacks must not overwrite
        // the surrounding caller, including callbacks that tail-call std.
        let saved = if self.in_standard_library() || matches!(opcode, 24 | 27) {
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
        if let CollectionOp::ListScalarSetRef(ty) = operation {
            let element = ty.element().unwrap();
            let (value, _) = self.pop_typed(element.clone())?;
            let (index, _) = self.pop_typed(Ty::I64)?;
            let (receiver, _) = self.pop_typed(Ty::Ref(Arc::new(ty.clone()), true))?;
            let packed = self.read_reference(receiver);
            let owner = self.raw_word(packed);
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
        let fast = match operation {
            CollectionOp::ListLenRef(_) => Some((self.runtime.list_len, true)),
            CollectionOp::ListScalarGetRef(_) => Some((self.runtime.list_scalar_get, true)),
            CollectionOp::Len(Ty::List(_)) => Some((self.runtime.list_len, false)),
            CollectionOp::Get(Ty::List(element)) | CollectionOp::IterGet(Ty::List(element))
                if element.is_numeric() || **element == Ty::Bool =>
            {
                Some((self.runtime.list_scalar_get, false))
            }
            _ => None,
        };
        if let Some((helper, borrowed)) = fast {
            let (inputs, output) = operation.signature();
            let index = if inputs.len() == 2 {
                Some(self.pop_typed(Ty::I64)?.0)
            } else {
                None
            };
            let (receiver, ty) = self.pop_typed(inputs[0].clone())?;
            let owner = if borrowed {
                let packed = self.read_reference(receiver);
                self.raw_word(packed)
            } else {
                receiver
            };
            let mut args = vec![owner];
            if let Some(index) = index {
                // Iteration stays inside the list, so only indexing names a site.
                let site = if matches!(operation, CollectionOp::IterGet(_)) {
                    self.bcx.ins().iconst(PTR_TY, 0)
                } else {
                    self.trap_site()?
                };
                args.extend([index, site]);
            }
            let function = self.module.declare_func_in_func(helper, self.bcx.func);
            let call = self.bcx.ins().call(function, &args);
            let result = self.bcx.inst_results(call)[0];
            let result = self.unpack(result, &output);
            if !borrowed {
                self.release(receiver, &ty);
            }
            self.stack.push((result, output));
            return Ok(());
        }
        let (inputs, output) = operation.signature();
        let mut values = Vec::new();
        for input in inputs.iter().rev() {
            let (value, _) = self.pop_typed(input.clone())?;
            values.push(self.pack(value, input));
        }
        values.reverse();
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
            for (value, ty) in values.iter().zip(&inputs) {
                self.release(*value, ty);
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
            if !(inserts && i > 0 && ty.affine()) {
                self.release(*value, ty);
            }
        }
        self.stack.push((result, output));
        Ok(())
    }

    pub(super) fn lower_loop(&mut self, condition: &[Op], body: &[Op]) -> Result<()> {
        let initial = self.stack.clone();
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
        for op in body {
            if self.terminated {
                break;
            }
            self.lower(op)?;
        }
        self.loop_targets.pop();
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
