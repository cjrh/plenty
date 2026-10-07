//! Capture-free callable values use the normal Plenty ABI.
use super::*;
use crate::op::CallableSig;

pub(super) fn validate(sig: &CallableSig, at: &Token) -> Result<()> {
    fn frame(ty: &Ty) -> bool {
        match ty {
            Ty::Generator(_) => true,
            Ty::Ref(inner, _) => frame(inner),
            Ty::Enum(t) => t.variants.iter().flat_map(|v| &v.fields).any(frame),
            _ => false,
        }
    }
    if sig.inputs.iter().chain(sig.output.iter()).any(frame) {
        return Err(at.error("callable signatures do not yet support generator frames"));
    }
    if matches!(sig.output, Some(Ty::Ref(..))) {
        return Err(at.error("callable signatures do not yet support returned references"));
    }
    Ok(())
}

impl Lower<'_> {
    pub(super) fn function_value(
        &mut self,
        name: &str,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        self.ensure_concrete_output(name, at)?;
        let sig = Rc::new(CallableSig::from_function(&self.sigs[name]));
        validate(&sig, at)?;
        ops.push(Op::FunctionAddress(name.into(), sig.clone()));
        Ok(Ty::Callable(sig))
    }

    pub(super) fn call_value(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let ty = self.value(callee, ops)?;
        let Ty::Callable(sig) = ty else {
            return Err(callee
                .at
                .error(format!("value of type {ty} is not callable")));
        };
        if args.len() != sig.inputs.len() {
            return Err(callee.at.error(format!(
                "callable expects {} arguments, got {}",
                sig.inputs.len(),
                args.len()
            )));
        }
        let loans = self.call_arguments(args, &sig.function().inputs, ops)?;
        ops.push(Op::CallIndirect(sig.clone()));
        Self::end_reads(loans, ops);
        Ok(sig.output.clone())
    }
}
