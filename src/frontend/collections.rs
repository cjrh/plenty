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
                        let mut name = vec![self.name()?];
                        while self.eat(",") {
                            name.push(self.name()?);
                        }
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
    pub(super) fn unpack_tuple(
        &mut self,
        names: &[String],
        mutable: bool,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let ty = self.value(value, ops)?;
        let Ty::Enum(t) = &ty else {
            return Err(value.at.error("unpacking requires a tuple"));
        };
        if !t.tuple() {
            return Err(value.at.error("unpacking requires a tuple"));
        }
        let fields = &t.variants[0].fields;
        if fields.len() != names.len() {
            return Err(value.at.error("unpacking arity does not match tuple"));
        }
        let source = self.slot(ty.clone(), &value.at)?;
        ops.push(Op::StoreLocal(source));
        let mut seen = HashSet::new();
        for (i, (name, ty)) in names.iter().zip(fields).enumerate() {
            if name != "_" && !seen.insert(name) {
                return Err(value.at.error("duplicate unpacking binding"));
            }
            ops.push(Op::LoadLocal(source));
            ops.push(Op::Enum(if ty.affine() {
                crate::sum::EnumOp::Take(t.clone(), 0, i)
            } else {
                crate::sum::EnumOp::Field(t.clone(), 0, i)
            }));
            if name == "_" {
                ops.push(Op::Drop);
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
                self.names.insert(
                    name.clone(),
                    Local {
                        slot,
                        ty: ty.clone(),
                        mutable,
                    },
                );
                slot
            };
            ops.push(Op::StoreLocal(slot));
        }
        ops.push(Op::DropLocal(source));
        Ok(())
    }
    pub(super) fn tuple(
        &mut self,
        values: &[Expr],
        fallible: bool,
        expected: Type,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let expected = if fallible {
            match expected {
                Some(Ty::Enum(t)) if t.name.starts_with("Result[") => {
                    Some(t.variants[0].fields[0].clone())
                }
                _ => None,
            }
        } else {
            expected
        };
        let fields = match &expected {
            Some(Ty::Enum(t)) if t.tuple() => Some(&t.variants[0].fields),
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
            if actual.restricted_storage() {
                return Err(value
                    .at
                    .error("references and generators cannot be stored in tuples"));
            }
            if actual == Ty::Unit {
                ops.push(Op::PushUnit);
            }
            types.push(actual);
        }
        if types.iter().map(|t| t.to_string().len()).sum::<usize>() > 16_384
            || types.iter().any(|t| t.layout_depth() >= 64)
        {
            return Err(at.error("tuple exceeds the implementation type nesting limit"));
        }
        let Ty::Enum(t) = crate::sum::tuple(types) else {
            unreachable!()
        };
        let op = if fallible {
            crate::sum::EnumOp::TryNew(t, 0)
        } else {
            crate::sum::EnumOp::New(t, 0)
        };
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
            let (actual, reads) = self.observe(arg, ops)?;
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
        match &e.kind {
            Expression::Call(name, args)
                if name == "range"
                    && matches!(expected, Some(Ty::Range(_)))
                    && !self.names.contains_key(name) =>
            {
                let Some(Ty::Range(t)) = expected else {
                    unreachable!()
                };
                self.range(args, (*t).clone(), &e.at, ops).map(Some)
            }
            Expression::Number(n)
                if expected.as_ref().is_some_and(Ty::is_numeric) && !numeric_suffix(n) =>
            {
                self.number(&format!("{n}{}", expected.unwrap()), false, &e.at, ops)
                    .map(Some)
            }
            Expression::Unary(op, inner)
                if op == "-"
                    && expected.as_ref().is_some_and(Ty::is_numeric)
                    && matches!(&ungroup(inner).kind, Expression::Number(n) if !numeric_suffix(n)) =>
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
            Expression::Tuple(values, fallible) => self
                .tuple(values, *fallible, expected, &e.at, ops)
                .map(Some),
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
            Expression::FallibleCollection(inner) => {
                self.fallible_display(inner, expected, ops).map(Some)
            }
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
        self.comprehension(clauses, entries, kind, result, &mut ty, None, &mut body)?;
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
                    && t.variants[1].fields == vec![crate::sum::alloc_error()] =>
            {
                Some(t.variants[0].fields[0].clone())
            }
            None => None,
            _ => {
                return Err(e
                    .at
                    .error("try display requires a Result[collection, AllocError] context"))
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
            e.at.error("empty collection needs a Result type annotation")
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
                Op::MoveLocal(pending, "display allocation failure".into()),
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
                    if inferred.element().is_some_and(|t| t.restricted_storage())
                        || matches!(&inferred, Ty::Dict(_, v) if v.restricted_storage())
                    {
                        return Err(key.at.error("generators cannot be stored in collections"));
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
        let by_ref = mutable || value.affine();
        let value_ty = if by_ref {
            Ty::Ref(value.clone(), mutable)
        } else {
            (**value).clone()
        };
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
        let key_slot = self.slot((**key).clone(), &base.at)?;
        let value_slot = self.slot(value_ty.clone(), &base.at)?;
        ops.extend([
            Op::StoreLocal(source),
            Op::PushInt(Value::I64(0)),
            Op::StoreLocal(index),
        ]);
        let mut body = vec![
            Op::LoadLocal(source),
            Op::LoadLocal(index),
            Op::Collection(CollectionOp::IterGet(ty.clone())),
            Op::StoreLocal(key_slot),
        ];
        body.push(if by_ref {
            Op::BorrowLocal(source, mutable)
        } else {
            Op::LoadLocal(source)
        });
        body.extend([
            Op::LoadLocal(key_slot),
            Op::Collection(if by_ref {
                CollectionOp::ElementRef(ty.clone(), mutable)
            } else {
                CollectionOp::Get(ty.clone())
            }),
            Op::StoreLocal(value_slot),
            Op::UseLoan(loan),
        ]);
        if by_ref {
            self.reference_locals.insert(value_slot, loan);
        }
        for (name, slot, ty) in [
            (&names[0], key_slot, (**key).clone()),
            (&names[1], value_slot, value_ty.clone()),
        ] {
            if name != "_" {
                self.names.insert(
                    name.clone(),
                    Local {
                        slot,
                        ty,
                        mutable: false,
                    },
                );
            }
        }
        let mut step = Vec::new();
        increment(index, &mut step);
        step.push(Op::DropLocal(key_slot));
        step.push(Op::DropLocal(value_slot));
        Ok(Iteration {
            condition: vec![
                Op::LoadLocal(index),
                Op::LoadLocal(source),
                Op::Collection(CollectionOp::Len(ty)),
                Op::Lt,
                Op::UseLoan(loan),
            ],
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
        if let Some((base, mutable)) = dictionary_items(iterable) {
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
        let range_args = match &iterable.kind {
            Expression::Call(function, args)
                if function == "range" && !self.names.contains_key(function) =>
            {
                Some(args)
            }
            _ => None,
        };
        let (ty, loans) = if let Some(args) = range_args {
            (
                self.range(
                    args,
                    hint.unwrap_or_else(|| {
                        args.iter()
                            .take(2)
                            .find_map(|e| self.numeric_hint(e))
                            .filter(Ty::is_int)
                            .unwrap_or(Ty::I64)
                    }),
                    &iterable.at,
                    ops,
                )?,
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
            self.observe(iterable, ops)?
        } else {
            (self.value(iterable, ops)?, vec![])
        };
        if borrowed && matches!(ty, Ty::Generator(_)) {
            return Err(iterable.at.error("borrowed generator iteration is not supported; use next with an exclusive reference"));
        }
        let references = borrowed
            && matches!(ty, Ty::List(_))
            && (mutable || ty.element().is_some_and(|t| t.affine()));
        if borrowed && !references && ty.element().is_some_and(|t| t.affine()) {
            return Err(iterable.at.error("borrowed iteration of owned elements is not supported; iterate an owned collection or copy it"));
        }
        let mut plan = if references {
            self.borrowed_list_iteration(ty.clone(), mutable, &iterable.at, ops)?
        } else {
            self.iteration_on_stack(ty.clone(), &iterable.at, ops)?
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
        }
        self.finish_temporaries(temporary_start, ops);
        plan.condition
            .extend(loans.iter().copied().map(Op::UseLoan));
        plan.body.extend(loans.into_iter().map(Op::UseLoan));
        self.names.insert(
            name.clone(),
            Local {
                slot: plan.target,
                ty: if references {
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

    fn borrowed_list_iteration(
        &mut self,
        ty: Ty,
        mutable: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let source = self.slot(ty.clone(), at)?;
        let target = self.slot(Ty::Ref(Rc::new(ty.element().unwrap()), mutable), at)?;
        let index = self.slot(Ty::I64, at)?;
        ops.extend([
            Op::StoreLocal(source),
            Op::PushInt(Value::I64(0)),
            Op::StoreLocal(index),
        ]);
        let mut step = Vec::new();
        increment(index, &mut step);
        Ok(Iteration {
            condition: vec![
                Op::LoadLocal(index),
                Op::LoadLocal(source),
                Op::Collection(CollectionOp::Len(ty.clone())),
                Op::Lt,
            ],
            body: vec![
                Op::BorrowLocal(source, mutable),
                Op::LoadLocal(index),
                Op::Collection(CollectionOp::ElementRef(ty, mutable)),
                Op::StoreLocal(target),
            ],
            step,
            target,
        })
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

    pub(super) fn index(&mut self, base: &Expr, index: &Expr, ops: &mut Vec<Op>) -> Result<Ty> {
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
                let value = t.variants[0]
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
        if name == "try_new" {
            if let Expression::Name(function) = &ungroup(base).kind {
                let constructor = generators::constructor(function);
                if !self.names.contains_key(function) && self.sigs.contains_key(&constructor) {
                    return self.call_named(&constructor, args, &base.at, ops);
                }
            }
            if let Expression::Member(owner, variant) = &ungroup(base).kind {
                if let Some(Ty::Enum(t)) = self.qualified_type(owner)? {
                    if t.depth >= 64 {
                        return Err(base
                            .at
                            .error("type nesting exceeds the implementation limit of 64"));
                    }
                    let nullary = t
                        .variants
                        .iter()
                        .find(|v| v.name == *variant)
                        .is_some_and(|v| v.fields.is_empty());
                    let ty = self.variant(
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
                    let Some(Op::Enum(crate::sum::EnumOp::New(t, tag))) = ops.pop() else {
                        unreachable!()
                    };
                    ops.push(Op::Enum(crate::sum::EnumOp::TryNew(t, tag)));
                    return Ok(Some(crate::sum::result(
                        ty.unwrap(),
                        crate::sum::alloc_error(),
                    )));
                }
            }
        }
        if let Some(ty) = self.qualified_type(base)? {
            if let Ty::Class(class) = &ty {
                if name != "try_new" {
                    return Err(base
                        .at
                        .error("class construction uses Class(...) or Class.try_new(...)"));
                }
                if class.depth >= 64 {
                    return Err(base
                        .at
                        .error("type nesting exceeds the implementation limit of 64"));
                }
                modules::check_member(self.access, &class.name, "__new__", &base.at)?;
                return self.call_named(
                    &crate::record::method(&class.name, "try_new"),
                    args,
                    &base.at,
                    ops,
                );
            }
            if ty == Ty::Str && name == "try_repr" {
                if args.len() != 1 {
                    return Err(base.at.error("str.try_repr takes one value"));
                }
                let (source, loans) = self.observe(&args[0], ops)?;
                if source.restricted_storage() {
                    return Err(base.at.error("generators cannot be formatted"));
                }
                let operation = CollectionOp::FormatValue(source);
                let output = operation.signature().1;
                ops.push(Op::Collection(operation));
                Self::end_reads(loans, ops);
                return Ok(Some(output));
            }
            if ty == Ty::Str && name == "try_from" {
                if args.len() != 1 {
                    return Err(base
                        .at
                        .error("str.try_from takes one numeric or bool argument"));
                }
                let (source, loans) = self.observe(&args[0], ops)?;
                if !source.is_numeric() && source != Ty::Bool {
                    return Err(args[0]
                        .at
                        .error("str.try_from requires a numeric or bool value"));
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
            if matches!(ty, Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _)) {
                return self.fallible_constructor(ty, name, args, &base.at, ops);
            }
            return self.variant(ty, name, Some(args), &base.at, ops);
        }
        if let Some(Ty::Class(class)) = self.place_type(base) {
            return self.class_method(base, class, name, args, ops);
        }
        if self.place_type(base) == Some(Ty::File) {
            return self.file_method(base, name, args, ops);
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
        if name == "pop" && matches!(self.place_type(base), Some(Ty::Dict(..) | Ty::List(_))) {
            return self.collection_removal(base, args, ops);
        }
        if name == "reverse" && matches!(self.place_type(base), Some(Ty::List(_))) {
            return self.collection_unit_mutation(base, name, args, ops);
        }
        if name == "try_extend" && matches!(self.place_type(base), Some(Ty::List(_))) {
            return self.fallible_mutation(base, name, args, ops);
        }
        if name == "try_update" && matches!(self.place_type(base), Some(Ty::Dict(..) | Ty::Set(_)))
        {
            return self.fallible_mutation(base, name, args, ops);
        }
        if name == "clear"
            && matches!(
                self.place_type(base),
                Some(Ty::List(_) | Ty::Set(_) | Ty::Dict(..))
            )
        {
            return self.collection_unit_mutation(base, name, args, ops);
        }
        if name == "discard" && matches!(self.place_type(base), Some(Ty::Set(_))) {
            return self.collection_removal(base, args, ops);
        }
        if matches!(name, "intersection_update" | "difference_update")
            && matches!(self.place_type(base), Some(Ty::Set(_)))
        {
            return self.set_filter_mutation(base, name, args, ops);
        }
        let (ty, loans) = self.observe(base, ops)?;
        if let Ty::Class(class) = &ty {
            if self
                .sigs
                .get(&crate::record::method(&class.name, name))
                .is_some_and(|sig| matches!(sig.outputs.first(), Some(Ty::Ref(..))))
            {
                return Err(base
                    .at
                    .error("reference-returning methods require a named receiver or class field"));
            }
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
        if name == "try_extend" {
            return Err(base
                .at
                .error("try_extend requires a mutable list binding or class field"));
        }
        if name == "try_update" {
            return Err(base
                .at
                .error("try_update requires a mutable dictionary or set binding or class field"));
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
        if name == "try_splitlines" {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            if args.len() > 1 {
                return Err(base
                    .at
                    .error("try_splitlines takes an optional bool argument"));
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
        if matches!(name, "isascii" | "isspace") {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            ops.push(Op::Collection(if name == "isascii" {
                CollectionOp::TextIsAscii
            } else {
                CollectionOp::TextIsSpace
            }));
            Self::end_reads(loans, ops);
            return Ok(Some(Ty::Bool));
        }
        if matches!(name, "try_strip" | "try_lstrip" | "try_rstrip") {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            let operation = match name {
                "try_strip" => CollectionOp::TextTryStrip,
                "try_lstrip" => CollectionOp::TextTryLStrip,
                _ => CollectionOp::TextTryRStrip,
            };
            let (_, result) = operation.signature();
            ops.push(Op::Collection(operation));
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        if name == "try_slice" {
            if args.len() != 2 {
                return Err(base.at.error("try_slice requires start and stop arguments"));
            }
            let operation = match &ty {
                Ty::Str => CollectionOp::TextTrySlice,
                Ty::List(element) => {
                    if element.affine() && !loans.is_empty() {
                        return Err(base.at.error(
                            "try_slice with owned elements requires an owned temporary; use try_copy(items)?.try_slice(start, stop) for fallible duplication",
                        ));
                    }
                    CollectionOp::ListTrySlice(ty.clone())
                }
                _ => {
                    return Err(base
                        .at
                        .error("try_slice requires a list or string receiver"))
                }
            };
            let mut argument_loans = vec![];
            for argument in args {
                let (actual, reads) = self.observe(argument, ops)?;
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
                let (actual, reads) = self.observe(&args[0], ops)?;
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
        if matches!(
            name,
            "try_concat"
                | "try_join"
                | "try_split"
                | "try_get"
                | "try_replace"
                | "startswith"
                | "endswith"
                | "find"
                | "rfind"
                | "count"
                | "try_repeat"
                | "try_removeprefix"
                | "try_removesuffix"
        ) {
            self.same(Some(ty), Some(Ty::Str), &base.at)?;
            let arity = if name == "try_replace" { 2 } else { 1 };
            if args.len() != arity {
                return Err(base.at.error(if name == "try_replace" {
                    "try_replace requires old and new string arguments".to_owned()
                } else {
                    format!("{name} requires one argument")
                }));
            }
            let (expected, operation) = match name {
                "try_concat" => (Ty::Str, CollectionOp::TextTryConcat),
                "try_split" => (Ty::Str, CollectionOp::TextTrySplit),
                "try_get" => (Ty::I64, CollectionOp::TextTryGet),
                "try_replace" => (Ty::Str, CollectionOp::TextTryReplace),
                "startswith" => (Ty::Str, CollectionOp::TextStartsWith),
                "endswith" => (Ty::Str, CollectionOp::TextEndsWith),
                "find" => (Ty::Str, CollectionOp::TextFind),
                "rfind" => (Ty::Str, CollectionOp::TextRFind),
                "count" => (Ty::Str, CollectionOp::TextCount),
                "try_repeat" => (Ty::I64, CollectionOp::TextTryRepeat),
                "try_removeprefix" => (Ty::Str, CollectionOp::TextTryRemovePrefix),
                "try_removesuffix" => (Ty::Str, CollectionOp::TextTryRemoveSuffix),
                _ => (Ty::List(Rc::new(Ty::Str)), CollectionOp::TextTryJoin),
            };
            let mut argument_loans = vec![];
            for argument in args {
                if matches!(ungroup(argument).kind, Expression::Collection { .. }) {
                    let actual = self.expr_expected(argument, Some(expected.clone()), ops)?;
                    self.same(actual, Some(expected.clone()), &argument.at)?;
                } else {
                    let (actual, loans) = self.observe(argument, ops)?;
                    self.same(Some(actual), Some(expected.clone()), &argument.at)?;
                    argument_loans.extend(loans);
                }
            }
            let (_, result) = operation.signature();
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
                | "try_union"
                | "try_intersection"
                | "try_difference"
                | "try_symmetric_difference"
        ) {
            if !matches!(ty, Ty::Set(_)) {
                return Err(base.at.error(format!("{name} requires a set receiver")));
            }
            if args.len() != 1 {
                return Err(base.at.error(format!("{name} requires one set argument")));
            }
            let argument = &args[0];
            let reads = if matches!(ungroup(argument).kind, Expression::Collection { .. }) {
                let actual = self.expr_expected(argument, Some(ty.clone()), ops)?;
                self.same(actual, Some(ty.clone()), &argument.at)?;
                vec![]
            } else {
                let (actual, reads) = self.observe(argument, ops)?;
                self.same(Some(actual), Some(ty.clone()), &argument.at)?;
                reads
            };
            let operation = match name {
                "issubset" => CollectionOp::SetIsSubset(ty),
                "issuperset" => CollectionOp::SetIsSuperset(ty),
                "try_union" => CollectionOp::SetTryUnion(ty),
                "try_intersection" => CollectionOp::SetTryIntersection(ty),
                "try_difference" => CollectionOp::SetTryDifference(ty),
                "try_symmetric_difference" => CollectionOp::SetTrySymmetricDifference(ty),
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
            if args.len() != 1 {
                return Err(base.at.error(if collection == "list" {
                    "get requires one index argument"
                } else {
                    "get requires one key argument"
                }));
            }
            if element.affine() {
                return Err(base.at.error(format!(
                    "get cannot return owned {collection} values; use pop to remove and take ownership"
                )));
            }
            let (key, key_loans) = self.observe(&args[0], ops)?;
            self.same(Some(key), Some(expected), &args[0].at)?;
            let result = crate::sum::option(element);
            ops.push(Op::Collection(operation));
            Self::end_reads(key_loans, ops);
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
        let Ty::Dict(k, v) = &ty else {
            return Err(base.at.error(format!("unsupported method `{name}`")));
        };
        if matches!(name, "try_keys" | "try_values") {
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            if name == "try_values" && v.affine() && !loans.is_empty() {
                return Err(base.at.error(
                    "try_values with owned payloads requires an owned temporary; use try_copy(dictionary)?.try_values() for fallible duplication",
                ));
            }
            let operation = if name == "try_keys" {
                CollectionOp::TryKeys(ty)
            } else {
                CollectionOp::TryValues(ty)
            };
            let (_, result) = operation.signature();
            ops.push(Op::Collection(operation));
            Self::end_reads(loans, ops);
            return Ok(Some(result));
        }
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

    fn collection_removal(
        &mut self,
        base: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let ty = self.place_type(base).expect("collection place");
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
            let (actual, loans) = self.observe(argument, ops)?;
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
        ops.push(Op::MoveLocal(slot, "collection removal argument".into()));
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
        let ty = self.place_type(base).expect("set place");
        let argument = &args[0];
        let reads = if matches!(ungroup(argument).kind, Expression::Collection { .. }) {
            let actual = self.expr_expected(argument, Some(ty.clone()), ops)?;
            self.same(actual, Some(ty.clone()), &argument.at)?;
            vec![]
        } else {
            let (actual, reads) = self.observe(argument, ops)?;
            self.same(Some(actual), Some(ty.clone()), &argument.at)?;
            reads
        };
        let slot = self.slot(ty.clone(), &argument.at)?;
        ops.push(Op::StoreLocal(slot));
        let loan = self.mutation_place(base, ops)?;
        ops.push(Op::MoveLocal(slot, "set filter argument".into()));
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
        let ty = self.place_type(base).expect("collection place");
        let loan = self.mutation_place(base, ops)?;
        ops.push(Op::Collection(if name == "reverse" {
            CollectionOp::ListReverse(ty)
        } else {
            CollectionOp::Clear(ty)
        }));
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
        if name == "try_from" && matches!(ty, Ty::List(_) | Ty::Set(_)) {
            if args.len() != 1 {
                return Err(at.error("try_from requires one owned iterable"));
            }
            if ty.layout_depth() >= 64 {
                return Err(at.error("type nesting exceeds the implementation limit of 64"));
            }
            let source = self.value(&args[0], ops)?;
            if !matches!(
                source,
                Ty::List(_) | Ty::Set(_) | Ty::Dict(..) | Ty::Range(_) | Ty::Generator(_)
            ) {
                return Err(at.error("try_from requires an owned collection, range, or generator"));
            }
            self.same(source.element(), ty.element(), at)?;
            return self.try_collect_on_stack(source, ty, at, ops).map(Some);
        }
        let arity = match name {
            "try_new" => 0,
            "try_with_capacity" => 1,
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
        let ty = self.place_type(base).ok_or_else(|| {
            base.at
                .error("mutation requires a named binding or class field")
        })?;
        let inputs = match (&ty, name) {
            (Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _), "try_reserve") => vec![Ty::I64],
            (Ty::List(t), "try_append") | (Ty::Set(t), "try_add") => vec![(**t).clone()],
            (Ty::List(_), "try_extend") => vec![ty.clone()],
            (Ty::Dict(..) | Ty::Set(_), "try_update") => vec![ty.clone()],
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
        } else if name == "try_extend" {
            CollectionOp::TryExtend(ty)
        } else if name == "try_update" {
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
        if let Ty::Range(element) = &ty {
            return self.range(args, (**element).clone(), at, ops);
        }
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
                    Op::MoveLocal(pending, "collection allocation error".into()),
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

fn increment(index: u8, ops: &mut Vec<Op>) {
    ops.extend([
        Op::LoadLocal(index),
        Op::PushInt(Value::I64(1)),
        Op::Add,
        Op::StoreLocal(index),
    ]);
}
