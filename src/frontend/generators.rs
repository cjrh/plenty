use super::*;

pub(super) fn yields(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match &stmt.kind {
        Statement::Yield(_) => true,
        Statement::If { yes, no, .. } => yields(yes) || yields(no),
        Statement::For { body, .. }
        | Statement::While { body, .. }
        | Statement::With { body, .. } => yields(body),
        Statement::Match { cases, .. } => cases.iter().any(|case| yields(&case.body)),
        _ => false,
    })
}

impl Lower<'_> {
    pub(super) fn refine_return(&mut self, actual: Type, at: &Token) -> Result<()> {
        if actual
            .as_ref()
            .is_some_and(|t| !matches!(t, Ty::Ref(..)) && t.contains_reference())
        {
            return Err(at.error("a closure with borrowed captures cannot escape its function"));
        }
        let expected = self.return_type.clone().flatten();
        self.same(actual.clone(), expected.clone(), at)?;
        if let (Some(expected), Some(actual)) = (expected, actual) {
            self.return_type = Some(crate::generator::refine(&expected, &actual));
        }
        Ok(())
    }

    pub(super) fn ensure_concrete_output(&mut self, name: &str, at: &Token) -> Result<()> {
        if self
            .sigs
            .get(name)
            .is_some_and(|s| s.outputs.iter().any(Ty::unresolved_generator))
        {
            if self.generics.active.contains(name) {
                return Err(at.error(
                    "recursive generator factory cannot infer a finite concrete return layout",
                ));
            }
            let f = self
                .generics
                .functions
                .get(name)
                .cloned()
                .ok_or_else(|| at.error("missing generator factory body"))?;
            lower_function(
                f,
                self.heap,
                self.sigs,
                self.generics,
                self.aliases,
                self.access,
                self.returned_fields,
            )?;
        }
        Ok(())
    }

    pub(super) fn specialize_frames(
        &mut self,
        name: &str,
        actual: Vec<Ty>,
        at: &Token,
    ) -> Result<String> {
        if actual
            .iter()
            .any(|ty| !matches!(ty, Ty::Ref(..)) && ty.contains_reference())
        {
            return Err(at.error("pass a borrowing closure by reference"));
        }
        if !self.sigs[name]
            .inputs
            .iter()
            .any(|(_, t)| t.unresolved_generator())
        {
            self.ensure_concrete_output(name, at)?;
            return Ok(name.into());
        }
        let key = (name.to_owned(), actual.clone());
        if let Some(symbol) = self.generics.frames.get(&key).cloned() {
            self.ensure_concrete_output(&symbol, at)?;
            return Ok(symbol);
        }
        if self.generics.frames.len() >= 256 {
            return Err(at.error(
                "generator specialization limit of 256 exceeded; check for expanding recursion",
            ));
        }
        let mut f = self.generics.functions[name].clone();
        let symbol = format!("__plenty_frame_call_{}", self.generics.frames.len());
        f.name = symbol.clone();
        for ((_, pattern), ty) in f.inputs.iter_mut().zip(&actual) {
            *pattern = generics::type_ref(ty, &pattern.at);
        }
        register_signature(&f, self.aliases, self.sigs, self.returned_fields)?;
        self.generics.frames.insert(key, symbol.clone());
        self.generics.functions.insert(symbol.clone(), f.clone());
        self.generics.pending.push_back(f);
        self.ensure_concrete_output(&symbol, at)?;
        Ok(symbol)
    }

    pub(super) fn frame_call(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let signature = self.sigs[name].clone();
        if args.len() != signature.inputs.len() {
            return Err(at.error(format!(
                "`{name}` expects {} arguments, got {}",
                signature.inputs.len(),
                args.len()
            )));
        }
        let mut actual = Vec::new();
        let mut loans = Vec::new();
        for (arg, (_, expected)) in args.iter().zip(&signature.inputs) {
            let ty = if let Ty::Ref(inner, mutable) = expected {
                let (ty, loan) = self.call_borrow(arg, *mutable, Some((**inner).clone()), ops)?;
                loans.push(loan);
                ty
            } else {
                self.expr_expected(arg, Some(expected.clone()), ops)?
                    .ok_or_else(|| arg.at.error("expected a generator argument, got ()"))?
            };
            if !matches!(ty, Ty::Ref(..)) && ty.contains_reference() {
                return Err(arg.at.error("pass a borrowing closure by reference"));
            }
            self.same(Some(ty.clone()), Some(expected.clone()), &arg.at)?;
            if ty.unresolved_generator() {
                return Err(arg.at.error("cannot infer a concrete generator argument; pass a value with a known producer"));
            }
            actual.push(ty);
        }
        let symbol = self.specialize_frames(name, actual, at)?;
        let sig = self.sigs[&symbol].clone();
        self.record_direct_call(&symbol, at, &loans, ops);
        ops.push(Op::Call(symbol.clone()));
        self.call_reference_result(&symbol, &sig, &loans, ops);
        Self::end_reads(loans, ops);
        Ok(sig.outputs.first().cloned())
    }

    pub(super) fn cleanup(&self, start: usize, ops: &mut Vec<Op>) {
        // A scope-end marker must not claim the cleanup that follows it, and no
        // marker may end the sequence, so the source position is restored lazily.
        let mut restore = None;
        for i in (start..self.locals.len()).rev() {
            let slot = (self.parameters + i) as u8;
            if let Some(context) = self.contexts.iter().find(|context| context.slot == slot) {
                ops.push(Op::Site(context.at));
                restore = self.site.get();
                self.context_receiver(context, ops);
                ops.extend(context.exit.iter().cloned());
            }
            if self.locals[i].managed() {
                ops.extend(restore.take().map(Op::Site));
                ops.push(Op::DropLocal(slot));
            }
        }
    }
    pub(super) fn next(&mut self, args: &[Expr], at: &Token, ops: &mut Vec<Op>) -> Result<Type> {
        if let [arg] = args {
            if let Expression::Unary(op, base) = &ungroup(arg).kind {
                if op == "&mut" {
                    let (ty, loan) = self.borrow(base, true, ops)?;
                    let Ty::Ref(inner, _) = ty else {
                        unreachable!()
                    };
                    let Ty::Generator(element) = &*inner else {
                        return Err(at.error("next requires a generator"));
                    };
                    let output = crate::sum::option(element.element.clone());
                    ops.push(Op::ReadRef((*inner).clone()));
                    ops.push(Op::Collection(CollectionOp::Next((*inner).clone())));
                    ops.push(Op::UseLoan(loan));
                    return Ok(Some(output));
                }
            }
        }
        let [Expr {
            kind: Expression::Name(name),
            ..
        }] = args
        else {
            return Err(at.error("next requires one named mutable generator binding"));
        };
        let local = self
            .names
            .get(name)
            .ok_or_else(|| at.error(format!("unknown binding `{name}`")))?
            .clone();
        if let Ty::Ref(inner, true) = &local.ty {
            let Ty::Generator(element) = inner.as_ref() else {
                return Err(at.error("next requires a generator"));
            };
            let output = crate::sum::option(element.element.clone());
            let (ty, loan) = self.read_place(&local, ops);
            ops.push(Op::Collection(CollectionOp::Next(ty)));
            ops.push(Op::UseLoan(loan));
            return Ok(Some(output));
        }
        let Ty::Generator(element) = &local.ty else {
            return Err(at.error("next requires a generator"));
        };
        if !local.mutable {
            return Err(
                at.error("next requires a mutable generator binding; declare it with `mut`")
            );
        }
        self.mark(at, ops);
        ops.push(Op::Next(local.slot));
        Ok(Some(crate::sum::option(element.element.clone())))
    }
}
