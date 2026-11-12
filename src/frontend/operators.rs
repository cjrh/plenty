//! Operator syntax resolves to ordinary direct calls, before built-in arithmetic.
//! A `@newtype` class without a selected hook forwards to its field's operator.
use super::*;
use crate::record::ClassType;

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

/// Ordering comparisons have no hooks; only `@newtype` forwarding gives a
/// class one.
fn ordering(op: &str) -> bool {
    matches!(op, "<" | "<=" | ">" | ">=")
}

fn unreferenced(ty: &Ty) -> &Ty {
    match ty {
        Ty::Ref(inner, _) => inner,
        other => other,
    }
}

/// Suggests `@newtype` for a one-field class that does not forward.
pub(super) fn newtype_hint(ty: &Ty) -> &'static str {
    match ty {
        Ty::Class(class)
            if class
                .try_get()
                .is_some_and(|c| c.fields.len() == 1 && !c.forwards_operators) =>
        {
            "; declare an operator hook, or annotate the one-field class `@newtype` to forward its field's operators"
        }
        _ => "",
    }
}

fn unary(op: &str) -> Option<&'static str> {
    match op {
        "+" => Some("__pos__"),
        "-" => Some("__neg__"),
        "~" => Some("__invert__"),
        _ => None,
    }
}

/// Hooks the compiler calls: lifecycle, context management and the operator
/// tables above.
fn known_hook(name: &str) -> bool {
    matches!(
        name,
        "__init__" | "__del__" | "__new__" | "__enter__" | "__exit__" | "__encode__" | "__decode__"
    ) || ["+", "-", "~"]
        .into_iter()
        .any(|op| unary(op) == Some(name))
        || ["+", "-", "*", "/", "//", "%", "&", "|", "^", "<<", ">>"]
            .into_iter()
            .filter_map(binary)
            .any(|(a, b, c)| [a, b, c].contains(&name))
}

/// Every other `__name__` method is rejected: Plenty would never call it, and
/// reserving the pattern keeps the names free for later hooks. Python's
/// comparison and hashing hooks get their own message; B37 (`__cmp__`), B74
/// (user-defined keys) replace this for the hooks they admit.
/// See book/src/design/language/04-expressions/operators.md#reserved-method-names.
pub(super) fn reject_unsupported_hook(f: &Function) -> Result<()> {
    let name = f.name.as_str();
    if name.len() <= 4 || !name.starts_with("__") || !name.ends_with("__") || known_hook(name) {
        return Ok(());
    }
    let comparison = match name {
        "__eq__" | "__ne__" => Some("`==` and `!=` always compare fields structurally"),
        "__lt__" | "__le__" | "__gt__" | "__ge__" | "__cmp__" => {
            Some("`<`, `<=`, `>` and `>=` on a user type come only from `@newtype` forwarding")
        }
        "__hash__" | "__key__" => Some("dictionary keys and set elements hash structurally"),
        _ => None,
    };
    if let Some(rule) = comparison {
        return Err(f.at.error(format!(
            "`{name}` is not supported: {rule} and never call methods; rename the method if it is not meant as a hook. See \"Equality, ordering and hashing\" in the operators reference"
        )));
    }
    let advice = match name {
        "__str__" | "__repr__" | "__format__" => {
            "`print`, `str.repr` and f-strings call `display` and `debug` methods; define those instead (see std.fmt)"
        }
        "__pow__" | "__rpow__" | "__ipow__" => {
            "there is no `**` operator; define a `pow` method and call `base.pow(exponent)`"
        }
        "__iter__" | "__next__" => {
            "`for` never calls user methods; write a generator method that yields the elements and iterate over its result"
        }
        "__len__" | "__contains__" => {
            "`len()` calls an ordinary shared `len` method returning i64; define `len` instead of `__len__`. `in` does not call user methods; call a named search method directly"
        }
        "__bool__" => {
            "conditions require a `bool`; define a predicate method such as `is_empty` and call it"
        }
        "__getitem__" | "__setitem__" | "__delitem__" => {
            "indexing never calls user methods; define named methods such as `get` and `set`"
        }
        _ => "Plenty never calls it",
    };
    Err(f.at.error(format!(
        "`{name}` is not supported: {advice}. Method names of the form `__name__` are reserved for the hooks Plenty calls; rename the method. See \"Reserved method names\" in the operators reference"
    )))
}

