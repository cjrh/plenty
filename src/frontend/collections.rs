use super::*;

struct Iteration {
    condition: Vec<Op>,
    // Initialize pending generator results when a paired condition is skipped.
    // This keeps conditional advancement compatible with definite ownership.
    absent: Vec<Op>,
    body: Vec<Op>,
    step: Vec<Op>,
    target: u8,
}

impl Parser {
    pub(super) fn arguments(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.eat(")") {
            // `=` is not an expression operator, so `name =` here can only be
            // a keyword argument.
            if let (Kind::Word(name), Some(next)) = (&self.peek().kind, self.token(self.pos + 1)) {
                if next.is("=") {
                    return Err(self.peek().error(format!(
                        "keyword arguments are not supported; pass the value by position, without `{name}=`"
                    )));
                }
            }
            args.push(self.expr(0)?);
            if self.eat(")") {
                break;
            }
            self.expect(",")?;
        }
        args.shrink_to_fit();
        Ok(args)
    }

    pub(super) fn collection_display(&mut self, open: &str) -> Result<Expression> {
        let close = if open == "[" { "]" } else { "}" };
        let mut kind = if open == "[" { "list" } else { "dict" };
        let mut entries = Vec::new();
        let mut clauses = Vec::new();
        if !self.eat(close) {
            let first = self.expr(0)?;
            let value = if open == "{" && self.eat(":") {
                Some(self.expr(0)?)
            } else {
                None
            };
            if open == "{" && value.is_none() {
                kind = "set";
            }
            entries.push((first, value));
            if self.peek().is("for") {
                let mut element = self.deepest;
                loop {
                    let keyword = self.pos;
                    if self.eat("for") {
                        let mut name = vec![self.name()?];
                        while self.eat(",") {
                            name.push(self.name()?);
                        }
                        self.expect("in")?;
                        let iterable = self.clause(keyword, clauses.len(), &mut element)?;
                        clauses.push(Clause::For(name, iterable));
                    } else if self.eat("if") {
                        let condition = self.clause(keyword, clauses.len(), &mut element)?;
                        clauses.push(Clause::If(condition));
                    } else {
                        break;
                    }
                }
                self.deepest = self.deepest.max(element);
                self.expect(close)?;
            } else {
                while !self.eat(close) {
                    self.expect(",")?;
                    if self.eat(close) {
                        break;
                    }
                    let key = self.expr(0)?;
                    let value = if kind == "dict" {
                        self.expect(":")?;
                        Some(self.expr(0)?)
                    } else {
                        None
                    };
                    entries.push((key, value));
                }
            }
        }
        Ok(Expression::Collection {
            kind: kind.into(),
            entries,
            clauses,
        })
    }
}

