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
        function.name = format!(
            "__plenty_closure_{}_{}_{}",
            self.function_name, at.line, at.column
        );
        let mut captures = Vec::new();
        let mut hidden = Vec::new();
        let mut loans = Vec::new();
        for capture in &function.captures {
            let source = Expr {
                at: at.clone(),
                kind: Expression::Name(capture.name.clone()),
            };
            let ty = if capture.borrowed {
                if self.yield_type.is_some() {
                    return Err(at.error("borrowed captures cannot be suspended in generators"));
                }
                let (ty, loan) = self.borrow(&source, capture.mutable, ops)?;
                loans.push(loan);
                ty
            } else {
                self.value(&source, ops)?
            };
            if !capture.borrowed && ty.restricted_storage() {
                return Err(at.error("this type cannot be captured yet"));
            }
            let parameter = if capture.borrowed {
                ty.clone()
            } else {
                Ty::Ref(Rc::new(ty.clone()), capture.mutable)
            };
            hidden.push((
                capture.name.clone(),
                TypeRef {
                    at: at.clone(),
                    concrete: Some(parameter),
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
            writable: function.captures.iter().map(|c| c.mutable).collect(),
            mutable: function.captures.iter().any(|c| c.mutable),
        });
        hidden.append(&mut function.inputs);
        function.inputs = hidden;
        if !self.sigs.contains_key(&function.name) {
            register_signature(&function, self.aliases, self.sigs, self.returned_fields)?;
            self.generics
                .functions
                .insert(function.name.clone(), function.clone());
            self.generics.pending.push_back(function);
        }
        self.closure_loans
            .insert(closure.name.clone(), loans.clone());
        ops.push(Op::ClosureNew(closure.clone()));
        Self::end_reads(loans, ops);
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
        if let Some(loans) = self.closure_loans.get(&closure.name) {
            ops.extend(loans.iter().copied().map(Op::UseLoan));
        }
        Ok(closure.signature.output.clone())
    }
}
