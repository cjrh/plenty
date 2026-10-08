//! Native environments use the same typed owner-local slots as generator frames.
use super::*;
use crate::closure::ClosureType;
use cranelift_codegen::ir::MemFlags;

impl Lowerer<'_, '_> {
    pub(super) fn lower_closure_new(&mut self, t: &Rc<ClosureType>) -> Result<()> {
        let ty = Ty::Closure(t.clone());
        let storage = self.inline_storage(t.bytes());
        let id = metadata::declare(self.module, self.runtime, &ty)?;
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

    pub(super) fn lower_closure_call(&mut self, t: &Rc<ClosureType>) -> Result<()> {
        let split = self
            .stack
            .len()
            .checked_sub(t.signature.inputs.len())
            .ok_or("closure call underflow")?;
        let args: Vec<_> = self.stack.drain(split..).collect();
        let (reference, _) = self.stack.pop().ok_or("missing closure environment")?;
        let storage = self
            .bcx
            .ins()
            .load(PTR_TY, MemFlags::trusted(), reference, 0);
        for (i, (_, field)) in t.captures.iter().enumerate() {
            let slot = self.bcx.ins().iadd_imm(storage, t.offset(i) as i64);
            self.stack
                .push((slot, Ty::Ref(Rc::new(field.clone()), t.mutable)));
        }
        self.stack.extend(args);
        self.lower_call(&t.name)
    }
}
