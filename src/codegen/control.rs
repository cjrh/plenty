use super::*;
use crate::control::ControlOp;
use cranelift_codegen::ir::MemFlagsData;

impl Lowerer<'_, '_> {
    pub(super) fn lower_control(&mut self, operation: &ControlOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let mut values = Vec::new();
        for input in inputs.iter().rev() {
            let (value, _) = self.pop_typed(input.clone())?;
            values.push(self.pack(value, input));
        }
        values.reverse();
        let args = self.inline_storage(inputs.len().max(1) * 16);
        for (i, value) in values.iter().enumerate() {
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), *value, args, (i * 16) as i32);
        }
        let out = self.inline_storage(output.slot_bytes());
        let code = self.bcx.ins().iconst(types::I64, operation.opcode());
        let function = self
            .module
            .declare_func_in_func(self.runtime.control, self.bcx.func);
        self.bcx.ins().call(function, &[code, args, out]);
        let value = self
            .bcx
            .ins()
            .load(types::I128, MemFlagsData::trusted(), out, 0);
        let value = self.unpack(value, &output);
        if !matches!(operation, ControlOp::New) {
            self.release(values[0], &inputs[0]);
        }
        self.stack.push((value, output));
        Ok(())
    }
}