pub(super) fn validate(f: &Function, aliases: &TypeAliases) -> Result<()> {
    reject_unsupported_hook(f)?;
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
    /// Lowers a left-associated `and`/`or` chain such as `a or b or c` by
    /// walking its left spine in a loop. Recursing per operand through
    /// `expr`, `observe` and their callers used about 77 KiB of debug-build
    /// stack per operand, so 45 operands overflowed a 2 MiB thread (issue
    /// #841). Each node still runs the checks and emits the ops that lowering
    /// it on its own would: an inner chain is a `bool` value with no loans.
    pub(super) fn logical(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Type> {
        self.forget_condition(e);
        let mut spine = Vec::new();
        let mut node = e;
        let (mut a, mut loans) = loop {
            let Expression::Binary(op, left, right) = &node.kind else {
                unreachable!("logical chains are binary nodes")
            };
            if let Some(result) = self.overloaded_binary(op, left, right, &node.at, ops)? {
                if spine.is_empty() {
                    return Ok(result);
                }
                let ty = result.ok_or_else(|| node.at.error("expected a value, got ()"))?;
                break (ty, Vec::new());
            }
            spine.push(node);
            match &left.kind {
                Expression::Binary(inner, ..) if inner == "and" || inner == "or" => node = left,
                _ => break self.observe(left, ops)?,
            }
        };
        let mut result = None;
        for node in spine.into_iter().rev() {
            let Expression::Binary(op, left, right) = &node.kind else {
                unreachable!("logical chains are binary nodes")
            };
            let mut rhs = Vec::new();
            // The right operand runs only when `and`'s left one is true and
            // `or`'s is false, so it sees what that outcome proves.
            let saved = self.narrowed.clone();
            let facts = self.condition_facts(left, op == "and");
            self.narrow(&facts);
            let observed = self.observe_right(op, &a, right, &mut rhs);
            self.narrowed = saved;
            let (b, right_loans) = observed?;
            Self::end_reads(right_loans, &mut rhs);
            result = self.binary_values(
                node,
                (a, b),
                intervals::Operands::default(),
                rhs,
                std::mem::take(&mut loans),
                ops,
            )?;
            self.connectives.insert(intervals::node(&node.at));
            a = result
                .clone()
                .ok_or_else(|| node.at.error("expected a value, got ()"))?;
        }
        Ok(result)
    }

    /// Observes a binary operator's right operand, given the left's type.
    pub(super) fn observe_right(
        &mut self,
        op: &str,
        a: &Ty,
        right: &Expr,
        rhs: &mut Vec<Op>,
    ) -> Result<(Ty, Vec<usize>)> {
        if a.is_numeric() {
            self.observe_expected(right, Some(a.clone()), rhs)
        } else if matches!(a, Ty::KeyView(_))
            && matches!(op, "==" | "!=")
            && list_display(right).is_none()
        {
            self.observe_expected(right, Some(Ty::Set(Arc::new(a.element().unwrap()))), rhs)
        } else {
            self.observe(right, rhs)
        }
    }

    /// Checks and emits a non-overloaded binary operator over observed
    /// operands. It is separate from `expr` so the many locals here do not
    /// enlarge the frame of every recursive `expr` call.
    #[inline(never)]
    pub(super) fn binary_values(
        &mut self,
        e: &Expr,
        (a, b): (Ty, Ty),
        operands: intervals::Operands,
        rhs: Vec<Op>,
        loans: Vec<usize>,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let Expression::Binary(op, left, right) = &e.kind else {
            unreachable!("binary_values lowers binary nodes")
        };
        if matches!(op.as_str(), "==" | "!=")
            && (matches!(a, Ty::KeyView(_)) || matches!(b, Ty::KeyView(_)))
        {
            if !matches!(a, Ty::Set(_) | Ty::KeyView(_))
                || !matches!(b, Ty::Set(_) | Ty::KeyView(_))
                || a.element() != b.element()
            {
                return Err(e
                    .at
                    .error("KeyView equality requires a set or KeyView with the same key type"));
            }
            ops.extend(rhs);
            ops.push(Op::Collection(CollectionOp::key_set_relation(
                a,
                b,
                crate::collection_opcodes::KEY_VIEW_EQUAL,
            )));
            Self::end_reads(loans, ops);
            if op == "!=" {
                ops.push(Op::Not);
            }
            return Ok(Some(Ty::Bool));
        }
        if matches!(a, Ty::KeyView(_)) || matches!(b, Ty::KeyView(_)) {
            return Err(e.at.error(format!("KeyView does not support binary operator `{op}`; use ==/!= for set equality or the set relationship/algebra methods")));
        }
        if matches!(a, Ty::View(..)) || matches!(b, Ty::View(..)) {
            return self.view_equality(op, (a, b), rhs, loans, &e.at, ops);
        }
        if a.restricted_storage() {
            if matches!(a, Ty::Closure(_)) {
                return Err(e.at.error("closures do not support binary operators"));
            }
            return Err(e.at.error("generators do not support binary operators"));
        }
        if a.is_numeric() && b.is_numeric() && a != b {
            return Err(mixed_numeric(left, &a, right, &b));
        }
        if op == "*" {
            match (&a, &b) {
                (Ty::List(_), other) if other.is_int() => {
                    return Err(left.at.error(LIST_REPETITION));
                }
                (other, Ty::List(_)) if other.is_int() => {
                    return Err(right.at.error(LIST_REPETITION));
                }
                (Ty::Str, other) if other.is_int() => {
                    return Err(left.at.error(STR_REPETITION));
                }
                (other, Ty::Str) if other.is_int() => {
                    return Err(right.at.error(STR_REPETITION));
                }
                _ => {}
            }
        }
        self.same(Some(b.clone()), Some(a.clone()), &e.at)?;
        if matches!(op.as_str(), "==" | "!=") && a.facts().closure {
            return Err(e
                .at
                .error("values containing closures do not support equality"));
        }
        if matches!(op.as_str(), "==" | "!=") && a.facts().concurrent_handle {
            return Err(e
                .at
                .error("values containing channel endpoints, executors, or futures do not support equality"));
        }
        if matches!(op.as_str(), "==" | "!=") && a.facts().shared {
            return Err(e.at.error(shared::EQUALITY));
        }
        if matches!(op.as_str(), "==" | "!=") && a.facts().atomic {
            if mutex::holds_mutex(&a) {
                return Err(e.at.error(
                    "values containing mutexes do not support equality; compare values read inside `with mutex.lock() as value:`",
                ));
            }
            return Err(e.at.error(
                "values containing atomics do not support equality; compare values read with load()",
            ));
        }
        if matches!(op.as_str(), "==" | "!=") && a.recursive_data() {
            return Err(e.at.error(
                "automatic equality is not supported for recursive data; compare fields explicitly",
            ));
        }
        Ok(if op == "and" || op == "or" {
            self.same(Some(a.clone()), Some(Ty::Bool), &e.at)?;
            let constant = vec![Op::PushBool(op == "or")];
            let (yes, no) = if op == "and" {
                (rhs, constant)
            } else {
                (constant, rhs)
            };
            ops.push(branch(yes, no));
            Self::end_reads(loans, ops);
            Some(Ty::Bool)
        } else {
            if !a.is_numeric()
                && !(matches!(op.as_str(), "==" | "!=")
                    || (a == Ty::Str && op == "+")
                    || (a.is_orderable() && is_comparison(op)))
            {
                return Err(self.value_demand_error(
                    &e.at,
                    e,
                    format!(
                        "operator `{op}` does not accept {}{}",
                        type_name(Some(a.clone())),
                        operators::newtype_hint(&a)
                    ),
                    crate::ownership::Demand::Operator(op.clone(), a.clone()),
                    false,
                ));
            }
            if op == "/" && !a.is_float() {
                return Err(e
                    .at
                    .error("use `//` for integer division; `/` requires floats"));
            }
            if matches!(op.as_str(), "//" | "%") && a.is_float() {
                return Err(e
                    .at
                    .error("floor division and modulo currently require integers"));
            }
            ops.extend(rhs);
            if let Some(kind) = IntegerOp::binary(op) {
                if !a.is_int() {
                    return Err(e.at.error("bitwise operations require integers"));
                }
                let outcome = self.outcome(op, &a, operands);
                self.block_errors(op, &a, Some(right), &e.at)?;
                if matches!(kind, IntegerOp::ShiftLeft | IntegerOp::ShiftRight) {
                    self.shift_operation(kind, &a, right, outcome, &e.at, ops)?;
                } else {
                    ops.push(Op::Integer(kind));
                }
                self.note_outcome(operands.node, &a, outcome);
                Self::end_reads(loans, ops);
                return Ok(Some(a));
            }
            if op == "+" && a == Ty::Str {
                ops.push(Op::Collection(CollectionOp::TextTryConcat));
                Self::end_reads(loans, ops);
                return Ok(Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())));
            }
            self.mark_arithmetic(op, &a, &e.at, ops);
            let outcome = self.outcome(op, &a, operands);
            self.block_errors(op, &a, Some(right), &e.at)?;
            if self.proved_arithmetic(op, &a, outcome, ops) {
                self.note_outcome(operands.node, &a, outcome);
                Self::end_reads(loans, ops);
                return Ok(Some(a));
            }
            let proof = self.divisor_proof(op, &a, operands);
            if a.is_int()
                && let Some(operation) = crate::numeric::Checked::named(op, proof)
                && self.checked_arith(operation, &a, &e.at, ops)?
            {
                Self::end_reads(loans, ops);
                return Ok(Some(a));
            }
            if !is_comparison(op) {
                self.note_outcome(operands.node, &a, outcome);
            }
            let operation = match op.as_str() {
                "+" => Op::Add,
                "-" => Op::Sub,
                "*" => Op::Mul,
                "/" => Op::Div,
                "//" => Op::FloorDiv(proof),
                "%" => Op::Modulo(proof),
                "==" => Op::Eq,
                "!=" => Op::Ne,
                "<" => Op::Lt,
                ">" => Op::Gt,
                "<=" => Op::Le,
                ">=" => Op::Ge,
                _ => unreachable!(),
            };
            ops.push(operation);
            Self::end_reads(loans, ops);
            if is_comparison(op) {
                Some(Ty::Bool)
            } else {
                Some(a.clone())
            }
        })
    }

    // Hints avoid materializing operands solely for dispatch. An unknown result
    // is evaluated once and lent below; hints never decide whether code is legal.
    pub(super) fn operator_hint(&self, e: &Expr) -> Type {
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
            Expression::Bytes(_) => Some(Ty::ByteStr),
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
            // Always `bool` when legal; not recursing keeps hints for long
            // chains constant-time and shallow (issue #841).
            Expression::Binary(op, ..) if op == "and" || op == "or" => Some(Ty::Bool),
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
                if is_comparison(op) || matches!(op.as_str(), "in" | "not in") {
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

    pub(super) fn hold_operator_operand(
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

    pub(super) fn release_operator_operand(
        &mut self,
        e: &Expr,
        loan: Option<usize>,
        ops: &mut Vec<Op>,
    ) {
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
        // Slice views need their dedicated observation path to retain the
        // source loan, including when reporting unsupported ordering.
        if ordering(op) && (self.is_view(left) || self.is_view(right)) {
            return Ok(None);
        }
        if let Some(result) = self.quantity_binary(op, left, right, at, ops)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.domain_division(op, left, right, at, ops)? {
            return Ok(Some(result));
        }
        let hooks = binary(op);
        let ordering = ordering(op);
        if let Some(result) = self.nonzero_literal_operand(op, left, right, at, ops)? {
            return Ok(Some(result));
        }
        if hooks.is_none() && !ordering {
            return Ok(None);
        }
        let a = self.operator_hint(left);
        let b = self.operator_hint(right);
        let mut selected = None;
        if let Some((normal, reflected, _)) = hooks {
            selected = match &a {
                Some(ty) => self
                    .operator_symbol(ty, normal, at)?
                    .map(|name| (name, false)),
                None => None,
            };
            if selected.is_none() && (a.is_some() || !self.may_be_operator_data(left)) {
                selected = match &b {
                    Some(ty) => self
                        .operator_symbol(ty, reflected, at)?
                        .map(|name| (name, true)),
                    None => None,
                };
            }
        }
        if let Some((name, reverse)) = selected {
            return self
                .emit_operator_binary(name, reverse, left, right, at, ops)
                .map(Some);
        }
        // Forwarding is the fallback after hook selection, and only between
        // two operands of the same `@newtype` class: identity stays nominal.
        if let (Some(a), Some(b)) = (&a, &b)
            && unreferenced(a) == unreferenced(b)
            && let Some((class, declared)) = self.forwarding(a)
        {
            return self
                .forward_binary(op, &class, &declared, left, right, at, ops)
                .map(Some);
        }
        if let (Some(a), Some(b)) = (&a, &b)
            && let Some(class) = [a, b].into_iter().find(|ty| self.forwarding(ty).is_some())
        {
            let (a, b) = (unreferenced(a), unreferenced(b));
            return Err(at.error(format!(
                "`@newtype` class `{}` forwards `{op}` only between two `{}` values, not `{a}` and `{b}`; read the field, or declare an operator hook for these operands",
                unreferenced(class),
                unreferenced(class)
            )));
        }
        if ordering {
            // Compound quantities and inferred generic calls may hide both
            // types. Resolve the left once, then dispatch again. Holding both
            // here would discard the numeric context the actual left type
            // supplies to a conditional or unsuffixed literal on the right.
            if a.is_none() && b.is_none() {
                let (held, loan) = self.hold_operator_operand(left, ops)?;
                let expression = Expr {
                    at: at.clone(),
                    kind: Expression::Binary(
                        op.into(),
                        Box::new(held.clone()),
                        Box::new(right.clone()),
                    ),
                };
                let result = self.expr(&expression, ops)?;
                self.release_operator_operand(&held, loan, ops);
                return Ok(Some(result));
            }
            // Built-in comparisons keep their lowering. An operand of unknown
            // type beside a forwarding one is evaluated once and retried.
            let forwards = |ty: &Type| ty.as_ref().is_some_and(|ty| self.forwarding(ty).is_some());
            if !((a.is_none() && forwards(&b)) || (b.is_none() && forwards(&a))) {
                return Ok(None);
            }
            return self.retry_binary(op, left, right, at, ops).map(Some);
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
            return self.retry_binary(op, left, right, at, ops).map(Some);
        }
        Ok(None)
    }

    /// Evaluates both operands once, in source order, and lowers the operation
    /// again with their types known.
    pub(super) fn retry_binary(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (left, l) = self.hold_operator_operand(left, ops)?;
        let (right, r) = self.hold_operator_operand(right, ops)?;
        let expression = Expr {
            at: at.clone(),
            kind: Expression::Binary(op.into(), Box::new(left.clone()), Box::new(right.clone())),
        };
        let result = self.expr(&expression, ops)?;
        self.release_operator_operand(&right, r, ops);
        self.release_operator_operand(&left, l, ops);
        Ok(result)
    }

    /// The class and its `@newtype` annotation when `ty` forwards operators.
    fn forwarding(&self, ty: &Ty) -> Option<(crate::nominal::Nominal<ClassType>, Token)> {
        let Ty::Class(class) = unreferenced(ty) else {
            return None;
        };
        if !class.try_get()?.forwards_operators {
            return None;
        }
        Some((class.clone(), self.aliases.newtype_annotation(&class.name)?))
    }

    /// `operand.field`, written in the class's module so that a private field
    /// stays reachable for the forwarding the class declared.
    fn forwarded_field(
        class: &crate::nominal::Nominal<ClassType>,
        declared: &Token,
        operand: &Expr,
    ) -> Expr {
        Expr {
            at: declared.clone(),
            kind: Expression::Member(
                Box::new(operand.clone()),
                class.get().fields[0].0.clone(),
                Pos::default(),
            ),
        }
    }

    /// Lowers `left OP right` for two values of one `@newtype` class as the
    /// field type's own operation, through ordinary operator resolution, then
    /// rewraps a result of the field's type. Ordering comparisons keep their
    /// `bool`. See book/src/design/language/04-expressions/operators.md#newtype-forwarding.
    #[allow(clippy::too_many_arguments)]
    fn forward_binary(
        &mut self,
        op: &str,
        class: &crate::nominal::Nominal<ClassType>,
        declared: &Token,
        left: &Expr,
        right: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let domain = Ty::Class(class.clone())
            .integer_domain()
            .map(|(domain, _)| domain);
        let nonzero = domain.is_some();
        if nonzero && !nonzero::forwards(op) {
            return Err(at.error(nonzero::unsupported(op, &Ty::Class(class.clone()))));
        }
        let (held_left, l) = self.hold_operator_operand(left, ops)?;
        let (held_right, r) = self.hold_operator_operand(right, ops)?;
        let inner = Expr {
            at: at.clone(),
            kind: Expression::Binary(
                op.into(),
                Box::new(Self::forwarded_field(class, declared, &held_left)),
                Box::new(Self::forwarded_field(class, declared, &held_right)),
            ),
        };
        let result = self.expr(&inner, ops)?;
        self.release_operator_operand(&held_right, r, ops);
        self.release_operator_operand(&held_left, l, ops);
        // A sum or difference of nonzero values can be zero.
        if ordering(op)
            || (nonzero
                && op != "*"
                && (op != "+" || domain == Some(crate::op::IntegerDomain::NonZero)))
        {
            return Ok(result);
        }
        self.rewrap(class, declared, result, ops)
    }

    /// A forwarded result of the field's type becomes the class; so does the
    /// payload of `Result[field, E]`, giving `Result[Class, E]`. Any other
    /// result is the wrapped operation's own and is returned unchanged.
    pub(super) fn rewrap(
        &mut self,
        class: &crate::nominal::Nominal<ClassType>,
        declared: &Token,
        result: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let Some(ty) = result else {
            return Ok(None);
        };
        let field = class.get().fields[0].1.clone();
        let wrapped = Ty::Class(class.clone());
        let at = declared;
        if class.get().quantity.is_some() && ty == field {
            ops.push(Op::Class(crate::record::ClassOp::WrapQuantity(
                class.clone(),
            )));
            return Ok(Some(wrapped));
        }
        let error = match &ty {
            _ if ty == field => None,
            Ty::Enum(sum) if sum.is_result() && sum.get().variants[0].fields[0] == field => {
                Some(sum.get().variants[1].fields[0].clone())
            }
            _ => return Ok(Some(ty)),
        };
        let slot = self.slot(ty.clone(), at)?;
        ops.push(Op::StoreLocal(slot));
        if ty.managed() {
            self.expression_temps.push(slot);
        }
        let held = format!("$forwarded{slot}");
        self.bind(
            held.clone(),
            Local {
                slot,
                ty: ty.clone(),
                mutable: false,
            },
        );
        let expression = |kind| Expr {
            at: at.clone(),
            kind,
        };
        let value = expression(Expression::Name(held.clone()));
        let construct = |value: Expr| {
            expression(Expression::Constructor(
                Box::new(TypeRef {
                    concrete: Some(Box::new(wrapped.clone())),
                    metadata: None,
                    at: at.clone(),
                    name: None,
                    args: vec![],
                }),
                vec![value],
            ))
        };
        let out = match error {
            None => self.expr(&construct(value), ops),
            Some(error) => {
                let (payload, failure) = ("$forwarded_ok", "$forwarded_err");
                let case = |variant: &str, binding: &str, body: Expr| enums::Case {
                    at: at.clone(),
                    pattern: patterns::Pat {
                        at: at.clone(),
                        kind: patterns::PatKind::Variant(
                            None,
                            variant.into(),
                            Some(vec![patterns::Pat {
                                at: at.clone(),
                                kind: patterns::PatKind::Binding(binding.into()),
                            }]),
                            Pos::default(),
                        ),
                    },
                    body: vec![Stmt {
                        at: at.clone(),
                        kind: Statement::Expr(body),
                    }],
                };
                let cases = [
                    case(
                        "Ok",
                        payload,
                        expression(Expression::Call(
                            "Ok".into(),
                            vec![construct(expression(Expression::Name(payload.into())))],
                        )),
                    ),
                    case(
                        "Err",
                        failure,
                        expression(Expression::Call(
                            "Err".into(),
                            vec![expression(Expression::Name(failure.into()))],
                        )),
                    ),
                ];
                self.match_value(
                    at,
                    &value,
                    &cases,
                    Some(crate::sum::result(wrapped.clone(), error)),
                    ops,
                )
            }
        };
        self.names.remove(&held);
        out
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
        self.push_call(Op::Call(name), &sig.inputs, &loans, at, ops)?;
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
        at: &Token,
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
        if let Some(result) = self.quantity_unary(op, value, &ty, at, ops)? {
            return Ok(Some(result));
        }
        let Some(name) = self.operator_symbol(&ty, hook, &value.at)? else {
            let Some((class, declared)) = self.forwarding(&ty) else {
                return Ok(None);
            };
            if op == "~" && unreferenced(&ty).domain_element().is_some() {
                return Err(value.at.error(nonzero::unsupported(op, unreferenced(&ty))));
            }
            let (held, loan) = self.hold_operator_operand(value, ops)?;
            // A field read carries its declaration's token; report failures
            // at the user's operator instead.
            let inner = Expr {
                at: at.clone(),
                kind: Expression::Unary(
                    op.into(),
                    Box::new(Self::forwarded_field(&class, &declared, &held)),
                ),
            };
            let nonnegative = op == "-"
                && unreferenced(&ty)
                    .integer_domain()
                    .is_some_and(|(domain, _)| domain != crate::op::IntegerDomain::NonZero);
            // Negating a Positive or NonNegative value cannot overflow, so it
            // adds no OverflowError to the enclosing contract.
            let result = if nonnegative {
                self.checked.push(checked::Scope {
                    start: self.locals.len(),
                    function: false,
                    arithmetic: false,
                    failures: Vec::new(),
                    casts: false,
                });
                let result = self.expr(&inner, ops);
                self.checked.pop().expect("pushed above");
                result?
            } else {
                self.expr(&inner, ops)?
            };
            self.release_operator_operand(&held, loan, ops);
            if nonnegative {
                return Ok(Some(result));
            }
            return self.rewrap(&class, &declared, result, ops).map(Some);
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
        if self.quantity_augment(target, op, value, ops)? {
            return Ok(true);
        }
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
            if let Some((domain, _)) = unreferenced(&ty).integer_domain() {
                if op != "*" && !(op == "+" && domain != crate::op::IntegerDomain::NonZero) {
                    return Err(target
                        .at
                        .error(nonzero::unsupported(&format!("{op}="), unreferenced(&ty))));
                }
                // `*=` by a literal: the literal is checked as a `NonZero`,
                // then its checked constant multiplies the field as a plain
                // integer.
                if nonzero::literal(value)
                    && let Some((class, declared)) = self.forwarding(&ty)
                {
                    let value = self.domain_constant(value, unreferenced(&ty))?;
                    let target = Self::forwarded_field(&class, &declared, target);
                    self.augment(&target, op, &value, ops)?;
                    return Ok(true);
                }
            }
            // A `@newtype` updates its field with the field type's own
            // in-place operation, so the destination keeps its storage.
            if let Some((class, declared)) = self.forwarding(&ty)
                && self
                    .operator_hint(value)
                    .is_some_and(|other| unreferenced(&other) == unreferenced(&ty))
            {
                let target = Self::forwarded_field(&class, &declared, target);
                let value = Self::forwarded_field(&class, &declared, value);
                self.augment(&target, op, &value, ops)?;
                return Ok(true);
            }
            if self.forwarding(&ty).is_some() {
                return Err(target.at.error(format!(
                    "`@newtype` class `{}` forwards `{op}=` only with another `{}` value; read the field, or declare `{hook}`",
                    unreferenced(&ty),
                    unreferenced(&ty)
                )));
            }
            return Err(target.at.error(format!("operator `{op}=` requires `{hook}`; use explicit assignment for a value-producing or fallible operation")));
        }
        self.class_method(target, owner, hook, None, std::slice::from_ref(value), ops)?;
        Ok(true)
    }
}
