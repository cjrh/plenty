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
            let existing = self.runtime.type_data.borrow().get(ty).copied();
            let id = if let Some(id) = existing {
                id
            } else {
                let encoding = ty.descriptor();
                let id = self.module.declare_data(
                    &format!("__plenty_type_{encoding}"),
                    Linkage::Local,
                    false,
                    false,
                )?;
                let mut data = DataDescription::new();
                data.define(format!("{encoding}\0").into_bytes().into_boxed_slice());
                self.module.define_data(id, &data)?;
                self.runtime.type_data.borrow_mut().insert(ty.clone(), id);
                id
            };
            let gv = self.module.declare_data_in_func(id, self.bcx.func);
            self.bcx.ins().global_value(PTR_TY, gv)
        } else {
            zero
        };
        let args = [
            code,
            values.first().copied().unwrap_or(zero),
            values.get(1).copied().unwrap_or(zero),
            values.get(2).copied().unwrap_or(zero),
            descriptor,
        ];
        let f = self
            .module
            .declare_func_in_func(self.runtime.collection, self.bcx.func);
        let call = self.bcx.ins().call(f, &args);
        Ok(self.bcx.inst_results(call)[0])
    }

    pub(super) fn lower_collection(&mut self, operation: &CollectionOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let mut values = Vec::new();
        for input in inputs.iter().rev() {
            let (mut value, _) = self.pop_typed(input.clone())?;
            if input.is_int() || *input == Ty::Bool {
                let actual = clif_type(input.clone());
                if actual != types::I64 {
                    value = if is_signed(input.clone()) {
                        self.bcx.ins().sextend(types::I64, value)
                    } else {
                        self.bcx.ins().uextend(types::I64, value)
                    };
                }
            }
            values.push(value);
        }
        values.reverse();
        let descriptor = match operation {
            CollectionOp::New(ty) => Some(ty),
            CollectionOp::Len(Ty::Str)
            | CollectionOp::Get(Ty::Str)
            | CollectionOp::IterGet(Ty::Str)
            | CollectionOp::Contains(Ty::Str) => Some(&Ty::Str),
            _ => None,
        };
        let mut result = self.collection_call(operation.opcode(), &values, descriptor)?;
        let result_type = clif_type(output.clone());
        if result_type != types::I64 {
            result = self.bcx.ins().ireduce(result_type, result);
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
        for op in body {
            if self.terminated {
                break;
            }
            self.lower(op)?;
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
