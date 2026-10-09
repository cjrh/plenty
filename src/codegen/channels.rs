use super::*;
use crate::channel::ChannelOp;
use cranelift_codegen::ir::MemFlags;

impl Lowerer<'_, '_> {
    pub(super) fn lower_channel(&mut self, operation: &ChannelOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let mut values = Vec::new();
        for input in inputs.iter().rev() {
            let (value, _) = self.pop_typed(input.clone())?;
            values.push(self.pack(value, input));
        }
        values.reverse();
        let args = self.inline_storage(inputs.len() * 16);
        for (i, value) in values.iter().enumerate() {
            self.bcx
                .ins()
                .store(MemFlags::trusted(), *value, args, (i * 16) as i32);
        }
        let out = self.inline_storage(output.slot_bytes());
        let id = metadata::declare(self.module, self.runtime, &output)?;
        let gv = self.module.declare_data_in_func(id, self.bcx.func);
        let descriptor = self.bcx.ins().global_value(PTR_TY, gv);
        let code = self.bcx.ins().iconst(types::I64, operation.opcode());
        let function = self
            .module
            .declare_func_in_func(self.runtime.channel, self.bcx.func);
        self.bcx
            .ins()
            .call(function, &[code, args, descriptor, out]);
        let result = self
            .bcx
            .ins()
            .load(types::I128, MemFlags::trusted(), out, 0);
        let result = self.unpack(result, &output);
        if !matches!(operation, ChannelOp::New(_)) {
            // The observed endpoint is temporary ownership. Message ownership
            // has moved either into the ring or into the returned error.
            self.release(values[0], &inputs[0]);
        }
        self.stack.push((result, output));
        Ok(())
    }
}
