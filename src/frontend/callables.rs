//! Capture-free callable values use the normal Plenty ABI.
use super::*;
use crate::op::CallableSig;

pub(super) fn signature(ty: &Ty) -> Option<&CallableSig> {
    match ty {
        Ty::Callable(sig) | Ty::BoxedCallable(sig) => Some(sig),
        Ty::Closure(t) => Some(&t.signature),
        _ => None,
    }
}

/// Explain representation mismatches without suggesting a bound that cannot
/// accept the actual call signature or invocation mode.
pub(super) fn mismatch_hint(expected: &Ty, actual: &Ty) -> Option<String> {
    let (expected, actual) = match (expected, actual) {
        (Ty::Ref(expected, em), Ty::Ref(actual, am)) if em == am => {
            return mismatch_hint(expected, actual);
        }
        (Ty::Callable(expected), Ty::Closure(actual)) => (expected, actual),
        _ => return None,
    };
    let explanation =
        "; a Callable value stores only a code address and cannot hold a closure environment";
    if expected.as_ref() != &actual.signature {
        return Some(format!("{explanation}; the call signatures also differ"));
    }
    if actual.once && actual.borrowed {
        return Some(format!(
            "{explanation}; a one-shot closure with borrowed captures can only be consumed locally"
        ));
    }
    let (bound, parameter) = if actual.once {
        ("OnceCallable", "F")
    } else if actual.mutable {
        ("Callable", "&mut F")
    } else {
        ("Callable", "&F")
    };
    let requirement = expected.to_string().replacen("Callable", bound, 1);
    Some(format!("{explanation}; for a generic callback API, use `F: {requirement}` with `callback: {parameter}`"))
}