impl Lower<'_> {
    pub(super) fn augment(
        &mut self,
        target: &Expr,
        op: &str,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let ty = self.place_type(target).ok_or_else(|| {
            target.at.error("augmented assignment requires an existing writable binding, field, or collection element")
        })?;
        if !ty.is_numeric() {
            return Err(target
                .at
                .error("augmented assignment currently requires a numeric destination"));
        }
        let mut indices = Vec::new();
        self.assignment_indices(target, &mut indices, ops)?;
        // ElementRef consumes its key. Hashable keys are copyable, so the read
        // and write can use separate slots without reevaluating an index.
        let mut read_indices = Vec::new();
        for index in &indices {
            let key = self.locals[*index as usize - self.parameters].clone();
            let slot = self.slot(key, &target.at)?;
            ops.push(Op::LoadLocal(*index));
            ops.push(Op::StoreLocal(slot));
            read_indices.push(slot);
        }
        let (_, read) =
            self.borrow_with_indices(target, false, &mut Some(read_indices.iter()), ops)?;
        ops.push(Op::ReadRef(ty.clone()));
        // Keep the shared loan through RHS evaluation: it permits observation
        // but prevents replacing or resizing the owner behind the saved path.
        let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
        self.same(actual, Some(ty.clone()), &value.at)?;
        self.mark_arithmetic(op, &ty, &target.at, ops);
        Self::numeric_operation(op, ty.clone(), &target.at, ops)?;
        let result = self.slot(ty.clone(), &target.at)?;
        ops.push(Op::StoreLocal(result));
        ops.push(Op::UseLoan(read));
        let (_, write) = self.borrow_with_indices(target, true, &mut Some(indices.iter()), ops)?;
        ops.push(Op::MoveLocal(result));
        ops.push(Op::Swap);
        ops.push(Op::WriteRef(ty));
        ops.push(Op::UseLoan(write));
        Ok(())
    }

    pub(super) fn unpack_tuple(
        &mut self,
        names: &[String],
        mutable: bool,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        Self::check_unpacked_arity(names.len(), value)?;
        // Reassigned bindings already have types that literals should take.
        let expected = names
            .iter()
            .map(|name| self.names.get(name).map(|local| local.ty.clone()))
            .collect::<Option<Vec<_>>>()
            .map(crate::sum::tuple);
        let ty = self
            .expr_expected(value, expected, ops)?
            .ok_or_else(|| value.at.error("expected a value, got ()"))?;
        let Ty::Enum(t) = &ty else {
            return Err(value.at.error("unpacking requires a tuple"));
        };
        if !t.tuple() {
            return Err(value.at.error("unpacking requires a tuple"));
        }
        let fields = &t.get().variants[0].fields;
        if fields.len() != names.len() {
            return Err(value.at.error("unpacking arity does not match tuple"));
        }
        let source = self.slot(ty.clone(), &value.at)?;
        ops.push(Op::StoreLocal(source));
        let mut seen = HashSet::new();
        let mut targets = Vec::new();
        for (name, ty) in names.iter().zip(fields) {
            if name != "_" && !seen.insert(name) {
                return Err(value.at.error("duplicate unpacking binding"));
            }
            if name == "_" {
                targets.push(None);
                continue;
            }
            if *ty == Ty::Unit {
                return Err(value.at.error("unit bindings are not supported yet; use _"));
            }
            let slot = if let Some(local) = self.names.get(name) {
                if mutable || !local.mutable {
                    return Err(value
                        .at
                        .error(format!("`{name}` is already bound or immutable")));
                }
                self.same(Some(ty.clone()), Some(local.ty.clone()), &value.at)?;
                local.slot
            } else {
                let slot = self.slot(ty.clone(), &value.at)?;
                self.bind(
                    name.clone(),
                    Local {
                        slot,
                        ty: ty.clone(),
                        mutable,
                    },
                );
                slot
            };
            targets.push(Some(slot));
        }
        // Move every element out at once; `_` positions are dropped here.
        ops.push(Op::MoveLocal(source));
        if let [ty] = fields.as_slice() {
            ops.push(Op::Enum(if ty.affine() {
                crate::sum::EnumOp::Take(t.clone(), 0, 0)
            } else {
                crate::sum::EnumOp::Field(t.clone(), 0, 0)
            }));
        } else {
            ops.push(Op::Split(t.clone(), 0));
        }
        ops.extend(
            targets
                .iter()
                .rev()
                .map(|slot| slot.map_or(Op::Drop, Op::StoreLocal)),
        );
        ops.push(Op::DropLocal(source));
        Ok(())
    }
    /// Report a literal of the wrong length as an unpacking error before a
    /// type hint built from the targets can describe it as an annotation.
    fn check_unpacked_arity(targets: usize, value: &Expr) -> Result<()> {
        match &value.kind {
            Expression::Tuple(values) if values.len() != targets => {
                Err(value.at.error("unpacking arity does not match tuple"))
            }
            _ => Ok(()),
        }
    }
    /// Assign each component of `value` to its target, left to right, exactly
    /// as a single assignment to that target would. The right side is complete
    /// before the first write, so targets may exchange their values.
    pub(super) fn assign_targets(
        &mut self,
        targets: &[Expr],
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let mut seen = HashSet::new();
        for target in targets {
            match &target.kind {
                Expression::Tuple(_) => {
                    return Err(target
                        .at
                        .error("nested assignment targets are not supported"));
                }
                Expression::Name(name) if name != "_" && !seen.insert(name) => {
                    return Err(target.at.error("duplicate unpacking binding"));
                }
                _ => {}
            }
        }
        Self::check_unpacked_arity(targets.len(), value)?;
        // Known target types let literals on the right take those types.
        let expected = targets
            .iter()
            .map(|target| self.place_type(target))
            .collect::<Option<Vec<_>>>()
            .map(crate::sum::tuple);
        let ty = self
            .expr_expected(value, expected, ops)?
            .ok_or_else(|| value.at.error("expected a value, got ()"))?;
        let Ty::Enum(t) = &ty else {
            return Err(value.at.error("unpacking requires a tuple"));
        };
        if !t.tuple() {
            return Err(value.at.error("unpacking requires a tuple"));
        }
        let fields = t.get().variants[0].fields.clone();
        if fields.len() != targets.len() {
            return Err(value.at.error("unpacking arity does not match tuple"));
        }
        let mut slots = Vec::new();
        for (target, ty) in targets.iter().zip(&fields) {
            slots.push(
                if matches!(&target.kind, Expression::Name(name) if name == "_") {
                    None
                } else if *ty == Ty::Unit {
                    return Err(target
                        .at
                        .error("unit bindings are not supported yet; use _"));
                } else {
                    Some(self.slot(ty.clone(), &value.at)?)
                },
            );
        }
        ops.push(Op::Split(t.clone(), 0));
        ops.extend(
            slots
                .iter()
                .rev()
                .map(|slot| slot.map_or(Op::Drop, Op::StoreLocal)),
        );
        let values = match &value.kind {
            Expression::Tuple(values) => Some(values),
            _ => None,
        };
        for (i, (target, ty)) in targets.iter().zip(fields).enumerate() {
            let Some(slot) = slots[i] else {
                continue;
            };
            // No identifier contains `<`, so this cannot shadow a source name.
            let component = format!("<component {slot}>");
            self.names.insert(
                component.clone(),
                Local {
                    slot,
                    ty,
                    mutable: false,
                },
            );
            let source = Expr {
                at: values.map_or(&value.at, |values| &values[i].at).clone(),
                kind: Expression::Name(component.clone()),
            };
            let kind = match &target.kind {
                Expression::Name(name) => Statement::Assign {
                    name: name.clone(),
                    mutable: false,
                    annotation: None,
                    value: source,
                },
                _ => Statement::SetIndex {
                    target: target.clone(),
                    value: source,
                },
            };
            let assignment = Stmt {
                at: target.at.clone(),
                kind,
            };
            self.block_inner(std::slice::from_ref(&assignment), ops, false)?;
            self.names.remove(&component);
            // A copied component must not stay alive until the block ends.
            ops.push(Op::DropLocal(slot));
        }
        Ok(())
    }
    pub(super) fn tuple(
        &mut self,
        values: &[Expr],
        expected: Type,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let fields = match &expected {
            Some(Ty::Enum(t)) if t.tuple() => Some(&t.get().variants[0].fields),
            _ => None,
        };
        if fields.is_some_and(|f| f.len() != values.len()) {
            return Err(at.error("tuple arity does not match its annotation"));
        }
        let mut types = Vec::new();
        for (i, value) in values.iter().enumerate() {
            let expected = fields.map(|f| f[i].clone());
            let actual = self
                .expr_expected(value, expected.clone(), ops)?
                .unwrap_or(Ty::Unit);
            if let Some(expected) = expected {
                self.same(Some(actual.clone()), Some(expected), &value.at)?;
            }
            if let Some(barrier) = actual.storage_barrier() {
                return Err(value.at.error(barrier.message("tuples")));
            }
            if actual == Ty::Unit {
                ops.push(Op::PushUnit);
            }
            types.push(actual);
        }
        if types.iter().map(|t| t.identity_name().len()).sum::<usize>() > 16_384
            || types.iter().any(|t| t.layout_depth() >= 64)
        {
            return Err(at.error("tuple exceeds the implementation type nesting limit"));
        }
        let Ty::Enum(t) = crate::sum::tuple(types) else {
            unreachable!()
        };
        let op = crate::sum::EnumOp::New(t, 0);
        let output = op.signature().unwrap().1;
        ops.push(Op::Enum(op));
        Ok(output)
    }
    pub(super) fn system_call(
        &mut self,
        operation: CollectionOp,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (inputs, output) = operation.signature();
        if args.len() != inputs.len() {
            return Err(at.error(format!(
                "expected {} arguments, got {}",
                inputs.len(),
                args.len()
            )));
        }
        let mut loans = Vec::new();
        for (arg, expected) in args.iter().zip(inputs) {
            let (actual, reads) = self.observe_expected(arg, Some(expected.clone()), ops)?;
            self.same(Some(actual), Some(expected), &arg.at)?;
            loans.extend(reads);
        }
        ops.push(Op::Collection(operation));
        Self::end_reads(loans, ops);
        Ok(Some(output))
    }

    pub(super) fn slot(&mut self, ty: Ty, at: &Token) -> Result<u8> {
        let slot = u8::try_from(self.parameters + self.locals.len())
            .map_err(|_| at.error("at most 256 parameter/local slots are supported"))?;
        self.locals.push(ty);
        Ok(slot)
    }

    pub(super) fn expr_expected(
        &mut self,
        e: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let ty = self.expr_hinted(e, expected.clone(), ops)?;
        Ok(match (ty, expected) {
            (Some(ty), Some(expected)) => Some(self.unbox_to(ty, &expected, ops)),
            (ty, _) => ty,
        })
    }

    fn expr_hinted(&mut self, e: &Expr, expected: Type, ops: &mut Vec<Op>) -> Result<Type> {
        match &e.kind {
            Expression::Name(name)
                if !self.names.contains_key(name)
                    && self.generics.templates.contains_key(name)
                    && matches!(expected, Some(Ty::Callable(_))) =>
            {
                let Some(Ty::Callable(signature)) = expected else {
                    unreachable!()
                };
                self.infer_function_value(name, &signature, &e.at, ops)
                    .map(Some)
            }
            Expression::Try(value) => self.propagate(e, value, expected, ops),
            Expression::Method(base, name, args)
                if matches!(name.as_str(), "unwrap_or" | "ok_or")
                    && !matches!(
                        self.place_type(base).map(classes::unbox),
                        Some(Ty::Class(_))
                    ) =>
            {
                self.option_method(base, name, args, expected, ops)
            }
            Expression::Method(base, name, args)
                if name == "unwrap" && !matches!(self.place_type(base), Some(Ty::Class(_))) =>
            {
                self.unwrap_result(base, args, expected, ops)
            }
            Expression::Call(name, args)
                if name == "range"
                    && matches!(&expected, Some(Ty::Range(_)))
                    && !self.names.contains_key(name) =>
            {
                let Some(Ty::Range(t)) = expected else {
                    unreachable!()
                };
                self.range(args, (*t).clone(), &e.at, ops).map(Some)
            }
            Expression::Number(n)
                if expected.as_ref().is_some_and(|ty| {
                    ty.is_numeric() && (ty.is_float() || !n.contains(['.', 'e', 'E']))
                }) && !numeric_suffix(n) =>
            {
                self.number(&format!("{n}{}", expected.unwrap()), false, &e.at, ops)
                    .map(Some)
            }
            Expression::Unary(op, inner)
                if op == "-"
                    && expected.as_ref().is_some_and(Ty::is_numeric)
                    && matches!(&ungroup(inner).kind, Expression::Number(n)
                        if !numeric_suffix(n) && (expected.as_ref().is_some_and(Ty::is_float)
                            || !n.contains(['.', 'e', 'E']))) =>
            {
                let Expression::Number(n) = &ungroup(inner).kind else {
                    unreachable!()
                };
                self.number(&format!("{n}{}", expected.unwrap()), true, &e.at, ops)
                    .map(Some)
            }
            Expression::Binary(op, left, right)
                if matches!(op.as_str(), "+" | "-" | "*" | "/" | "//" | "%")
                    && expected.as_ref().is_some_and(Ty::is_numeric) =>
            {
                self.numeric_binary(op, left, right, expected.unwrap(), &e.at, ops)
                    .map(Some)
            }
            Expression::Tuple(values) => self.tuple(values, expected, &e.at, ops).map(Some),
            Expression::Call(name, args)
                if enums::prelude_variant(name) && !self.names.contains_key(name) =>
            {
                self.prelude_constructor(name, Some(args), expected, &e.at, ops)
            }
            Expression::Name(name)
                if enums::prelude_variant(name) && !self.names.contains_key(name) =>
            {
                self.prelude_constructor(name, None, expected, &e.at, ops)
            }
            Expression::Collection { .. } => self.fallible_display(e, expected, ops).map(Some),
            Expression::Call(name, args)
                if matches!(name.as_str(), "list" | "dict" | "set")
                    && args.is_empty()
                    && !self.names.contains_key(name) =>
            {
                let expected = match expected {
                    Some(Ty::Enum(t)) if t.propagatable() && !t.is_option() => {
                        Some(t.get().variants[0].fields[0].clone())
                    }
                    other => other,
                };
                let ty = expected.ok_or_else(|| {
                    e.at.error("empty collection needs a type annotation or typed constructor")
                })?;
                if !kind_matches(name, &ty) {
                    return Err(e.at.error("collection type does not match its annotation"));
                }
                self.construct(ty, args, &e.at, ops).map(Some)
            }
            Expression::Group(inner) => self.expr_expected(inner, expected, ops),
            Expression::Conditional { condition, yes, no } => {
                let cond = self.expr(condition, ops)?;
                self.same(cond, Some(Ty::Bool), &condition.at)?;
                let (mut a, mut b) = (Vec::new(), Vec::new());
                let ty = self.expr_expected(yes, expected.clone(), &mut a)?;
                let other = self.expr_expected(no, expected.or_else(|| ty.clone()), &mut b)?;
                self.same(other, ty.clone(), &e.at)?;
                ops.push(branch(a, b));
                Ok(ty)
            }
            _ => self.expr(e, ops),
        }
    }

    pub(super) fn fallible_display(
        &mut self,
        e: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        use crate::sum::EnumOp;
        let Expression::Collection {
            kind,
            entries,
            clauses,
        } = &e.kind
        else {
            unreachable!()
        };
        let expected = match expected {
            Some(Ty::Enum(t))
                if t.propagatable()
                    && !t.is_option()
                    && (t.get().variants[1].fields == vec![crate::sum::alloc_error()]
                        || t.discards_error()) =>
            {
                Some(t.get().variants[0].fields[0].clone())
            }
            None => None,
            _ => {
                return Err(e
                    .at
                    .error("collection literal returns Result[collection, AllocError]; use `?` to propagate allocation failure"))
            }
        };
        if expected.as_ref().is_some_and(|ty| !kind_matches(kind, ty)) {
            return Err(e.at.error("collection type does not match its annotation"));
        }
        let start = self.locals.len();
        let owner = self.slot(Ty::I64, &e.at)?;
        let pending = self.slot(crate::sum::allocation_result(), &e.at)?;
        let mut ty = expected;
        let mut body = Vec::new();
        let saved = self.names.clone();
        self.comprehension(
            clauses,
            entries,
            kind,
            owner,
            &mut ty,
            Some(pending),
            &mut body,
        )?;
        self.names = saved;
        let ty = ty.ok_or_else(|| {
            let (annotation, display) = if kind == "list" {
                ("list[i64]", "[]")
            } else {
                ("dict[str, i64]", "{}")
            };
            e.at.error(format!(
                "empty collection needs a type; annotate the binding, as in `name: {annotation} = {display}?`, or use a typed constructor, as in `{annotation}()?`"
            ))
        })?;
        if ty.layout_depth() >= 64 {
            return Err(e
                .at
                .error("type nesting exceeds the implementation limit of 64"));
        }
        self.locals[owner as usize - self.parameters] = ty.clone();
        let output = crate::sum::result(ty.clone(), crate::sum::alloc_error());
        let Ty::Enum(result_type) = &output else {
            unreachable!()
        };
        let Ty::Enum(mutation) = crate::sum::allocation_result() else {
            unreachable!()
        };
        let result = self.slot(output.clone(), &e.at)?;
        ops.extend([
            Op::PushInt(Value::I64(0)),
            Op::Collection(CollectionOp::TryNew(ty)),
            Op::StoreLocal(result),
            Op::LoadLocal(result),
            Op::Enum(EnumOp::Tag(result_type.clone())),
            Op::PushInt(Value::I64(0)),
            Op::Eq,
        ]);
        let mut success = vec![
            Op::LoadLocal(result),
            Op::Enum(EnumOp::Field(result_type.clone(), 0, 0)),
            Op::StoreLocal(owner),
            Op::PushUnit,
            Op::Enum(EnumOp::New(mutation.clone(), 0)),
            Op::StoreLocal(pending),
        ];
        success.extend(body);
        success.extend(allocation_succeeded(pending));
        success.push(branch(
            vec![],
            vec![
                Op::MoveLocal(pending),
                Op::Enum(EnumOp::Take(mutation, 1, 0)),
                Op::Enum(EnumOp::New(result_type.clone(), 1)),
                Op::StoreLocal(result),
            ],
        ));
        ops.push(branch(success, vec![]));
        ops.push(Op::LoadLocal(result));
        self.cleanup(start, ops);
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    fn comprehension(
        &mut self,
        clauses: &[Clause],
        entries: &[(Expr, Option<Expr>)],
        kind: &str,
        result: u8,
        ty: &mut Type,
        pending: Option<u8>,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        match clauses.split_first() {
            Some((Clause::For(name, iterable), rest)) => {
                let saved = self.names.clone();
                let start = self.locals.len();
                let Iteration {
                    condition,
                    mut body,
                    step,
                    ..
                } = self.iteration(
                    name,
                    iterable,
                    comprehension_hint(name, entries, ty.as_ref()),
                    ops,
                )?;
                let temporary_start = self.expression_temps.len();
                self.comprehension(rest, entries, kind, result, ty, pending, &mut body)?;
                self.finish_temporaries(temporary_start, &mut body);
                if let Some(pending) = pending {
                    body.extend(allocation_succeeded(pending));
                    body.push(branch(vec![], vec![Op::Break]));
                }
                body.extend(step);
                ops.push(Op::Loop {
                    condition: condition.into(),
                    body: body.into(),
                });
                self.cleanup(start, ops);
                self.names = saved;
            }
            Some((Clause::If(condition), rest)) => {
                let temporary_start = self.expression_temps.len();
                let cond = self.expr(condition, ops)?;
                self.same(cond, Some(Ty::Bool), &condition.at)?;
                self.finish_temporaries(temporary_start, ops);
                let mut body = Vec::new();
                self.comprehension(rest, entries, kind, result, ty, pending, &mut body)?;
                ops.push(branch(body, Vec::new()));
            }
            None => {
                for (key, value) in entries {
                    let entry_start = ops.len();
                    ops.push(Op::LoadLocal(result));
                    let element = ty.as_ref().and_then(Ty::element);
                    let k = self
                        .expr_expected(key, element, ops)?
                        .ok_or_else(|| key.at.error("collection elements cannot be unit"))?;
                    let inferred = if let Some(value) = value {
                        let expected = match ty {
                            Some(Ty::Dict(_, v)) => Some((**v).clone()),
                            _ => None,
                        };
                        let v = self
                            .expr_expected(value, expected, ops)?
                            .ok_or_else(|| value.at.error("collection elements cannot be unit"))?;
                        if !k.hashable() {
                            return Err(key
                                .at
                                .error("dictionary keys must be integers, bool, or str"));
                        }
                        Ty::Dict(Rc::new(k), Rc::new(v))
                    } else if kind == "set" {
                        if !k.hashable() {
                            return Err(key
                                .at
                                .error("set elements must be integers, bool, or str"));
                        }
                        Ty::Set(Rc::new(k))
                    } else {
                        Ty::List(Rc::new(k))
                    };
                    if let Some(expected) = ty {
                        self.same(Some(inferred.clone()), Some(expected.clone()), &key.at)?;
                    }
                    // Keys are hashable scalars or text, which are always storable.
                    let barrier = match &inferred {
                        Ty::Dict(_, t) | Ty::List(t) | Ty::Set(t) => t.storage_barrier(),
                        _ => None,
                    };
                    if let Some(barrier) = barrier {
                        return Err(key.at.error(barrier.message("collections")));
                    }
                    *ty = Some(inferred.clone());
                    if let Some(pending) = pending {
                        ops.push(Op::Collection(CollectionOp::TryInsert(inferred)));
                        ops.push(Op::StoreLocal(pending));
                        let entry = ops.split_off(entry_start);
                        ops.extend(allocation_succeeded(pending));
                        ops.push(branch(entry, vec![]));
                    } else {
                        ops.push(Op::Collection(CollectionOp::Insert(inferred)));
                        ops.push(Op::StoreLocal(result));
                    }
                }
            }
        }
        Ok(())
    }

    fn dictionary_iteration(
        &mut self,
        names: &[String],
        base: &Expr,
        mutable: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        if names.len() != 2 || names[0] != "_" && names[0] == names[1] {
            return Err(base
                .at
                .error("dictionary items loops require two distinct bindings"));
        }
        let ty = self
            .place_type(base)
            .ok_or_else(|| base.at.error("items requires a named dictionary or field"))?;
        let Ty::Dict(key, value) = &ty else {
            return Err(base.at.error("items requires a dictionary"));
        };
        let value_ty = Ty::Ref(value.clone(), mutable);
        let key_ty = Ty::Ref(key.clone(), false);
        let (_, loan) = self.borrow(base, mutable, ops)?;
        self.loans[loan].precise = false;
        for op in ops.iter_mut() {
            if let Op::Loan(fact) = op {
                if fact.id == loan {
                    fact.precise = false;
                }
            }
        }
        ops.push(Op::ReadRef(ty.clone()));
        let source = self.slot(ty.clone(), &base.at)?;
        let index = self.slot(Ty::I64, &base.at)?;
        let key_slot = self.slot(key_ty.clone(), &base.at)?;
        let value_slot = self.slot(value_ty.clone(), &base.at)?;
        ops.extend([
            Op::StoreLocal(source),
            Op::LoadLocal(source),
            Op::Collection(CollectionOp::HashIterFirst(ty.clone())),
            Op::StoreLocal(index),
        ]);
        let mut body = vec![
            Op::BorrowLocal(source, false),
            Op::LoadLocal(index),
            Op::Collection(CollectionOp::HashIterKeyRef(ty.clone())),
            Op::StoreLocal(key_slot),
        ];
        body.push(Op::BorrowLocal(source, mutable));
        body.extend([
            Op::LoadLocal(key_slot),
            Op::ReadRef((**key).clone()),
            Op::Collection(CollectionOp::ElementRef(ty.clone(), mutable)),
            Op::StoreLocal(value_slot),
            Op::UseLoan(loan),
        ]);
        self.reference_locals.insert(key_slot, loan);
        self.reference_locals.insert(value_slot, loan);
        for (name, slot, ty) in [
            (&names[0], key_slot, key_ty),
            (&names[1], value_slot, value_ty.clone()),
        ] {
            if name != "_" {
                self.bind(
                    name.clone(),
                    Local {
                        slot,
                        ty,
                        mutable: false,
                    },
                );
            }
        }
        let mut step = vec![
            Op::LoadLocal(source),
            Op::LoadLocal(index),
            Op::Collection(CollectionOp::HashIterNext(ty)),
            Op::StoreLocal(index),
        ];
        step.push(Op::DropLocal(key_slot));
        step.push(Op::DropLocal(value_slot));
        Ok(Iteration {
            condition: vec![
                Op::LoadLocal(index),
                Op::PushInt(Value::I64(0)),
                Op::Ne,
                Op::UseLoan(loan),
            ],
            absent: vec![],
            body,
            step,
            target: value_slot,
        })
    }

    fn iteration(
        &mut self,
        names: &[String],
        iterable: &Expr,
        hint: Option<Ty>,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        if let Expression::Call(function, args) = &ungroup(iterable).kind {
            if matches!(function.as_str(), "enumerate" | "zip" | "reversed")
                && !self.names.contains_key(function)
                && !self.sigs.contains_key(function)
                && !self.generics.templates.contains_key(function)
                && !self.aliases.contains_key(function)
            {
                if function == "reversed" {
                    let [source] = args.as_slice() else {
                        return Err(iterable.at.error("reversed requires one iterable"));
                    };
                    return self.iteration_source(names, source, hint, true, ops);
                }
                return self.iteration_adapter(names, function, args, &iterable.at, ops);
            }
        }
        self.iteration_source(names, iterable, hint, false, ops)
    }

    fn iteration_source(
        &mut self,
        names: &[String],
        iterable: &Expr,
        hint: Option<Ty>,
        reverse: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        if let Some((base, mutable)) = dictionary_items(iterable) {
            if reverse {
                return Err(iterable.at.error("reversed requires a list, str, or range"));
            }
            return self.dictionary_iteration(names, base, mutable, ops);
        }
        let name = if names.len() == 1 {
            names[0].clone()
        } else {
            format!(
                "__plenty_unpack_{}_{}",
                iterable.at.line, iterable.at.column
            )
        };
        let temporary_start = self.expression_temps.len();
        let iterable = ungroup(iterable);
        let borrowed = matches!(&iterable.kind, Expression::Unary(op, _) if op == "&" || op == "&mut")
            || matches!(&iterable.kind, Expression::Name(n) if self.names.get(n).is_some_and(|l| matches!(l.ty, Ty::Ref(..))));
        let mutable = matches!(&iterable.kind, Expression::Unary(op, _) if op == "&mut")
            || matches!(&iterable.kind, Expression::Name(n) if self.names.get(n).is_some_and(|l| matches!(l.ty, Ty::Ref(_, true))));
        let range_expr = match &iterable.kind {
            Expression::Try(inner) => ungroup(inner),
            Expression::Method(inner, name, args) if name == "unwrap" && args.is_empty() => {
                ungroup(inner)
            }
            _ => iterable,
        };
        let range_args = match &range_expr.kind {
            Expression::Call(function, args)
                if function == "range" && !self.names.contains_key(function) =>
            {
                Some(args)
            }
            _ => None,
        };
        let (ty, loans) = if let Some(args) = range_args {
            (
                self.expr_expected(
                    iterable,
                    Some(Ty::Range(Rc::new(hint.unwrap_or_else(|| {
                        args.iter()
                            .take(2)
                            .find_map(|e| self.numeric_hint(e))
                            .filter(Ty::is_int)
                            .unwrap_or(Ty::I64)
                    })))),
                    ops,
                )?
                .ok_or_else(|| iterable.at.error("expected a range value"))?,
                vec![],
            )
        } else if mutable {
            let base = match &iterable.kind {
                Expression::Unary(_, base) => &**base,
                _ => iterable,
            };
            let (reference, loan) = self.borrow(base, true, ops)?;
            let Ty::Ref(ty, _) = reference else {
                unreachable!()
            };
            ops.push(Op::ReadRef((*ty).clone()));
            ((*ty).clone(), vec![loan])
        } else if borrowed {
            let base = match &iterable.kind {
                Expression::Unary(_, base) => &**base,
                _ => iterable,
            };
            let (reference, loan) = self.shared_argument(base, None, ops)?;
            let Ty::Ref(ty, _) = reference else {
                unreachable!()
            };
            ops.push(Op::ReadRef((*ty).clone()));
            ((*ty).clone(), vec![loan])
        } else {
            (self.value(iterable, ops)?, vec![])
        };
        if reverse && !matches!(ty, Ty::List(_) | Ty::Str | Ty::Range(_)) {
            return Err(iterable.at.error("reversed requires a list, str, or range"));
        }
        if borrowed && matches!(ty, Ty::Generator(_)) {
            return Err(iterable.at.error("borrowed generator iteration is not supported; use next with an exclusive reference"));
        }
        let references = borrowed && matches!(ty, Ty::List(_) | Ty::Dict(..) | Ty::Set(_));
        if mutable && !matches!(ty, Ty::List(_)) {
            return Err(iterable.at.error(
                "mutable iteration requires a list; use items for mutable dictionary values",
            ));
        }
        if borrowed && !references && ty.element().is_some_and(|t| t.affine()) {
            return Err(iterable.at.error("borrowed iteration of owned elements is not supported; iterate an owned collection or copy it"));
        }
        let mut plan = if references {
            self.borrowed_collection_iteration(ty.clone(), mutable, reverse, &iterable.at, ops)?
        } else {
            self.iteration_on_stack_direction(ty.clone(), reverse, &iterable.at, ops)?
        };
        if references {
            let loan = loans[0];
            self.loans[loan].precise = false;
            for op in ops.iter_mut() {
                if let Op::Loan(fact) = op {
                    if fact.id == loan {
                        fact.precise = false;
                    }
                }
            }
            self.reference_locals.insert(plan.target, loan);
        } else if borrowed {
            // Text scalars and range elements are computed values. Their shared
            // reference points to an iteration-local slot, not into the source.
            let value_slot = plan.target;
            let reference_slot =
                self.slot(Ty::Ref(Rc::new(ty.element().unwrap()), false), &iterable.at)?;
            let loan = self.new_loan(value_slot, false, None, &mut plan.body);
            plan.body.extend([
                Op::BorrowLocal(value_slot, false),
                Op::StoreLocal(reference_slot),
            ]);
            self.reference_locals.insert(reference_slot, loan);
            plan.target = reference_slot;
            plan.step.push(Op::DropLocal(value_slot));
        }
        if borrowed {
            // The caller's loop scope owns these locals. A borrowed temporary
            // must survive the source loan's uses through the backedge.
            self.expression_temps.truncate(temporary_start);
        } else {
            self.finish_temporaries(temporary_start, ops);
        }
        plan.condition
            .extend(loans.iter().copied().map(Op::UseLoan));
        plan.body.extend(loans.into_iter().map(Op::UseLoan));
        self.bind(
            name.clone(),
            Local {
                slot: plan.target,
                ty: if borrowed {
                    Ty::Ref(Rc::new(ty.element().unwrap()), mutable)
                } else {
                    ty.element().unwrap()
                },
                mutable: false,
            },
        );
        if names.len() > 1 {
            for name in names {
                self.names.remove(name);
            }
            self.unpack_tuple(
                names,
                false,
                &Expr {
                    at: iterable.at.clone(),
                    kind: Expression::Name(name),
                },
                &mut plan.body,
            )?;
        }
        Ok(plan)
    }

    fn iteration_adapter(
        &mut self,
        names: &[String],
        function: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        if (function == "enumerate" && !(1..=2).contains(&args.len()))
            || (function == "zip" && args.len() != 2)
        {
            return Err(at.error(if function == "enumerate" {
                "enumerate requires an iterable and an optional i64 start"
            } else {
                "zip requires two iterables"
            }));
        }
        let internal = format!("<iteration {}>", self.locals.len());
        let mut left = self.iteration(std::slice::from_ref(&internal), &args[0], None, ops)?;
        let left_local = self.names.remove(&internal).unwrap();
        let (fields, slots) = if function == "enumerate" {
            let counter = self.slot(Ty::I64, at)?;
            let first = self.slot(Ty::Bool, at)?;
            if let Some(start) = args.get(1) {
                let actual = self.expr_expected(start, Some(Ty::I64), ops)?;
                self.same(actual, Some(Ty::I64), &start.at)?;
            } else {
                ops.push(Op::PushInt(Value::I64(0)));
            }
            ops.extend([
                Op::StoreLocal(counter),
                Op::PushBool(true),
                Op::StoreLocal(first),
            ]);
            // Increment only when another item exists, so a final i64::MAX
            // index is valid. The next index reports checked overflow.
            let mut advance = Vec::new();
            self.mark(at, &mut advance);
            increment(counter, &mut advance);
            let mut body = vec![
                Op::LoadLocal(first),
                branch(vec![Op::PushBool(false), Op::StoreLocal(first)], advance),
            ];
            body.extend(left.body);
            left.body = body;
            (vec![Ty::I64, left_local.ty], vec![counter, left.target])
        } else {
            let right = self.iteration(std::slice::from_ref(&internal), &args[1], None, ops)?;
            let right_local = self.names.remove(&internal).unwrap();
            // Test left before right, including next() on a generator. Stop
            // immediately when either input ends.
            let mut skipped = right.absent.clone();
            skipped.push(Op::PushBool(false));
            left.condition.push(branch(right.condition, skipped));
            left.absent.extend(right.absent);
            left.body.extend(right.body);
            left.step.extend(right.step);
            (
                vec![left_local.ty, right_local.ty],
                vec![left.target, right.target],
            )
        };
        if fields.iter().any(|ty| matches!(ty, Ty::Ref(..))) {
            if names.len() != 2 {
                return Err(at.error("borrowed enumerate/zip requires two unpacked loop bindings"));
            }
            if names[0] == names[1] && names[0] != "_" {
                return Err(at.error("duplicate unpacking binding"));
            }
            for ((name, ty), slot) in names.iter().zip(fields).zip(slots) {
                if name != "_" {
                    self.bind(
                        name.clone(),
                        Local {
                            slot,
                            ty,
                            mutable: false,
                        },
                    );
                }
            }
        } else {
            if let Some(barrier) = fields.iter().find_map(Ty::storage_barrier) {
                return Err(at.error(barrier.message("iteration item tuples")));
            }
            if fields
                .iter()
                .map(|ty| ty.identity_name().len())
                .sum::<usize>()
                > 16_384
                || fields.iter().any(|ty| ty.layout_depth() >= 64)
            {
                return Err(at.error("type nesting exceeds the implementation limit of 64"));
            }
            let field_types = fields.clone();
            let ty = crate::sum::tuple(fields);
            let Ty::Enum(tuple) = &ty else { unreachable!() };
            for (slot, ty) in slots.into_iter().zip(field_types) {
                left.body.push(if ty.affine() {
                    Op::MoveLocal(slot)
                } else {
                    Op::LoadLocal(slot)
                });
            }
            left.body
                .push(Op::Enum(crate::sum::EnumOp::New(tuple.clone(), 0)));
            let target = self.slot(ty.clone(), at)?;
            left.body.push(Op::StoreLocal(target));
            left.step.push(Op::DropLocal(target));
            left.target = target;
            let name = format!("<adapter {target}>");
            self.bind(
                name.clone(),
                Local {
                    slot: target,
                    ty,
                    mutable: false,
                },
            );
            if names.len() == 1 {
                let local = self.names.remove(&name).unwrap();
                self.bind(names[0].clone(), local);
            } else {
                for name in names {
                    self.names.remove(name);
                }
                self.unpack_tuple(
                    names,
                    false,
                    &Expr {
                        at: at.clone(),
                        kind: Expression::Name(name.clone()),
                    },
                    &mut left.body,
                )?;
                self.names.remove(&name);
            }
        }
        Ok(left)
    }

    fn borrowed_collection_iteration(
        &mut self,
        ty: Ty,
        mutable: bool,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let source = self.slot(ty.clone(), at)?;
        let target = self.slot(Ty::Ref(Rc::new(ty.element().unwrap()), mutable), at)?;
        let index = self.slot(Ty::I64, at)?;
        ops.push(Op::StoreLocal(source));
        if reverse {
            ops.extend([
                Op::BorrowLocal(source, false),
                Op::Collection(CollectionOp::ListLenRef(ty.clone())),
            ]);
        } else {
            ops.push(Op::PushInt(Value::I64(0)));
        }
        ops.push(Op::StoreLocal(index));
        let mut step = Vec::new();
        let hashed = matches!(ty, Ty::Dict(..) | Ty::Set(_));
        if hashed {
            ops.extend([
                Op::LoadLocal(source),
                Op::Collection(CollectionOp::HashIterFirst(ty.clone())),
                Op::StoreLocal(index),
            ]);
            step.extend([
                Op::LoadLocal(source),
                Op::LoadLocal(index),
                Op::Collection(CollectionOp::HashIterNext(ty.clone())),
                Op::StoreLocal(index),
            ]);
        } else if !reverse {
            increment(index, &mut step);
        }
        let mut body = Vec::new();
        if reverse {
            decrement(index, &mut body);
        }
        body.extend([
            Op::BorrowLocal(source, mutable),
            Op::LoadLocal(index),
            Op::Collection(if hashed {
                CollectionOp::HashIterKeyRef(ty.clone())
            } else {
                CollectionOp::ElementRef(ty.clone(), mutable)
            }),
            Op::StoreLocal(target),
        ]);
        Ok(Iteration {
            condition: if hashed {
                vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Ne]
            } else if reverse {
                vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Gt]
            } else {
                vec![
                    Op::LoadLocal(index),
                    Op::BorrowLocal(source, false),
                    Op::Collection(CollectionOp::ListLenRef(ty.clone())),
                    Op::Lt,
                ]
            },
            absent: vec![],
            body,
            step,
            target,
        })
    }

    fn iteration_on_stack(&mut self, ty: Ty, at: &Token, ops: &mut Vec<Op>) -> Result<Iteration> {
        self.iteration_on_stack_direction(ty, false, at, ops)
    }

    fn iteration_on_stack_direction(
        &mut self,
        ty: Ty,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let element = ty
            .element()
            .ok_or_else(|| at.error("for requires an iterable"))?;
        let source = self.slot(ty.clone(), at)?;
        let target = self.slot(element.clone(), at)?;
        ops.push(Op::StoreLocal(source));
        if ty.restricted_storage() {
            let option = crate::sum::option(element.clone());
            let Ty::Enum(enum_type) = &option else {
                unreachable!()
            };
            let item = self.slot(option.clone(), at)?;
            return Ok(Iteration {
                absent: vec![
                    Op::Enum(crate::sum::EnumOp::New(enum_type.clone(), 0)),
                    Op::StoreLocal(item),
                ],
                condition: vec![
                    Op::Next(source),
                    Op::StoreLocal(item),
                    Op::LoadLocal(item),
                    Op::Enum(crate::sum::EnumOp::Tag(enum_type.clone())),
                    Op::PushInt(Value::I64(1)),
                    Op::Eq,
                ],
                body: vec![
                    Op::MoveLocal(item),
                    Op::Enum(if element.affine() {
                        crate::sum::EnumOp::Take(enum_type.clone(), 1, 0)
                    } else {
                        crate::sum::EnumOp::Field(enum_type.clone(), 1, 0)
                    }),
                    Op::StoreLocal(target),
                    Op::DropLocal(item),
                ],
                step: vec![Op::DropLocal(target)],
                target,
            });
        }
        let index = self.slot(Ty::I64, at)?;
        let hashed = matches!(ty, Ty::Set(_) | Ty::Dict(..));
        let text = ty == Ty::Str;
        let list = matches!(ty, Ty::List(_));
        if hashed {
            ops.extend([
                Op::LoadLocal(source),
                Op::Collection(CollectionOp::HashIterFirst(ty.clone())),
                Op::StoreLocal(index),
            ]);
        } else if reverse {
            ops.extend([
                if list {
                    Op::BorrowLocal(source, false)
                } else {
                    Op::LoadLocal(source)
                },
                Op::Collection(if text {
                    CollectionOp::TextByteLen
                } else if list {
                    CollectionOp::ListLenRef(ty.clone())
                } else {
                    CollectionOp::Len(ty.clone())
                }),
                Op::StoreLocal(index),
            ]);
        } else {
            ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(index)]);
        }
        let scalar_list = list && (element.is_numeric() || element == Ty::Bool);
        let condition = if hashed {
            vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Ne]
        } else if reverse {
            vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Gt]
        } else {
            vec![
                Op::LoadLocal(index),
                if list {
                    Op::BorrowLocal(source, false)
                } else {
                    Op::LoadLocal(source)
                },
                Op::Collection(if text {
                    CollectionOp::TextByteLen
                } else if list {
                    CollectionOp::ListLenRef(ty.clone())
                } else {
                    CollectionOp::Len(ty.clone())
                }),
                Op::Lt,
            ]
        };
        let mut body = Vec::new();
        if reverse {
            if text {
                body.extend([
                    Op::LoadLocal(source),
                    Op::LoadLocal(index),
                    Op::Collection(CollectionOp::TextPrevByte),
                    Op::StoreLocal(index),
                ]);
            } else {
                decrement(index, &mut body);
            }
        }
        body.extend([
            if scalar_list {
                Op::BorrowLocal(source, false)
            } else {
                Op::LoadLocal(source)
            },
            Op::LoadLocal(index),
            Op::Collection(if text {
                CollectionOp::TextAtByte
            } else if scalar_list {
                CollectionOp::ListScalarGetRef(ty.clone())
            } else if hashed {
                CollectionOp::HashIterGet(ty.clone())
            } else {
                if element.affine() {
                    CollectionOp::IterTake(ty.clone())
                } else {
                    CollectionOp::IterGet(ty.clone())
                }
            }),
            Op::StoreLocal(target),
        ]);
        let mut step = Vec::new();
        if hashed {
            step.extend([
                Op::LoadLocal(source),
                Op::LoadLocal(index),
                Op::Collection(CollectionOp::HashIterNext(ty)),
                Op::StoreLocal(index),
            ]);
        } else if !reverse && text {
            step.extend([
                Op::LoadLocal(source),
                Op::LoadLocal(index),
                Op::Collection(CollectionOp::TextNextByte),
                Op::StoreLocal(index),
            ]);
        } else if !reverse {
            increment(index, &mut step);
        }
        if element.managed() {
            step.push(Op::DropLocal(target));
        }
        Ok(Iteration {
            condition,
            absent: vec![],
            body,
            step,
            target,
        })
    }

    pub(super) fn boolean_reduction(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let [source] = args else {
            return Err(at.error(format!("{name} requires one boolean iterable")));
        };
        let start = self.locals.len();
        let saved = self.names.clone();
        let internal = format!("<boolean reduction {start}>");
        let Iteration {
            condition,
            mut body,
            step,
            target,
            ..
        } = self.iteration(std::slice::from_ref(&internal), source, None, ops)?;
        let item = &self.names[&internal].ty;
        let borrowed = matches!(item, Ty::Ref(inner, _) if **inner == Ty::Bool);
        if *item != Ty::Bool && !borrowed {
            return Err(source.at.error(format!(
                "{name} requires bool elements; use an explicit comparison"
            )));
        }
        self.names = saved;
        let result = self.slot(Ty::Bool, at)?;
        let identity = name == "all";
        ops.extend([Op::PushBool(identity), Op::StoreLocal(result)]);
        body.push(Op::LoadLocal(target));
        if borrowed {
            body.push(Op::ReadRef(Ty::Bool));
        }
        if identity {
            body.push(Op::Not);
        }
        body.push(branch(
            vec![Op::PushBool(!identity), Op::StoreLocal(result), Op::Break],
            vec![],
        ));
        body.extend(step);
        ops.push(Op::Loop {
            condition: condition.into(),
            body: body.into(),
        });
        ops.push(Op::LoadLocal(result));
        self.cleanup(start, ops);
        Ok(Ty::Bool)
    }

    pub(super) fn for_statement(
        &mut self,
        name: &[String],
        iterable: &Expr,
        statements: &[Stmt],
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let saved = self.names.clone();
        let start = self.locals.len();
        let Iteration {
            condition,
            mut body,
            step,
            ..
        } = self.iteration(name, iterable, None, ops)?;
        self.loop_steps.push(step.clone());
        self.loop_scopes.push(self.locals.len());
        let result = self.block(statements, &mut body, false);
        self.loop_steps.pop();
        self.loop_scopes.pop();
        self.names = saved;
        if matches!(result?, BlockResult::Continues(_)) {
            body.extend(step);
        }
        ops.push(Op::Loop {
            condition: condition.into(),
            body: body.into(),
        });
        self.cleanup(start, ops);
        Ok(())
    }

    pub(super) fn index(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Ty> {
        let Expression::Index(base, index) = &e.kind else {
            unreachable!()
        };
        if matches!(self.place_type(base), Some(Ty::Enum(t)) if t.tuple()) {
            let (reference, loan) = self.borrow(e, false, ops)?;
            let Ty::Ref(value, _) = reference else {
                unreachable!()
            };
            if value.affine() {
                return Err(index
                    .at
                    .error("cannot move out of a tuple index; unpack the tuple instead"));
            }
            ops.push(Op::ReadRef((*value).clone()));
            Self::end_reads(vec![loan], ops);
            return Ok((*value).clone());
        }
        if let Some(ty @ Ty::List(_)) = self.place_type(base) {
            let value = ty.element().unwrap();
            if value.is_numeric() || value == Ty::Bool {
                let (_, loan) = self.borrow(base, false, ops)?;
                let (got, mut reads) = self.observe_expected(index, Some(Ty::I64), ops)?;
                self.same(Some(got), Some(Ty::I64), &index.at)?;
                self.mark(&e.at, ops);
                ops.push(Op::Collection(CollectionOp::ListScalarGetRef(ty)));
                reads.push(loan);
                Self::end_reads(reads, ops);
                return Ok(value);
            }
        }
        let (ty, loans) = self.observe(base, ops)?;
        if let Ty::Enum(t) = &ty {
            if t.tuple() {
                let Expression::Number(n) = &ungroup(index).kind else {
                    return Err(index
                        .at
                        .error("tuple index must be a nonnegative integer literal"));
                };
                let i = n.parse::<usize>().map_err(|_| {
                    index
                        .at
                        .error("tuple index must be a nonnegative integer literal")
                })?;
                let value = t.get().variants[0]
                    .fields
                    .get(i)
                    .ok_or_else(|| index.at.error("tuple index out of bounds"))?
                    .clone();
                if value.affine() {
                    return Err(index
                        .at
                        .error("cannot move out of a tuple index; unpack the tuple instead"));
                }
                ops.push(Op::Enum(crate::sum::EnumOp::Field(t.clone(), 0, i)));
                Self::end_reads(loans, ops);
                return Ok(value);
            }
        }
        let (key, value) = match &ty {
            Ty::List(v) => (Ty::I64, (**v).clone()),
            Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
            Ty::Str => (Ty::I64, Ty::Str),
            Ty::Range(t) => (Ty::I64, (**t).clone()),
            _ => return Err(base.at.error("indexing requires list, dict, str, or range")),
        };
        if value.affine() {
            return Err(base
                .at
                .error("cannot move out of indexed storage; use copy(collection[index])"));
        }
        let (got, reads) = self.observe_expected(index, Some(key.clone()), ops)?;
        self.same(Some(got), Some(key), &index.at)?;
        Self::end_reads(reads, ops);
        self.mark(&e.at, ops);
        if ty == Ty::Str {
            ops.push(Op::Collection(CollectionOp::TextIndex));
            Self::end_reads(loans, ops);
            return Ok(CollectionOp::TextIndex.signature().1);
        }
        ops.push(Op::Collection(CollectionOp::Get(ty)));
        Self::end_reads(loans, ops);
        Ok(value)
    }

    pub(super) fn mutation_place(&mut self, base: &Expr, ops: &mut Vec<Op>) -> Result<usize> {
        let (reference, loan) = self.borrow(base, true, ops)?;
        let reference = self.unbox_reference(reference, ops);
        let Ty::Ref(ty, _) = reference else {
            unreachable!()
        };
        ops.push(Op::ReadRef((*ty).clone()));
        Ok(loan)
    }

    pub(super) fn set_index(
        &mut self,
        target: &Expr,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        self.set_field(target, value, ops)
    }

    fn unwrap_result(
        &mut self,
        base: &Expr,
        args: &[Expr],
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if !args.is_empty() {
            return Err(base.at.error("unwrap takes no arguments"));
        }
        let context = expected.map(|t| crate::sum::result(t, crate::sum::alloc_error()));
        let Some(Ty::Enum(t)) = self.expr_expected(base, context, ops)? else {
            return Err(base.at.error("unwrap requires a Result or Option"));
        };
        if !t.propagatable() {
            return Err(base.at.error("unwrap requires a Result or Option"));
        }
        let operation = crate::sum::EnumOp::Unwrap(t);
        let result = operation.signature().unwrap().1;
        self.mark(&base.at, ops);
        ops.push(Op::Enum(operation));
        if result == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(result))
        }
    }

    fn option_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let payload_hint = if name == "unwrap_or" {
            expected
                .clone()
                .or_else(|| {
                    match self
                        .place_type(base)
                        .or_else(|| self.expression_type_hint(base))
                    {
                        Some(Ty::Enum(t)) if t.is_option() => {
                            Some(t.get().variants[1].fields[0].clone())
                        }
                        _ => None,
                    }
                })
                .or_else(|| {
                    args.first().and_then(|argument| {
                        self.place_type(argument)
                            .or_else(|| self.numeric_hint(argument))
                    })
                })
        } else {
            match &expected {
                Some(Ty::Enum(t)) if t.is_result() => Some(t.get().variants[0].fields[0].clone()),
                _ => None,
            }
        };
        let start = self.locals.len();
        let ty = self.expr_expected(base, payload_hint.map(crate::sum::option), ops)?;
        if let Some(actual) = &ty {
            let class = classes::unbox(actual.clone());
            if matches!(class, Ty::Class(_)) {
                self.unbox_to(actual.clone(), &class, ops);
                return self.temporary_class_method(base, name, None, args, (class, vec![]), ops);
            }
        }
        let [argument] = args else {
            return Err(base.at.error(format!("{name} requires one argument")));
        };
        let Some(Ty::Enum(option)) = ty else {
            return Err(base.at.error(format!("{name} requires an Option")));
        };
        if !option.is_option() {
            return Err(base.at.error(format!("{name} requires an Option")));
        }
        let source = self.slot(Ty::Enum(option.clone()), &base.at)?;
        ops.push(Op::StoreLocal(source));
        let payload = option.get().variants[1].fields[0].clone();
        let argument_hint = if name == "unwrap_or" {
            Some(payload.clone())
        } else {
            match &expected {
                Some(Ty::Enum(t)) if t.is_result() => Some(t.get().variants[1].fields[0].clone()),
                _ => None,
            }
        };
        let actual = self
            .expr_expected(argument, argument_hint.clone(), ops)?
            .unwrap_or_else(|| {
                ops.push(Op::PushUnit);
                Ty::Unit
            });
        if let Some(hint) = argument_hint {
            self.same(Some(actual.clone()), Some(hint), &argument.at)?;
        }
        if let Some(barrier) = actual.storage_barrier() {
            return Err(argument
                .at
                .error(barrier.message("Option convenience arguments")));
        }
        let fallback = self.slot(actual.clone(), &argument.at)?;
        ops.push(Op::StoreLocal(fallback));
        let result = if name == "unwrap_or" {
            payload.clone()
        } else {
            crate::sum::result(payload.clone(), actual)
        };
        self.select_option(
            option,
            source,
            fallback,
            result.clone(),
            name == "ok_or",
            ops,
        );
        self.cleanup(start, ops);
        if result == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(result))
        }
    }

    /// Select from already evaluated operands. Lookup defaults and Option
    /// methods share the ownership transfer and unused-argument cleanup.
    fn select_option(
        &self,
        option: crate::nominal::Nominal<crate::sum::EnumType>,
        source: u8,
        fallback: u8,
        result: Ty,
        convert: bool,
        ops: &mut Vec<Op>,
    ) {
        let payload = &option.get().variants[1].fields[0];
        let mut yes = vec![
            Op::MoveLocal(source),
            Op::Enum(if payload.affine() {
                crate::sum::EnumOp::Take(option.clone(), 1, 0)
            } else {
                crate::sum::EnumOp::Field(option.clone(), 1, 0)
            }),
        ];
        let mut no = vec![Op::MoveLocal(fallback)];
        if let Ty::Enum(t) = &result {
            if convert {
                yes.push(Op::Enum(crate::sum::EnumOp::New(t.clone(), 0)));
                no.push(Op::Enum(crate::sum::EnumOp::New(t.clone(), 1)));
            }
        }
        ops.extend([
            Op::LoadLocal(source),
            Op::Enum(crate::sum::EnumOp::Tag(option)),
            Op::PushInt(Value::I64(1)),
            Op::Eq,
            branch(yes, no),
            Op::DropLocal(source),
            Op::DropLocal(fallback),
        ]);
    }

    pub(super) fn method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if matches!(self.place_type(base), Some(Ty::Task(_))) {
            return self.join_thread(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::Channel(..))) {
            return self.channel_method(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::CancellationToken)) {
            return self.control_method(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::Executor | Ty::Future(_))) {
            return self.executor_method(base, name, args, ops);
        }
        let field = Expr {
            at: base.at.clone(),
            kind: Expression::Member(Box::new(base.clone()), name.into()),
        };
        if matches!(
            self.place_type(&field),
            Some(Ty::Callable(_) | Ty::BoxedCallable(_) | Ty::Closure(_))
        ) {
            return self.call_value(&field, args, ops);
        }
        if name == "null" {
            if let Some(ty @ Ty::ForeignPtr(_)) = self.qualified_type(base)? {
                if !args.is_empty() {
                    return Err(base.at.error("null takes no arguments"));
                }
                ops.push(Op::ForeignNull(ty.clone()));
                return Ok(Some(ty));
            }
        }
        if name == "unwrap" && !matches!(self.place_type(base), Some(Ty::Class(_))) {
            return self.unwrap_result(base, args, None, ops);
        }
        if matches!(name, "unwrap_or" | "ok_or")
            && !matches!(
                self.place_type(base).map(classes::unbox),
                Some(Ty::Class(_))
            )
        {
            return self.option_method(base, name, args, None, ops);
        }
        if name == "new" {
            if let Expression::Name(function) = &ungroup(base).kind {
                if !self.names.contains_key(function)
                    && self
                        .generics
                        .functions
                        .get(function)
                        .is_some_and(|f| f.yields)
                {
                    return self.call_named(function, args, &base.at, ops);
                }
            }
            if let Expression::Member(owner, variant) = &ungroup(base).kind {
                if let Some(Ty::Enum(t)) = self.qualified_type(owner)? {
                    if Ty::Enum(t.clone()).layout_depth() >= 64 {
                        return Err(base
                            .at
                            .error("type nesting exceeds the implementation limit of 64"));
                    }
                    let definition = t.get();
                    let nullary = definition
                        .tag(variant)
                        .is_some_and(|tag| definition.variants[tag].fields.is_empty());
                    let result = self.variant(
                        Ty::Enum(t.clone()),
                        variant,
                        if nullary && args.is_empty() {
                            None
                        } else {
                            Some(args)
                        },
                        &base.at,
                        ops,
                    )?;
                    return Ok(result);
                }
            }
        }
        if let Some(ty) = self.qualified_type(base)? {
            if let Ty::Class(class) = &ty {
                if name != "new" {
                    return Err(base
                        .at
                        .error("class construction uses Class(...) or Class.new(...)"));
                }
                if Ty::Class(class.clone()).layout_depth() >= 64 {
                    return Err(base
                        .at
                        .error("type nesting exceeds the implementation limit of 64"));
                }
                modules::check_member(self.access, &class.name, "__new__", &base.at)?;
                return self.call_named(
                    &crate::record::method(&class.name, "new"),
                    args,
                    &base.at,
                    ops,
                );
            }
            if ty == Ty::Str && name == "repr" {
                if args.len() != 1 {
                    return Err(base.at.error("str.repr takes one value"));
                }
                let (source, loans) = self.observe(&args[0], ops)?;
                if source.restricted_storage() {
                    return Err(base.at.error("generators cannot be formatted"));
                }
                if source.recursive_data() {
                    return Err(base.at.error("automatic formatting is not supported for recursive data; format selected fields"));
                }
                let operation = CollectionOp::FormatValue(source);
                let output = operation.signature().1;
                ops.push(Op::Collection(operation));
                Self::end_reads(loans, ops);
                return Ok(Some(output));
            }
            if ty == Ty::Str && name == "from" {
                if args.len() != 1 {
                    return Err(base.at.error("str.from takes one numeric or bool argument"));
                }
                let (source, loans) = self.observe(&args[0], ops)?;
                if !source.is_numeric() && source != Ty::Bool {
                    return Err(args[0]
                        .at
                        .error("str.from requires a numeric or bool value"));
                }
                let operation = CollectionOp::FormatScalar(source);
                let (_, result) = operation.signature();
                ops.push(Op::Collection(operation));
                Self::end_reads(loans, ops);
                return Ok(Some(result));
            }
            if ty.is_numeric() && name == "parse" {
                if args.len() != 1 {
                    return Err(base.at.error("parse takes one string argument"));
                }
                let (actual, loans) = self.observe(&args[0], ops)?;
                self.same(Some(actual), Some(Ty::Str), &args[0].at)?;
                let operation = CollectionOp::ParseNumber(ty);
                let (_, result) = operation.signature();
                ops.push(Op::Collection(operation));
                Self::end_reads(loans, ops);
                return Ok(Some(result));
            }
            if ty.is_numeric() && name == "try_from" {
                if !ty.is_int() {
                    return Err(base
                        .at
                        .error("try_from is defined for integer types; use a cast for floats"));
                }
                if args.len() != 1 {
                    return Err(base.at.error("try_from takes one integer argument"));
                }
                let source = self.value(&args[0], ops)?;
                if !source.is_int() {
                    return Err(args[0].at.error("try_from requires an integer value"));
                }
                ops.push(Op::TryCast(ty.clone()));
                return Ok(Some(crate::sum::result(ty, crate::sum::range_error())));
            }
            if name == "wrapping_from" && !matches!(ty, Ty::Enum(_)) {
                return self.wrapping_from(ty, args, &base.at, ops).map(Some);
            }
            if matches!(ty, Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _)) {
                return self.fallible_constructor(ty, name, args, &base.at, ops);
            }
            return self.variant(ty, name, Some(args), &base.at, ops);
        }
        if let Some(Ty::Class(class)) = self.place_type(base).map(classes::unbox) {
            return self.class_method(base, class, name, None, args, ops);
        }
        if self.place_type(base) == Some(Ty::File) {
            return self.file_method(base, name, args, ops);
        }
        if matches!(name, "__init__" | "__del__" | "__new__") {
            return Err(base.at.error("lifecycle methods cannot be called directly"));
        }
        if matches!(name, "reserve" | "append" | "add" | "insert") {
            return self.fallible_mutation(base, name, args, ops);
        }
        let collection = self.place_type(base).map(classes::unbox);
        if name == "pop" && matches!(collection, Some(Ty::Dict(..) | Ty::List(_))) {
            return self.collection_removal(base, args, ops);
        }
        if name == "reverse" && matches!(collection, Some(Ty::List(_))) {
            return self.collection_unit_mutation(base, name, args, ops);
        }
        if name == "sort" && matches!(collection, Some(Ty::List(_))) {
            return self.list_sort(base, args, ops);
        }
        if name == "sort_by_key" && matches!(collection, Some(Ty::List(_))) {
            return self.list_sort_by_key(base, args, ops);
        }
        if name == "swap" && matches!(collection, Some(Ty::List(_))) {
            return self.list_swap(base, args, ops);
        }
        if name == "extend" && matches!(collection, Some(Ty::List(_))) {
            return self.fallible_mutation(base, name, args, ops);
        }
        if name == "update" && matches!(collection, Some(Ty::Dict(..) | Ty::Set(_))) {
            return self.fallible_mutation(base, name, args, ops);
        }
        if name == "clear" && matches!(collection, Some(Ty::List(_) | Ty::Set(_) | Ty::Dict(..))) {
            return self.collection_unit_mutation(base, name, args, ops);
        }
        if name == "discard" && matches!(collection, Some(Ty::Set(_))) {
            return self.collection_removal(base, args, ops);
        }
        if matches!(name, "intersection_update" | "difference_update")
            && matches!(collection, Some(Ty::Set(_)))
        {
            return self.set_filter_mutation(base, name, args, ops);
        }
        let (ty, loans) = if matches!(self.place_type(base), Some(Ty::Box(_))) {
            let (reference, loan) = self.borrow(base, false, ops)?;
            let Ty::Ref(content, _) = self.unbox_reference(reference, ops) else {
                unreachable!()
            };
            ops.push(Op::ReadRef((*content).clone()));
            ((*content).clone(), vec![loan])
        } else {
            self.observe_method_receiver(base, ops)?
        };
        if ty.is_int() && matches!(name, "wrapping_add" | "wrapping_sub" | "wrapping_mul") {
            if args.len() != 1 {
                return Err(base.at.error("wrapping arithmetic takes one argument"));
            }
            let argument = self.expr_expected(&args[0], Some(ty.clone()), ops)?;
            self.same(argument, Some(ty.clone()), &args[0].at)?;
            ops.push(Op::Integer(match name {
                "wrapping_add" => IntegerOp::WrappingAdd,
                "wrapping_sub" => IntegerOp::WrappingSub,
                _ => IntegerOp::WrappingMul,
            }));
            Self::end_reads(loans, ops);
            return Ok(Some(ty));
        }
        if ty.is_numeric() && numeric::method(name) {
            return self
                .numeric_method(base, name, args, (ty, loans), ops)
                .map(Some);
        }
        if name == "is_null" && matches!(ty, Ty::ForeignPtr(_)) {
            if !args.is_empty() {
                return Err(base.at.error("is_null takes no arguments"));
            }
            ops.push(Op::ForeignNull(ty));
            ops.push(Op::Eq);
            Self::end_reads(loans, ops);
            return Ok(Some(Ty::Bool));
        }
        if matches!(ty, Ty::Class(_)) {
            return self.temporary_class_method(base, name, None, args, (ty, loans), ops);
        }
        if matches!(ty, Ty::Executor | Ty::Future(_)) {
            return self.temporary_executor_method(base, name, args, (ty, loans), ops);
        }
        if ty == Ty::CancellationToken {
            return self.observed_control_method(base, name, args, (ty, loans), ops);
        }
        if name == "pop" {
            return Err(base
                .at
                .error("pop requires a mutable list or dictionary binding or class field"));
        }
        if name == "discard" {
            return Err(base
                .at
                .error("discard requires a mutable set binding or class field"));
        }
        if name == "reverse" {
            return Err(base
                .at
                .error("reverse requires a mutable list binding or class field"));
        }
        if matches!(name, "sort" | "sort_by_key" | "swap") && matches!(ty, Ty::List(_)) {
            return Err(base.at.error(format!(
                "{name} requires a mutable list binding or class field"
            )));
        }
        if name == "extend" {
            return Err(base
                .at
                .error("extend requires a mutable list binding or class field"));
        }
        if name == "update" {
            return Err(base
                .at
                .error("update requires a mutable dictionary or set binding or class field"));
        }
        if matches!(name, "intersection_update" | "difference_update") {
            return Err(base.at.error(format!(
                "{name} requires a mutable set binding or class field"
            )));
        }
        if name == "clear" {
            return Err(base.at.error(
                "clear requires a mutable list, dictionary, or set binding or class field",
            ));
        }
        if name == "split" && args.is_empty() {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            let operation = CollectionOp::TextTrySplitWhitespace;
            let result = operation.signature().1;
            ops.push(Op::Collection(operation));
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        if matches!(name, "lower" | "upper") {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            let operation = if name == "lower" {
                CollectionOp::TextTryLower
            } else {
                CollectionOp::TextTryUpper
            };
            let result = operation.signature().1;
            ops.push(Op::Collection(operation));
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        if name == "splitlines" {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            if args.len() > 1 {
                return Err(base.at.error("splitlines takes an optional bool argument"));
            }
            let mut reads = vec![];
            if let Some(arg) = args.first() {
                let (actual, borrowed) = self.observe(arg, ops)?;
                self.same(Some(actual), Some(Ty::Bool), &arg.at)?;
                reads = borrowed;
            } else {
                ops.push(Op::PushBool(false));
            }
            let operation = CollectionOp::TextTrySplitLines;
            let result = operation.signature().1;
            ops.push(Op::Collection(operation));
            Self::end_reads(reads, ops);
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        if matches!(name, "isascii" | "isspace" | "isdigit" | "isalpha") {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            ops.push(Op::Collection(match name {
                "isascii" => CollectionOp::TextIsAscii,
                "isspace" => CollectionOp::TextIsSpace,
                "isdigit" => CollectionOp::TextIsDigit,
                _ => CollectionOp::TextIsAlpha,
            }));
            Self::end_reads(loans, ops);
            return Ok(Some(Ty::Bool));
        }
        if matches!(name, "strip" | "lstrip" | "rstrip") {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            let operation = match name {
                "strip" => CollectionOp::TextTryStrip,
                "lstrip" => CollectionOp::TextTryLStrip,
                _ => CollectionOp::TextTryRStrip,
            };
            let (_, result) = operation.signature();
            ops.push(Op::Collection(operation));
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        if name == "slice" {
            if args.len() != 2 {
                return Err(base.at.error("slice requires start and stop arguments"));
            }
            let operation = match &ty {
                Ty::Str => CollectionOp::TextTrySlice,
                Ty::List(element) => {
                    if element.affine() && !loans.is_empty() {
                        return Err(base.at.error(
                            "slice with owned elements requires an owned temporary; use copy(items)?.slice(start, stop) for fallible duplication",
                        ));
                    }
                    CollectionOp::ListTrySlice(ty.clone())
                }
                _ => return Err(base.at.error("slice requires a list or string receiver")),
            };
            let mut argument_loans = vec![];
            for argument in args {
                let (actual, reads) = self.observe_expected(argument, Some(Ty::I64), ops)?;
                self.same(Some(actual), Some(Ty::I64), &argument.at)?;
                argument_loans.extend(reads);
            }
            let (_, result) = operation.signature();
            ops.push(Op::Collection(operation));
            Self::end_reads(argument_loans, ops);
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        if let Ty::List(element) = &ty {
            if matches!(name, "count" | "find" | "rfind") {
                if !(element.is_int()
                    || element.is_float()
                    || matches!(**element, Ty::Bool | Ty::Str))
                {
                    return Err(base
                        .at
                        .error("list search supports integer, float, bool, and str elements"));
                }
                if args.len() != 1 {
                    return Err(base
                        .at
                        .error(format!("{name} requires one element argument")));
                }
                let (actual, reads) =
                    self.observe_expected(&args[0], Some((**element).clone()), ops)?;
                self.same(Some(actual), Some((**element).clone()), &args[0].at)?;
                let operation = match name {
                    "count" => CollectionOp::ListCount(ty),
                    "find" => CollectionOp::ListFind(ty),
                    _ => CollectionOp::ListRFind(ty),
                };
                let (_, result) = operation.signature();
                ops.push(Op::Collection(operation));
                Self::end_reads(reads, ops);
                Self::end_reads(loans, ops);
                return Ok(Some(result));
            }
        }
        if (name != "get" || ty == Ty::Str)
            && matches!(
                name,
                "concat"
                    | "join"
                    | "split"
                    | "get"
                    | "replace"
                    | "startswith"
                    | "endswith"
                    | "find"
                    | "rfind"
                    | "count"
                    | "repeat"
                    | "removeprefix"
                    | "removesuffix"
            )
        {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            let arity = if name == "replace" { 2 } else { 1 };
            if args.len() != arity {
                return Err(base.at.error(if name == "replace" {
                    "replace requires old and new string arguments".to_owned()
                } else {
                    format!("{name} requires one argument")
                }));
            }
            let (expected, operation) = match name {
                "concat" => (Ty::Str, CollectionOp::TextTryConcat),
                "split" => (Ty::Str, CollectionOp::TextTrySplit),
                "get" => (Ty::I64, CollectionOp::TextGet),
                "replace" => (Ty::Str, CollectionOp::TextTryReplace),
                "startswith" => (Ty::Str, CollectionOp::TextStartsWith),
                "endswith" => (Ty::Str, CollectionOp::TextEndsWith),
                "find" => (Ty::Str, CollectionOp::TextFind),
                "rfind" => (Ty::Str, CollectionOp::TextRFind),
                "count" => (Ty::Str, CollectionOp::TextCount),
                "repeat" => (Ty::I64, CollectionOp::TextTryRepeat),
                "removeprefix" => (Ty::Str, CollectionOp::TextTryRemovePrefix),
                "removesuffix" => (Ty::Str, CollectionOp::TextTryRemoveSuffix),
                _ => (Ty::List(Rc::new(Ty::Str)), CollectionOp::TextTryJoin),
            };
            let mut argument_loans = vec![];
            for argument in args {
                let (actual, loans) =
                    self.observe_expected(argument, Some(expected.clone()), ops)?;
                self.same(Some(actual), Some(expected.clone()), &argument.at)?;
                argument_loans.extend(loans);
            }
            let (_, result) = operation.signature();
            if operation.traps() {
                self.mark(&base.at, ops);
            }
            ops.push(Op::Collection(operation));
            Self::end_reads(argument_loans, ops);
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        if matches!(
            name,
            "issubset"
                | "issuperset"
                | "isdisjoint"
                | "union"
                | "intersection"
                | "difference"
                | "symmetric_difference"
        ) {
            if !matches!(ty, Ty::Set(_)) {
                return Err(base.at.error(format!("{name} requires a set receiver")));
            }
            if args.len() != 1 {
                return Err(base.at.error(format!("{name} requires one set argument")));
            }
            let argument = &args[0];
            let (actual, reads) = self.observe_expected(argument, Some(ty.clone()), ops)?;
            self.same(Some(actual), Some(ty.clone()), &argument.at)?;
            let operation = match name {
                "issubset" => CollectionOp::SetIsSubset(ty),
                "issuperset" => CollectionOp::SetIsSuperset(ty),
                "union" => CollectionOp::SetTryUnion(ty),
                "intersection" => CollectionOp::SetTryIntersection(ty),
                "difference" => CollectionOp::SetTryDifference(ty),
                "symmetric_difference" => CollectionOp::SetTrySymmetricDifference(ty),
                _ => CollectionOp::SetIsDisjoint(ty),
            };
            let (_, result) = operation.signature();
            ops.push(Op::Collection(operation));
            Self::end_reads(reads, ops);
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        if name == "get" {
            let (expected, element, operation, collection) = match &ty {
                Ty::List(v) => (
                    Ty::I64,
                    (**v).clone(),
                    CollectionOp::ListGet(ty.clone()),
                    "list",
                ),
                Ty::Dict(k, v) => (
                    (**k).clone(),
                    (**v).clone(),
                    CollectionOp::DictGet(ty.clone()),
                    "dictionary",
                ),
                _ => return Err(base.at.error("unsupported method `get`")),
            };
            if args.len() != 1 && !(collection == "dictionary" && args.len() == 2) {
                return Err(base.at.error(if collection == "list" {
                    "get requires one index argument"
                } else {
                    "get requires one key argument and an optional default"
                }));
            }
            if element.affine() {
                return Err(base.at.error(format!(
                    "get cannot return owned {collection} values; use pop to remove and take ownership"
                )));
            }
            let (key, key_loans) = self.observe_expected(&args[0], Some(expected.clone()), ops)?;
            self.same(Some(key), Some(expected), &args[0].at)?;
            let default = if let Some(argument) = args.get(1) {
                let actual = self.expr_expected(argument, Some(element.clone()), ops)?;
                self.same(actual, Some(element.clone()), &argument.at)?;
                let slot = self.slot(element.clone(), &argument.at)?;
                ops.push(Op::StoreLocal(slot));
                Some(slot)
            } else {
                None
            };
            let result = crate::sum::option(element.clone());
            ops.push(Op::Collection(operation));
            Self::end_reads(key_loans, ops);
            Self::end_reads(loans, ops);
            if let Some(default) = default {
                let slot = self.slot(result.clone(), &base.at)?;
                ops.push(Op::StoreLocal(slot));
                let Ty::Enum(option) = result else {
                    unreachable!()
                };
                self.select_option(option, slot, default, element.clone(), false, ops);
                return Ok(Some(element));
            }
            return Ok(Some(result));
        }
        let Ty::Dict(_, v) = &ty else {
            return Err(base.at.error(format!("unsupported method `{name}`")));
        };
        if matches!(name, "keys" | "values") {
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            if name == "values" && v.affine() && !loans.is_empty() {
                return Err(base.at.error(
                    "values with owned payloads requires an owned temporary; use copy(dictionary)?.values() for fallible duplication",
                ));
            }
            let operation = if name == "keys" {
                CollectionOp::TryKeys(ty)
            } else {
                CollectionOp::TryValues(ty)
            };
            let (_, result) = operation.signature();
            ops.push(Op::Collection(operation));
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        Err(base
            .at
            .error(format!("unsupported dictionary method `{name}`")))
    }

    fn collection_removal(
        &mut self,
        base: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let ty = classes::unbox(self.place_type(base).expect("collection place"));
        let operation = match &ty {
            Ty::Dict(..) => {
                if args.len() != 1 {
                    return Err(base.at.error("pop requires one key argument"));
                }
                CollectionOp::DictPop(ty)
            }
            Ty::List(_) => {
                if args.len() > 1 {
                    return Err(base.at.error("list pop takes at most one i64 index"));
                }
                CollectionOp::ListPop(ty)
            }
            Ty::Set(_) => {
                if args.len() != 1 {
                    return Err(base.at.error("discard requires one value argument"));
                }
                CollectionOp::SetDiscard(ty)
            }
            _ => unreachable!(),
        };
        let (inputs, result) = operation.signature();
        let key = inputs[1].clone();
        // Observe the key/index before borrowing the receiver exclusively. Its
        // retained value remains valid even if derived from the same collection.
        let loans = if let Some(argument) = args.first() {
            let (actual, loans) = self.observe_expected(argument, Some(key.clone()), ops)?;
            self.same(Some(actual), Some(key.clone()), &argument.at)?;
            loans
        } else {
            ops.push(Op::PushInt(Value::I64(-1)));
            vec![]
        };
        let slot = self.slot(key, &base.at)?;
        ops.push(Op::StoreLocal(slot));
        Self::end_reads(loans, ops);
        let loan = self.mutation_place(base, ops)?;
        ops.push(Op::MoveLocal(slot));
        ops.push(Op::Collection(operation));
        ops.push(Op::UseLoan(loan));
        Ok(Some(result))
    }

    fn set_filter_mutation(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if args.len() != 1 {
            return Err(base.at.error(format!("{name} requires one set argument")));
        }
        let ty = classes::unbox(self.place_type(base).expect("set place"));
        let argument = &args[0];
        let (actual, reads) = self.observe_expected(argument, Some(ty.clone()), ops)?;
        self.same(Some(actual), Some(ty.clone()), &argument.at)?;
        let slot = self.slot(ty.clone(), &argument.at)?;
        ops.push(Op::StoreLocal(slot));
        let loan = self.mutation_place(base, ops)?;
        ops.push(Op::MoveLocal(slot));
        ops.push(Op::Collection(if name == "intersection_update" {
            CollectionOp::SetIntersectionUpdate(ty)
        } else {
            CollectionOp::SetDifferenceUpdate(ty)
        }));
        ops.push(Op::Drop);
        ops.push(Op::UseLoan(loan));
        // Keep the source shared loan live across the exclusive operation.
        // This rejects self-aliases rather than exposing aliased Rust references.
        Self::end_reads(reads, ops);
        Ok(None)
    }

    fn collection_unit_mutation(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if !args.is_empty() {
            return Err(base.at.error(format!("{name} takes no arguments")));
        }
        let ty = classes::unbox(self.place_type(base).expect("collection place"));
        let operation = match (name, &ty) {
            ("reverse", _) => CollectionOp::ListReverse(ty),
            _ => CollectionOp::Clear(ty),
        };
        let loan = self.mutation_place(base, ops)?;
        ops.push(Op::Collection(operation));
        ops.push(Op::Drop);
        ops.push(Op::UseLoan(loan));
        Ok(None)
    }

    /// Whether the runtime's total order covers `ty`. Tuples order by their
    /// components from left to right.
    pub(super) fn sortable(ty: &Ty) -> bool {
        match ty {
            Ty::Str => true,
            Ty::Enum(t) if t.tuple() => t.get().variants[0].fields.iter().all(Self::sortable),
            _ => ty.is_numeric(),
        }
    }

    fn list_swap(&mut self, base: &Expr, args: &[Expr], ops: &mut Vec<Op>) -> Result<Type> {
        if args.len() != 2 {
            return Err(base.at.error("swap requires two i64 indices"));
        }
        let ty = classes::unbox(self.place_type(base).expect("collection place"));
        // Observe both indices before borrowing the receiver exclusively, so
        // an index may be computed from the same list.
        let mut slots = Vec::new();
        for argument in args {
            let (actual, loans) = self.observe_expected(argument, Some(Ty::I64), ops)?;
            self.same(Some(actual), Some(Ty::I64), &argument.at)?;
            let slot = self.slot(Ty::I64, &argument.at)?;
            ops.push(Op::StoreLocal(slot));
            Self::end_reads(loans, ops);
            slots.push(slot);
        }
        let loan = self.mutation_place(base, ops)?;
        ops.extend(slots.into_iter().map(Op::MoveLocal));
        self.mark(&base.at, ops);
        ops.push(Op::Collection(CollectionOp::ListSwap(ty)));
        ops.push(Op::Drop);
        ops.push(Op::UseLoan(loan));
        Ok(None)
    }

    fn fallible_constructor(
        &mut self,
        ty: Ty,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if name == "from" && matches!(ty, Ty::List(_) | Ty::Set(_)) {
            if args.len() != 1 {
                return Err(at.error("from requires one owned iterable"));
            }
            if ty.layout_depth() >= 64 {
                return Err(at.error("type nesting exceeds the implementation limit of 64"));
            }
            let source = self.value(&args[0], ops)?;
            if !matches!(
                source,
                Ty::List(_) | Ty::Set(_) | Ty::Dict(..) | Ty::Range(_) | Ty::Generator(_)
            ) {
                return Err(at.error("from requires an owned collection, range, or generator"));
            }
            self.same(source.element(), ty.element(), at)?;
            return self.try_collect_on_stack(source, ty, at, ops).map(Some);
        }
        let arity = match name {
            "new" => 0,
            "with_capacity" => 1,
            _ => return Err(at.error(format!("unsupported collection type method `{name}`"))),
        };
        if args.len() != arity {
            return Err(at.error(format!("{name} requires {arity} argument(s)")));
        }
        if ty.layout_depth() >= 64 {
            return Err(at.error("type nesting exceeds the implementation limit of 64"));
        }
        if let Some(capacity) = args.first() {
            let actual = self.expr_expected(capacity, Some(Ty::I64), ops)?;
            self.same(actual, Some(Ty::I64), &capacity.at)?;
        } else {
            ops.push(Op::PushInt(Value::I64(0)));
        }
        let result = crate::sum::result(ty.clone(), crate::sum::alloc_error());
        ops.push(Op::Collection(CollectionOp::TryNew(ty)));
        Ok(Some(result))
    }

    fn fallible_mutation(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let ty = self.place_type(base).map(classes::unbox).ok_or_else(|| {
            base.at
                .error("mutation requires a named binding or class field")
        })?;
        let inputs = match (&ty, name) {
            (Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _), "reserve") => vec![Ty::I64],
            (Ty::List(t), "append") | (Ty::Set(t), "add") => vec![(**t).clone()],
            (Ty::List(_), "extend") => vec![ty.clone()],
            (Ty::Dict(..) | Ty::Set(_), "update") => vec![ty.clone()],
            (Ty::Dict(k, v), "insert") => vec![(**k).clone(), (**v).clone()],
            _ => {
                return Err(base
                    .at
                    .error(format!("unsupported method `{name}` for {ty}")))
            }
        };
        if args.len() != inputs.len() {
            return Err(base
                .at
                .error(format!("{name} requires {} argument(s)", inputs.len())));
        }
        // Evaluate arguments before taking the exclusive receiver loan, as for
        // append. Hidden locals also own earlier inputs if a later argument exits.
        let mut slots = Vec::new();
        for (arg, expected) in args.iter().zip(inputs) {
            let actual = self.expr_expected(arg, Some(expected.clone()), ops)?;
            self.same(actual, Some(expected.clone()), &arg.at)?;
            let slot = self.slot(expected, &arg.at)?;
            ops.push(Op::StoreLocal(slot));
            slots.push(slot);
        }
        let loan = self.mutation_place(base, ops)?;
        for slot in slots {
            ops.push(Op::MoveLocal(slot));
        }
        ops.push(Op::Collection(if name == "reserve" {
            CollectionOp::TryReserve(ty)
        } else if name == "extend" {
            CollectionOp::TryExtend(ty)
        } else if name == "update" {
            if matches!(ty, Ty::Set(_)) {
                CollectionOp::SetTryUpdate(ty)
            } else {
                CollectionOp::DictTryUpdate(ty)
            }
        } else {
            CollectionOp::TryInsert(ty)
        }));
        ops.push(Op::UseLoan(loan));
        Ok(Some(crate::sum::allocation_result()))
    }

    pub(super) fn range(
        &mut self,
        args: &[Expr],
        element: Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if args.is_empty() || args.len() > 3 {
            return Err(at.error("range takes one to three arguments"));
        }
        if args.len() == 1 {
            ops.push(Op::PushInt(integer(&format!("0{element}"), false, at)?));
        }
        for (i, arg) in args.iter().enumerate() {
            let expected = if i == 2 { Ty::I64 } else { element.clone() };
            let actual = self.expr_expected(arg, Some(expected.clone()), ops)?;
            self.same(actual, Some(expected), &arg.at)?;
        }
        if args.len() < 3 {
            ops.push(Op::PushInt(Value::I64(1)));
        }
        self.mark(at, ops);
        ops.push(Op::Collection(CollectionOp::Range(element.clone())));
        Ok(Ty::Range(Rc::new(element)))
    }

    pub(super) fn builtin_collection(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if name == "range" {
            return self.range(
                args,
                args.iter()
                    .take(2)
                    .find_map(|e| self.numeric_hint(e))
                    .filter(Ty::is_int)
                    .unwrap_or(Ty::I64),
                at,
                ops,
            );
        }
        if args.len() != 1 {
            return Err(at.error(format!("{name} requires one iterable; empty collections need a type annotation or typed constructor")));
        }
        if name == "len" {
            if let Some(ty @ Ty::List(_)) = self.place_type(&args[0]) {
                let (_, loan) = self.borrow(&args[0], false, ops)?;
                ops.push(Op::Collection(CollectionOp::ListLenRef(ty)));
                Self::end_reads(vec![loan], ops);
                return Ok(Ty::I64);
            }
            let (ty, loans) = self.observe(&args[0], ops)?;
            if ty.element().is_none() || ty.restricted_storage() {
                return Err(at.error("len requires an iterable"));
            }
            ops.push(Op::Collection(CollectionOp::Len(ty)));
            Self::end_reads(loans, ops);
            return Ok(Ty::I64);
        }
        let ty = self.value(&args[0], ops)?;
        let element = ty
            .element()
            .ok_or_else(|| at.error("collection constructor requires an iterable"))?;
        let out = match name {
            "list" => Ty::List(Rc::new(element)),
            "set" if element.hashable() => Ty::Set(Rc::new(element)),
            "dict" if matches!(ty, Ty::Dict(_, _)) => {
                let Ty::Enum(result) = crate::sum::result(ty, crate::sum::alloc_error()) else {
                    unreachable!()
                };
                ops.push(Op::Enum(crate::sum::EnumOp::New(result.clone(), 0)));
                return Ok(Ty::Enum(result));
            }
            _ => {
                return Err(at
                    .error("set elements must be hashable; dict conversion requires a dictionary"))
            }
        };
        self.try_collect_on_stack(ty, out, at, ops)
    }

    pub(super) fn construct(
        &mut self,
        ty: Ty,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if let Ty::Class(class) = &ty {
            if Ty::Class(class.clone()).layout_depth() >= 64 {
                return Err(at.error("type nesting exceeds the implementation limit of 64"));
            }
            modules::check_member(self.access, &class.name, "__new__", at)?;
            return self
                .call_named(&crate::record::method(&class.name, "new"), args, at, ops)?
                .ok_or_else(|| at.error("class constructor must return Result"));
        }
        if ty.is_numeric() {
            if args.len() != 1 {
                return Err(at.error("numeric casts take one argument"));
            }
            let source = self.value(&args[0], ops)?;
            if !source.is_numeric() {
                return Err(at.error("numeric casts require a number"));
            }
            self.checked_cast(&source, &ty, &args[0], at, ops)?;
            return Ok(ty);
        }
        if let Ty::Range(element) = &ty {
            return self.range(args, (**element).clone(), at, ops);
        }
        if !matches!(ty, Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _)) {
            return Err(at.error("expected a collection type"));
        }
        if args.is_empty() {
            return self
                .fallible_constructor(ty, "new", args, at, ops)
                .map(Option::unwrap);
        }
        if args.len() != 1 {
            return Err(at.error("collection constructor takes at most one iterable"));
        }
        let source = self.value(&args[0], ops)?;
        if matches!(ty, Ty::Dict(..)) {
            self.same(Some(source), Some(ty.clone()), at)?;
            let Ty::Enum(result) = crate::sum::result(ty, crate::sum::alloc_error()) else {
                unreachable!()
            };
            ops.push(Op::Enum(crate::sum::EnumOp::New(result.clone(), 0)));
            Ok(Ty::Enum(result))
        } else {
            self.same(source.element(), ty.element(), at)?;
            self.try_collect_on_stack(source, ty, at, ops)
        }
    }

    /// Keep the output Result in a local throughout iteration. Replacing its Ok
    /// owner with Err releases the partial collection; cleanup releases the
    /// current element and unconsumed source on either path.
    fn try_collect_on_stack(
        &mut self,
        source: Ty,
        target_ty: Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        use crate::sum::EnumOp;
        let start = self.locals.len();
        let Iteration {
            condition,
            mut body,
            step,
            target,
            ..
        } = self.iteration_on_stack(source, at, ops)?;
        let output = crate::sum::result(target_ty.clone(), crate::sum::alloc_error());
        let Ty::Enum(result_type) = &output else {
            unreachable!()
        };
        let mutation = crate::sum::allocation_result();
        let Ty::Enum(mutation_type) = &mutation else {
            unreachable!()
        };
        let result = self.slot(output.clone(), at)?;
        let pending = self.slot(mutation.clone(), at)?;
        ops.extend([
            Op::PushInt(Value::I64(0)),
            Op::Collection(CollectionOp::TryNew(target_ty.clone())),
            Op::StoreLocal(result),
            Op::LoadLocal(result),
            Op::Enum(EnumOp::Tag(result_type.clone())),
            Op::PushInt(Value::I64(0)),
            Op::Eq,
        ]);
        body.extend([
            Op::LoadLocal(result),
            Op::Enum(EnumOp::Field(result_type.clone(), 0, 0)),
            Op::LoadLocal(target),
            Op::Collection(CollectionOp::TryInsert(target_ty)),
            Op::StoreLocal(pending),
            Op::LoadLocal(pending),
            Op::Enum(EnumOp::Tag(mutation_type.clone())),
            Op::PushInt(Value::I64(1)),
            Op::Eq,
            branch(
                vec![
                    Op::MoveLocal(pending),
                    Op::Enum(EnumOp::Take(mutation_type.clone(), 1, 0)),
                    Op::Enum(EnumOp::New(result_type.clone(), 1)),
                    Op::StoreLocal(result),
                    Op::Break,
                ],
                vec![],
            ),
            Op::DropLocal(pending),
        ]);
        body.extend(step);
        ops.push(branch(
            vec![Op::Loop {
                condition: condition.into(),
                body: body.into(),
            }],
            vec![],
        ));
        ops.push(Op::LoadLocal(result));
        self.cleanup(start, ops);
        Ok(output)
    }
}

fn comprehension_hint(names: &[String], entries: &[(Expr, Option<Expr>)], ty: Option<&Ty>) -> Type {
    fn uses(e: &Expr, name: &str) -> bool {
        match &ungroup(e).kind {
            Expression::Name(n) => n == name,
            Expression::Unary(op, e) if op == "+" || op == "-" => uses(e, name),
            Expression::Binary(op, a, b)
                if matches!(op.as_str(), "+" | "-" | "*" | "/" | "//" | "%") =>
            {
                uses(a, name) || uses(b, name)
            }
            _ => false,
        }
    }
    if names.len() != 1 {
        return None;
    }
    let (key, value) = entries.first()?;
    match ty? {
        Ty::List(t) | Ty::Set(t) if t.is_int() && uses(key, &names[0]) => Some((**t).clone()),
        Ty::Dict(k, _) if k.is_int() && uses(key, &names[0]) => Some((**k).clone()),
        Ty::Dict(_, v) if v.is_int() && value.as_ref().is_some_and(|e| uses(e, &names[0])) => {
            Some((**v).clone())
        }
        _ => None,
    }
}

fn dictionary_items(iterable: &Expr) -> Option<(&Expr, bool)> {
    let mut e = ungroup(iterable);
    let mut mutable = false;
    if let Expression::Unary(op, base) = &e.kind {
        if op != "&" && op != "&mut" {
            return None;
        }
        mutable = op == "&mut";
        e = ungroup(base);
    }
    let Expression::Method(base, method, args) = &e.kind else {
        return None;
    };
    if method != "items" || !args.is_empty() {
        return None;
    }
    let mut base = ungroup(base);
    if let Expression::Unary(op, inner) = &base.kind {
        if op != "&" && op != "&mut" {
            return None;
        }
        mutable |= op == "&mut";
        base = ungroup(inner);
    }
    Some((base, mutable))
}

fn kind_matches(name: &str, ty: &Ty) -> bool {
    matches!(
        (name, ty),
        ("list", Ty::List(_)) | ("set", Ty::Set(_)) | ("dict", Ty::Dict(_, _))
    )
}

fn allocation_succeeded(slot: u8) -> Vec<Op> {
    let Ty::Enum(ty) = crate::sum::allocation_result() else {
        unreachable!()
    };
    vec![
        Op::LoadLocal(slot),
        Op::Enum(crate::sum::EnumOp::Tag(ty)),
        Op::PushInt(Value::I64(0)),
        Op::Eq,
    ]
}

fn decrement(index: u8, ops: &mut Vec<Op>) {
    ops.extend([
        Op::LoadLocal(index),
        Op::PushInt(Value::I64(1)),
        Op::Sub,
        Op::StoreLocal(index),
    ]);
}

fn increment(index: u8, ops: &mut Vec<Op>) {
    ops.extend([
        Op::LoadLocal(index),
        Op::PushInt(Value::I64(1)),
        Op::Add,
        Op::StoreLocal(index),
    ]);
}
