use super::*;

struct Iteration {
    condition: Vec<Op>,
    body: Vec<Op>,
    step: Vec<Op>,
    target: u8,
}

impl Parser {
    pub(super) fn arguments(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.eat(")") {
            args.push(self.expr(0)?);
            if self.eat(")") {
                break;
            }
            self.expect(",")?;
        }
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
                loop {
                    if self.eat("for") {
                        let name = self.name()?;
                        self.expect("in")?;
                        clauses.push(Clause::For(name, self.expr(1)?));
                    } else if self.eat("if") {
                        clauses.push(Clause::If(self.expr(1)?));
                    } else {
                        break;
                    }
                }
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
        match &e.kind {
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
            Expression::Collection { .. } => self.collection(e, expected, ops).map(Some),
            Expression::Call(name, args)
                if matches!(name.as_str(), "list" | "dict" | "set")
                    && args.is_empty()
                    && !self.names.contains_key(name) =>
            {
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

    pub(super) fn collection(&mut self, e: &Expr, expected: Type, ops: &mut Vec<Op>) -> Result<Ty> {
        let Expression::Collection {
            kind,
            entries,
            clauses,
        } = &e.kind
        else {
            unreachable!()
        };
        if expected.as_ref().is_some_and(|ty| !kind_matches(kind, ty)) {
            return Err(e.at.error("collection type does not match its annotation"));
        }
        let result = self.slot(Ty::I64, &e.at)?;
        let mut ty = expected;
        let mut body = Vec::new();
        let saved = self.names.clone();
        self.comprehension(clauses, entries, kind, result, &mut ty, &mut body)?;
        self.names = saved;
        let ty = ty.ok_or_else(|| {
            e.at.error("empty collection needs a type annotation or typed constructor")
        })?;
        self.locals[result as usize - self.parameters] = ty.clone();
        ops.push(Op::Collection(CollectionOp::New(ty.clone())));
        ops.push(Op::StoreLocal(result));
        ops.extend(body);
        ops.push(Op::LoadLocal(result));
        ops.push(Op::DropLocal(result));
        Ok(ty)
    }

    fn comprehension(
        &mut self,
        clauses: &[Clause],
        entries: &[(Expr, Option<Expr>)],
        kind: &str,
        result: u8,
        ty: &mut Type,
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
                } = self.iteration(name, iterable, ops)?;
                let temporary_start = self.expression_temps.len();
                self.comprehension(rest, entries, kind, result, ty, &mut body)?;
                self.finish_temporaries(temporary_start, &mut body);
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
                self.comprehension(rest, entries, kind, result, ty, &mut body)?;
                ops.push(branch(body, Vec::new()));
            }
            None => {
                for (key, value) in entries {
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
                    if inferred.element().is_some_and(|t| t.restricted_storage())
                        || matches!(&inferred, Ty::Dict(_, v) if v.restricted_storage())
                    {
                        return Err(key.at.error("generators cannot be stored in collections"));
                    }
                    *ty = Some(inferred.clone());
                    ops.push(Op::Collection(CollectionOp::Insert(inferred)));
                    ops.push(Op::StoreLocal(result));
                }
            }
        }
        Ok(())
    }

    fn iteration(&mut self, name: &str, iterable: &Expr, ops: &mut Vec<Op>) -> Result<Iteration> {
        let temporary_start = self.expression_temps.len();
        let iterable = ungroup(iterable);
        let borrowed = matches!(&iterable.kind, Expression::Unary(op, _) if op == "&" || op == "&mut")
            || matches!(&iterable.kind, Expression::Name(n) if self.names.get(n).is_some_and(|l| matches!(l.ty, Ty::Ref(..))));
        let (ty, loans) = if borrowed {
            self.observe(iterable, ops)?
        } else {
            (self.value(iterable, ops)?, vec![])
        };
        if borrowed && matches!(ty, Ty::Generator(_)) {
            return Err(iterable.at.error("borrowed generator iteration is not supported; use next with an exclusive reference"));
        }
        if borrowed && ty.element().is_some_and(|t| t.affine()) {
            return Err(iterable.at.error("borrowed iteration of owned elements is not supported; iterate an owned collection or copy it"));
        }
        let mut plan = self.iteration_on_stack(ty.clone(), &iterable.at, ops)?;
        self.finish_temporaries(temporary_start, ops);
        plan.condition
            .extend(loans.iter().copied().map(Op::UseLoan));
        plan.body.extend(loans.into_iter().map(Op::UseLoan));
        self.names.insert(
            name.into(),
            Local {
                slot: plan.target,
                ty: ty.element().unwrap(),
                mutable: false,
            },
        );
        Ok(plan)
    }

