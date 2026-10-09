//! Capture-free callable values use the normal Plenty ABI.
use super::*;
use crate::op::CallableSig;

pub(super) fn signature(ty: &Ty) -> Option<&CallableSig> {
    match ty {
        Ty::Callable(sig) => Some(sig),
        Ty::Closure(t) => Some(&t.signature),
        _ => None,
    }
}

fn output(ty: &Ty) -> Type {
    match ty {
        Ty::Callable(sig) => sig.output.clone(),
        Ty::Closure(t) => t.signature.output.clone(),
        Ty::Ref(t, _) => output(t),
        _ => None,
    }
}

pub(super) fn validate(sig: &CallableSig, at: &Token) -> Result<()> {
    fn frame(ty: &Ty) -> bool {
        match ty {
            Ty::Generator(_) | Ty::Closure(_) => true,
            Ty::Ref(inner, _) => frame(inner),
            Ty::Enum(t) if t.get().restricted_storage => {
                t.get().variants.iter().flat_map(|v| &v.fields).any(frame)
            }
            _ => false,
        }
    }
    if sig.inputs.iter().chain(sig.output.iter()).any(frame) {
        return Err(at.error(
            "callable signatures do not yet support generator frames or closure environments",
        ));
    }
    if let Some(Ty::Ref(_, mutable)) = &sig.output {
        let references: Vec<_> = sig
            .inputs
            .iter()
            .filter(|ty| matches!(ty, Ty::Ref(..)))
            .collect();
        if references.len() != 1 {
            return Err(at.error("returned references require exactly one reference parameter"));
        }
        if *mutable && !matches!(references[0], Ty::Ref(_, true)) {
            return Err(
                at.error("mutable returned references require a mutable reference parameter")
            );
        }
    }
    Ok(())
}

impl Lower<'_> {
    /// Read already-known signatures without evaluating or specializing expressions.
    pub(super) fn expression_type_hint(&self, e: &Expr) -> Type {
        match &ungroup(e).kind {
            Expression::Name(name) => self.names.get(name).map(|l| l.ty.clone()).or_else(|| {
                self.sigs
                    .get(name)
                    .map(|sig| Ty::Callable(Rc::new(CallableSig::from_function(sig))))
            }),
            Expression::Call(name, _) => {
                if name == "CancellationToken" {
                    return Some(crate::control::ControlOp::New.signature().1);
                }
                if name == "ThreadPoolExecutor" {
                    return Some(crate::executor::ExecutorOp::New.signature().1);
                }
                if let Some(local) = self.names.get(name) {
                    output(&local.ty)
                } else {
                    self.sigs
                        .get(name)
                        .and_then(|sig| sig.outputs.first().cloned())
                        .or_else(|| lookup_type(name, self.aliases).flatten())
                }
            }
            Expression::Invoke(callee, _) => output(&self.expression_type_hint(callee)?),
            Expression::GenericCall(name, types, _) => {
                self.generics.explicit_output(name, types, self.aliases)
            }
            Expression::GenericMethod(base, name, types, _) => {
                let Ty::Class(class) = self.place_type(base)? else {
                    return None;
                };
                self.generics.explicit_output(
                    &crate::record::method(&class.name, name),
                    types,
                    self.aliases,
                )
            }
            Expression::Method(base, name, _) => {
                if self.place_type(base) == Some(Ty::CancellationToken) {
                    return match name.as_str() {
                        "share" => Some(Ty::CancellationToken),
                        "is_cancelled" | "wait_timeout" => Some(Ty::Bool),
                        _ => None,
                    };
                }
                if let Some(Ty::Future(t)) = self.place_type(base) {
                    return match name.as_str() {
                        "result" => Some(
                            crate::executor::ExecutorOp::Result((*t).clone())
                                .signature()
                                .1,
                        ),
                        "done" | "cancel" | "wait_timeout" => Some(Ty::Bool),
                        _ => None,
                    };
                }
                if let Some(ty @ Ty::Channel(_, _)) = self.place_type(base) {
                    if name == "share" {
                        return Some(ty);
                    }
                    let Ty::Channel(message, sender) = ty else {
                        unreachable!()
                    };
                    if sender && name == "send_timeout" {
                        return Some(
                            crate::channel::ChannelOp::SendTimeout((*message).clone())
                                .signature()
                                .1,
                        );
                    }
                    if !sender && name == "recv_timeout" {
                        return Some(
                            crate::channel::ChannelOp::RecvTimeout((*message).clone())
                                .signature()
                                .1,
                        );
                    }
                    if sender && matches!(name.as_str(), "send" | "send_nowait") {
                        return Some(
                            crate::channel::ChannelOp::Send(
                                (*message).clone(),
                                name == "send_nowait",
                            )
                            .signature()
                            .1,
                        );
                    }
                    if !sender && matches!(name.as_str(), "recv" | "recv_nowait") {
                        return Some(
                            crate::channel::ChannelOp::Recv(
                                (*message).clone(),
                                name == "recv_nowait",
                            )
                            .signature()
                            .1,
                        );
                    }
                }
                let field = Expr {
                    at: e.at.clone(),
                    kind: Expression::Member(base.clone(), name.clone()),
                };
                output(&self.place_type(&field)?)
            }
            Expression::Member(..) | Expression::Index(..) => self.place_type(e),
            _ => None,
        }
    }

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
        if let Some(Ty::Closure(closure)) = self.place_type(callee) {
            return self.call_closure(callee, args, closure, ops);
        }
        let ty = if matches!(self.place_type(callee), Some(Ty::Callable(_))) {
            let (ty, loans) = self.observe(callee, ops)?;
            Self::end_reads(loans, ops);
            ty
        } else {
            self.value(callee, ops)?
        };
        if let Ty::Closure(closure) = ty {
            return self.call_temporary_closure(&callee.at, args, closure, ops);
        }
        let Ty::Callable(sig) = ty else {
            if let Expression::Name(name) = &ungroup(callee).kind {
                return Err(callee.at.error(format!("binding `{name}` is not callable")));
            }
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
        // A structural callable has no function-specific field projection summary.
        // Retain the full origin loan even when today's value happens to be named.
        self.call_reference_result("", &sig.function(), &loans, ops);
        Self::end_reads(loans, ops);
        Ok(sig.output.clone())
    }
}
