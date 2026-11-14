//! Capture-free callable values use the normal Plenty ABI.
use super::*;
use crate::op::{BoxedMode, CallableSig};

pub(super) fn signature(ty: &Ty) -> Option<&CallableSig> {
    match ty {
        Ty::Callable(sig) | Ty::BoxedCallable(sig, _) => Some(sig),
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
        (Ty::Closure(expected), Ty::Closure(actual)) if expected.signature == actual.signature => {
            return Some(producer_hint(expected, actual));
        }
        (Ty::BoxedCallable(expected, to), Ty::BoxedCallable(actual, from))
            if expected == actual && to != from =>
        {
            return Some(format!(
                "; boxes do not convert between modes: box the original callback with `{}(callback)?`",
                to.name()
            ));
        }
        (domain, integer) if domain.domain_element().as_ref() == Some(integer) => {
            return Some(format!(
                "; convert it with `{}.try_from(value)?`{}",
                domain.integer_domain().unwrap().0.name(),
                if domain.nonzero_element().is_some() {
                    ", which rejects zero"
                } else {
                    ""
                }
            ));
        }
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
    Some(format!(
        "{explanation}; for a generic callback API, use `F: {requirement}` with `callback: {parameter}`"
    ))
}

/// Two closure expressions never share an environment type, even with equal
/// signatures; point at the explicit erased owner that accepts both.
fn producer_hint(
    expected: &crate::closure::ClosureType,
    actual: &crate::closure::ClosureType,
) -> String {
    let explanation = "; each closure expression has its own environment type";
    if expected.borrowed || actual.borrowed {
        return format!(
            "{explanation}; a closure with borrowed captures cannot be stored, so pass each to a generic `F: Callable[...]` parameter"
        );
    }
    let mode = if expected.once || actual.once {
        BoxedMode::Once
    } else if expected.mutable || actual.mutable {
        BoxedMode::Mut
    } else {
        BoxedMode::Shared
    };
    let owner = Ty::BoxedCallable(Arc::new(expected.signature.clone()), mode);
    format!(
        "{explanation}; to return or store either one, box each as {owner} with `{}(closure)?`",
        mode.name()
    )
}

fn output(ty: &Ty) -> Type {
    match ty {
        Ty::Callable(sig) | Ty::BoxedCallable(sig, _) => sig.output.clone(),
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
    if let Some(Ty::Ref(inner, _)) = sig
        .inputs
        .iter()
        .find(|ty| matches!(ty, Ty::Ref(inner, _) if inner.carried_reference().is_some()))
    {
        return Err(at.error(format!(
            "callable parameters cannot borrow {inner}: take the {inner} itself, which lends its borrow to the call"
        )));
    }
    if let Some(output) = sig.output.as_ref().filter(|ty| ty.carries_loan()) {
        let Some(origin) = sig.inputs.iter().find(|ty| references::lends(ty)) else {
            return Err(at.error(super::no_origin(output)));
        };
        if output.carries_mutable_loan() && !references::lends_mutably(origin) {
            return Err(at.error(super::immutable_origin(output)));
        }
    }
    Ok(())
}

impl Lower<'_> {
    pub(super) fn boxed_callable(
        &mut self,
        args: &[Expr],
        mode: BoxedMode,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let name = mode.name();
        let [callback] = args else {
            return Err(at.error(format!("{name} takes one owned callback")));
        };
        let source = self.value(callback, ops)?;
        if let Some(barrier) = source.storage_barrier() {
            return Err(callback.at.error(barrier.message(&format!("a {name}"))));
        }
        let signature = match (&source, mode.accepts(&source)) {
            (Ty::Callable(sig), _) => sig.clone(),
            (_, Some(sig)) => Arc::new(sig.clone()),
            (Ty::Closure(t), None) if t.once => {
                return Err(at.error(format!(
                    "{name} cannot call a one-shot closure, which a call consumes; use BoxedOnceCallable"
                )));
            }
            (Ty::Closure(_), None) => {
                return Err(at.error(format!(
                    "{name} cannot call a mutable closure through a shared borrow; use BoxedMutCallable"
                )));
            }
            (Ty::BoxedCallable(_, from), None) => {
                return Err(at.error(format!(
                    "{name} requires an owned Callable or closure, got a {}; boxes do not convert between modes, so box the original callback with {name}",
                    from.name()
                )));
            }
            _ => return Err(at.error(format!("{name} requires an owned Callable or closure"))),
        };
        if signature.output.as_ref().is_some_and(Ty::carries_loan) {
            return Err(at.error(format!("{name} callbacks cannot return references yet")));
        }
        if signature.inputs.len() >= 256 {
            return Err(at.error(format!(
                "{name} supports at most 255 parameters plus its environment slot"
            )));
        }
        let adapter = format!(
            "__plenty_boxed_callable_{}_{}_{}",
            self.function_name, at.line, at.column
        );
        if !self.sigs.contains_key(&adapter) {
            self.define_boxed_adapter(&adapter, &source, &signature, mode, at);
        }
        let output = crate::sum::result(
            Ty::BoxedCallable(signature.clone(), mode),
            crate::sum::alloc_error(),
        );
        ops.push(Op::BoxedCallableNew(
            Box::new(source),
            adapter,
            signature,
            mode,
        ));
        Ok(output)
    }

    /// Generate the function a boxed call enters: it receives the
    /// environment that `BoxedMode::environment` names, then the arguments.
    fn define_boxed_adapter(
        &mut self,
        adapter: &str,
        source: &Ty,
        signature: &Arc<CallableSig>,
        mode: BoxedMode,
        at: &Token,
    ) {
        let mut adapted = signature.function();
        adapted.inputs.insert(
            0,
            ("environment".into(), mode.environment(source, signature)),
        );
        let arguments = signature.inputs.iter().enumerate().map(|(i, t)| {
            if t.affine() {
                Op::MoveLocal((i + 1) as u8)
            } else {
                Op::LoadLocal((i + 1) as u8)
            }
        });
        let mut locals = Vec::new();
        let mut body = Vec::new();
        match (mode, source) {
            (BoxedMode::Once, Ty::Closure(t)) if !t.once => {
                // A reusable closure is called through a borrow of a local,
                // which the adapter then drops: the consuming call owns it.
                let content = (signature.inputs.len() + 1) as u8;
                locals.push(source.clone());
                body.extend([
                    Op::MoveLocal(0),
                    Op::BoxedCallableTake(Box::new(source.clone()), signature.clone()),
                    Op::StoreLocal(content),
                    Op::BorrowLocal(content, t.mutable),
                ]);
                body.extend(arguments);
                body.push(Op::ClosureCall(t.clone()));
                if let Some(output) = &signature.output {
                    locals.push(output.clone());
                    body.push(Op::StoreLocal(content + 1));
                    body.push(Op::DropLocal(content));
                    body.push(Op::MoveLocal(content + 1));
                } else {
                    body.push(Op::DropLocal(content));
                }
            }
            (BoxedMode::Once, _) => {
                body.extend([
                    Op::MoveLocal(0),
                    Op::BoxedCallableTake(Box::new(source.clone()), signature.clone()),
                ]);
                body.extend(arguments);
                body.push(match source {
                    Ty::Closure(t) => Op::ClosureCall(t.clone()),
                    _ => Op::CallIndirect(signature.clone()),
                });
            }
            _ => {
                body.push(Op::LoadLocal(0));
                if matches!(source, Ty::Callable(_)) {
                    body.push(Op::ReadRef(source.clone()));
                }
                body.extend(arguments);
                body.push(match source {
                    Ty::Closure(t) => Op::ClosureCall(t.clone()),
                    _ => Op::CallIndirect(signature.clone()),
                });
            }
        }
        body.push(Op::Return);
        let adapted = Arc::new(adapted);
        self.sigs.insert(adapter.to_owned(), adapted.clone());
        self.generics.compiled.push(Op::DefineFn(
            adapter.to_owned(),
            Box::new(CompiledFn {
                location: Some(at.position()),
                origins: Arc::default(),
                generator: None,
                sig: adapted,
                doc: "".into(),
                body: body.into(),
                locals: locals.into(),
            }),
        ));
    }

    fn boxed_callable_arguments(
        &mut self,
        args: &[Expr],
        signature: Arc<CallableSig>,
        mode: BoxedMode,
        loan: Option<usize>,
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
        ops.push(Op::BoxedCallableCall(signature.clone(), mode));
        Self::end_reads(loans, ops);
        ops.extend(loan.map(Op::UseLoan));
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
            Expression::Method(..) if self.text_bytes_receiver(e).is_some() => {
                Some(crate::bytes::byte_view())
            }
            Expression::Method(base, name, args, _) => {
                if let Some(result) = self.byte_string_method_hint(base, name, args) {
                    return Some(result);
                }
                if let Some(output) = self.text_method_output_hint(base, name) {
                    return Some(output);
                }
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
                            "is_cancelled" | "wait_timeout" | "wait_timeout_ms" => Some(Ty::Bool),
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
                            "done" | "cancel" | "wait_timeout" | "wait_timeout_ms" => {
                                Some(Ty::Bool)
                            }
                            _ => None,
                        };
                    }
                    Some(ty @ Ty::Channel(_, _)) if name == "share" => return Some(ty),
                    Some(Ty::Channel(message, sender)) => {
                        if sender && matches!(name.as_str(), "send_timeout" | "send_timeout_ms") {
                            return Some(
                                crate::channel::ChannelOp::SendTimeout((*message).clone())
                                    .signature()
                                    .1,
                            );
                        }
                        if !sender && matches!(name.as_str(), "recv_timeout" | "recv_timeout_ms") {
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
            Expression::Try(inner) => match self.expression_type_hint(inner)? {
                Ty::Enum(sum) if sum.is_result() || sum.is_option() => sum.get().variants
                    [usize::from(sum.is_option())]
                .fields
                .first()
                .cloned(),
                _ => None,
            },
            Expression::Binary(op, left, right)
                if op == "+"
                    && self.slice_view_type(left, false) == Some(text_view())
                    && self.slice_view_type(right, false) == Some(text_view()) =>
            {
                Some(crate::sum::result(Ty::Str, crate::sum::alloc_error()))
            }
            Expression::Index(base, _)
                if self.slice_view_type(base, false) == Some(text_view()) =>
            {
                Some(Ty::Str)
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
        if self.generics.serial_stack.contains_key(name) {
            return Err(at.error("a typed serialization entry point cannot be taken as a function value because its native stack check requires a direct call; use an ordinary wrapper function with concrete adapter types containing the direct call"));
        }
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
        if let Some(Ty::BoxedCallable(sig, mode)) = self.place_type(callee) {
            if mode == BoxedMode::Once {
                if let Expression::Name(name) = &ungroup(callee).kind
                    && self
                        .names
                        .get(name)
                        .is_some_and(|local| matches!(local.ty, Ty::Ref(..)))
                {
                    return Err(callee.at.error(
                        "a BoxedOnceCallable must be owned to call it; a call consumes it",
                    ));
                }
                self.value(callee, ops)?;
                return self.boxed_callable_arguments(args, sig, mode, None, &callee.at, ops);
            }
            let (_, loan) = self.borrow(callee, mode == BoxedMode::Mut, ops)?;
            return self.boxed_callable_arguments(args, sig, mode, Some(loan), &callee.at, ops);
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
        if let Ty::BoxedCallable(sig, mode) = ty {
            if mode == BoxedMode::Once {
                return self.boxed_callable_arguments(args, sig, mode, None, &callee.at, ops);
            }
            let mutable = mode == BoxedMode::Mut;
            let slot = self.slot(Ty::BoxedCallable(sig.clone(), mode), &callee.at)?;
            ops.push(Op::StoreLocal(slot));
            self.expression_temps.push(slot);
            let loan = self.new_loan(slot, mutable, None, ops);
            ops.push(Op::BorrowLocal(slot, mutable));
            return self.boxed_callable_arguments(args, sig, mode, Some(loan), &callee.at, ops);
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
        self.push_call(
            Op::CallIndirect(sig.clone()),
            &function.inputs,
            &loans,
            &callee.at,
            ops,
        )?;
        // A structural callable has no function-specific field projection summary.
        // Retain the full origin loan even when today's value happens to be named.
        self.call_reference_result("", &function, &loans, &callee.at, ops)?;
        Self::end_reads(loans, ops);
        Ok(sig.output.clone())
    }
}
