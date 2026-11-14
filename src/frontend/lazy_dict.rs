//! Lazy insertion keeps a native probe private while ordinary checked Plenty
//! calls construct the value. No native borrow crosses the user callback.
use super::*;
use crate::sum::EnumOp;

impl Lower<'_> {
    pub(super) fn dict_get_or_insert_with(
        &mut self,
        base: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let [key_arg, factory_arg] = args else {
            return Err(base
                .at
                .error("get_or_insert_with requires a key and a zero-argument factory"));
        };
        let ty = classes::unbox(self.place_type(base).expect("dictionary place"));
        let Ty::Dict(key_ty, value_ty) = &ty else {
            unreachable!()
        };
        let start = self.locals.len();
        let key_actual = self.expr_expected(key_arg, Some((**key_ty).clone()), ops)?;
        self.same(key_actual, Some((**key_ty).clone()), &key_arg.at)?;
        let key = self.slot((**key_ty).clone(), &key_arg.at)?;
        ops.push(Op::StoreLocal(key));
        let factory_ty = self.value(factory_arg, ops)?;
        let factory_loan = factory_ty
            .carries_loan()
            .then(|| self.reference_origin(factory_arg, ops))
            .transpose()?;
        let mut callable = &factory_ty;
        while let Ty::Ref(inner, _) = callable {
            callable = inner;
        }
        let signature = callables::signature(callable)
            .ok_or_else(|| {
                factory_arg
                    .at
                    .error("get_or_insert_with requires a zero-argument function or closure")
            })?
            .clone();
        if !signature.inputs.is_empty() {
            return Err(factory_arg
                .at
                .error("get_or_insert_with factory must take no arguments"));
        }
        let returned = signature.output.clone().unwrap_or(Ty::Unit);
        let (made, factory_error) = match &returned {
            _ if returned == **value_ty => (returned.clone(), None),
            Ty::Enum(sum) if sum.is_result() => (
                sum.get().variants[0].fields[0].clone(),
                Some(sum.get().variants[1].fields[0].clone()),
            ),
            _ => (returned.clone(), None),
        };
        self.same(Some(made), Some((**value_ty).clone()), &factory_arg.at)?;
        let factory = self.slot(factory_ty.clone(), &factory_arg.at)?;
        ops.push(Op::StoreLocal(factory));
        if let Some(loan) = factory_loan {
            self.reference_locals.insert(factory, loan);
        }
        let name = format!("<dictionary factory {factory}>");
        self.bind(
            name.clone(),
            Local {
                slot: factory,
                ty: factory_ty,
                mutable: true,
            },
        );
        let callee = Expr {
            at: factory_arg.at.clone(),
            kind: Expression::Name(name.clone()),
        };
        let (reference, loan) = self.borrow(base, true, ops)?;
        let reference = self.unbox_reference(reference, ops);
        let receiver = self.slot(reference, &base.at)?;
        ops.push(Op::StoreLocal(receiver));
        self.reference_locals.insert(receiver, loan);
        let state_ty = crate::sum::tuple(vec![Ty::U64; 4]);
        let Ty::Enum(state_sum) = &state_ty else {
            unreachable!()
        };
        let state = self.slot(state_ty.clone(), &base.at)?;
        let error = factory_error
            .as_ref()
            .map_or_else(crate::sum::alloc_error, |error| {
                crate::sum::dict_init_error(error.clone())
            });
        let output = crate::sum::result(Ty::Ref(value_ty.clone(), true), error.clone());
        let Ty::Enum(result_sum) = &output else {
            unreachable!()
        };
        let result = self.slot(output.clone(), &base.at)?;
        ops.extend([
            Op::LoadLocal(receiver),
            Op::LoadLocal(key),
            Op::Collection(CollectionOp::DictLazyProbe(ty.clone())),
            Op::StoreLocal(state),
            Op::LoadLocal(state),
            Op::Enum(EnumOp::Field(state_sum.clone(), 0, 0)),
            Op::PushInt(Value::U64(0)),
            Op::Ne,
        ]);
        let hit = vec![
            Op::LoadLocal(state),
            Op::Collection(CollectionOp::DictLazyHit(ty.clone())),
            Op::Enum(EnumOp::New(result_sum.clone(), 0)),
            Op::StoreLocal(result),
        ];
        let mut miss = Vec::new();
        let called = self.call_value(&callee, &[], &mut miss)?;
        self.same(called, signature.output.clone(), &factory_arg.at)?;
        if signature.output.is_none() {
            miss.push(Op::PushUnit);
        }
        let value = self.slot((**value_ty).clone(), &factory_arg.at)?;
        let mut insert = vec![
            Op::StoreLocal(value),
            Op::LoadLocal(receiver),
            Op::MoveLocal(key),
            Op::MoveLocal(value),
            Op::LoadLocal(state),
            Op::Collection(CollectionOp::DictLazyFinish(ty.clone())),
        ];
        if factory_error.is_some() {
            let allocated =
                crate::sum::result(Ty::Ref(value_ty.clone(), true), crate::sum::alloc_error());
            let Ty::Enum(allocated_sum) = allocated else {
                unreachable!()
            };
            let pending = self.slot(Ty::Enum(allocated_sum.clone()), &base.at)?;
            let Ty::Enum(error_sum) = &error else {
                unreachable!()
            };
            insert.extend([
                Op::StoreLocal(pending),
                Op::LoadLocal(pending),
                Op::Enum(EnumOp::Tag(allocated_sum.clone())),
                Op::PushInt(Value::I64(0)),
                Op::Eq,
                branch(
                    vec![
                        Op::MoveLocal(pending),
                        Op::Enum(EnumOp::Take(allocated_sum.clone(), 0, 0)),
                        Op::Enum(EnumOp::New(result_sum.clone(), 0)),
                        Op::StoreLocal(result),
                    ],
                    vec![
                        Op::MoveLocal(pending),
                        Op::Enum(EnumOp::Take(allocated_sum, 1, 0)),
                        Op::Enum(EnumOp::New(error_sum.clone(), 1)),
                        Op::Enum(EnumOp::New(result_sum.clone(), 1)),
                        Op::StoreLocal(result),
                    ],
                ),
            ]);
        } else {
            insert.push(Op::StoreLocal(result));
        }
        if factory_error.is_some() {
            let Ty::Enum(factory_sum) = returned else {
                unreachable!()
            };
            let made = self.slot(Ty::Enum(factory_sum.clone()), &factory_arg.at)?;
            let Ty::Enum(error_sum) = error else {
                unreachable!()
            };
            let mut success = vec![
                Op::MoveLocal(made),
                Op::Enum(EnumOp::Take(factory_sum.clone(), 0, 0)),
            ];
            success.extend(insert);
            miss.extend([
                Op::StoreLocal(made),
                Op::LoadLocal(made),
                Op::Enum(EnumOp::Tag(factory_sum.clone())),
                Op::PushInt(Value::I64(0)),
                Op::Eq,
                branch(
                    success,
                    vec![
                        Op::MoveLocal(made),
                        Op::Enum(EnumOp::Take(factory_sum, 1, 0)),
                        Op::Enum(EnumOp::New(error_sum, 0)),
                        Op::Enum(EnumOp::New(result_sum.clone(), 1)),
                        Op::StoreLocal(result),
                    ],
                ),
            ]);
        } else {
            miss.extend(insert);
        }
        ops.push(branch(hit, miss));
        // Captures, including uncalled factories on hits, end at this call.
        // Only the receiver's origin is published for the returned reference.
        ops.push(Op::MoveLocal(result));
        self.cleanup(start, ops);
        self.names.remove(&name);
        if let Some(loan) = factory_loan {
            ops.push(Op::UseLoan(loan));
        }
        let id = self.new_loan(self.loans[loan].root, true, Some(loan), ops);
        self.loans[id].precise = false;
        if let Some(Op::Loan(fact)) = ops.last_mut() {
            *fact = self.loans[id].clone();
        }
        ops.push(Op::UseLoan(loan));
        Ok(Some(output))
    }
}

fn branch(then: Vec<Op>, otherwise: Vec<Op>) -> Op {
    Op::If {
        then: then.into(),
        elifs: [].into(),
        otherwise: otherwise.into(),
    }
}
