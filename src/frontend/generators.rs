use super::*;

/// Concrete frame specializations of one generator consumer.
const MAX_CONSUMER_FRAMES: usize = 16_384;
/// Frame specializations per compilation, a resource budget like generic
/// functions' total.
const MAX_FRAMES: usize = 65_536;

/// Producers joined where a value's type is fixed before a different
/// producer meets it: a binding reassigned another producer, or a choice
/// whose branches build `Option`/`Result` payloads around different
/// producers. The first pass records the joined type at the position of the
/// value that must hold it and lowers the function again, so that value is
/// built with the joined type from the start (issue #872).
#[derive(Default)]
pub(super) struct Rejoin {
    /// Joined types from earlier passes, by the position of the value.
    hints: HashMap<Position, Ty>,
    /// The position of each generator-holding binding's declared value.
    declared: HashMap<u8, Position>,
    /// The producers of the last generator mismatch: expected, then got.
    mismatch: std::cell::RefCell<Option<(Ty, Ty)>>,
    /// A factory's joined output, for its signature in the next pass.
    pub(super) returned: Option<Ty>,
    /// Whether an earlier pass joined this factory's output.
    inferred_return: bool,
    /// The `match` values whose later case is being lowered, innermost
    /// last, with the earlier cases' type; see [`Lower::enter_case`].
    cases: Vec<(Position, Ty)>,
    /// Whether this pass recorded a new joined type.
    pub(super) pending: bool,
}

impl Rejoin {
    pub(super) fn new(hints: Option<&HashMap<Position, Ty>>, inferred_return: bool) -> Self {
        Self {
            hints: hints.cloned().unwrap_or_default(),
            inferred_return,
            ..Self::default()
        }
    }

    /// Notes the producers `same` found different, for [`Lower::request_join`].
    pub(super) fn mismatched(&self, expected: &Ty, got: &Ty) {
        *self.mismatch.borrow_mut() = Some((expected.clone(), got.clone()));
    }
}

