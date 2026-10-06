use super::*;
use crate::collection::CollectionOp;

impl Lowerer<'_, '_> {
    pub(super) fn collection_call(
        &mut self,
        opcode: i64,
        values: &[cranelift_codegen::ir::Value],
        ty: Option<&Ty>,
    ) -> Result<cranelift_codegen::ir::Value> {
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let code = self.bcx.ins().iconst(types::I64, opcode);
        let descriptor = if let Some(ty) = ty {
            let id = metadata::declare(self.module, self.runtime, ty)?;
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
                    64,
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
        let out = self.bcx.ins().iadd_imm(args, 48);
        let f = self
            .module
            .declare_func_in_func(self.runtime.collection, self.bcx.func);
        self.bcx.ins().call(f, &[code, args, descriptor, out]);
        Ok(self.bcx.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlags::trusted(),
            out,
            0,
        ))
    }

    pub(super) fn lower_collection(&mut self, operation: &CollectionOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let mut values = Vec::new();
        for input in inputs.iter().rev() {
            let (value, _) = self.pop_typed(input.clone())?;
            values.push(self.pack(value, input));
        }
        values.reverse();
        let descriptor = match operation {
            CollectionOp::Next(_) | CollectionOp::Values(_) | CollectionOp::Range => Some(&output),
            CollectionOp::Copy(ty) | CollectionOp::TryCopy(ty) => Some(ty),
            CollectionOp::New(ty) | CollectionOp::TryNew(ty) => Some(ty),
            CollectionOp::Len(Ty::Str)
            | CollectionOp::Get(Ty::Str)
            | CollectionOp::IterGet(Ty::Str)
            | CollectionOp::Contains(Ty::Str) => Some(&Ty::Str),
            CollectionOp::TextByteLen | CollectionOp::TextAtByte => Some(&Ty::Str),
            _ => None,
        };
        let result = self.collection_call(operation.opcode(), &values, descriptor)?;
        for (value, ty) in values.iter().zip(&inputs) {
            self.release(*value, ty);
        }
        let result = self.unpack(result, &output);
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