    fn iteration_on_stack(&mut self, ty: Ty, at: &Token, ops: &mut Vec<Op>) -> Result<Iteration> {
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
                condition: vec![
                    Op::Next(source, format!("{}:{}: iterator", at.line, at.column)),
                    Op::StoreLocal(item),
                    Op::LoadLocal(item),
                    Op::Enum(crate::sum::EnumOp::Tag(enum_type.clone())),
                    Op::PushInt(Value::I64(1)),
                    Op::Eq,
                ],
                body: vec![
                    Op::MoveLocal(item, format!("{}:{}: yielded payload", at.line, at.column)),
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
        ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(index)]);
        let text = ty == Ty::Str;
        let condition = vec![
            Op::LoadLocal(index),
            Op::LoadLocal(source),
            Op::Collection(if text {
                CollectionOp::TextByteLen
            } else {
                CollectionOp::Len(ty.clone())
            }),
            Op::Lt,
        ];
        let body = vec![
            Op::LoadLocal(source),
            Op::LoadLocal(index),
            Op::Collection(if text {
                CollectionOp::TextAtByte
            } else {
                if element.affine() {
                    CollectionOp::IterTake(ty)
                } else {
                    CollectionOp::IterGet(ty)
                }
            }),
            Op::StoreLocal(target),
        ];
        let mut step = Vec::new();
        if text {
            step.extend([
                Op::LoadLocal(index),
                Op::LoadLocal(target),
                Op::Collection(CollectionOp::TextByteLen),
                Op::Add,
                Op::StoreLocal(index),
            ]);
        } else {
            increment(index, &mut step);
        }
        if element.managed() {
            step.push(Op::DropLocal(target));
        }
        Ok(Iteration {
            condition,
            body,
            step,
            target,
        })
    }

    pub(super) fn for_statement(
        &mut self,
        name: &str,
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
        } = self.iteration(name, iterable, ops)?;
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

    pub(super) fn index(&mut self, base: &Expr, index: &Expr, ops: &mut Vec<Op>) -> Result<Ty> {
        let (ty, loans) = self.observe(base, ops)?;
        let (key, value) = match &ty {
            Ty::List(v) => (Ty::I64, (**v).clone()),
            Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
            Ty::Str => (Ty::I64, Ty::Str),
            Ty::Range => (Ty::I64, Ty::I64),
            _ => return Err(base.at.error("indexing requires list, dict, str, or range")),
        };
        if value.affine() {
            return Err(base
                .at
                .error("cannot move out of indexed storage; use copy(collection[index])"));
        }
        let got = self.expr_expected(index, Some(key.clone()), ops)?;
        self.same(got, Some(key), &index.at)?;
        ops.push(Op::Collection(CollectionOp::Get(ty)));
        Self::end_reads(loans, ops);
        Ok(value)
    }

    fn mutation_place(&mut self, base: &Expr, ops: &mut Vec<Op>) -> Result<usize> {
        let (reference, loan) = self.borrow(base, true, ops)?;
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
        let Expression::Index(base, index) = &target.kind else {
            return Err(target
                .at
                .error("assignment target must be a binding or an index"));
        };
        let target_ty = self.place_type(base).ok_or_else(|| {
            base.at
                .error("mutation requires a named binding or class field")
        })?;
        let (key, val) = match &target_ty {
            Ty::List(v) => (Ty::I64, (**v).clone()),
            Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
            _ => return Err(base.at.error("indexed assignment requires list or dict")),
        };
        // Like Python, evaluate the value before the target's index.
        let actual = self.expr_expected(value, Some(val.clone()), ops)?;
        self.same(actual, Some(val.clone()), &value.at)?;
        let temp = self.slot(val, &value.at)?;
        ops.push(Op::StoreLocal(temp));
        let actual = self.expr_expected(index, Some(key.clone()), ops)?;
        self.same(actual, Some(key.clone()), &index.at)?;
        let index_slot = self.slot(key, &index.at)?;
        ops.push(Op::StoreLocal(index_slot));
        let loan = self.mutation_place(base, ops)?;
        ops.push(Op::MoveLocal(index_slot, "mutation index".into()));
        ops.push(Op::LoadLocal(temp));
        ops.push(Op::Collection(CollectionOp::Put(target_ty)));
        ops.push(Op::Drop);
        ops.push(Op::UseLoan(loan));
        ops.push(Op::DropLocal(temp));
        Ok(())
    }

    pub(super) fn method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if let Some(ty) = self.qualified_type(base)? {
            return self.variant(ty, name, Some(args), &base.at, ops);
        }
        if let Some(Ty::Class(class)) = self.place_type(base) {
            return self.class_method(base, class, name, args, ops);
        }
        if matches!(name, "__init__" | "__del__" | "__new__") {
            return Err(base.at.error("lifecycle methods cannot be called directly"));
        }
        if matches!(
            name,
            "try_reserve" | "try_append" | "try_add" | "try_insert"
        ) {
            return self.fallible_mutation(base, name, args, ops);
        }
        if matches!(name, "append" | "add") {
            let target_ty = self.place_type(base).ok_or_else(|| {
                base.at
                    .error("mutation requires a named binding or class field")
            })?;
            if !matches!(
                (&target_ty, name),
                (Ty::List(_), "append") | (Ty::Set(_), "add")
            ) || args.len() != 1
            {
                return Err(base
                    .at
                    .error("append takes one list element; add takes one set element"));
            }
            let expected = target_ty.element().unwrap();
            let actual = self.expr_expected(&args[0], Some(expected.clone()), ops)?;
            self.same(actual, Some(expected.clone()), &args[0].at)?;
            let argument = self.slot(expected, &args[0].at)?;
            ops.push(Op::StoreLocal(argument));
            let loan = self.mutation_place(base, ops)?;
            ops.push(Op::MoveLocal(argument, "mutation argument".into()));
            ops.push(Op::Collection(CollectionOp::Append(target_ty)));
            ops.push(Op::Drop);
            ops.push(Op::UseLoan(loan));
            return Ok(None);
        }
        let (ty, loans) = self.observe(base, ops)?;
        if let Ty::Class(class) = &ty {
            let slot = self.slot(ty.clone(), &base.at)?;
            ops.push(Op::StoreLocal(slot));
            let receiver = format!("__plenty_receiver_{slot}");
            self.names.insert(
                receiver.clone(),
                Local {
                    slot,
                    ty: ty.clone(),
                    mutable: loans.is_empty(),
                },
            );
            let expr = Expr {
                at: base.at.clone(),
                kind: Expression::Name(receiver.clone()),
            };
            let result = self.class_method(&expr, class.clone(), name, args, ops)?;
            self.names.remove(&receiver);
            self.expression_temps.push(slot);
            Self::end_reads(loans, ops);
            return Ok(result);
        }
        let Ty::Dict(k, v) = &ty else {
            return Err(base.at.error(format!("unsupported method `{name}`")));
        };
        if !args.is_empty() {
            return Err(base.at.error("keys and values take no arguments"));
        }
        if name == "values" {
            if v.affine() && !loans.is_empty() {
                return Err(base
                    .at
                    .error("values with owned payloads require copy(dictionary).values()"));
            }
            let out = Ty::List(v.clone());
            ops.push(Op::Collection(CollectionOp::Values(ty)));
            Self::end_reads(loans, ops);
            Ok(Some(out))
        } else if name == "keys" {
            // Dictionaries already iterate over their keys.
            let out = Ty::List(k.clone());
            self.convert_on_stack(ty, out.clone(), &base.at, ops)?;
            Self::end_reads(loans, ops);
            Ok(Some(out))
        } else {
            Err(base.at.error(format!(
                "unsupported dictionary method `{name}`; iterate keys and index values"
            )))
        }
    }

    fn fallible_mutation(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let ty = self.place_type(base).ok_or_else(|| {
            base.at
                .error("mutation requires a named binding or class field")
        })?;
        let inputs = match (&ty, name) {
            (Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _), "try_reserve") => vec![Ty::I64],
            (Ty::List(t), "try_append") | (Ty::Set(t), "try_add") => vec![(**t).clone()],
            (Ty::Dict(k, v), "try_insert") => vec![(**k).clone(), (**v).clone()],
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
            ops.push(Op::MoveLocal(slot, "fallible mutation argument".into()));
        }
        ops.push(Op::Collection(if name == "try_reserve" {
            CollectionOp::TryReserve(ty)
        } else {
            CollectionOp::TryInsert(ty)
        }));
        ops.push(Op::UseLoan(loan));
        Ok(Some(crate::sum::allocation_result()))
    }

    pub(super) fn builtin_collection(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if name == "range" {
            if args.is_empty() || args.len() > 3 {
                return Err(at.error("range takes one to three i64 arguments"));
            }
            if args.len() == 1 {
                ops.push(Op::PushInt(Value::I64(0)));
            }
            for arg in args {
                let ty = self.expr(arg, ops)?;
                self.same(ty, Some(Ty::I64), &arg.at)?;
            }
            if args.len() < 3 {
                ops.push(Op::PushInt(Value::I64(1)));
            }
            ops.push(Op::Collection(CollectionOp::Range));
            return Ok(Ty::Range);
        }
        if args.len() != 1 {
            return Err(at.error(format!("{name} requires one iterable; empty collections need a type annotation or typed constructor")));
        }
        if name == "len" {
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
            "dict" if matches!(ty, Ty::Dict(_, _)) => return Ok(ty),
            _ => {
                return Err(at
                    .error("set elements must be hashable; dict conversion requires a dictionary"))
            }
        };
        self.convert_on_stack(ty, out.clone(), at, ops)?;
        Ok(out)
    }

    pub(super) fn construct(
        &mut self,
        ty: Ty,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if !matches!(ty, Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _)) {
            return Err(at.error("expected a collection type"));
        }
        if args.is_empty() {
            ops.push(Op::Collection(CollectionOp::New(ty.clone())));
        } else if args.len() == 1 {
            let source = self.value(&args[0], ops)?;
            if matches!(ty, Ty::Dict(_, _)) {
                self.same(Some(source), Some(ty.clone()), at)?;
            } else {
                self.same(source.element(), ty.element(), at)?;
                self.convert_on_stack(source, ty.clone(), at, ops)?;
            }
        } else {
            return Err(at.error("collection constructor takes at most one iterable"));
        }
        Ok(ty)
    }

    fn convert_on_stack(
        &mut self,
        source_ty: Ty,
        target_ty: Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let start = self.locals.len();
        let Iteration {
            condition,
            mut body,
            step,
            target,
        } = self.iteration_on_stack(source_ty, at, ops)?;
        let result = self.slot(target_ty.clone(), at)?;
        ops.extend([
            Op::Collection(CollectionOp::New(target_ty.clone())),
            Op::StoreLocal(result),
        ]);
        body.extend([
            Op::LoadLocal(result),
            Op::LoadLocal(target),
            Op::Collection(CollectionOp::Insert(target_ty)),
            Op::StoreLocal(result),
        ]);
        body.extend(step);
        ops.push(Op::Loop {
            condition: condition.into(),
            body: body.into(),
        });
        ops.push(Op::LoadLocal(result));
        self.cleanup(start, ops);
        Ok(())
    }
}

fn kind_matches(name: &str, ty: &Ty) -> bool {
    matches!(
        (name, ty),
        ("list", Ty::List(_)) | ("set", Ty::Set(_)) | ("dict", Ty::Dict(_, _))
    )
}

fn increment(index: u8, ops: &mut Vec<Op>) {
    ops.extend([
        Op::LoadLocal(index),
        Op::PushInt(Value::I64(1)),
        Op::Add,
        Op::StoreLocal(index),
    ]);
}