/// Whether `ty` is a generator or carries one as a success payload.
fn carries_generator(ty: &Ty) -> bool {
    match ty {
        Ty::Generator(_) => true,
        Ty::Enum(t) if t.is_option() => carries_generator(&t.get().variants[1].fields[0]),
        Ty::Enum(t) if t.is_result() => carries_generator(&t.get().variants[0].fields[0]),
        _ => false,
    }
}

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
        | Statement::Return(Some(value))
        | Statement::Expr(value) => match &value.kind {
            Expression::Match(_, cases) => cases.iter().any(|case| yields(&case.body)),
            Expression::Checked(_, body) | Expression::If(body) => yields(body),
            _ => false,
        },
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
                Clause::For(names, source) | Clause::Unpack(names, _, source) => {
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
                Clause::Unpack(names, targets, value) => {
                    body.insert(
                        0,
                        Stmt {
                            at: value.at.clone(),
                            kind: Statement::Unpack {
                                names,
                                targets: Some(targets),
                                fresh: true,
                                mutable: false,
                                value,
                            },
                        },
                    );
                    continue;
                }
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
            register_signature(&function, self.aliases, self.sigs)?;
            self.generics
                .inherit_requirement(self.sigs, &self.function_name, &name);
            let function = Arc::new(function);
            let symbol = self.generics.record(&function, self.sigs);
            lower_function(
                symbol,
                function,
                self.heap,
                self.sigs,
                self.generics,
                self.aliases,
                self.access,
            )?;
        }
        let signature = self.sigs[&name].clone();
        self.record_direct_call(&name, at, &[], ops);
        self.push_call(Op::Call(name), &signature.inputs, &[], at, ops)?;
        Ok(signature.outputs[0].clone())
    }

    fn generator_source(
        &mut self,
        source: &Expr,
        inputs: &mut Vec<(String, TypeRef)>,
        ops: &mut Vec<Op>,
    ) -> Result<Expr> {
        if let Expression::Call(function, args) = &ungroup(source).kind
            && let Some(name @ ("enumerate" | "zip" | "reversed")) = builtin_call(function)
        {
            let (valid, expected) = match name {
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
                kind: Expression::Call(function.clone(), args),
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

    /// The type to build the value at `at` with, given the type its context
    /// expects: the context's joined with a type an earlier pass recorded
    /// there. An inner choice's recorded join can be narrower than the
    /// outer's, and an annotation names only the item type.
    #[inline(never)]
    pub(super) fn hinted_expected(&self, at: &Token, expected: Type) -> Type {
        let Some(hint) = self.join_hint(at) else {
            return expected;
        };
        let Some(context) = expected else {
            return Some(hint);
        };
        crate::generator::join_carried(&hint, &context)
            .or_else(|| crate::generator::refine(&context, &hint))
            .or(Some(context))
    }

    /// The joined type an earlier pass recorded for the value at `at`.
    pub(super) fn join_hint(&self, at: &Token) -> Option<Ty> {
        if self.joins.hints.is_empty() {
            return None;
        }
        self.joins.hints.get(&at.position()).cloned()
    }

    /// The joined type of the `match` value at `at`, as the type its cases
    /// join to. Outlined to keep the recursive match frame small.
    #[inline(never)]
    pub(super) fn case_hint(&self, at: &Token, tail: &Tail) -> Option<Type> {
        self.join_hint(at)?;
        let expected = match tail {
            Tail::Value(expected) => expected.clone(),
            _ => None,
        };
        Some(self.hinted_expected(at, expected))
    }

    /// Records `joined` for the value at `at` and requests another pass,
    /// unless an earlier pass already lowered that value with it.
    pub(super) fn record_join(&mut self, at: Position, joined: Ty) -> bool {
        let hints = self.generics.join_hints.entry(self.symbol).or_default();
        // Hints only widen, so one pass can record several producers met at
        // the same value, such as successive reassignments of a local, and
        // re-lowering ends. A join that does not widen the hint, or does not
        // join it at all, keeps it and asks for no pass.
        let joined = match hints.get(&at) {
            Some(hint) => match crate::generator::join_carried(hint, &joined) {
                Some(wider) if wider != *hint => wider,
                _ => return false,
            },
            None => joined,
        };
        hints.insert(at, joined);
        self.joins.pending = true;
        true
    }

    /// After lowering a value failed, records the join of `base`, the type
    /// the value was expected to have, with the producer that differed from
    /// one `base` holds. False when the failure was not such a mismatch.
    pub(super) fn request_join(&mut self, at: Position, base: &Ty) -> bool {
        let Some((expected, got)) = self.joins.mismatch.take() else {
            return false;
        };
        let joined = crate::generator::replace_carried(base, &expected, &got)
            .and_then(|other| crate::generator::join_carried(base, &other))
            // A context naming only the item type, such as an annotated
            // `Option[Generator[i64]]`, holds no producer to replace: the
            // mismatched values themselves join.
            .or_else(|| {
                crate::generator::join_carried(&expected, &got)
                    .filter(|joined| crate::generator::refine(base, joined).is_some())
            });
        joined.is_some_and(|joined| self.record_join(at, joined))
    }

    /// Records the join of two chosen values that differ only in producers
    /// held as `Option`/`Result` payloads, which conversion after the fact
    /// cannot move: another pass builds both with the joined type.
    #[inline(never)]
    /// True when it recorded one: the pass then continues so that it can
    /// record joins at other values, and is lowered again.
    pub(super) fn join_mismatched(&mut self, at: &Token, ty: &Type, other: &Type) -> bool {
        if let (Some(a), Some(b)) = (ty, other)
            && let Some(joined) = crate::generator::join_carried(a, b)
        {
            return self.record_join(at.position(), joined);
        }
        false
    }

    /// A later branch of the choice at `at`, lowered expecting the earlier
    /// branches' type `base`: when it failed on a different producer, records
    /// their join for another pass. Clear the mismatch before lowering it.
    /// When it records a join, the pass continues with `base`, so that one
    /// pass records the joins of every such choice; its code is discarded.
    #[inline(never)]
    pub(super) fn joined_branch(
        &mut self,
        at: &Token,
        base: &Type,
        lowered: Result<Type>,
    ) -> Result<Type> {
        if lowered.is_err()
            && let Some(base) = base
            && self.request_join(at.position(), base)
        {
            return Ok(Some(base.clone()));
        }
        lowered
    }

    /// A factory's returned value: when it failed on a producer different
    /// from one the output inferred so far holds, as an `Option`/`Result`
    /// payload, the next pass returns their join. Clear the mismatch first.
    #[inline(never)]
    pub(super) fn joined_return(&mut self, lowered: Result<Type>) -> Result<Type> {
        if lowered.is_err()
            && (self.joins.inferred_return || self.inferring_frame())
            && let Some(Some(base)) = self.return_type.clone()
            && let Some((expected, got)) = self.joins.mismatch.take()
            && let Some(joined) = crate::generator::replace_carried(&base, &expected, &got)
                .and_then(|other| crate::generator::join_carried(&base, &other))
            && joined != base
        {
            self.joins.returned = Some(joined);
            self.joins.pending = true;
        }
        lowered
    }

    /// The tail of an `if` value at `at` that an earlier pass gave a joined
    /// type, which is more specific than its context's.
    #[inline(never)]
    pub(super) fn hinted_tail(&self, tail: &Tail, at: &Token) -> Option<Box<Tail>> {
        // Boxed so the recursive `if` and `match` lowering keeps small frames.
        let Tail::Value(expected) = tail else {
            return None;
        };
        self.join_hint(at)?;
        Some(Box::new(Tail::Value(
            self.hinted_expected(at, expected.clone()),
        )))
    }

    /// Notes that a case of the `match` value at `at` is lowered expecting
    /// the earlier cases' type in `tail`. A failure on a different producer
    /// inside it is joined by [`Lower::join_failed_case`]. A stack, rather
    /// than a check around each case, keeps deeply nested matches' frames
    /// small.
    #[inline(never)]
    pub(super) fn enter_case(&mut self, at: &Token, tail: &Tail) {
        let base = match tail {
            Tail::Value(Some(base)) if carries_generator(base) => base.clone(),
            _ => Ty::Unit,
        };
        self.clear_mismatch();
        self.joins.cases.push((at.position(), base));
    }

    #[inline(never)]
    pub(super) fn leave_case(&mut self) {
        self.joins.cases.pop();
    }

    /// After the function failed to lower: when the innermost open `match`
    /// case failed on a different producer than its earlier cases hold,
    /// records their join for another pass.
    pub(super) fn join_failed_case(&mut self) {
        if !self.joins.pending
            && let Some((at, base)) = self.joins.cases.last().cloned()
            && base != Ty::Unit
        {
            self.request_join(at, &base);
        }
    }

    /// A later branch body of the `if` or `match` at `at`, through
    /// [`Lower::joined_case`].
    /// Outlined so deeply nested matches keep their recursion frames small.
    #[inline(never)]
    pub(super) fn case_block(
        &mut self,
        at: &Token,
        suite: &[Stmt],
        case_at: &Token,
        body: &mut Vec<Op>,
        tail: &Tail,
    ) -> Result<BlockResult> {
        self.clear_mismatch();
        let lowered = self.branch_block(suite, case_at, body, tail);
        self.joined_case(at, tail, lowered)
    }

    /// A later branch of the `if` or `match` value at `at`, lowered with
    /// `tail`, which holds the earlier branches' type: when it failed on a
    /// different producer, records their join for another pass.
    #[inline(never)]
    pub(super) fn joined_case(
        &mut self,
        at: &Token,
        tail: &Tail,
        lowered: Result<BlockResult>,
    ) -> Result<BlockResult> {
        if lowered.is_err()
            && let Tail::Value(Some(base)) = tail
        {
            self.request_join(at.position(), base);
        }
        lowered
    }

    /// Forgets an earlier mismatch before lowering a value whose failure
    /// [`Lower::request_join`] may inspect.
    pub(super) fn clear_mismatch(&self) {
        self.joins.mismatch.take();
    }

    /// Notes where a binding holding a generator was declared, so a later
    /// reassignment can join its producer there.
    pub(super) fn note_declared(&mut self, slot: u8, ty: &Ty, value: &Token) {
        if carries_generator(ty) {
            self.joins.declared.insert(slot, value.position());
        }
    }

    /// A reassigned binding's value: a member producer of the binding's
    /// joined type moves into its storage. When the value holds a producer
    /// the binding's type lacks, records their join at the declaration.
    pub(super) fn join_reassigned(
        &mut self,
        slot: u8,
        binding: &Ty,
        ty: Ty,
        ops: &mut Vec<Op>,
    ) -> Ty {
        if crate::generator::joins_into(&ty, binding) {
            ops.push(Op::JoinFrame(Box::new((ty, binding.clone()))));
            return binding.clone();
        }
        if let Some(joined) = crate::generator::join_carried(binding, &ty)
            && let Some(at) = self.joins.declared.get(&slot).cloned()
            && self.record_join(at, joined)
        {
            // The pass continues to record later reassignments' producers
            // too; its code is discarded.
            return binding.clone();
        }
        ty
    }

    /// After a reassigned value failed, records the producer join that
    /// would have let it be built with the binding's type.
    pub(super) fn request_reassigned_join(&mut self, slot: u8, binding: &Ty) {
        if let Some(at) = self.joins.declared.get(&slot).cloned() {
            self.request_join(at, binding);
        }
    }

    /// [`Lower::refine_return`] for a function's tail value. A value whose
    /// payload joined a wider producer than earlier returns gave the output
    /// joins them too. Outlined to keep the recursive block frame small.
    #[inline(never)]
    pub(super) fn refine_joined_return(&mut self, actual: Type, at: &Token) -> Result<()> {
        let refined = self.refine_return(actual, at).map(|()| None);
        self.joined_return(refined).map(|_| ())
    }

    pub(super) fn refine_return(&mut self, actual: Type, at: &Token) -> Result<()> {
        if actual
            .as_ref()
            .is_some_and(|t| !t.carries_loan() && t.contains_reference())
        {
            return Err(at.error("a closure with borrowed captures cannot escape its function"));
        }
        let expected = self.return_type.clone().flatten();
        if let (Some(expected), Some(actual)) = (&expected, &actual)
            && let Some(joined) = crate::generator::join(expected, actual)
            && self.inferring_frame()
        {
            // Issue #872: a factory returning different producers returns
            // their joined frame. Lowering repeats with the joined signature,
            // and `join_returned_frame` then converts each returned member.
            self.return_type = Some(Some(joined));
            return Ok(());
        }
        self.same(actual.clone(), expected.clone(), at)?;
        if let (Some(expected), Some(actual)) = (expected, actual) {
            self.return_type = Some(crate::generator::refine(&expected, &actual));
        }
        Ok(())
    }

    /// Checks that a branch's type `ty` matches the earlier branches'
    /// `expected`, or that both are producers that join: in a function's
    /// tail, into its returned generator. That joined type is returned. Kept out of line so
    /// deeply nested branches keep their recursion frames small.
    #[inline(never)]
    pub(super) fn join_branch(
        &mut self,
        expected: &Type,
        ty: &Type,
        function_tail: bool,
        at: &Token,
    ) -> Result<Option<Type>> {
        if function_tail && let Some(joined) = self.joined_tail(expected, ty) {
            return Ok(Some(joined));
        }
        // Elsewhere the caller converts each member branch's frame once every
        // branch type is known; see [`Lower::join_values`].
        if !function_tail
            && let (Some(a), Some(b)) = (expected, ty)
            && let Some(joined) = crate::generator::join(a, b)
        {
            return Ok(Some(Some(joined)));
        }
        self.same(ty.clone(), expected.clone(), at)?;
        Ok(None)
    }

    /// Joins different producers chosen by a conditional expression: both
    /// branch types are known here, so each branch's frame moves into storage
    /// for their joined type (issue #872). None when they do not join.
    #[inline(never)]
    pub(super) fn join_conditional(
        &self,
        yes: &Type,
        no: &Type,
        yes_ops: &mut Vec<Op>,
        no_ops: &mut Vec<Op>,
    ) -> Option<Ty> {
        let (Some(a), Some(b)) = (yes, no) else {
            return None;
        };
        let joined = crate::generator::join(a, b)?;
        for (ty, ops) in [(a, yes_ops), (b, no_ops)] {
            if crate::generator::joins_into(ty, &joined) {
                ops.push(Op::JoinFrame(Box::new((ty.clone(), joined.clone()))));
            }
        }
        Some(joined)
    }

    /// Joins the different producers an `if` or `match` value chooses outside
    /// a function's tail: each member branch's frame moves into storage for
    /// the joined type at the end of its body (issue #872). None leaves the
    /// branches unchanged, when they all have one type or do not join.
    #[inline(never)]
    pub(super) fn join_values(&self, values: &mut [(&Ty, &mut Vec<Op>)]) -> Option<Ty> {
        let (first, rest) = values.split_first()?;
        let joined = rest.iter().try_fold(first.0.clone(), |joined, (ty, _)| {
            crate::generator::join(&joined, ty)
        })?;
        if !matches!(&joined, Ty::Generator(t) if !t.members.is_empty()) {
            return None;
        }
        for (ty, ops) in values.iter_mut() {
            if crate::generator::joins_into(ty, &joined) {
                ops.push(Op::JoinFrame(Box::new(((*ty).clone(), joined.clone()))));
            }
        }
        Some(joined)
    }

    /// [`Lower::join_values`] for the bodies of a `match`'s cases, given the
    /// case number and type of each one that continues with a value.
    #[inline(never)]
    pub(super) fn join_cases(
        &self,
        bodies: &mut [Option<Vec<Op>>],
        continuing: &[(usize, Ty)],
    ) -> Option<Ty> {
        let mut values: Vec<_> = bodies
            .iter_mut()
            .enumerate()
            .filter_map(|(arm, body)| {
                let (_, ty) = continuing.iter().find(|(case, _)| *case == arm)?;
                Some((ty, body.as_mut()?))
            })
            .collect();
        self.join_values(&mut values)
    }

    /// The function's joined generator type, when two branches of its tail
    /// returned different producers. Each branch already converted its frame
    /// or recorded the join when it returned.
    fn joined_tail(&self, a: &Type, b: &Type) -> Option<Type> {
        let (Some(a), Some(b)) = (a, b) else {
            return None;
        };
        crate::generator::join(a, b)?;
        let returned = self.return_type.clone().flatten()?;
        let fits = |ty: &Ty| *ty == returned || crate::generator::joins_into(ty, &returned);
        (fits(a) && fits(b)).then_some(Some(returned))
    }

    /// Whether this function's declared output still awaits a concrete
    /// producer, so its returns may join different producers.
    fn inferring_frame(&self) -> bool {
        self.yield_type.is_none()
            && self
                .sigs
                .get(&self.function_name)
                .is_some_and(|sig| sig.outputs.iter().any(Ty::unresolved_generator))
    }

    /// Converts a returned member producer's frame to the function's joined
    /// generator type, appending the conversion to the value's operations.
    pub(super) fn join_returned_frame(&self, actual: Type, ops: &mut Vec<Op>) -> Type {
        if let (Some(Some(expected)), Some(member)) = (&self.return_type, &actual)
            && crate::generator::joins_into(member, expected)
        {
            ops.push(Op::JoinFrame(Box::new((member.clone(), expected.clone()))));
            return Some(expected.clone());
        }
        actual
    }

    pub(super) fn ensure_concrete_output(&mut self, name: &str, at: &Token) -> Result<()> {
        if self
            .sigs
            .get(name)
            .is_some_and(|s| s.outputs.iter().any(Ty::unresolved_generator))
        {
            let symbol = self
                .sigs
                .symbol(name)
                .expect("a function with a signature has a symbol");
            if self.generics.active.contains(&symbol) {
                return Err(at.error(
                    "recursive generator factory cannot infer a finite concrete return layout",
                ));
            }
            let declared = self
                .sigs
                .declared_at(symbol)
                .ok_or_else(|| at.error("missing generator factory body"))?;
            // A released declaration is already lowered.
            let Some(f) = declared.function.clone() else {
                return Ok(());
            };
            // An unused instance method is lowered here, ahead of its queue
            // entry, so its errors need the same attribution.
            let outer = self.generics.required.get(&self.symbol).cloned();
            self.generics
                .demand_method(name, || Some(at.position()), outer.as_ref());
            lower_function(
                symbol,
                f,
                self.heap,
                self.sigs,
                self.generics,
                self.aliases,
                self.access,
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
        if actual.iter().any(references::borrowing_closure) {
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
        // The same scheme as generic functions (issue #430): the caller's
        // dependency depth bounds expansion, while the per-consumer and total
        // counts bound breadth and resources.
        let depth = self.generics.dependency_depth(self.symbol, at)?;
        let count = self
            .generics
            .frame_instances
            .get(name)
            .copied()
            .unwrap_or(0);
        if count >= MAX_CONSUMER_FRAMES {
            return Err(at.error(format!(
                "generator specialization limit of {MAX_CONSUMER_FRAMES} frames of `{}` exceeded; check for expanding recursion",
                match self.generics.requirement_of(self.sigs, name) {
                    Some(required) => required.instance.to_string(),
                    None => crate::record::method_display(name).to_string(),
                }
            )));
        }
        if self.generics.frames.len() >= MAX_FRAMES {
            return Err(at.error(format!(
                "total generator specialization budget of {MAX_FRAMES} exceeded"
            )));
        }
        let mut f = Function::clone(
            self.sigs
                .declared(name)
                .and_then(|declared| declared.function.as_ref())
                .expect("a function with a generator parameter keeps its declaration"),
        );
        let symbol = format!("__plenty_frame_call_{}", self.generics.frames.len());
        f.name = symbol.clone();
        for ((_, pattern), ty) in f.inputs.iter_mut().zip(&actual) {
            *pattern = generics::type_ref(ty, &pattern.at);
        }
        // A lent pipeline's state follows the declared parameters.
        let hidden: Vec<Ty> = actual
            .iter()
            .filter_map(pipelines::lent_identity)
            .flat_map(|id| self.generics.lent[id].hidden().collect::<Vec<_>>())
            .collect();
        for (k, ty) in hidden.iter().enumerate() {
            f.inputs
                .push((format!("<lent {k}>"), generics::type_ref(ty, &f.at)));
        }
        if !f.defaults.is_empty() {
            f.defaults.resize(f.inputs.len(), None);
        }
        register_signature(&f, self.aliases, self.sigs)?;
        self.generics.frames.insert(key, symbol.clone());
        self.generics.inherit_requirement(self.sigs, name, &symbol);
        let frame = self.sigs.intern(&symbol);
        self.generics.specialization_depths.insert(frame, depth);
        match self.generics.frame_instances.get_mut(name) {
            Some(count) => *count += 1,
            None => {
                self.generics.frame_instances.insert(name.to_owned(), 1);
                // A chain through distinct consumers is finite like one
                // through distinct generic functions (issue #435).
                self.generics.specializable += 1;
            }
        }
        self.generics.declare(f, self.sigs);
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
        let mut hidden = (Vec::new(), Vec::new());
        let mut lent_loans = Vec::new();
        for (index, (arg, (_, expected))) in args.iter().zip(&signature.inputs).enumerate() {
            if pipelines::lent_declared(expected) {
                let (ty, loan) =
                    self.lend_argument(arg, expected, &mut hidden, &mut lent_loans, ops)?;
                loans.push(loan);
                actual.push(ty);
                continue;
            }
            let ty = if let Ty::Ref(inner, mutable) = expected {
                let (ty, loan) = self.call_borrow(arg, *mutable, Some((**inner).clone()), ops)?;
                loans.push(loan);
                self.lend_content(ty, expected, ops)
            } else if let Ty::View(_, mutable) = expected {
                let text = *expected == text_view();
                let (ty, loan) = self.view_argument(arg, *mutable, text, ops)?;
                loans.push(loan);
                ty
            } else {
                self.expr_expected(arg, Some(expected.clone()), ops)?
                    .ok_or_else(|| arg.at.error("expected a generator argument, got ()"))?
            };
            if references::borrowing_closure(&ty) {
                return Err(arg.at.error("pass a borrowing closure by reference"));
            }
            if ty.carried_reference().is_some() {
                loans.push(self.carried_argument(arg, &ty, ops)?);
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
        let (pushes, state) = hidden;
        ops.extend(pushes);
        loans.extend(state);
        self.record_direct_call(&symbol, at, &loans, ops);
        self.passes(start, &sig.inputs, sig.outputs.first(), ops);
        self.push_call(Op::Call(symbol.clone()), &sig.inputs, &loans, at, ops)?;
        self.call_reference_result(&symbol, &sig, &loans, ops);
        Self::end_reads(loans, ops);
        // The lender's own loans, such as of a borrowed source, stay live
        // across the call: the borrower reads through them.
        ops.extend(lent_loans.into_iter().map(Op::UseLoan));
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
        if let [arg] = args {
            let mut place = ungroup(arg);
            if let Expression::Unary(op, base) = &place.kind
                && op == "&mut"
            {
                place = ungroup(base);
            }
            if let Expression::Name(name) = &place.kind
                && let Some(local) = self.names.get(name)
                && (matches!(local.ty, Ty::Iter(..))
                    || self.pipeline.lent_parameters.contains_key(&local.slot))
            {
                let local = local.clone();
                return self.pipeline_next(&local, place, at, ops).map(Some);
            }
        }
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
        Expression::Slice(base, bounds) => {
            free_names(base, bound, names);
            for value in bounds.iter() {
                free_names(value, bound, names);
            }
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
                    Clause::For(bindings, iterable) | Clause::Unpack(bindings, _, iterable) => {
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
        Expression::Match(..) | Expression::Checked(..) | Expression::If(_) => {
            unreachable!("a block value is a statement's whole value")
        }
        Expression::ClassNew(_)
        | Expression::Type(_)
        | Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bytes(_)
        | Expression::Bool(_)
        | Expression::Unit => {}
    }
}
