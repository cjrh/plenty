//! Native environments use the same typed owner-local slots as generator frames.
use super::*;
use crate::closure::ClosureType;
use cranelift_codegen::ir::MemFlagsData;

/// Where a callable or protocol box's content starts: after its header,
/// content descriptor and adapter words (the runtime's `AdaptedBox`).
const ADAPTED_BOX_CONTENT: i64 = 32;

impl Lowerer<'_, '_> {
    pub(super) fn lower_boxed_callable_new(
        &mut self,
        source: &Ty,
        adapter: &str,
        signature: &Arc<crate::op::CallableSig>,
        mode: crate::op::BoxedMode,
    ) -> Result<()> {
        let (value, _) = self.pop_typed(source.clone())?;
        let value = self.pack(value, source);
        let function = self
            .module
            .declare_func_in_func(self.user_fns[adapter].id, self.bcx.func);
        let code = self.bcx.ins().func_addr(PTR_TY, function);
        let code = self.bcx.ins().uextend(types::I128, code);
        let descriptor = Ty::Box(Arc::new(source.clone()));
        let packed = self.collection_call(128, &[value, code], Some(&descriptor))?;
        let output = crate::sum::result(
            Ty::BoxedCallable(signature.clone(), mode),
            crate::sum::alloc_error(),
        );
        let result = self.unpack(packed, &output);
        self.stack.push((result, output));
        Ok(())
    }

    pub(super) fn lower_boxed_callable_call(
        &mut self,
        signature: &Arc<crate::op::CallableSig>,
        mode: crate::op::BoxedMode,
    ) -> Result<()> {
        let split = self
            .stack
            .len()
            .checked_sub(signature.inputs.len())
            .ok_or("boxed callable call underflow")?;
        let args: Vec<_> = self.stack.drain(split..).collect();
        let (owner, owner_ty) = self.stack.pop().ok_or("missing boxed callable owner")?;
        let raw = self.raw_word(owner);
        // The adapter knows the concrete type while this call site knows only
        // the public signature. A one-shot adapter takes the owned box, moves
        // its content out and frees it; the others take the content slot.
        let (boxed, environment, environment_ty) = if mode == crate::op::BoxedMode::Once {
            (raw, owner, owner_ty)
        } else {
            let boxed = self.bcx.ins().load(PTR_TY, MemFlagsData::trusted(), raw, 0);
            let slot = self.bcx.ins().iadd_imm_s(boxed, ADAPTED_BOX_CONTENT);
            let slot = self.bcx.ins().uextend(types::I128, slot);
            (boxed, slot, Ty::Ref(Arc::new(Ty::Unit), false))
        };
        let code = self
            .bcx
            .ins()
            .load(PTR_TY, MemFlagsData::trusted(), boxed, 24);
        let mut adapted = (**signature).clone();
        adapted.inputs.insert(0, environment_ty.clone());
        let adapted = Arc::new(adapted);
        self.stack.push((code, Ty::Callable(adapted.clone())));
        self.stack.push((environment, environment_ty));
        self.stack.extend(args);
        self.lower_indirect_call(&adapted, false)
    }

    /// Move a one-shot box's content out, then free the box without
    /// dropping that content.
    pub(super) fn lower_boxed_callable_take(
        &mut self,
        source: &Ty,
        signature: &Arc<crate::op::CallableSig>,
    ) -> Result<()> {
        let owner = Ty::BoxedCallable(signature.clone(), crate::op::BoxedMode::Once);
        let (boxed, _) = self.pop_typed(owner)?;
        let boxed = self.raw_word(boxed);
        let slot = self.bcx.ins().iadd_imm_s(boxed, ADAPTED_BOX_CONTENT);
        let value = self
            .bcx
            .ins()
            .load(types::I128, MemFlagsData::trusted(), slot, 0);
        let value = self.unpack(value, source);
        let value = self.snapshot_inline(value, source);
        let boxed = self.bcx.ins().uextend(types::I128, boxed);
        self.collection_call(crate::collection_opcodes::ADAPTED_BOX_TAKE, &[boxed], None)?;
        self.stack.push((value, source.clone()));
        Ok(())
    }

    pub(super) fn lower_closure_new(&mut self, t: &Arc<ClosureType>) -> Result<()> {
        let ty = Ty::Closure(t.clone());
        let storage = self.inline_storage(t.bytes());
        let id = self.module.metadata(&ty);
        let gv = self.module.declare_data_in_func(id, self.bcx.func);
        let descriptor = self.bcx.ins().symbol_value(PTR_TY, gv);
        self.bcx
            .ins()
            .store(MemFlagsData::trusted(), descriptor, storage, 0);
        let track = self.scalar_captures.enabled()
            && !t.mutable
            && !t.once
            && t.captures
                .iter()
                .all(|(_, field)| leaf_calls::scalar(field));
        let mut captured = Vec::new();
        for (i, (_, field)) in t.captures.iter().enumerate().rev() {
            let (value, _) = self.pop_typed(field.clone())?;
            if track {
                captured.push(value);
            }
            let slot = self.bcx.ins().iadd_imm_s(storage, t.offset(i) as i64);
            self.store_slot(slot, value, field);
        }
        if track {
            captured.reverse();
            self.remember_capture_values(storage, captured);
        }
        self.stack.push((storage, ty));
        Ok(())
    }

