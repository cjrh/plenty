use super::*;

pub(super) fn yields(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match &stmt.kind {
        Statement::Yield(_) => true,
        Statement::If { yes, elifs, no, .. } => {
            yields(yes) || elifs.iter().any(|elif| yields(&elif.body)) || yields(no)
        }
        Statement::For { body, .. }
        | Statement::While { body, .. }
        | Statement::With { body, .. } => yields(body),
        Statement::Match { cases, .. } => cases.iter().any(|case| yields(&case.body)),
        Statement::Assign { value, .. }
        | Statement::SetIndex { value, .. }
        | Statement::Return(Some(value)) => {
            matches!(&value.kind, Expression::Match(_, cases)
                if cases.iter().any(|case| yields(&case.body)))
        }
        _ => false,
    })
}

impl Lower<'_> {
    pub(super) fn generator_expression(
        &mut self,
        element: &Expr,
        clauses: &[Clause],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let Some(Clause::For(names, iterable)) = clauses.first() else {
            return Err(at.error("a generator expression requires a for clause"));
        };
        // Evaluate the outer source now; filters, inner sources, and the yielded
        // expression execute only when the concrete frame is advanced.
        let mut inputs = Vec::new();
        let source = self.generator_source(iterable, &mut inputs, ops)?;
        let mut bound: HashSet<String> = names.iter().cloned().collect();
        let mut free = Vec::new();
        for clause in &clauses[1..] {
            match clause {
                Clause::For(names, source) => {
                    free_names(source, &bound, &mut free);
                    bound.extend(names.iter().cloned());
                }
                Clause::If(condition) => free_names(condition, &bound, &mut free),
            }
        }
        free_names(element, &bound, &mut free);
        let mut seen = HashSet::default();
        for name in free {
            if !seen.insert(name.clone()) || !self.names.contains_key(&name) {
                continue;
            }
            let value = Expr {
                at: at.clone(),
                kind: Expression::Name(name.clone()),
            };
            let ty = self.value(&value, ops)?;
            if ty.contains_reference() {
                return Err(at.error("generator expressions capture values by ownership; references cannot be suspended"));
            }
            inputs.push((name, generics::type_ref(&ty, at)));
        }
        let mut clauses = clauses.to_vec();
        clauses[0] = Clause::For(names.clone(), source);
        let mut body = vec![Stmt {
            at: element.at.clone(),
            kind: Statement::Yield(element.clone()),
        }];
        for clause in clauses.into_iter().rev() {
            let (at, kind) = match clause {
                Clause::For(names, iterable) => (
                    iterable.at.clone(),
                    Statement::For {
                        name: names,
                        iterable,
                        body,
                    },
                ),
                Clause::If(condition) => (
                    condition.at.clone(),
                    Statement::If {
                        condition,
                        yes: body,
                        elifs: vec![],
                        no: vec![],
                    },
                ),
            };
            body = vec![Stmt { at, kind }];
        }
        let name = format!(
            "__plenty_genexpr_{}_{}_{}",
            self.function_name, at.line, at.column
        );
        if !self.sigs.contains_key(&name) {
            let function = Function {
                once: false,
                captures: vec![],
                foreign: None,
                export: None,
                name: name.clone(),
                type_params: vec![],
                at: at.clone(),
                inputs,
                defaults: vec![],
                // Unit is an internal inference marker, never a source annotation.
                output: generics::type_ref(&crate::generator::ty(Ty::Unit, None), at),
                doc: String::new(),
                doc_at: None,
                body,
            };
            register_signature(&function, self.aliases, self.sigs, self.returned_fields)?;
            self.generics
                .inherit_requirement(&self.function_name, &name);
            let function = Arc::new(function);
            self.generics.record(&function);
            lower_function(
                function,
                self.heap,
                self.sigs,
                self.generics,
                self.aliases,
                self.access,
                self.returned_fields,
            )?;
        }
        let signature = self.sigs[&name].clone();
        self.record_direct_call(&name, at, &[], ops);
        self.push_call(Op::Call(name), &signature.inputs, &[], ops);
        Ok(signature.outputs[0].clone())
    }

    fn generator_source(
        &mut self,
        source: &Expr,
        inputs: &mut Vec<(String, TypeRef)>,
        ops: &mut Vec<Op>,
    ) -> Result<Expr> {
        if let Expression::Call(name, args) = &ungroup(source).kind
            && matches!(name.as_str(), "enumerate" | "zip" | "reversed")
            && !self.names.contains_key(name)
            && !self.sigs.contains_key(name)
            && !self.generics.templates.contains_key(name)
            && !self.aliases.contains_key(name)
        {
            let (valid, expected) = match name.as_str() {
                "enumerate" => ((1..=2).contains(&args.len()), "1 or 2"),
                "zip" => (args.len() == 2, "2"),
                _ => (args.len() == 1, "1"),
            };
            if !valid {
                return Err(source.at.error(format!(
                    "`{name}` expects {expected} arguments, got {}",
                    args.len()
                )));
            }
            let args = args
                .iter()
                .map(|arg| self.generator_source(arg, inputs, ops))
                .collect::<Result<_>>()?;
            return Ok(Expr {
                at: source.at.clone(),
                kind: Expression::Call(name.clone(), args),
            });
        }
        let ty = self.value(source, ops)?;
        if ty.contains_reference() {
            return Err(source.at.error(
                "generator expressions require owned inputs; references cannot be suspended",
            ));
        }
        let name = format!("<generator source {}>", inputs.len());
        inputs.push((name.clone(), generics::type_ref(&ty, &source.at)));
        Ok(Expr {
            at: source.at.clone(),
            kind: Expression::Name(name),
        })
    }

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
            let declared = self
                .generics
                .functions
                .get(name)
                .ok_or_else(|| at.error("missing generator factory body"))?;
            // A released declaration is already lowered.
            let Some(f) = declared.function.clone() else {
                return Ok(());
            };
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
        let mut f = Function::clone(
            self.generics.functions[name]
                .function
                .as_ref()
                .expect("a function with a generator parameter keeps its declaration"),
        );
        let symbol = format!("__plenty_frame_call_{}", self.generics.frames.len());
        f.name = symbol.clone();
        for ((_, pattern), ty) in f.inputs.iter_mut().zip(&actual) {
            *pattern = generics::type_ref(ty, &pattern.at);
        }
        register_signature(&f, self.aliases, self.sigs, self.returned_fields)?;
        self.generics.frames.insert(key, symbol.clone());
        self.generics.inherit_requirement(name, &symbol);
        self.generics.declare(f);
        self.ensure_concrete_output(&symbol, at)?;
        Ok(symbol)
    }

    pub(super) fn frame_call_with_receiver(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        method: bool,
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
        let mut reservation = None;
        let start = ops.len();
        for (index, (arg, (_, expected))) in args.iter().zip(&signature.inputs).enumerate() {
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
            if method && index == 0 && matches!(actual[0], Ty::Ref(_, true)) {
                reservation = Some(loans[0]);
                self.reserve_receiver(loans[0], ops);
            }
        }
        let symbol = self.specialize_frames(name, actual, at)?;
        let sig = self.sigs[&symbol].clone();
        if let Some(loan) = reservation {
            self.mark(at, ops);
            loans[0] = self.activate_receiver(loan, ops);
        }
        self.record_direct_call(&symbol, at, &loans, ops);
        self.passes(start, &sig.inputs, sig.outputs.first(), ops);
        self.push_call(Op::Call(symbol.clone()), &sig.inputs, &loans, ops);
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
    /// [`Self::cleanup`] for the `return`, `break` or `continue` that follows,
    /// marked so code generation can share it between exits (issue #417).
    pub(super) fn leaving(&self, start: usize, ops: &mut Vec<Op>) {
        let marker = ops.len();
        ops.push(Op::Leaving);
        self.cleanup(start, ops);
        if ops.len() == marker + 1 {
            ops.pop();
        }
    }
    pub(super) fn next(&mut self, args: &[Expr], at: &Token, ops: &mut Vec<Op>) -> Result<Type> {
        if let [arg] = args
            && let Expression::Unary(op, base) = &ungroup(arg).kind
            && op == "&mut"
        {
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
        let [
            Expr {
                kind: Expression::Name(name),
                ..
            },
        ] = args
        else {
            return Err(at.error("next requires one named mutable generator binding"));
        };
        let local = self
            .names
            .get(name)
            .ok_or_else(|| at.error(self.unknown_binding(name)))?
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

fn free_names(expression: &Expr, bound: &HashSet<String>, names: &mut Vec<String>) {
    let mut name = |name: &String| {
        if !bound.contains(name) {
            names.push(name.clone());
        }
    };
    match &expression.kind {
        Expression::Name(n) | Expression::GenericValue(n, _) => name(n),
        Expression::Call(n, args) | Expression::GenericCall(n, _, args) => {
            name(n);
            for arg in args {
                free_names(arg, bound, names);
            }
        }
        Expression::Tuple(values) | Expression::Constructor(_, values) => {
            for value in values {
                free_names(value, bound, names);
            }
        }
        Expression::Format(format) => {
            for value in format.operands() {
                free_names(value, bound, names);
            }
        }
        Expression::Method(base, _, args, _)
        | Expression::GenericMethod(base, _, _, args, _)
        | Expression::Invoke(base, args) => {
            free_names(base, bound, names);
            for arg in args {
                free_names(arg, bound, names);
            }
        }
        Expression::Try(base)
        | Expression::NamedArgument(_, base)
        | Expression::Member(base, _, _)
        | Expression::Group(base)
        | Expression::Unary(_, base)
        | Expression::ClassReady(_, base) => free_names(base, bound, names),
        Expression::Index(left, right) | Expression::Binary(_, left, right) => {
            free_names(left, bound, names);
            free_names(right, bound, names);
        }
        Expression::Conditional { condition, yes, no } => {
            free_names(condition, bound, names);
            free_names(yes, bound, names);
            free_names(no, bound, names);
        }
        Expression::Collection {
            entries, clauses, ..
        } => {
            let mut inner = bound.clone();
            for clause in clauses {
                match clause {
                    Clause::For(bindings, iterable) => {
                        free_names(iterable, &inner, names);
                        inner.extend(bindings.iter().cloned());
                    }
                    Clause::If(condition) => free_names(condition, &inner, names),
                }
            }
            for (key, value) in entries {
                free_names(key, &inner, names);
                if let Some(value) = value {
                    free_names(value, &inner, names);
                }
            }
        }
        Expression::Anonymous(function) => {
            for capture in &function.captures {
                name(&capture.name);
            }
        }
        Expression::Ambiguous { .. } => unreachable!("module resolution precedes lowering"),
        Expression::Match(..) => unreachable!("a match block is a statement's whole value"),
        Expression::ClassNew(_)
        | Expression::Type(_)
        | Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bytes(_)
        | Expression::Bool(_)
        | Expression::Unit => {}
    }
}
