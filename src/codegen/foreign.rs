//! C calls are ordinary target-ABI calls, never Plenty tail-convention calls.
use super::*;

fn parameter(ty: &Ty) -> AbiParam {
    let parameter = AbiParam::new(clif_type(ty.clone()));
    match ty {
        Ty::I8 | Ty::I16 | Ty::I32 => parameter.sext(),
        Ty::U8 | Ty::U16 | Ty::U32 => parameter.uext(),
        _ => parameter,
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_foreign_call(&mut self, symbol: &str, sig: &FnSig) -> Result<()> {
        let mut signature = self.module.make_signature();
        signature
            .params
            .extend(sig.inputs.iter().map(|(_, t)| parameter(t)));
        signature.returns.extend(sig.outputs.iter().map(parameter));
        let id = self
            .module
            .declare_function(symbol, Linkage::Import, &signature)?;
        let callee = self.module.declare_func_in_func(id, self.bcx.func);
        let mut arguments = Vec::new();
        for (_, ty) in sig.inputs.iter().rev() {
            arguments.push(self.pop_typed(ty.clone())?.0);
        }
        arguments.reverse();
        let call = self.bcx.ins().call(callee, &arguments);
        for (value, ty) in self
            .bcx
            .inst_results(call)
            .iter()
            .copied()
            .zip(&sig.outputs)
        {
            self.stack.push((value, ty.clone()));
        }
        Ok(())
    }
}
