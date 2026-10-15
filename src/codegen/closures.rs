//! Native environments use the same typed owner-local slots as generator frames.
use super::*;
use crate::closure::ClosureType;
use cranelift_codegen::ir::MemFlags;

impl Lowerer<'_, '_> {
    pub(super) fn lower_boxed_callable_new(
        &mut self,
        source: &Ty,
        adapter: &str,
        signature: &Arc<crate::op::CallableSig>,
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
            Ty::BoxedCallable(signature.clone()),
            crate::sum::alloc_error(),
        );
        let result = self.unpack(packed, &output);
        self.stack.push((result, output));
        Ok(())
    }

    pub(super) fn lower_boxed_callable_call(
        &mut self,
        signature: &Arc<crate::op::CallableSig>,
    ) -> Result<()> {
        let split = self
            .stack
            .len()
            .checked_sub(signature.inputs.len())
            .ok_or("boxed callable call underflow")?;
        let args: Vec<_> = self.stack.drain(split..).collect();
        let (reference, _) = self.stack.pop().ok_or("missing boxed callable owner")?;
        let reference = self.raw_word(reference);
        let boxed = self
            .bcx
            .ins()
            .load(PTR_TY, MemFlags::trusted(), reference, 0);
        let code = self.bcx.ins().load(PTR_TY, MemFlags::trusted(), boxed, 24);
        let slot = self.bcx.ins().iadd_imm(boxed, 32);
        let environment = self.bcx.ins().uextend(types::I128, slot);
        // All environment references use the same native ABI; the adapter knows
        // the concrete type while this call site knows only the public signature.
        let mut adapted = (**signature).clone();
        adapted.inputs.insert(0, Ty::Ref(Arc::new(Ty::Unit), false));
        let adapted = Arc::new(adapted);
        self.stack.push((code, Ty::Callable(adapted.clone())));
        self.stack
            .push((environment, Ty::Ref(Arc::new(Ty::Unit), false)));
        self.stack.extend(args);
        self.lower_indirect_call(&adapted, false)
    }

    pub(super) fn lower_closure_new(&mut self, t: &Arc<ClosureType>) -> Result<()> {
        let ty = Ty::Closure(t.clone());
        let storage = self.inline_storage(t.bytes());
        let id = self.module.metadata(&ty);
        let gv = self.module.declare_data_in_func(id, self.bcx.func);
        let descriptor = self.bcx.ins().global_value(PTR_TY, gv);
        self.bcx
            .ins()
            .store(MemFlags::trusted(), descriptor, storage, 0);
        for (i, (_, field)) in t.captures.iter().enumerate().rev() {
            let (value, _) = self.pop_typed(field.clone())?;
            let slot = self.bcx.ins().iadd_imm(storage, t.offset(i) as i64);
            self.store_slot(slot, value, field);
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
        let storage = if t.once {
            reference
        } else {
            let reference = self.raw_word(reference);
            self.bcx
                .ins()
                .load(PTR_TY, MemFlags::trusted(), reference, 0)
        };
        for (i, (_, field)) in t.captures.iter().enumerate() {
            let slot = self.bcx.ins().iadd_imm(storage, t.offset(i) as i64);
            let pointer = if t.once {
                let packed = self
                    .bcx
                    .ins()
                    .load(types::I128, MemFlags::trusted(), slot, 0);
                self.unpack(packed, field)
            } else if matches!(field, Ty::Ref(..)) {
                self.bcx
                    .ins()
                    .load(types::I128, MemFlags::trusted(), slot, 0)
            } else {
                self.bcx.ins().uextend(types::I128, slot)
            };
            self.stack.push((pointer, t.parameter(i)));
        }
        self.stack.extend(args);
        self.lower_call(&t.name)
    }
}
