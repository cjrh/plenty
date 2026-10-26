//! Operator syntax resolves to ordinary direct calls, before built-in arithmetic.
use super::*;

fn binary(op: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match op {
        "+" => ("__add__", "__radd__", "__iadd__"),
        "-" => ("__sub__", "__rsub__", "__isub__"),
        "*" => ("__mul__", "__rmul__", "__imul__"),
        "/" => ("__truediv__", "__rtruediv__", "__itruediv__"),
        "//" => ("__floordiv__", "__rfloordiv__", "__ifloordiv__"),
        "%" => ("__mod__", "__rmod__", "__imod__"),
        "&" => ("__and__", "__rand__", "__iand__"),
        "|" => ("__or__", "__ror__", "__ior__"),
        "^" => ("__xor__", "__rxor__", "__ixor__"),
        "<<" => ("__lshift__", "__rlshift__", "__ilshift__"),
        ">>" => ("__rshift__", "__rrshift__", "__irshift__"),
        _ => return None,
    })
}

fn unary(op: &str) -> Option<&'static str> {
    match op {
        "+" => Some("__pos__"),
        "-" => Some("__neg__"),
        "~" => Some("__invert__"),
        _ => None,
    }
}

pub(super) fn validate(f: &Function, aliases: &TypeAliases) -> Result<()> {
    let unary = ["+", "-", "~"]
        .into_iter()
        .any(|op| unary(op) == Some(&f.name));
    let binary = ["+", "-", "*", "/", "//", "%", "&", "|", "^", "<<", ">>"]
        .into_iter()
        .filter_map(binary)
        .find(|(a, b, c)| [*a, *b, *c].contains(&f.name.as_str()));
    if !unary && binary.is_none() {
        return Ok(());
    }
    let inplace = binary.is_some_and(|(_, _, name)| name == f.name);
    if !f.type_params.is_empty()
        || f.defaults.iter().any(Option::is_some)
        || generators::yields(&f.body)
    {
        return Err(f
            .at
            .error("operator hooks cannot have method type parameters, defaults, or yield"));
    }
    if f.inputs.len() != if unary { 1 } else { 2 } {
        return Err(f
            .at
            .error("operator hook requires self and exactly its operator's operands"));
    }
    let receiver = f.inputs[0].1.resolve(aliases)?;
    if !matches!(receiver, Some(Ty::Ref(_, mutable)) if mutable == inplace) {
        return Err(f.at.error(if inplace {
            "in-place operator hook requires self: &mut Class (or Enum)"
        } else {
            "value-producing operator hook requires shared self"
        }));
    }
    if !unary {
        let other = f.inputs[1].1.resolve(aliases)?;
        if !matches!(&other, Some(Ty::Ref(_, false)))
            && other
                .as_ref()
                .is_none_or(|ty| ty.affine() || matches!(ty, Ty::Ref(..)))
        {
            return Err(f
                .at
                .error("operator operand must be a shared reference or a copyable value"));
        }
    }
    let output = f.output.resolve(aliases)?;
    if inplace {
        if output.is_some() {
            return Err(f.at.error("in-place operator hook must return (); use an explicit method call for fallible updates"));
        }
    } else if output.is_none() || matches!(output, Some(Ty::Ref(..))) {
        return Err(f.at.error("value-producing operator hook must return an owned value (possibly Result), not () or a reference"));
    }
    Ok(())
}