    pub(super) fn lower_closure_call(&mut self, t: &Arc<ClosureType>) -> Result<()> {
        let split = self
            .stack
            .len()
            .checked_sub(t.signature.inputs.len())
            .ok_or("closure call underflow")?;
        let args: Vec<_> = self.stack.drain(split..).collect();
        let (reference, _) = self.stack.pop().ok_or("missing closure environment")?;
        let captures = self.captured_arguments(reference);
        if let Some(captures) = &captures
            && self.user_fns[&t.name].inline.is_some()
        {
            self.stack.extend(
                captures
                    .iter()
                    .enumerate()
                    .map(|(i, value)| (*value, t.parameter(i))),
            );
            self.stack.extend(args);
            return self.lower_call_captured(&t.name, captures);
        }
        let storage = if t.once {
            reference
        } else {
            let reference = self.raw_word(reference);
            self.bcx
                .ins()
                .load(PTR_TY, MemFlagsData::trusted(), reference, 0)
        };
        for (i, (_, field)) in t.captures.iter().enumerate() {
            let slot = self.bcx.ins().iadd_imm_s(storage, t.offset(i) as i64);
            let pointer = if t.once {
                let packed = self
                    .bcx
                    .ins()
                    .load(types::I128, MemFlagsData::trusted(), slot, 0);
                self.unpack(packed, field)
            } else if matches!(field, Ty::Ref(..)) {
                self.bcx
                    .ins()
                    .load(types::I128, MemFlagsData::trusted(), slot, 0)
            } else {
                self.bcx.ins().uextend(types::I128, slot)
            };
            self.stack.push((pointer, t.parameter(i)));
        }
        self.stack.extend(args);
        self.lower_call_captured(&t.name, captures.as_deref().unwrap_or(&[]))
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_protocol_box_new(
        &mut self,
        source: &Ty,
        adapters: &[String],
        protocol: &Arc<crate::boxed::ProtocolType>,
    ) -> Result<()> {
        let (value, _) = self.pop_typed(source.clone())?;
        let value = self.pack(value, source);
        let table = self.module.protocol_table(
            adapters
                .iter()
                .map(|adapter| self.user_fns[adapter].id)
                .collect(),
        );
        let table = self.module.declare_data_in_func(table, self.bcx.func);
        let pointer = self.bcx.ins().symbol_value(PTR_TY, table);
        let pointer = self.bcx.ins().uextend(types::I128, pointer);
        let descriptor = Ty::Box(Arc::new(source.clone()));
        let packed = self.collection_call(128, &[value, pointer], Some(&descriptor))?;
        let output = crate::sum::result(
            Ty::BoxedProtocol(protocol.clone()),
            crate::sum::alloc_error(),
        );
        let result = self.unpack(packed, &output);
        self.stack.push((result, output));
        Ok(())
    }

    pub(super) fn lower_protocol_box_call(
        &mut self,
        protocol: &Arc<crate::boxed::ProtocolType>,
        index: usize,
    ) -> Result<()> {
        let method = &protocol.methods[index];
        let split = self
            .stack
            .len()
            .checked_sub(method.signature.inputs.len())
            .ok_or("protocol call underflow")?;
        let args: Vec<_> = self.stack.drain(split..).collect();
        let (reference, _) = self.stack.pop().ok_or("missing protocol receiver")?;
        let reference = self.raw_word(reference);
        let boxed = self
            .bcx
            .ins()
            .load(PTR_TY, MemFlagsData::trusted(), reference, 0);
        let table = self
            .bcx
            .ins()
            .load(PTR_TY, MemFlagsData::trusted(), boxed, 24);
        let code = self.bcx.ins().load(
            PTR_TY,
            MemFlagsData::trusted(),
            table,
            i32::try_from(index * 8)?,
        );
        let slot = self.bcx.ins().iadd_imm_s(boxed, 32);
        let environment = self.bcx.ins().uextend(types::I128, slot);
        let receiver = Ty::Ref(Arc::new(Ty::Unit), method.mutable);
        let mut adapted = method.signature.clone();
        adapted.inputs.insert(0, receiver.clone());
        let adapted = Arc::new(adapted);
        self.stack.push((code, Ty::Callable(adapted.clone())));
        self.stack.push((environment, receiver));
        self.stack.extend(args);
        self.lower_indirect_call(&adapted, false)
    }
}
