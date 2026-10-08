//! Explicit capture construction and statically dispatched environment borrows.
use super::*;
use crate::closure::ClosureType;
use crate::op::CallableSig;

impl Lower<'_> {
    pub(super) fn closure_value(
        &mut self,
        function: &Function,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let mut function = function.clone();
        function.name = format!("__plenty_closure_{}", self.generics.functions.len());
        let mut captures = Vec::new();
        let mut hidden = Vec::new();
        for capture in &function.captures {
            if capture.borrowed || capture.mutable {
                return Err(at.error("borrowed and mutable captures are not supported yet"));
            }
            let source = Expr {
                at: at.clone(),
                kind: Expression::Name(capture.name.clone()),
            };
            let ty = self.value(&source, ops)?;
            if ty.restricted_storage() {
                return Err(at.error("this type cannot be captured yet"));
            }
            hidden.push((
                capture.name.clone(),
                TypeRef {
                    at: at.clone(),
                    concrete: Some(Ty::Ref(Rc::new(ty.clone()), false)),
                    name: None,
                    args: vec![],
                },
            ));
            captures.push((capture.name.clone(), ty));
        }
        let signature = CallableSig {
            inputs: function
                .inputs
                .iter()
                .map(|(_, t)| {
                    t.resolve(self.aliases)?
                        .ok_or_else(|| at.error("unit parameters are not supported yet"))
                })
                .collect::<Result<_>>()?,
            output: function.output.resolve(self.aliases)?,
        };
        callables::validate(&signature, at)?;
        if matches!(signature.output, Some(Ty::Ref(..))) || generators::yields(&function.body) {
            return Err(at.error("capturing closures cannot return references or yield yet"));
        }
        let closure = Rc::new(ClosureType {
            name: function.name.clone(),
            signature,
            captures,
            mutable: false,
        });
        hidden.append(&mut function.inputs);
        function.inputs = hidden;
        register_signature(&function, self.aliases, self.sigs, self.returned_fields)?;
        self.generics
            .functions
            .insert(function.name.clone(), function.clone());
        self.generics.pending.push_back(function);
        ops.push(Op::ClosureNew(closure.clone()));
        Ok(Ty::Closure(closure))
    }

    pub(super) fn call_closure(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        closure: Rc<ClosureType>,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if args.len() != closure.signature.inputs.len() {
            return Err(callee.at.error(format!(
                "closure expects {} arguments, got {}",
                closure.signature.inputs.len(),
                args.len()
            )));
        }
        let (_, loan) = self.borrow(callee, closure.mutable, ops)?;
        let loans = self.call_arguments(args, &closure.signature.function().inputs, ops)?;
        ops.push(Op::ClosureCall(closure.clone()));
        Self::end_reads(loans, ops);
        ops.push(Op::UseLoan(loan));
        Ok(closure.signature.output.clone())
    }
}