fn output(ty: &Ty) -> Type {
    match ty {
        Ty::Callable(sig) | Ty::BoxedCallable(sig) => sig.output.clone(),
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
    pub(super) fn boxed_callable(
        &mut self,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let [callback] = args else {
            return Err(at.error("BoxedCallable takes one owned callback"));
        };
        let source = self.value(callback, ops)?;
        if let Some(barrier) = source.storage_barrier() {
            return Err(callback.at.error(barrier.message("a BoxedCallable")));
        }
        let signature = match &source {
            Ty::Callable(sig) => sig.clone(),
            Ty::Closure(t) if t.once => return Err(at.error("BoxedCallable requires a reusable callback; one-shot closures cannot be erased yet")),
            Ty::Closure(t) if t.mutable => return Err(at.error("BoxedCallable requires a shared-call callback; mutable closures cannot be erased yet")),
            Ty::Closure(t) => Arc::new(t.signature.clone()),
            _ => return Err(at.error("BoxedCallable requires an owned Callable or closure")),
        };
        if matches!(signature.output, Some(Ty::Ref(..))) {
            return Err(at.error("BoxedCallable callbacks cannot return references yet"));
        }
        if signature.inputs.len() >= 256 {
            return Err(
                at.error("BoxedCallable supports at most 255 parameters plus its environment slot")
            );
        }
        let adapter = format!(
            "__plenty_boxed_callable_{}_{}_{}",
            self.function_name, at.line, at.column
        );
        if !self.sigs.contains_key(&adapter) {
            let mut adapted = signature.function();
            adapted.inputs.insert(
                0,
                (
                    "environment".into(),
                    Ty::Ref(Arc::new(source.clone()), false),
                ),
            );
            let adapted = Arc::new(adapted);
            let mut body = vec![Op::LoadLocal(0)];
            if matches!(source, Ty::Callable(_)) {
                body.push(Op::ReadRef(source.clone()));
            }
            body.extend(signature.inputs.iter().enumerate().map(|(i, t)| {
                if t.affine() {
                    Op::MoveLocal((i + 1) as u8)
                } else {
                    Op::LoadLocal((i + 1) as u8)
                }
            }));
            body.push(match &source {
                Ty::Closure(t) => Op::ClosureCall(t.clone()),
                _ => Op::CallIndirect(signature.clone()),
            });
            body.push(Op::Return);
            self.sigs.insert(adapter.clone(), adapted.clone());
            self.generics.compiled.push(Op::DefineFn(
                adapter.clone(),
                Box::new(CompiledFn {
                    location: Some(at.location().into()),
                    origins: Arc::default(),
                    generator: None,
                    sig: adapted,
                    doc: "".into(),
                    body: body.into(),
                    locals: Arc::default(),
                }),
            ));
        }
        let output = crate::sum::result(
            Ty::BoxedCallable(signature.clone()),
            crate::sum::alloc_error(),
        );
        ops.push(Op::BoxedCallableNew(Box::new(source), adapter, signature));
        Ok(output)
    }

    fn boxed_callable_arguments(
        &mut self,
        args: &[Expr],
        signature: Arc<CallableSig>,
        loan: usize,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if args.len() != signature.inputs.len() {
            return Err(at.error(format!(
                "boxed callable expects {} arguments, got {}",
                signature.inputs.len(),
                args.len()
            )));
        }
        let function = signature.function();
        let start = ops.len();
        let loans = self.call_arguments(args, &function.inputs, ops)?;
        self.mark_call(at, ops);
        self.passes(start, &function.inputs, function.outputs.first(), ops);
        ops.push(Op::BoxedCallableCall(signature.clone()));
        Self::end_reads(loans, ops);
        ops.push(Op::UseLoan(loan));
        Ok(signature.output.clone())
    }

    /// Read already-known signatures without evaluating or specializing expressions.
    pub(super) fn expression_type_hint(&self, e: &Expr) -> Type {
        match &ungroup(e).kind {
            Expression::Name(name) => self.names.get(name).map(|l| l.ty.clone()).or_else(|| {
                self.sigs
                    .get(name)
                    .map(|sig| Ty::Callable(Arc::new(CallableSig::from_function(sig))))
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
            Expression::GenericCall(name, types, _) if name == crate::atomic::ATOMIC => types
                .first()?
                .resolve(self.aliases)
                .ok()
                .flatten()
                .map(crate::atomic::ty),
            Expression::GenericCall(name, types, _) => {
                self.generics
                    .explicit_output(name, types, self.aliases, &e.at)
            }
            Expression::GenericMethod(base, name, types, _, _) => {
                let Ty::Class(class) = self.place_type(base)? else {
                    return None;
                };
                self.generics.explicit_output(
                    &crate::record::method(&class.name, name),
                    types,
                    self.aliases,
                    &e.at,
                )
            }
            Expression::Method(base, name, _, _) => {
                match self.place_type(base) {
                    Some(Ty::Atomic(element)) => {
                        return super::atomic::method_output(&element, name);
                    }
                    Some(ty) if crate::mutex::class(&ty).is_some() => {
                        let class = crate::mutex::class(&ty)?;
                        return (name == "into_inner").then(|| crate::mutex::element(class));
                    }
                    Some(Ty::CancellationToken) => {
                        return match name.as_str() {
                            "share" => Some(Ty::CancellationToken),
                            "is_cancelled" | "wait_timeout" => Some(Ty::Bool),
                            _ => None,
                        };
                    }
                    Some(Ty::Future(t)) => {
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
                    Some(ty @ Ty::Channel(_, _)) if name == "share" => return Some(ty),
                    Some(Ty::Channel(message, sender)) => {
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
                    _ => {}
                }
                let field = Expr {
                    at: e.at.clone(),
                    kind: Expression::Member(base.clone(), name.clone(), Pos::default()),
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
        let sig = Arc::new(CallableSig::from_function(&self.sigs[name]));
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
        if let Some(Ty::BoxedCallable(sig)) = self.place_type(callee) {
            let (_, loan) = self.borrow(callee, false, ops)?;
            return self.boxed_callable_arguments(args, sig, loan, &callee.at, ops);
        }
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
        if let Ty::BoxedCallable(sig) = ty {
            let slot = self.slot(Ty::BoxedCallable(sig.clone()), &callee.at)?;
            ops.push(Op::StoreLocal(slot));
            self.expression_temps.push(slot);
            let loan = self.new_loan(slot, false, None, ops);
            ops.push(Op::BorrowLocal(slot, false));
            return self.boxed_callable_arguments(args, sig, loan, &callee.at, ops);
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
        let function = sig.function();
        let start = ops.len();
        let loans = self.call_arguments(args, &function.inputs, ops)?;
        self.mark_call(&callee.at, ops);
        self.passes(start, &function.inputs, function.outputs.first(), ops);
        self.push_call(Op::CallIndirect(sig.clone()), &function.inputs, &loans, ops);
        // A structural callable has no function-specific field projection summary.
        // Retain the full origin loan even when today's value happens to be named.
        self.call_reference_result("", &function, &loans, ops);
        Self::end_reads(loans, ops);
        Ok(sig.output.clone())
    }
}
