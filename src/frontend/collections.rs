use super::*;

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
    fn slot(&mut self, ty: Ty, at: &Token) -> Result<u8> {
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
                let (condition, mut body, index) = self.iteration(name, iterable, ops)?;
                self.comprehension(rest, entries, kind, result, ty, &mut body)?;
                increment(index, &mut body);
                ops.push(Op::Loop {
                    condition: condition.into(),
                    body: body.into(),
                });
                self.names = saved;
            }
            Some((Clause::If(condition), rest)) => {
                let cond = self.expr(condition, ops)?;
                self.same(cond, Some(Ty::Bool), &condition.at)?;
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
                    *ty = Some(inferred.clone());
                    ops.push(Op::Collection(CollectionOp::Insert(inferred)));
                    ops.push(Op::StoreLocal(result));
                }
            }
        }
        Ok(())
    }

    fn iteration(
        &mut self,
        name: &str,
        iterable: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<(Vec<Op>, Vec<Op>, u8)> {
        let ty = self.value(iterable, ops)?;
        let element = ty
            .element()
            .ok_or_else(|| iterable.at.error("for requires an iterable"))?;
        let source = self.slot(ty.clone(), &iterable.at)?;
        let index = self.slot(Ty::I64, &iterable.at)?;
        let target = self.slot(element.clone(), &iterable.at)?;
        ops.push(Op::StoreLocal(source));
        ops.push(Op::PushInt(Value::I64(0)));
        ops.push(Op::StoreLocal(index));
        self.names.insert(
            name.into(),
            Local {
                slot: target,
                ty: element,
                mutable: false,
            },
        );
        let condition = vec![
            Op::LoadLocal(index),
            Op::LoadLocal(source),
            Op::Collection(CollectionOp::Len(ty.clone())),
            Op::Lt,
        ];
        let body = vec![
            Op::LoadLocal(source),
            Op::LoadLocal(index),
            Op::Collection(CollectionOp::IterGet(ty)),
            Op::StoreLocal(target),
        ];
        Ok((condition, body, index))
    }

    pub(super) fn for_statement(
        &mut self,
        name: &str,
        iterable: &Expr,
        statements: &[Stmt],
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let saved = self.names.clone();
        let (condition, mut body, index) = self.iteration(name, iterable, ops)?;
        if matches!(
            self.block(statements, &mut body, false)?,
            BlockResult::Continues(_)
        ) {
            increment(index, &mut body);
        }
        self.names = saved;
        ops.push(Op::Loop {
            condition: condition.into(),
            body: body.into(),
        });
        Ok(())
    }

    pub(super) fn index(&mut self, base: &Expr, index: &Expr, ops: &mut Vec<Op>) -> Result<Ty> {
        let ty = self.value(base, ops)?;
        let (key, value) = match &ty {
            Ty::List(v) => (Ty::I64, (**v).clone()),
            Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
            Ty::Str => (Ty::I64, Ty::Str),
            Ty::Range => (Ty::I64, Ty::I64),
            _ => return Err(base.at.error("indexing requires list, dict, str, or range")),
        };
        let got = self.expr_expected(index, Some(key.clone()), ops)?;
        self.same(got, Some(key), &index.at)?;
        ops.push(Op::Collection(CollectionOp::Get(ty)));
        Ok(value)
    }

    fn mutable_collection(&self, base: &Expr) -> Result<Local> {
        let Expression::Name(name) = &base.kind else {
            return Err(base.at.error("collection mutation requires a named mutable binding; update nested values separately"));
        };
        let local = self
            .names
            .get(name)
            .ok_or_else(|| base.at.error(format!("unknown binding `{name}`")))?;
        if !local.mutable {
            return Err(base
                .at
                .error(format!("`{name}` is immutable; declare it with `mut`")));
        }
        Ok(local.clone())
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
        let local = self.mutable_collection(base)?;
        let (key, val) = match &local.ty {
            Ty::List(v) => (Ty::I64, (**v).clone()),
            Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
            _ => return Err(base.at.error("indexed assignment requires list or dict")),
        };
        // Like Python, evaluate the value before the target's index.
        let actual = self.expr_expected(value, Some(val.clone()), ops)?;
        self.same(actual, Some(val.clone()), &value.at)?;
        let temp = self.slot(val, &value.at)?;
        ops.push(Op::StoreLocal(temp));
        ops.push(Op::LoadLocal(local.slot));
        let actual = self.expr_expected(index, Some(key.clone()), ops)?;
        self.same(actual, Some(key), &index.at)?;
        ops.push(Op::LoadLocal(temp));
        ops.push(Op::Collection(CollectionOp::Put(local.ty)));
        ops.push(Op::StoreLocal(local.slot));
        Ok(())
    }

    pub(super) fn method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if matches!(name, "append" | "add") {
            let local = self.mutable_collection(base)?;
            if !matches!(
                (&local.ty, name),
                (Ty::List(_), "append") | (Ty::Set(_), "add")
            ) || args.len() != 1
            {
                return Err(base
                    .at
                    .error("append takes one list element; add takes one set element"));
            }
            ops.push(Op::LoadLocal(local.slot));
            let expected = local.ty.element().unwrap();
            let actual = self.expr_expected(&args[0], Some(expected.clone()), ops)?;
            self.same(actual, Some(expected), &args[0].at)?;
            ops.push(Op::Collection(CollectionOp::Append(local.ty)));
            ops.push(Op::StoreLocal(local.slot));
            return Ok(None);
        }
        let ty = self.value(base, ops)?;
        let Ty::Dict(k, v) = &ty else {
            return Err(base.at.error(format!("unsupported method `{name}`")));
        };
        if !args.is_empty() {
            return Err(base.at.error("keys and values take no arguments"));
        }
        if name == "values" {
            let out = Ty::List(v.clone());
            ops.push(Op::Collection(CollectionOp::Values(ty)));
            Ok(Some(out))
        } else if name == "keys" {
            // Dictionaries already iterate over their keys.
            let out = Ty::List(k.clone());
            self.convert_on_stack(ty, out.clone(), &base.at, ops)?;
            Ok(Some(out))
        } else {
            Err(base.at.error(format!(
                "unsupported dictionary method `{name}`; iterate keys and index values"
            )))
        }
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
        let ty = self.value(&args[0], ops)?;
        if name == "len" {
            if ty.element().is_none() {
                return Err(at.error("len requires an iterable"));
            }
            ops.push(Op::Collection(CollectionOp::Len(ty)));
            return Ok(Ty::I64);
        }
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
        let source = self.slot(source_ty.clone(), at)?;
        let index = self.slot(Ty::I64, at)?;
        let result = self.slot(target_ty.clone(), at)?;
        ops.extend([
            Op::StoreLocal(source),
            Op::PushInt(Value::I64(0)),
            Op::StoreLocal(index),
            Op::Collection(CollectionOp::New(target_ty.clone())),
            Op::StoreLocal(result),
        ]);
        let condition = vec![
            Op::LoadLocal(index),
            Op::LoadLocal(source),
            Op::Collection(CollectionOp::Len(source_ty.clone())),
            Op::Lt,
        ];
        let mut body = vec![
            Op::LoadLocal(result),
            Op::LoadLocal(source),
            Op::LoadLocal(index),
            Op::Collection(CollectionOp::IterGet(source_ty)),
            Op::Collection(CollectionOp::Insert(target_ty)),
            Op::StoreLocal(result),
        ];
        increment(index, &mut body);
        ops.push(Op::Loop {
            condition: condition.into(),
            body: body.into(),
        });
        ops.push(Op::LoadLocal(result));
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