impl Lower<'_> {
    // Hints avoid materializing operands solely for dispatch. An unknown result
    // is evaluated once and lent below; hints never decide whether code is legal.
    fn operator_hint(&self, e: &Expr) -> Type {
        if let Some(ty) = self.place_type(e) {
            return Some(ty);
        }
        match &ungroup(e).kind {
            Expression::Call(name, _)
                if name == "len"
                    && !self.names.contains_key(name)
                    && !self.sigs.contains_key(name)
                    && !self.generics.templates.contains_key(name)
                    && !self.aliases.contains_key(name) =>
            {
                Some(Ty::I64)
            }
            Expression::Text(_) => Some(Ty::Str),
            Expression::Bool(_) => Some(Ty::Bool),
            Expression::Number(n) => self.number(n, false, &e.at, &mut Vec::new()).ok(),
            Expression::Unary(op, inner) if op == "&" || op == "&mut" => self.operator_hint(inner),
            Expression::Unary(op, inner)
                if op == "-" && matches!(ungroup(inner).kind, Expression::Number(_)) =>
            {
                let Expression::Number(n) = &ungroup(inner).kind else {
                    unreachable!()
                };
                self.number(n, true, &e.at, &mut Vec::new()).ok()
            }
            Expression::Constructor(t, _) => t.resolve(self.aliases).ok().flatten(),
            Expression::Try(inner) => match self.operator_hint(inner)? {
                Ty::Enum(t) if t.is_result() || t.is_option() => {
                    t.get().variants[0].fields.first().cloned()
                }
                _ => None,
            },
            Expression::Method(base, name, _, _) => {
                let ty = self.operator_hint(base)?;
                if let Some(owner) = classes::method_owner(&ty)
                    && let Some(sig) = self.sigs.get(&crate::record::method(owner, name))
                {
                    return sig.outputs.first().cloned();
                }
                self.expression_type_hint(e)
                    .or_else(|| self.numeric_builtin_hint(e))
            }
            Expression::Unary(op, inner) if unary(op).is_some() => {
                let ty = self.operator_hint(inner)?;
                if let Some(owner) = classes::method_owner(&ty) {
                    self.sigs
                        .get(&crate::record::method(owner, unary(op)?))?
                        .outputs
                        .first()
                        .cloned()
                } else {
                    Some(ty)
                }
            }
            Expression::Binary(op, left, right) => {
                let a = self.operator_hint(left)?;
                let b = self.operator_hint(right)?;
                if let Some((normal, reflected, _)) = binary(op) {
                    for (ty, hook) in [(&a, normal), (&b, reflected)] {
                        if let Some(owner) = classes::method_owner(ty)
                            && let Some(sig) = self.sigs.get(&crate::record::method(owner, hook))
                        {
                            return sig.outputs.first().cloned();
                        }
                    }
                }
                if is_comparison(op) || matches!(op.as_str(), "and" | "or" | "in" | "not in") {
                    Some(Ty::Bool)
                } else if a == Ty::Str && b == Ty::Str && op == "+" {
                    Some(crate::sum::result(Ty::Str, crate::sum::alloc_error()))
                } else if a.is_numeric() && b.is_numeric() {
                    Some(a)
                } else {
                    None
                }
            }
            _ => self
                .expression_type_hint(e)
                .or_else(|| self.numeric_builtin_hint(e))
                .map(|ty| match ty {
                    Ty::Ref(inner, _) => (*inner).clone(),
                    other => other,
                }),
        }
    }

    fn operator_symbol(&mut self, ty: &Ty, hook: &str, at: &Token) -> Result<Option<String>> {
        let Some(owner) = classes::method_owner(ty) else {
            return Ok(None);
        };
        self.sync_data()?;
        let callee = crate::record::method(owner, hook);
        if !self.sigs.contains_key(&callee) {
            return Ok(None);
        }
        modules::check_member(self.access, owner, hook, at)?;
        Ok(Some(callee))
    }

    pub(super) fn operator_output_hint(&self, op: &str, left: &Expr, right: &Expr) -> Option<Type> {
        let (normal, reflected, _) = binary(op)?;
        for (expression, hook) in [(left, normal), (right, reflected)] {
            if let Some(ty) = self.operator_hint(expression)
                && let Some(owner) = classes::method_owner(&ty)
                && let Some(sig) = self.sigs.get(&crate::record::method(owner, hook))
            {
                return Some(sig.outputs.first().cloned());
            }
        }
        None
    }

    fn hold_operator_operand(
        &mut self,
        e: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<(Expr, Option<usize>)> {
        let (mut ty, mut loan) = if self.place_type(e).is_some_and(|ty| ty.affine()) {
            let (ty, loan) = self.shared_argument(e, None, ops)?;
            (ty, Some(loan))
        } else {
            let ty = self.value(e, ops)?;
            let loan = if matches!(ty, Ty::Ref(..)) {
                Some(self.reference_origin(e, ops)?)
            } else {
                None
            };
            (ty, loan)
        };
        if let Ty::Ref(inner, _) = &ty
            && !inner.affine()
        {
            let inner = (**inner).clone();
            ops.push(Op::ReadRef(inner.clone()));
            if let Some(loan) = loan.take() {
                ops.push(Op::UseLoan(loan));
            }
            ty = inner;
        }
        // Computed scalar values need no borrow or addressable temporary.
        // A computed owner is lent until the containing expression completes.
        if !matches!(ty, Ty::Ref(..)) && ty.affine() {
            let owner = self.slot(ty.clone(), &e.at)?;
            ops.push(Op::StoreLocal(owner));
            self.expression_temps.push(owner);
            loan = Some(self.new_loan(owner, false, None, ops));
            ops.push(Op::BorrowLocal(owner, false));
            ty = Ty::Ref(Arc::new(ty), false);
        }
        let slot = self.slot(ty.clone(), &e.at)?;
        ops.push(Op::StoreLocal(slot));
        if let Some(loan) = loan {
            self.bind_reference(slot, loan, false, ops);
        } else if ty.managed() {
            self.expression_temps.push(slot);
        }
        let name = format!("$operator{slot}");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        Ok((
            Expr {
                at: e.at.clone(),
                kind: Expression::Name(name),
            },
            loan,
        ))
    }

    fn release_operator_operand(&mut self, e: &Expr, loan: Option<usize>, ops: &mut Vec<Op>) {
        if let Expression::Name(name) = &e.kind {
            self.names.remove(name);
        }
        if let Some(loan) = loan {
            ops.push(Op::UseLoan(loan));
        }
    }

    pub(super) fn overloaded_binary(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        let Some((normal, reflected, _)) = binary(op) else {
            return Ok(None);
        };
        let a = self.operator_hint(left);
        let b = self.operator_hint(right);
        let selected = match &a {
            Some(ty) => self
                .operator_symbol(ty, normal, at)?
                .map(|name| (name, false)),
            None => None,
        };
        let selected = if selected.is_none() && (a.is_some() || !self.may_be_operator_data(left)) {
            match &b {
                Some(ty) => self
                    .operator_symbol(ty, reflected, at)?
                    .map(|name| (name, true)),
                None => None,
            }
        } else {
            selected
        };
        if let Some((name, reverse)) = selected {
            return self
                .emit_operator_binary(name, reverse, left, right, at, ops)
                .map(Some);
        }
        // Unknown expressions include inferred generic calls and compound
        // results. Save both operands in source order before retrying dispatch.
        // Known built-ins keep their existing numeric inference and fast path.
        if a.is_none() || b.is_none() {
            // Text/collection displays and built-in arithmetic should keep their
            // context-directed lowering unless an operand can be user data.
            if (a.is_some() || !self.may_be_operator_data(left))
                && (b.is_some() || !self.may_be_operator_data(right))
            {
                return Ok(None);
            }
            // Literals have no evaluation effects and still need the selected
            // hook's parameter context (for example `1 + make_i32_vector()`).
            let literal = |e: &Expr| {
                matches!(
                    ungroup(e).kind,
                    Expression::Number(_) | Expression::Text(_) | Expression::Bool(_)
                ) || matches!(&ungroup(e).kind, Expression::Unary(op, inner)
                    if matches!(op.as_str(), "+" | "-") && matches!(ungroup(inner).kind, Expression::Number(_)))
            };
            let left_literal = literal(left);
            let right_literal = literal(right);
            if left_literal || right_literal {
                let (held, loan) =
                    self.hold_operator_operand(if left_literal { right } else { left }, ops)?;
                let (a, b) = if left_literal {
                    (left.clone(), held.clone())
                } else {
                    (held.clone(), right.clone())
                };
                let expression = Expr {
                    at: at.clone(),
                    kind: Expression::Binary(op.into(), Box::new(a), Box::new(b)),
                };
                let result = self.expr(&expression, ops)?;
                self.release_operator_operand(&held, loan, ops);
                return Ok(Some(result));
            }
            let (left, l) = self.hold_operator_operand(left, ops)?;
            let (right, r) = self.hold_operator_operand(right, ops)?;
            let expression = Expr {
                at: at.clone(),
                kind: Expression::Binary(
                    op.into(),
                    Box::new(left.clone()),
                    Box::new(right.clone()),
                ),
            };
            let result = self.expr(&expression, ops)?;
            self.release_operator_operand(&right, r, ops);
            self.release_operator_operand(&left, l, ops);
            return Ok(Some(result));
        }
        Ok(None)
    }

    fn emit_operator_binary(
        &mut self,
        name: String,
        reverse: bool,
        left: &Expr,
        right: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let sig = self.sigs[&name].clone();
        let start = ops.len();
        let inputs = if reverse {
            vec![sig.inputs[1].clone(), sig.inputs[0].clone()]
        } else {
            sig.inputs.clone()
        };
        let loans = self.call_arguments(&[left.clone(), right.clone()], &inputs, ops)?;
        if reverse {
            ops.push(Op::Swap);
        }
        self.record_direct_call(&name, at, &loans, ops);
        self.passes(start, &sig.inputs, sig.outputs.first(), ops);
        self.push_call(Op::Call(name), &sig.inputs, &loans, ops);
        Self::end_reads(loans, ops);
        Ok(sig.outputs.first().cloned())
    }

    fn may_be_operator_data(&self, e: &Expr) -> bool {
        if list_display(e).is_some() {
            return false;
        }
        if matches!(
            ungroup(e).kind,
            Expression::Number(_) | Expression::Text(_) | Expression::Bool(_)
        ) || matches!(&ungroup(e).kind, Expression::Unary(op, inner)
                if matches!(op.as_str(), "+" | "-") && matches!(ungroup(inner).kind, Expression::Number(_)))
        {
            return false;
        }
        if let Some(ty) = self.operator_hint(e) {
            return classes::method_owner(&ty).is_some();
        }
        !matches!(
            &ungroup(e).kind,
            Expression::Collection { .. } | Expression::Tuple(_) | Expression::Format(_)
        )
    }

    pub(super) fn overloaded_unary(
        &mut self,
        op: &str,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        let Some(hook) = unary(op) else {
            return Ok(None);
        };
        // Signed literals have a dedicated path, including i64.MIN.
        if matches!(ungroup(value).kind, Expression::Number(_)) {
            return Ok(None);
        }
        let Some(ty) = self.operator_hint(value) else {
            if !self.may_be_operator_data(value) {
                return Ok(None);
            }
            let (held, loan) = self.hold_operator_operand(value, ops)?;
            let expr = Expr {
                at: value.at.clone(),
                kind: Expression::Unary(op.into(), Box::new(held.clone())),
            };
            let result = self.expr(&expr, ops)?;
            self.release_operator_operand(&held, loan, ops);
            return Ok(Some(result));
        };
        let Some(name) = self.operator_symbol(&ty, hook, &value.at)? else {
            return Ok(None);
        };
        self.call_named(&name, std::slice::from_ref(value), &value.at, ops)
            .map(Some)
    }

    pub(super) fn overloaded_augment(
        &mut self,
        target: &Expr,
        op: &str,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<bool> {
        let Some((_, _, hook)) = binary(op) else {
            return Ok(false);
        };
        let Some(ty) = self.place_type(target) else {
            return Ok(false);
        };
        let Some(owner) = classes::method_owner(&ty) else {
            return Ok(false);
        };
        if self.operator_symbol(&ty, hook, &target.at)?.is_none() {
            return Err(target.at.error(format!("operator `{op}=` requires `{hook}`; use explicit assignment for a value-producing or fallible operation")));
        }
        self.class_method(target, owner, hook, None, std::slice::from_ref(value), ops)?;
        Ok(true)
    }
}
