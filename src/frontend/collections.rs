use super::*;
use crate::collection::Query;
use crate::collection_opcodes as opcode;

const KEY_VIEW_COLLECTION: &str = "collection constructors do not accept borrowed KeyView values; use view.to_set(), dictionary.keys_snapshot(), or [*key for key in view]";

/// Per-loop metadata for immutable shared list references already in scope.
/// Owned lists, exclusive references and retargetable bindings deliberately
/// keep ordinary indexed access: their storage can change inside the loop.
pub(super) struct IndexedLists {
    before: usize,
    views: HashMap<u8, [u8; 3]>,
    setup: Vec<Op>,
}

/// Only literal i64 progressions: evaluating or skipping these elements has no
/// effects, traps, borrows, or destructors. Reuse counted loops without building
/// a collection or a stack array. Other displays retain eager materialization.
fn literal_progression(iterable: &Expr) -> Option<(i64, i64, i64)> {
    let literal = match &ungroup(iterable).kind {
        Expression::Try(inner) => ungroup(inner),
        Expression::Method(inner, name, args, _) if name == "unwrap" && args.is_empty() => {
            ungroup(inner)
        }
        _ => return None,
    };
    let Expression::Collection {
        kind,
        entries,
        clauses,
    } = &literal.kind
    else {
        return None;
    };
    if kind != "list" || !clauses.is_empty() || entries.is_empty() {
        return None;
    }
    let mut first = 0;
    let mut previous = 0;
    let mut stride = 0;
    for (index, (entry, value)) in entries.iter().enumerate() {
        if value.is_some() {
            return None;
        }
        let (entry, negative) = match &ungroup(entry).kind {
            Expression::Unary(op, inner) if op == "-" => (ungroup(inner), true),
            _ => (ungroup(entry), false),
        };
        let Expression::Number(text) = &entry.kind else {
            return None;
        };
        let Value::I64(number) = integer(text, negative, &entry.at).ok()? else {
            return None;
        };
        if index == 0 {
            first = number;
        } else {
            let difference = number.checked_sub(previous)?;
            if index == 1 {
                stride = difference;
            } else if difference != stride {
                return None;
            }
        }
        previous = number;
    }
    Some((first, stride, i64::try_from(entries.len()).ok()?))
}

#[derive(Clone)]
pub(super) struct Iteration {
    pub(super) exact_len: Option<Vec<Op>>,
    pub(super) cleanup: Vec<Op>,
    pub(super) condition: Vec<Op>,
    // Initialize pending generator results when a paired condition is skipped.
    // This keeps conditional advancement compatible with definite ownership.
    pub(super) absent: Vec<Op>,
    pub(super) body: Vec<Op>,
    pub(super) step: Vec<Op>,
    pub(super) target: u8,
}

impl Parser {
    pub(super) fn arguments(&mut self) -> Result<Vec<Expr>> {
        self.exact_list(|parser| &mut parser.scratch_exprs, Self::arguments_into)
    }

    /// Pushes the arguments up to the closing `)` onto `scratch_exprs`.
    fn arguments_into(&mut self) -> Result<()> {
        let base = self.scratch_exprs.len();
        let mut named = false;
        while !self.eat(")") {
            // `=` is not an expression operator, so `name =` here can only be
            // a keyword argument.
            let label = match (&self.peek().kind, self.token(self.pos + 1)) {
                (Kind::Word(name), Some(next)) if next.is("=") => Some(name.to_string()),
                _ => None,
            };
            let at = self.peek().clone();
            if label.is_some() {
                named = true;
                self.take();
                self.take();
            } else if named {
                return Err(at.error("positional arguments must precede named arguments"));
            }
            let mut argument = if label.is_some() {
                self.nested(|parser| parser.expr(0))?
            } else {
                self.expr(0)?
            };
            if self.peek().is("for") {
                if self.scratch_exprs.len() > base || label.is_some() {
                    return Err(argument.at.error("a generator expression must be parenthesized when there are other arguments"));
                }
                argument.kind = self.generator_display(argument.clone())?;
                self.expect(")")?;
                self.scratch_exprs.push(argument);
                break;
            }
            if let Some(label) = label {
                argument = Expr {
                    at,
                    kind: Expression::NamedArgument(label, Box::new(argument)),
                };
            }
            self.scratch_exprs.push(argument);
            if self.eat(")") {
                break;
            }
            self.expect(",")?;
        }
        Ok(())
    }

    pub(super) fn generator_display(&mut self, element: Expr) -> Result<Expression> {
        Ok(Expression::Collection {
            kind: "generator".into(),
            entries: vec![(element, None)],
            clauses: self.comprehension_clauses()?,
        })
    }

    fn comprehension_clauses(&mut self) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();
        let mut element = self.deepest;
        loop {
            let keyword = self.pos;
            if self.eat("for") {
                let mut names = vec![self.name()?];
                while self.eat(",") {
                    names.push(self.name()?);
                }
                self.expect("in")?;
                let iterable = self.clause(keyword, clauses.len(), &mut element)?;
                clauses.push(Clause::For(names, iterable));
            } else if self.eat("if") {
                let condition = self.clause(keyword, clauses.len(), &mut element)?;
                clauses.push(Clause::If(condition));
            } else {
                break;
            }
        }
        self.deepest = self.deepest.max(element);
        Ok(clauses)
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
                clauses = self.comprehension_clauses()?;
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
        if let Expression::Unary(star, call) = &ungroup(target).kind
            && star == "*"
            && references::reference_call(call)
        {
            return self.augment_returned_reference(target, call, op, value, ops);
        }
        if self.overloaded_augment(target, op, value, ops)? {
            return Ok(());
        }
        if self.dict_index_target(target) {
            return Err(target.at.error(references::dict_index_augment(op)));
        }
        if self.clist_projection(target)?.is_some() {
            return self.augment_clist_field(target, op, value, ops);
        }
        if let Expression::Index(base, index) = &ungroup(target).kind
            && let Some(class) = self.byte_place(base)
        {
            return self.augment_byte(&class, (base, index), op, value, &target.at, ops);
        }
        let ty = self.place_type(target).ok_or_else(|| {
            target.at.error("augmented assignment requires an existing writable binding, field, or collection element")
        })?;
        if !ty.is_numeric() {
            return Err(target
                .at
                .error("augmented assignment currently requires a numeric destination"));
        }
        if let Some(slot) = self.owned_local(target)? {
            return self.augment_local(target, slot, ty, op, value, ops);
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
        match scalar_element_ref(ops) {
            Some(list) => {
                ops.push(Op::Collection(CollectionOp::ListScalarGetRef(list)));
            }
            _ => {
                ops.push(Op::ReadRef(ty.clone()));
            }
        }
        // Keep the shared loan through RHS evaluation: it permits observation
        // but prevents replacing or resizing the owner behind the saved path.
        let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
        self.same(actual, Some(ty.clone()), &value.at)?;
        self.mark_arithmetic(op, &ty, &target.at, ops);
        self.numeric_operation(op, ty.clone(), &target.at, ops)?;
        let result = self.slot(ty.clone(), &target.at)?;
        ops.push(Op::StoreLocal(result));
        ops.push(Op::UseLoan(read));
        let (_, write) = self.borrow_with_indices(target, true, &mut Some(indices.iter()), ops)?;
        match scalar_element_ref(ops) {
            Some(list) => {
                ops.extend([
                    Op::MoveLocal(result),
                    Op::Collection(CollectionOp::ListScalarSetRef(list)),
                    Op::Drop,
                ]);
            }
            _ => {
                ops.push(Op::MoveLocal(result));
                ops.push(Op::Swap);
                ops.push(Op::WriteRef(ty));
            }
        }
        ops.push(Op::UseLoan(write));
        Ok(())
    }

    /// `*f(...) op= value` calls `f` once and updates its returned referent
    /// (issue #598). The call's loan stays live through the value, as for
    /// `r = f(...); *r op= value`.
    fn augment_returned_reference(
        &mut self,
        target: &Expr,
        call: &Expr,
        op: &str,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let (ty, loan) = self.returned_reference(target, call, ops)?;
        if !ty.is_numeric() {
            return Err(target
                .at
                .error("augmented assignment currently requires a numeric destination"));
        }
        ops.push(Op::Dup);
        ops.push(Op::ReadRef(ty.clone()));
        let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
        self.same(actual, Some(ty.clone()), &value.at)?;
        self.mark_arithmetic(op, &ty, &target.at, ops);
        self.numeric_operation(op, ty.clone(), &target.at, ops)?;
        ops.push(Op::Access(self.loans[loan].root, true, Some(loan)));
        ops.push(Op::Swap);
        ops.push(Op::WriteRef(ty));
        ops.push(Op::UseLoan(loan));
        Ok(())
    }

    /// The slot of a target naming a local that holds its value directly, as
    /// opposed to a reference binding or closure capture, which update the
    /// referent through a borrow.
    fn owned_local(&self, target: &Expr) -> Result<Option<u8>> {
        let Expression::Name(name) = &ungroup(target).kind else {
            return Ok(None);
        };
        if self.captures.contains(name) {
            return Ok(None);
        }
        let Some(local) = self.names.get(name) else {
            return Ok(None);
        };
        if matches!(local.ty, Ty::Ref(..)) {
            return Ok(None);
        }
        if !local.mutable {
            return Err(target.at.error(format!(
                "immutable binding: `{name}` is immutable; declare it with `mut`"
            )));
        }
        Ok(Some(local.slot))
    }

    /// `name op= value` on a numeric local reads and writes the local directly.
    /// Taking its address instead would keep the local in stack memory (issue
    /// #361). A shared loan without an address still covers the RHS, so the
    /// RHS cannot modify or exclusively borrow the destination.
    fn augment_local(
        &mut self,
        target: &Expr,
        slot: u8,
        ty: Ty,
        op: &str,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        self.mark(&target.at, ops);
        let read = self.new_loan(slot, false, None, ops);
        let current = self.expr(target, ops)?;
        self.same(current, Some(ty.clone()), &target.at)?;
        let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
        self.same(actual, Some(ty.clone()), &value.at)?;
        self.mark_arithmetic(op, &ty, &target.at, ops);
        self.numeric_operation(op, ty, &target.at, ops)?;
        ops.push(Op::UseLoan(read));
        self.mark(&target.at, ops);
        ops.push(Op::StoreLocal(slot));
        Ok(())
    }

    pub(super) fn unpack_tuple(
        &mut self,
        names: &[String],
        mutable: bool,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        if let Some((base, exclusive)) = self.borrowed_tuple(value) {
            return self.unpack_borrowed_tuple(names, mutable, base, exclusive, &value.at, ops);
        }
        Self::check_unpacked_arity(names.len(), value)?;
        // Reassigned bindings already have types that literals should take.
        let expected = names
            .iter()
            .map(|name| self.names.get(name).map(|local| local.ty.clone()))
            .collect::<Option<Vec<_>>>()
            .map(crate::sum::tuple);
        let expected = self.unpack_hint(expected, value);
        let ty = self
            .expr_expected(value, expected, ops)?
            .ok_or_else(|| value.at.error("expected a value, got ()"))?;
        let ty = self.unpack_list(ty, names.len(), &value.at, ops)?;
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
        let mut seen = HashSet::default();
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
    /// The tuple that unpacking a reference borrows: the place in `&t` or
    /// `&mut t`, or a `&T` or `&mut T` binding of a tuple, and whether the
    /// borrow is exclusive.
    fn borrowed_tuple<'e>(&self, value: &'e Expr) -> Option<(&'e Expr, bool)> {
        let (base, exclusive) = match &ungroup(value).kind {
            Expression::Unary(op, inner) if op == "&" || op == "&mut" => (&**inner, op == "&mut"),
            Expression::Name(n) => match self.names.get(n)?.ty {
                Ty::Ref(_, exclusive) => (value, exclusive),
                _ => return None,
            },
            _ => return None,
        };
        matches!(self.place_type(base)?, Ty::Enum(t) if t.tuple()).then_some((base, exclusive))
    }

    /// `a, b = r` for a reference `r` to a tuple binds `a = &r[0]` and
    /// `b = &r[1]`, or `&mut` ones for an exclusive `r`: each name borrows
    /// its element in place, as `for a, b in &pairs` needs.
    ///
    /// A precise loan of `r` gives each name a loan of its own field. An
    /// imprecise one, such as a loop element's, covers the whole owner, so
    /// sibling field loans derived from it would conflict. The names then
    /// share one reborrow of `r`: their fields are disjoint, none of them can
    /// be retargeted, and the reborrow still conflicts with uses of `r`.
    fn unpack_borrowed_tuple(
        &mut self,
        names: &[String],
        mutable: bool,
        base: &Expr,
        exclusive: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let Some(Ty::Enum(tuple)) = self.place_type(base) else {
            unreachable!("checked by borrowed_tuple")
        };
        if mutable {
            return Err(at.error(
                "unpacking a tuple reference binds references that cannot be reassigned; remove `mut`",
            ));
        }
        let fields = tuple.get().variants[0].fields.clone();
        if fields.len() != names.len() {
            return Err(at.error("unpacking arity does not match tuple"));
        }
        let imprecise = match &ungroup(base).kind {
            Expression::Name(name) => self
                .reference_locals
                .get(&self.names[name].slot)
                .is_some_and(|&parent| !self.loans[parent].precise),
            _ => false,
        };
        let shared = if imprecise {
            let (reference, loan) = self.borrow(base, exclusive, ops)?;
            let source = self.slot(reference, at)?;
            ops.push(Op::StoreLocal(source));
            self.reference_locals.insert(source, loan);
            Some((source, loan))
        } else {
            None
        };
        let borrow = if exclusive { "&mut" } else { "&" };
        let node = |kind| Expr {
            at: at.clone(),
            kind,
        };
        let mut seen = HashSet::default();
        for (position, (name, field)) in names.iter().zip(&fields).enumerate() {
            if name == "_" {
                continue;
            }
            if !seen.insert(name) {
                return Err(at.error("duplicate unpacking binding"));
            }
            if *field == Ty::Unit {
                return Err(at.error("unit bindings are not supported yet; use _"));
            }
            if self.names.contains_key(name) {
                return Err(at.error(format!(
                    "`{name}` is already bound; unpacking a tuple reference binds new names"
                )));
            }
            let ty = Ty::Ref(Arc::new(field.clone()), exclusive);
            let loan = if let Some((source, loan)) = shared {
                ops.extend([
                    Op::LoadLocal(source),
                    Op::Enum(crate::sum::EnumOp::FieldRef(
                        tuple.clone(),
                        0,
                        position,
                        exclusive,
                    )),
                ]);
                loan
            } else {
                let element = node(Expression::Unary(
                    borrow.into(),
                    Box::new(node(Expression::Index(
                        Box::new(base.clone()),
                        Box::new(node(Expression::Number(position.to_string()))),
                    ))),
                ));
                let (_, origin) = self.binding_value(&element, None, ops)?;
                let origin = origin.expect("a borrow has a loan");
                self.check_stored_reference(origin, at)?;
                origin
            };
            let slot = self.slot(ty.clone(), at)?;
            self.bind(
                name.clone(),
                Local {
                    slot,
                    ty,
                    mutable: false,
                },
            );
            if shared.is_some() {
                self.reference_locals.insert(slot, loan);
            } else {
                self.bind_reference(slot, loan, false, ops);
            }
            ops.push(Op::StoreLocal(slot));
        }
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

    fn unpack_list(&mut self, ty: Ty, count: usize, at: &Token, ops: &mut Vec<Op>) -> Result<Ty> {
        let Ty::List(element) = &ty else {
            return Ok(ty);
        };
        let element = (**element).clone();
        let source = self.slot(ty.clone(), at)?;
        self.mark(at, ops);
        ops.push(Op::PushInt(Value::I64(count as i64)));
        ops.push(Op::Collection(CollectionOp::ListUnpack(ty.clone())));
        ops.push(Op::StoreLocal(source));
        for index in 0..count {
            ops.push(Op::LoadLocal(source));
            ops.push(Op::PushInt(Value::I64(index as i64)));
            ops.push(Op::Collection(if element.affine() {
                CollectionOp::IterTake(ty.clone())
            } else {
                CollectionOp::IterGet(ty.clone())
            }));
        }
        let Ty::Enum(tuple) = crate::sum::tuple(vec![element; count]) else {
            unreachable!()
        };
        ops.push(Op::Enum(crate::sum::EnumOp::New(tuple.clone(), 0)));
        ops.push(Op::DropLocal(source));
        Ok(Ty::Enum(tuple))
    }

    fn unpack_hint(&self, expected: Type, value: &Expr) -> Type {
        if list_display(value).is_some() || matches!(self.place_type(value), Some(Ty::List(_))) {
            let Some(Ty::Enum(tuple)) = expected else {
                return None;
            };
            let fields = &tuple.get().variants[0].fields;
            let first = fields.first()?;
            fields
                .iter()
                .all(|field| field == first)
                .then(|| Ty::List(Arc::new(first.clone())))
        } else {
            expected
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
        let mut seen = HashSet::default();
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
                _ if self.dict_index_target(target) => {
                    return Err(target.at.error(references::DICT_INDEX_ASSIGNMENT));
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
        let expected = self.unpack_hint(expected, value);
        let ty = self
            .expr_expected(value, expected, ops)?
            .ok_or_else(|| value.at.error("expected a value, got ()"))?;
        let ty = self.unpack_list(ty, targets.len(), &value.at, ops)?;
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
            self.block_inner(std::slice::from_ref(&assignment), ops, &Tail::Discard)?;
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
            // `expr_expected` pushes the unit itself when it has an expectation.
            let actual = match self.expr_expected(value, expected.clone(), ops)? {
                Some(actual) => actual,
                None => {
                    ops.push(Op::PushUnit);
                    Ty::Unit
                }
            };
            if let Some(expected) = expected {
                self.same(Some(actual.clone()), Some(expected), &value.at)?;
            }
            if let Some(barrier) = actual.storage_barrier() {
                return Err(value.at.error(barrier.message("tuples")));
            }
            types.push(actual);
        }
        if Ty::identity_names_exceed_limit(&types) || types.iter().any(|t| t.layout_depth() >= 64) {
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
        let mut hint = expected.clone();
        if let Some(Ty::Enum(sum)) = &expected
            && sum.anonymous()
        {
            let candidates: Option<Vec<Ty>> = match &ungroup(e).kind {
                    Expression::Call(name, _) | Expression::Name(name)
                        if enums::prelude_variant(name) && !self.names.contains_key(name) =>
                    {
                        Some(sum.get().variants.iter().map(|v| &v.fields[0])
                            .filter(|ty| enums::prelude_owner(name, ty)).cloned().collect())
                    }
                    Expression::Tuple(values) => Some(sum.get().variants.iter().map(|v| &v.fields[0])
                        .filter(|ty| matches!(ty, Ty::Enum(t) if t.tuple() && t.get().variants[0].fields.len() == values.len()))
                        .cloned().collect()),
                    Expression::Collection { kind, .. } if kind != "generator" => Some(sum.get().variants.iter().map(|v| &v.fields[0])
                        .filter(|ty| matches!(ty, Ty::Enum(t) if t.is_result() && kind_matches(kind, &t.get().variants[0].fields[0])))
                        .cloned().collect()),
                    // `[..]?` and `list()?` produce the collection itself, so
                    // select the collection member before checking elements.
                    Expression::Try(inner) => match &ungroup(inner).kind {
                        Expression::Collection { kind, .. } if kind != "generator" => Some(sum.get().variants.iter().map(|v| &v.fields[0])
                            .filter(|ty| kind_matches(kind, ty)).cloned().collect()),
                        Expression::Call(name, args)
                            if matches!(name.as_str(), "list" | "dict" | "set")
                                && args.is_empty()
                                && !self.names.contains_key(name) =>
                        {
                            Some(sum.get().variants.iter().map(|v| &v.fields[0])
                                .filter(|ty| kind_matches(name, ty)).cloned().collect())
                        }
                        _ => None,
                    }
                    .filter(|candidates: &Vec<Ty>| !candidates.is_empty()),
                    _ => None,
                };
            if let Some(mut candidates) = candidates {
                if candidates.len() > 1 {
                    return Err(e.at.error("ambiguous anonymous sum constructor context; construct a member with an explicit type first"));
                }
                hint = candidates.pop();
            }
        }
        let ty = self.expr_hinted(e, hint, ops)?;
        Ok(match (ty, expected) {
            (None, Some(expected)) if crate::sum::accepts(&expected, &Ty::Unit) => {
                ops.push(Op::PushUnit);
                if expected != Ty::Unit {
                    ops.push(Op::Enum(crate::sum::EnumOp::Widen(
                        Box::new(Ty::Unit),
                        Box::new(expected.clone()),
                    )));
                }
                Some(expected)
            }
            (Some(ty), Some(expected)) => {
                let ty = self.unbox_to(ty, &expected, ops);
                // A closure signature among the members takes this producer,
                // as a returned closure resolves a declared one (issue #336).
                let expected = match crate::sum::infer_producers(&expected, &ty) {
                    Some(refined) if ty != expected && !crate::sum::accepts(&expected, &ty) => {
                        refined
                    }
                    _ => expected,
                };
                if ty != expected && crate::sum::accepts(&expected, &ty) {
                    ops.push(Op::Enum(crate::sum::EnumOp::Widen(
                        Box::new(ty),
                        Box::new(expected.clone()),
                    )));
                    Some(expected)
                } else {
                    Some(ty)
                }
            }
            (ty, _) => ty,
        })
    }

    fn expr_hinted(&mut self, e: &Expr, expected: Type, ops: &mut Vec<Op>) -> Result<Type> {
        match &e.kind {
            Expression::Method(base, name, args, _)
                if matches!(name.as_str(), "collect" | "try_collect")
                    && (self.is_pipeline(base) || self.is_generator_source(base)) =>
            {
                self.consume_in_place(|this| this.pipeline_collect(base, name, args, expected, ops))
                    .map(Some)
            }
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
            Expression::Slice(..) => self.slice(e, expected, ops).map(Some),
            Expression::Match(value, cases) => self.match_value(&e.at, value, cases, expected, ops),
            Expression::Checked(body) => self.checked_value(&e.at, body, expected, ops),

            Expression::Method(base, name, args, _)
                if matches!(name.as_str(), "unwrap_or" | "ok_or")
                    && !self
                        .place_type(base)
                        .map(classes::unbox)
                        .as_ref()
                        .is_some_and(classes::inherent_receiver) =>
            {
                self.option_method(base, name, args, expected, ops)
            }
            Expression::Method(base, name, args, _)
                if name == "unwrap"
                    && !self
                        .place_type(base)
                        .map(classes::unbox)
                        .as_ref()
                        .is_some_and(classes::inherent_receiver) =>
            {
                self.unwrap_result(base, args, expected, ops)
            }
            Expression::Call(name, args)
                if name == crate::atomic::ATOMIC && matches!(&expected, Some(Ty::Atomic(_))) =>
            {
                self.atomic_new(None, args, expected, &e.at, ops)
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
                if expected
                    .as_ref()
                    .is_some_and(|ty| ty.nonzero_element().is_some())
                    && !numeric_suffix(n)
                    && !n.contains(['.', 'e', 'E']) =>
            {
                self.nonzero_literal(e, expected.as_ref().unwrap(), ops)
            }
            Expression::Unary(op, inner)
                if op == "-"
                    && expected
                        .as_ref()
                        .is_some_and(|ty| ty.nonzero_element().is_some())
                    && matches!(&ungroup(inner).kind, Expression::Number(n)
                        if !numeric_suffix(n) && !n.contains(['.', 'e', 'E'])) =>
            {
                self.nonzero_literal(e, expected.as_ref().unwrap(), ops)
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
                if let Some(result) = self.overloaded_binary(op, left, right, &e.at, ops)? {
                    return Ok(result);
                }
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
        if kind == "generator" {
            return self.generator_expression(&entries[0].0, clauses, &e.at, ops);
        }
        let expected = match expected {
            Some(Ty::Enum(t))
                if t.propagatable()
                    && !t.is_option()
                    && (crate::sum::accepts(&t.get().variants[1].fields[0], &crate::sum::alloc_error())
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
        if let Some(output) =
            self.comprehension_pipeline(kind, entries, clauses, expected.clone(), &e.at, ops)?
        {
            return Ok(output);
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
        // A display has a known final size; a comprehension may be filtered or
        // unbounded. Reserve before evaluating elements, as with with_capacity.
        let capacity = if kind == "list" && clauses.is_empty() {
            entries.len() as i64
        } else {
            0
        };
        ops.extend([
            Op::PushInt(Value::I64(capacity)),
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
                    cleanup,
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
                ops.extend(cleanup);
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
                    let inferred = self.comprehension_entry(key, value.as_ref(), kind, ty, ops)?;
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

    /// Push one entry's key, then its value for a dict, and refine the
    /// collection type `ty` from them. Returns the refined collection type.
    fn comprehension_entry(
        &mut self,
        key: &Expr,
        value: Option<&Expr>,
        kind: &str,
        ty: &mut Type,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
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
                    .error("dictionary keys must be integers, bool, str, or tuples of these"));
            }
            Ty::Dict(Arc::new(k), Arc::new(v))
        } else if kind == "set" {
            if !k.hashable() {
                return Err(key
                    .at
                    .error("set elements must be integers, bool, str, or tuples of these"));
            }
            Ty::Set(Arc::new(k))
        } else {
            Ty::List(Arc::new(k))
        };
        if let Some(expected) = ty {
            self.same(Some(inferred.clone()), Some(expected.clone()), &key.at)?;
        }
        // Hashable keys are copyable scalars or structural tuples.
        let barrier = match &inferred {
            Ty::Dict(_, t) | Ty::List(t) | Ty::Set(t) => t.storage_barrier(),
            _ => None,
        };
        if let Some(barrier) = barrier {
            return Err(key.at.error(barrier.message("collections")));
        }
        *ty = Some(inferred.clone());
        Ok(inferred)
    }

    /// Lower a comprehension with one `for` and any `if` clauses as the
    /// pipeline `iter(s).filter(..).map(..).collect()`: the same iteration,
    /// filter steps and known-length reserve. Other shapes return `None`.
    fn comprehension_pipeline(
        &mut self,
        kind: &str,
        entries: &[(Expr, Option<Expr>)],
        clauses: &[Clause],
        expected: Type,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Ty>> {
        let ([Clause::For(names, iterable), filters @ ..], [(key, value)]) = (clauses, entries)
        else {
            return Ok(None);
        };
        if filters
            .iter()
            .any(|clause| matches!(clause, Clause::For(..)))
        {
            return Ok(None);
        }
        let start = self.locals.len();
        let saved = self.names.clone();
        let hint = comprehension_hint(names, entries, expected.as_ref());
        let mut iteration = self.iteration(names, iterable, hint, ops)?;
        for filter in filters {
            let Clause::If(condition) = filter else {
                unreachable!()
            };
            let temporaries = self.expression_temps.len();
            let accepted = self.expr(condition, &mut iteration.body)?;
            self.same(accepted, Some(Ty::Bool), &condition.at)?;
            self.finish_temporaries(temporaries, &mut iteration.body);
            iteration.exact_len = None;
            let mut rejected = iteration.step.clone();
            rejected.push(Op::Continue);
            iteration.body.push(branch(vec![], rejected));
        }
        let temporaries = self.expression_temps.len();
        let mut ty = expected;
        let target =
            self.comprehension_entry(key, value.as_ref(), kind, &mut ty, &mut iteration.body)?;
        let item = match &target {
            Ty::Dict(k, v) => {
                let item = crate::sum::tuple(vec![(**k).clone(), (**v).clone()]);
                let Ty::Enum(tuple) = &item else {
                    unreachable!()
                };
                iteration
                    .body
                    .push(Op::Enum(crate::sum::EnumOp::New(tuple.clone(), 0)));
                item
            }
            _ => target.element().unwrap(),
        };
        self.names = saved;
        if target.layout_depth() >= 64 {
            return Err(at.error("type nesting exceeds the implementation limit of 64"));
        }
        let slot = self.slot(item.clone(), at)?;
        iteration.body.push(Op::StoreLocal(slot));
        self.finish_temporaries(temporaries, &mut iteration.body);
        if item.managed() {
            iteration.step.push(Op::DropLocal(slot));
        }
        iteration.target = slot;
        let output = self.collect_iteration(iteration, target, None, at, ops)?;
        self.cleanup(start, ops);
        Ok(Some(output))
    }

    pub(super) fn dictionary_iteration(
        &mut self,
        names: &[String],
        base: &Expr,
        mutable: bool,
        values_only: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let value_names;
        let names = if values_only {
            if names.len() != 1 {
                return Err(base.at.error("dictionary values loops require one binding"));
            }
            value_names = vec!["_".to_owned(), names[0].clone()];
            &value_names
        } else {
            names
        };
        if names.len() != 2 || names[0] != "_" && names[0] == names[1] {
            return Err(base
                .at
                .error("dictionary items loops require two distinct bindings"));
        }
        let temporary_start = self.expression_temps.len();
        let (reference, loan) = if mutable {
            self.borrow(base, true, ops)?
        } else {
            self.shared_argument(base, None, ops)?
        };
        let reference = self.unbox_reference(reference, ops);
        let Ty::Ref(owner, _) = reference else {
            unreachable!()
        };
        let ty = (*owner).clone();
        let Ty::Dict(key, value) = &ty else {
            return Err(base.at.error("items requires a dictionary"));
        };
        let value_ty = Ty::Ref(value.clone(), mutable);
        let key_ty = Ty::Ref(key.clone(), false);
        self.expression_temps.truncate(temporary_start);
        self.loans[loan].precise = false;
        for op in ops.iter_mut() {
            if let Op::Loan(fact) = op
                && fact.id == loan
            {
                fact.precise = false;
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
            exact_len: None,
            cleanup: vec![],
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

    pub(super) fn iteration(
        &mut self,
        names: &[String],
        iterable: &Expr,
        hint: Option<Ty>,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        if self.is_clist_column(iterable) {
            return self.clist_iteration(names, iterable, ops);
        }
        if let Expression::Call(function, args) = &ungroup(iterable).kind
            && matches!(function.as_str(), "enumerate" | "zip" | "reversed")
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
            if self.yield_type.is_none() {
                return self.pipeline_iteration(names, iterable, ops);
            }
            return self.iteration_adapter(names, function, args, &iterable.at, ops);
        }
        // Raw adapter loops inside generators retain their legacy lowering.
        // Other adapters use pull plans so filters advance each zip arm alone.
        if dictionary_items(iterable).is_none() && self.is_pipeline(iterable) {
            return self.pipeline_iteration(names, iterable, ops);
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
        if let Expression::GenericCall(function, types, args) = &ungroup(iterable).kind
            && function == super::pipelines::parallel::ROWS
            && let [
                TypeRef {
                    concrete: Some(source),
                    ..
                },
            ] = types.as_slice()
        {
            return self.par_rows_iteration(names, (**source).clone(), args, &iterable.at, ops);
        }
        if names.len() == 1
            && let Some((first, stride, count)) = literal_progression(iterable)
        {
            // Eliding storage must not make an otherwise invalid `?` legal.
            if matches!(&ungroup(iterable).kind, Expression::Try(_)) {
                let target = self.propagation_target(iterable)?;
                let Ty::Enum(source) =
                    crate::sum::result(Ty::List(Arc::new(Ty::I64)), crate::sum::alloc_error())
                else {
                    unreachable!()
                };
                self.check_propagation(iterable, &source, target, true)?;
            }
            let target = self.slot(Ty::I64, &iterable.at)?;
            let step = self.slot(Ty::I64, &iterable.at)?;
            let remaining = self.slot(Ty::I64, &iterable.at)?;
            ops.extend([
                Op::PushInt(Value::I64(first)),
                Op::StoreLocal(target),
                Op::PushInt(Value::I64(stride)),
                Op::StoreLocal(step),
                Op::PushInt(Value::I64(count)),
                Op::StoreLocal(remaining),
            ]);
            let plan = self.counted_iteration(
                (target, step, remaining),
                Ty::I64,
                reverse,
                &iterable.at,
                ops,
            )?;
            self.bind(
                names[0].clone(),
                Local {
                    slot: target,
                    ty: Ty::I64,
                    mutable: false,
                },
            );
            return Ok(plan);
        }
        if let Some((base, mutable)) = dictionary_items(iterable)
            && !self
                .place_type(base)
                .or_else(|| self.expression_type_hint(base))
                .map(classes::unbox)
                .as_ref()
                .is_some_and(classes::inherent_receiver)
        {
            if reverse {
                return Err(iterable
                    .at
                    .error("reversed requires a list, str, range, or Bytes"));
            }
            let mut source = ungroup(iterable);
            if let Expression::Unary(_, inner) = &source.kind {
                source = ungroup(inner);
            }
            let values_only =
                matches!(&source.kind, Expression::Method(_, name, _, _) if name == "values");
            return self.dictionary_iteration(names, base, mutable, values_only, ops);
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
        let mut borrowed = matches!(&iterable.kind, Expression::Unary(op, _) if op == "&" || op == "&mut")
            || matches!(&iterable.kind, Expression::Name(n) if self.names.get(n).is_some_and(|l| matches!(l.ty, Ty::Ref(..))));
        let mut mutable = matches!(&iterable.kind, Expression::Unary(op, _) if op == "&mut")
            || matches!(&iterable.kind, Expression::Name(n) if self.names.get(n).is_some_and(|l| matches!(l.ty, Ty::Ref(_, true))));
        let range_expr = match &iterable.kind {
            Expression::Try(inner) => ungroup(inner),
            Expression::Method(inner, name, args, _) if name == "unwrap" && args.is_empty() => {
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
        let range_element = range_args.map(|args| {
            hint.unwrap_or_else(|| {
                args.iter()
                    .take(2)
                    .find_map(|e| self.numeric_hint(e))
                    .filter(Ty::is_int)
                    .unwrap_or(Ty::I64)
            })
        });
        // A direct `range(...)` call needs no range value.
        let counted = range_args
            .zip(range_element.clone())
            .filter(|(_, element)| std::ptr::eq(range_expr, iterable) && element.is_int());
        let (ty, mut loans) = if let Some((buffer, exclusive)) = self.borrowed_byte_buffer(iterable)
        {
            // Issue #594: a borrowed Bytes buffer loops over a view of all of
            // it, as `for b in &buffer[:]` does, so each `&u8` or `&mut u8`
            // points into the buffer rather than at an iteration-local copy.
            let whole = SliceBounds {
                start: None,
                stop: None,
                step: None,
            };
            let (view, loan) = self.slice_view(buffer, &whole, exclusive, ops)?;
            borrowed = true;
            mutable = exclusive;
            (view, vec![loan])
        } else if self.is_view(iterable) {
            let (view, loan) = self.view_source(iterable, ops)?;
            borrowed = true;
            mutable = matches!(view, Ty::View(_, true));
            (view, vec![loan])
        } else if let Some((_, element)) = &counted {
            (Ty::Range(Arc::new(element.clone())), vec![])
        } else if let Some(element) = range_element {
            (
                self.expr_expected(iterable, Some(Ty::Range(Arc::new(element))), ops)?
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
            // A list is iterated through its reference (`borrowed_list_iteration`).
            if !matches!(*ty, Ty::List(_)) {
                ops.push(Op::ReadRef((*ty).clone()));
            }
            ((*ty).clone(), vec![loan])
        } else if borrowed {
            let base = match &iterable.kind {
                Expression::Unary(_, base) => &**base,
                _ => iterable,
            };
            let (reference, loan) = self.shared_argument(base, None, ops)?;
            let Ty::Ref(ty, writable) = reference else {
                unreachable!()
            };
            if matches!(*ty, Ty::List(_)) {
                if writable {
                    ops.push(Op::Reborrow(Ty::Ref(ty.clone(), false)));
                }
            } else {
                ops.push(Op::ReadRef((*ty).clone()));
            }
            ((*ty).clone(), vec![loan])
        } else {
            let (ty, loan) = self.binding_value(iterable, None, ops)?;
            if let Ty::Ref(inner, writable) = ty {
                borrowed = true;
                mutable = writable;
                if !matches!(*inner, Ty::List(_)) {
                    ops.push(Op::ReadRef((*inner).clone()));
                }
                (
                    (*inner).clone(),
                    vec![loan.expect("reference value has an origin")],
                )
            } else {
                (ty, loan.into_iter().collect())
            }
        };
        // Checked before the ownership-mode errors, which would otherwise
        // suggest a list or a borrow (issue #574).
        if ty == Ty::File {
            return Err(iterable.at.error(
                "a file is not iterable; read lines with `line = file.readline()?` in a `while` loop until it returns `\"\"`, or collect them with `file.readlines()?`",
            ));
        }
        if matches!(ty, Ty::KeyView(_)) {
            borrowed = true;
            if loans.is_empty() {
                loans.push(self.reference_origin(iterable, ops)?);
            }
        }
        let bytes = byte_buffers::byte_buffer(&ty).is_some();
        if reverse
            && !matches!(
                ty,
                Ty::List(_) | Ty::Str | Ty::ByteStr | Ty::Range(_) | Ty::View(..)
            )
            && !bytes
        {
            return Err(iterable
                .at
                .error("reversed requires a list, str, bytes, range, or Bytes"));
        }
        // A named buffer was viewed above; a temporary one has no place for
        // the `&mut u8` rows to point into.
        if mutable && bytes {
            return Err(iterable.at.error(
                "mutable iteration over Bytes borrows a named buffer; bind the value to a name first",
            ));
        }
        let element = match &ty {
            _ if bytes => Some(Ty::U8),
            Ty::View(element, _) => Some((**element).clone()),
            _ => ty.element(),
        };
        if borrowed && matches!(ty, Ty::Generator(_)) {
            return Err(iterable.at.error("borrowed generator iteration is not supported; use next with an exclusive reference"));
        }
        let references = borrowed
            && matches!(
                ty,
                Ty::List(_) | Ty::Dict(..) | Ty::Set(_) | Ty::KeyView(_) | Ty::View(..)
            );
        if mutable && !matches!(ty, Ty::List(_) | Ty::View(..)) {
            return Err(iterable.at.error(
                "mutable iteration requires a list; use items for mutable dictionary values",
            ));
        }
        if borrowed && !references && element.as_ref().is_some_and(Ty::affine) {
            return Err(iterable.at.error("borrowed iteration of owned elements is not supported; iterate an owned collection or copy it"));
        }
        let mut plan = if let Some((args, element)) = counted {
            self.counted_range_iteration(args, element, reverse, &iterable.at, ops)?
        } else if let Ty::View(..) = ty {
            self.view_iteration(ty.clone(), reverse, &iterable.at, ops)?
        } else if references && matches!(ty, Ty::List(_)) {
            self.borrowed_list_iteration(ty.clone(), mutable, reverse, &iterable.at, ops)?
        } else if references {
            self.borrowed_collection_iteration(ty.clone(), &iterable.at, ops)?
        } else {
            self.iteration_on_stack_direction(ty.clone(), reverse, &iterable.at, ops)?
        };
        if references {
            let loan = loans[0];
            self.loans[loan].precise = false;
            for op in ops.iter_mut() {
                if let Op::Loan(fact) = op
                    && fact.id == loan
                {
                    fact.precise = false;
                }
            }
            self.reference_locals.insert(plan.target, loan);
        } else if borrowed {
            // Text scalars and range elements are computed values. Their shared
            // reference points to an iteration-local slot, not into the source.
            let value_slot = plan.target;
            let reference_slot = self.slot(
                Ty::Ref(Arc::new(element.clone().unwrap()), false),
                &iterable.at,
            )?;
            let loan = self.new_loan(value_slot, false, None, &mut plan.body);
            plan.body.extend([
                Op::BorrowLocal(value_slot, false),
                Op::StoreLocal(reference_slot),
            ]);
            self.reference_locals.insert(reference_slot, loan);
            plan.target = reference_slot;
            // A range's counted loop advances its element slot in place.
            if !matches!(ty, Ty::Range(_)) {
                plan.step.push(Op::DropLocal(value_slot));
            }
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
                    Ty::Ref(Arc::new(element.unwrap()), mutable)
                } else {
                    element.unwrap()
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

    /// `separator.join(iterable)` accepts every iterable whose `for` element is
    /// `str`. Collections and strings are observed and measured before the one
    /// output allocation; a generator is one-shot, so it is consumed like a
    /// loop source and its pieces are buffered (`text-building.md`).
    fn join_text(&mut self, argument: &Expr, ops: &mut Vec<Op>) -> Result<Ty> {
        let owned = match &ungroup(argument).kind {
            Expression::Name(name) => self.names.get(name).map(|local| &local.ty),
            _ => None,
        };
        let owned = matches!(owned, Some(Ty::Generator(_)));
        let place = self.place_type(argument).filter(|_| !owned);
        let (ty, loans) = if owned {
            let ty = self.value(argument, ops)?;
            let slot = self.slot(ty.clone(), &argument.at)?;
            ops.extend([Op::StoreLocal(slot), Op::LoadLocal(slot)]);
            self.expression_temps.push(slot);
            (ty, vec![])
        } else {
            // An empty display such as `[]?` or `[].unwrap()` has no element
            // to infer from; `join` supplies `str`.
            let literal = match &ungroup(argument).kind {
                Expression::Try(inner) => &ungroup(inner).kind,
                Expression::Method(inner, method, unwrap_args, _)
                    if method == "unwrap" && unwrap_args.is_empty() =>
                {
                    &ungroup(inner).kind
                }
                kind => kind,
            };
            let hint = match literal {
                Expression::Collection { kind, .. } if kind == "list" => {
                    Some(Ty::List(Arc::new(Ty::Str)))
                }
                Expression::Collection { kind, .. } if kind == "set" => {
                    Some(Ty::Set(Arc::new(Ty::Str)))
                }
                _ => None,
            };
            self.observe_expected(argument, hint, ops)?
        };
        if ty.element() != Some(Ty::Str) {
            return Err(argument.at.error(format!(
                "join requires an iterable of str (a list, set, dictionary keys, str, or generator), got {ty}"
            )));
        }
        // Advancing through a reference would leave the caller's generator
        // exhausted; `for` and `join` consume generators alike.
        if matches!(ty, Ty::Generator(_)) && (place.is_some() || !loans.is_empty()) {
            return Err(argument
                .at
                .error("join consumes a generator; pass it by value, not by reference"));
        }
        let operation = CollectionOp::TextTryJoin(ty);
        let (_, result) = operation.signature();
        ops.push(Op::Collection(operation));
        Self::end_reads(loans, ops);
        Ok(result)
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
            left.exact_len = None;
            left.cleanup.extend(right.cleanup);
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
            if Ty::identity_names_exceed_limit(&fields)
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

    /// Iterates a borrowed list through a row cursor. The loop holds the
    /// list's loan from the first condition to the last body use, so no
    /// mutation can grow, shrink, or reallocate the rows meanwhile: the length,
    /// first-row address, and row size are read once, and each element
    /// reference is the address of its row, as `ElementRef` would return.
    /// The list reference is on the stack.
    fn borrowed_list_iteration(
        &mut self,
        ty: Ty,
        mutable: bool,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let source = self.slot(Ty::Ref(Arc::new(ty.clone()), mutable), at)?;
        let length = self.slot(Ty::I64, at)?;
        let rows = self.slot(Ty::I64, at)?;
        let row_bytes = self.slot(Ty::I64, at)?;
        let index = self.slot(Ty::I64, at)?;
        let target = self.slot(Ty::Ref(Arc::new(ty.element().unwrap()), mutable), at)?;
        ops.push(Op::StoreLocal(source));
        let shared = Ty::Ref(Arc::new(ty.clone()), false);
        for (operation, slot) in [
            (CollectionOp::ListLenRef(ty.clone()), length),
            (CollectionOp::ListRowsRef(ty.clone()), rows),
            (CollectionOp::ListRowBytesRef(ty.clone()), row_bytes),
        ] {
            ops.push(Op::LoadLocal(source));
            if mutable {
                ops.push(Op::Reborrow(shared.clone()));
            }
            ops.extend([Op::Collection(operation), Op::StoreLocal(slot)]);
        }
        let mut body = Vec::new();
        let mut step = Vec::new();
        let condition = if reverse {
            ops.extend([Op::LoadLocal(length), Op::StoreLocal(index)]);
            body.extend([
                Op::LoadLocal(index),
                Op::PushInt(Value::I64(1)),
                Op::Integer(IntegerOp::WrappingSub),
                Op::StoreLocal(index),
            ]);
            vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Gt]
        } else {
            ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(index)]);
            // `index < length` bounds the index, so it cannot overflow.
            step.extend([
                Op::LoadLocal(index),
                Op::PushInt(Value::I64(1)),
                Op::Integer(IntegerOp::WrappingAdd),
                Op::StoreLocal(index),
            ]);
            vec![Op::LoadLocal(index), Op::LoadLocal(length), Op::Lt]
        };
        body.extend([
            Op::LoadLocal(rows),
            Op::LoadLocal(row_bytes),
            Op::LoadLocal(index),
            Op::Collection(CollectionOp::ListRowRef(ty, mutable)),
            Op::StoreLocal(target),
        ]);
        Ok(Iteration {
            exact_len: Some(vec![Op::LoadLocal(length)]),
            cleanup: vec![],
            condition,
            absent: vec![],
            body,
            step,
            target,
        })
    }

    /// A `.par` chunk's rows of the caller's list: `&T` items from a borrowed
    /// list; otherwise each item is copied, or taken when affine, leaving an
    /// empty row for the list's later drop. The caller keeps the list alive
    /// and unmodified until this chunk's job has finished
    /// (`pipelines::parallel`), so no loan here reaches the list itself.
    fn par_rows_iteration(
        &mut self,
        names: &[String],
        source: Ty,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let [name] = names else {
            unreachable!("par rows bind one item")
        };
        let (list, borrowed) = match source {
            Ty::Ref(list, _) => ((*list).clone(), true),
            list => (list, false),
        };
        let element = list.element().expect("par rows of a list");
        let mut slots = Vec::new();
        for arg in args {
            self.value(arg, ops)?;
            let slot = self.slot(Ty::I64, at)?;
            ops.push(Op::StoreLocal(slot));
            slots.push(slot);
        }
        let [rows, row_bytes, length] = slots[..] else {
            unreachable!("par rows take three integers")
        };
        let index = self.slot(Ty::I64, at)?;
        ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(index)]);
        let item = if borrowed {
            Ty::Ref(Arc::new(element.clone()), false)
        } else {
            element.clone()
        };
        let target = self.slot(item.clone(), at)?;
        let mut body = vec![
            Op::LoadLocal(rows),
            Op::LoadLocal(row_bytes),
            Op::LoadLocal(index),
        ];
        if !borrowed && element.affine() {
            body.push(Op::Collection(CollectionOp::ListRowTake(list)));
        } else {
            body.push(Op::Collection(CollectionOp::ListRowRef(list, false)));
            if !borrowed {
                body.push(Op::ReadRef(element.clone()));
            }
        }
        body.push(Op::StoreLocal(target));
        let mut step = Vec::new();
        increment(index, &mut step);
        let mut condition = vec![Op::LoadLocal(index), Op::LoadLocal(length), Op::Lt];
        if borrowed {
            // Item references are rooted in this chunk's row cursor, which
            // stays unchanged for the loop.
            let loan = self.new_loan(rows, false, None, ops);
            self.loans[loan].precise = false;
            for op in ops.iter_mut() {
                if let Op::Loan(fact) = op
                    && fact.id == loan
                {
                    fact.precise = false;
                }
            }
            self.reference_locals.insert(target, loan);
            condition.push(Op::UseLoan(loan));
            body.push(Op::UseLoan(loan));
        } else if element.managed() {
            step.push(Op::DropLocal(target));
        }
        self.bind(
            name.clone(),
            Local {
                slot: target,
                ty: item,
                mutable: false,
            },
        );
        Ok(Iteration {
            exact_len: Some(vec![Op::LoadLocal(length)]),
            cleanup: vec![],
            condition,
            absent: vec![],
            body,
            step,
            target,
        })
    }

    /// Iterates a borrowed dictionary's keys or a borrowed set's members by
    /// hash cursor. The collection is on the stack.
    fn borrowed_collection_iteration(
        &mut self,
        ty: Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let source = self.slot(ty.clone(), at)?;
        let target = self.slot(Ty::Ref(Arc::new(ty.element().unwrap()), false), at)?;
        let index = self.slot(Ty::I64, at)?;
        ops.extend([
            Op::StoreLocal(source),
            Op::LoadLocal(source),
            Op::Collection(CollectionOp::HashIterFirst(ty.clone())),
            Op::StoreLocal(index),
        ]);
        Ok(Iteration {
            exact_len: None,
            cleanup: vec![],
            condition: vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Ne],
            absent: vec![],
            body: vec![
                Op::BorrowLocal(source, false),
                Op::LoadLocal(index),
                Op::Collection(CollectionOp::HashIterKeyRef(ty.clone())),
                Op::StoreLocal(target),
            ],
            step: vec![
                Op::LoadLocal(source),
                Op::LoadLocal(index),
                Op::Collection(CollectionOp::HashIterNext(ty)),
                Op::StoreLocal(index),
            ],
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
        if let Some(class) = byte_buffers::byte_buffer(&ty) {
            return self.byte_iteration(class, ty, reverse, at, ops);
        }
        if let Ty::Range(element) = &ty
            && element.is_int()
        {
            return self.range_value_iteration((**element).clone(), reverse, at, ops);
        }
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
                exact_len: None,
                cleanup: vec![],
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
                if list || text {
                    Op::BorrowLocal(source, false)
                } else {
                    Op::LoadLocal(source)
                },
                Op::Collection(if text {
                    CollectionOp::QueryRef(Query::TextByteLen)
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
        let exact_len = list.then(|| {
            vec![
                Op::BorrowLocal(source, false),
                Op::Collection(CollectionOp::ListLenRef(ty.clone())),
            ]
        });
        let condition = if hashed {
            vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Ne]
        } else if reverse {
            vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Gt]
        } else {
            vec![
                Op::LoadLocal(index),
                if list || text {
                    Op::BorrowLocal(source, false)
                } else {
                    Op::LoadLocal(source)
                },
                Op::Collection(if text {
                    CollectionOp::QueryRef(Query::TextByteLen)
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
                    Op::BorrowLocal(source, false),
                    Op::LoadLocal(index),
                    Op::Collection(CollectionOp::QueryRef(Query::TextPrevByte)),
                    Op::StoreLocal(index),
                ]);
            } else {
                decrement(index, &mut body);
            }
        }
        body.extend([
            if scalar_list || text {
                Op::BorrowLocal(source, false)
            } else {
                Op::LoadLocal(source)
            },
            Op::LoadLocal(index),
            Op::Collection(if text {
                CollectionOp::QueryRef(Query::TextAtByte)
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
                Op::BorrowLocal(source, false),
                Op::LoadLocal(index),
                Op::Collection(CollectionOp::QueryRef(Query::TextNextByte)),
                Op::StoreLocal(index),
            ]);
        } else if !reverse {
            increment(index, &mut step);
        }
        if element.managed() {
            step.push(Op::DropLocal(target));
        }
        Ok(Iteration {
            exact_len,
            cleanup: vec![],
            condition,
            absent: vec![],
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
        let iterable_start = ops.len();
        let Iteration {
            condition,
            mut body,
            step,
            cleanup,
            ..
        } = self.iteration(name, iterable, None, ops)?;
        self.building(&iterable.at, iterable_start, ops, false);
        self.indexed_lists.push(IndexedLists {
            before: self.parameters + start,
            views: HashMap::default(),
            setup: Vec::new(),
        });
        self.loop_steps.push(step.clone());
        self.loop_scopes.push(self.locals.len());
        let result = self.block(statements, &mut body, &Tail::Discard);
        ops.extend(self.indexed_lists.pop().unwrap().setup);
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
        ops.extend(cleanup);
        self.cleanup(start, ops);
        Ok(())
    }

    fn indexed_list_view(&mut self, base: &Expr, ty: &Ty) -> Result<Option<[u8; 3]>> {
        let Expression::Name(name) = &ungroup(base).kind else {
            return Ok(None);
        };
        let Some(local) = self.names.get(name) else {
            return Ok(None);
        };
        let source = local.slot;
        let Some(context) = self.indexed_lists.last() else {
            return Ok(None);
        };
        if usize::from(source) >= context.before
            || local.mutable
            || self.captures.contains(name)
            || self.yield_type.is_some()
            || !matches!(&local.ty, Ty::Ref(inner, false) if matches!(&**inner, Ty::List(element) if element.is_numeric() || **element == Ty::Bool))
        {
            return Ok(None);
        }
        if let Some(view) = context.views.get(&source) {
            return Ok(Some(*view));
        }
        let view = [
            self.slot(Ty::I64, &base.at)?,
            self.slot(Ty::I64, &base.at)?,
            self.slot(Ty::I64, &base.at)?,
        ];
        let mut setup = Vec::new();
        self.mark(&base.at, &mut setup);
        let (_, loan) = self.borrow(base, false, &mut setup)?;
        setup.extend([
            Op::Collection(CollectionOp::ListLenRef(ty.clone())),
            Op::StoreLocal(view[0]),
            Op::UseLoan(loan),
        ]);
        for (operation, slot) in [
            (CollectionOp::ListRowsRef(ty.clone()), view[1]),
            (CollectionOp::ListRowBytesRef(ty.clone()), view[2]),
        ] {
            setup.extend([
                Op::LoadLocal(source),
                Op::Collection(operation),
                Op::StoreLocal(slot),
            ]);
        }
        let context = self.indexed_lists.last_mut().unwrap();
        context.views.insert(source, view);
        context.setup.extend(setup);
        Ok(Some(view))
    }

    pub(super) fn index(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Ty> {
        if let Expression::Index(base, _) = &ungroup(e).kind
            && matches!(self.place_type(base), Some(Ty::Clist(_)))
        {
            return Err(e.at.error(
                "clist rows are not contiguous objects; use items[i].field or items.copy_row(i)",
            ));
        }
        let Expression::Index(base, index) = &e.kind else {
            unreachable!()
        };
        if matches!(self.place_type(base), Some(Ty::Shared(_))) {
            let element = Expr {
                at: e.at.clone(),
                kind: Expression::Index(Box::new(super::shared::content(base)), index.clone()),
            };
            if self.place_type(&element).is_some_and(|ty| ty.affine()) {
                return Err(e.at.error(
                    "cannot move an element out of a Shared owner; borrow it with `&handle[i]`, or copy it with copy(handle[i])",
                ));
            }
            return self.index(&element, ops);
        }
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
        if let Some(class) = self.byte_place(base) {
            let reads = self.byte_item(&class, base, index, &e.at, ops)?;
            Self::end_reads(reads, ops);
            return Ok(Ty::U8);
        }
        if let Some(ty @ Ty::List(_)) = self.place_type(base) {
            let value = ty.element().unwrap();
            if value.is_numeric() || value == Ty::Bool {
                let view = self.indexed_list_view(base, &ty)?;
                let (_, loan) = self.borrow(base, false, ops)?;
                if let Some(view) = view {
                    ops.extend(view.into_iter().map(Op::LoadLocal));
                }
                let (got, mut reads) = self.observe_expected(index, Some(Ty::I64), ops)?;
                self.same(Some(got), Some(Ty::I64), &index.at)?;
                self.mark(&e.at, ops);
                ops.push(Op::Collection(if view.is_some() {
                    CollectionOp::ListScalarGetView(ty)
                } else {
                    CollectionOp::ListScalarGetRef(ty)
                }));
                reads.push(loan);
                Self::end_reads(reads, ops);
                return Ok(value);
            }
        }
        if self.place_type(base) == Some(Ty::Str) {
            // Index a text place in place (issue #364); the scalar result is
            // inline, so it owns nothing borrowed from the text.
            let (_, loan) = self.borrow(base, false, ops)?;
            let (got, mut reads) = self.observe_expected(index, Some(Ty::I64), ops)?;
            self.same(Some(got), Some(Ty::I64), &index.at)?;
            self.mark(&e.at, ops);
            ops.push(Op::Collection(CollectionOp::QueryRef(Query::TextAt)));
            reads.push(loan);
            Self::end_reads(reads, ops);
            return Ok(Ty::Str);
        }
        let (ty, loans) = self.observe(base, ops)?;
        if let Ty::Enum(t) = &ty
            && t.tuple()
        {
            let fields = &t.get().variants[0].fields;
            let i = slices::tuple_position(index, fields.len())?;
            let value = fields[i].clone();
            if value.affine() {
                return Err(index
                    .at
                    .error("cannot move out of a tuple index; unpack the tuple instead"));
            }
            ops.push(Op::Enum(crate::sum::EnumOp::Field(t.clone(), 0, i)));
            Self::end_reads(loans, ops);
            return Ok(value);
        }
        if let Some(class) = byte_buffers::byte_buffer(&ty) {
            let reads = self.byte_value_item(&class, index, &e.at, ops)?;
            Self::end_reads(reads, ops);
            Self::end_reads(loans, ops);
            return Ok(Ty::U8);
        }
        if let Ty::View(element, _) = &ty {
            if element.affine() {
                return Err(e.at.error(
                    "cannot move an element out of a slice view; borrow it with `&v[i]`, or copy it with `copy(v[i])?`",
                ));
            }
            let (value, reads) = self.view_get(&ty, index, &e.at, ops)?;
            Self::end_reads(reads, ops);
            Self::end_reads(loans, ops);
            return Ok(value);
        }
        let (key, value) = match &ty {
            Ty::List(v) => (Ty::I64, (**v).clone()),
            Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
            Ty::Str => (Ty::I64, Ty::Str),
            Ty::ByteStr => (Ty::I64, Ty::U8),
            Ty::Range(t) => (Ty::I64, (**t).clone()),
            _ => {
                return Err(base
                    .at
                    .error("indexing requires list, dict, str, bytes, range, or Bytes"));
            }
        };
        if value.affine() {
            return Err(self.value_demand_error(
                &base.at,
                e,
                "cannot move out of indexed storage; borrow it with &collection[index], take it with replace(collection[index], value) or collection.pop(index), or copy it with copy(collection[index])?",
                crate::ownership::Demand::Copy(value, "cannot move out of indexed storage"),
                false,
            ));
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
        if self.dict_index_target(target) {
            return Err(target.at.error(references::DICT_INDEX_ASSIGNMENT));
        }
        if let Expression::Index(base, index) = &ungroup(target).kind
            && let Some(class) = self.byte_place(base)
        {
            return self.set_byte(&class, base, index, value, &target.at, ops);
        }
        self.set_field(target, value, ops)
    }

    /// The `Bytes` class when `base` names a byte buffer place.
    pub(super) fn byte_place(
        &self,
        base: &Expr,
    ) -> Option<crate::nominal::Nominal<crate::record::ClassType>> {
        byte_buffers::byte_buffer(&classes::unbox(self.place_type(base)?))
    }

    fn unwrap_result(
        &mut self,
        base: &Expr,
        args: &[Expr],
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let context = expected.map(|t| crate::sum::result(t, crate::sum::alloc_error()));
        let actual = self.expr_expected(base, context, ops)?;
        if actual.as_ref().is_some_and(Ty::key_view) {
            return Err(base.at.error("keys() returns a borrowed KeyView, not a Result; use keys_snapshot(), into_keys(), or to_set() for owned results"));
        }
        if let Some(actual) = &actual {
            let receiver = classes::unbox(actual.clone());
            if classes::inherent_receiver(&receiver) {
                self.unbox_to(actual.clone(), &receiver, ops);
                return self.temporary_class_method(
                    base,
                    "unwrap",
                    None,
                    args,
                    (receiver, vec![]),
                    ops,
                );
            }
        }
        if !args.is_empty() {
            return Err(base.at.error("unwrap takes no arguments"));
        }
        let Some(Ty::Enum(t)) = actual else {
            return Err(base.at.error("unwrap requires a Result or Option"));
        };
        if !t.propagatable() {
            return Err(base.at.error("unwrap requires a Result or Option"));
        }
        let operation = crate::sum::EnumOp::Unwrap(t);
        let result = operation.signature().unwrap().1;
        let carried = if result.carries_loan() {
            Some(self.reference_origin(base, ops)?)
        } else {
            None
        };
        self.mark(&base.at, ops);
        ops.push(Op::Enum(operation));
        if let Some(parent) = carried {
            // The payload reference takes over the operand's loan.
            let root = self.loans[parent].root;
            self.new_loan(root, result.carries_mutable_loan(), Some(parent), ops);
        }
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
            if classes::inherent_receiver(&class) {
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
        let payload = option.get().variants[1].fields[0].clone();
        let carried = if payload.carries_loan() {
            if name == "unwrap_or" {
                return Err(base
                    .at
                    .error("`unwrap_or` cannot choose between two borrows; use `match`"));
            }
            Some(self.reference_origin(base, ops)?)
        } else {
            None
        };
        let source = self.slot(Ty::Enum(option.clone()), &base.at)?;
        ops.push(Op::StoreLocal(source));
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
        if let Some(parent) = carried {
            // The Result carries the Option's loan.
            let root = self.loans[parent].root;
            self.new_loan(root, payload.carries_mutable_loan(), Some(parent), ops);
        }
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
        if let Ty::Enum(t) = &result
            && convert
        {
            yes.push(Op::Enum(crate::sum::EnumOp::New(t.clone(), 0)));
            no.push(Op::Enum(crate::sum::EnumOp::New(t.clone(), 1)));
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
        if self.has_par(base) || (name == "par" && self.is_pipeline(base)) {
            return self.parallel_pipeline(base, name, args, ops);
        }
        if self.is_pipeline(base)
            || (self.is_generator_source(base)
                && matches!(
                    name,
                    "collect"
                        | "try_collect"
                        | "fold"
                        | "try_fold"
                        | "for_each"
                        | "try_for_each"
                        | "count"
                        | "any"
                        | "all"
                        | "find"
                        | "reduce"
                        | "sum"
                        | "min"
                        | "max"
                ))
        {
            return self.pipeline_terminal(base, name, args, ops);
        }
        if self.is_view(base) {
            return Err(base.at.error(format!(
                "a slice view has no method `{name}`; {}",
                views::OPERATIONS
            )));
        }
        if matches!(self.place_type(base), Some(Ty::Clist(_))) {
            return self.clist_method(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::Shared(_))) {
            return self.shared_method(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::Task(_))) {
            return self.join_thread(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::Channel(..))) {
            return self.channel_method(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::CancellationToken)) {
            return self.control_method(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::Atomic(_))) {
            return self.atomic_method(base, name, args, ops);
        }
        if self
            .place_type(base)
            .as_ref()
            .and_then(crate::mutex::class)
            .is_some()
        {
            return self.mutex_method(base, name, args, ops);
        }
        if matches!(self.place_type(base), Some(Ty::Executor | Ty::Future(_))) {
            return self.executor_method(base, name, args, ops);
        }
        if matches!(
            self.field_type(base, name),
            Some(Ty::Callable(_) | Ty::BoxedCallable(_) | Ty::Closure(_))
        ) {
            let field = Expr {
                at: base.at.clone(),
                kind: Expression::Member(Box::new(base.clone()), name.into(), Pos::default()),
            };
            return self.call_value(&field, args, ops);
        }
        if name == "null"
            && let Some(ty @ Ty::ForeignPtr(_)) = self.qualified_type(base)?
        {
            if !args.is_empty() {
                return Err(base.at.error("null takes no arguments"));
            }
            ops.push(Op::ForeignNull(ty.clone()));
            return Ok(Some(ty));
        }
        if name == "unwrap"
            && !self
                .place_type(base)
                .map(classes::unbox)
                .as_ref()
                .is_some_and(classes::inherent_receiver)
        {
            return self.unwrap_result(base, args, None, ops);
        }
        if matches!(name, "unwrap_or" | "ok_or")
            && !self
                .place_type(base)
                .map(classes::unbox)
                .as_ref()
                .is_some_and(classes::inherent_receiver)
        {
            return self.option_method(base, name, args, None, ops);
        }
        if name == "map_error"
            && !self
                .place_type(base)
                .map(classes::unbox)
                .as_ref()
                .is_some_and(classes::inherent_receiver)
        {
            return self.result_map_error(base, args, &base.at, ops);
        }
        if name == "drop_job"
            && self.place_type(base).map(classes::unbox).is_none_or(|ty| {
                crate::sum::job_free(&ty).is_some() || !classes::inherent_receiver(&ty)
            })
        {
            return self.drop_job_method(base, args, ops);
        }
        if name == "new" {
            if let Expression::Name(function) = &ungroup(base).kind
                && !self.names.contains_key(function)
                && self.sigs.declared(function).is_some_and(|f| f.yields)
            {
                return self.call_named(function, args, &base.at, ops);
            }
            if let Expression::Member(owner, variant, _) = &ungroup(base).kind
                && let Some(Ty::Enum(t)) = self.qualified_type(owner)?
            {
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
        if name == "try_from"
            && matches!(&base.kind, Expression::Name(n) if n == nonzero::CLASS && !self.names.contains_key(n))
        {
            return self.nonzero_try_from(None, args, &base.at, ops);
        }
        if let Some(ty) = self.qualified_type(base)? {
            if let Some(element) = ty.nonzero_element()
                && name == "try_from"
            {
                return self.nonzero_try_from(Some(element), args, &base.at, ops);
            }
            if matches!(ty, Ty::Clist(_)) {
                self.check_clist_access(&ty, &base.at)?;
                let operation = match (name, args) {
                    ("new", []) => crate::clist::ClistOp::New(ty),
                    ("with_capacity", [capacity]) => {
                        let actual = self.expr_expected(capacity, Some(Ty::I64), ops)?;
                        self.same(actual, Some(Ty::I64), &capacity.at)?;
                        crate::clist::ClistOp::WithCapacity(ty)
                    }
                    _ => {
                        return Err(base
                            .at
                            .error("clist type methods are new() and with_capacity(i64)"));
                    }
                };
                let output = operation.signature().1;
                ops.push(Op::Collection(CollectionOp::Clist(operation)));
                return Ok(Some(output));
            }
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
                if source.key_view() {
                    return Err(args[0].at.error("dictionary key views cannot be formatted; materialise keys_snapshot(), into_keys(), or to_set()"));
                }
                // A view formats as its rows do, like the reference it is.
                if source.restricted_storage()
                    && !matches!(source, Ty::View(..))
                    && (source.carried_reference().is_none() || source.format_blocker().is_some())
                {
                    let message = source
                        .format_blocker()
                        .map_or("generators cannot be formatted".into(), |b| {
                            b.message(&source)
                        });
                    return Err(base.at.error(message));
                }
                if source.facts().shared {
                    return Err(base.at.error(super::shared::FORMAT));
                }
                if source.facts().atomic && super::mutex::holds_mutex(&source) {
                    return Err(base.at.error(super::mutex::FORMAT));
                }
                if source.facts().atomic {
                    return Err(base
                        .at
                        .error("atomics cannot be formatted; format a value read with load()"));
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
                let (mut source, loans) = self.observe(&args[0], ops)?;
                if let Ty::Ref(inner, _) = &source
                    && inner.nonzero_element().is_some()
                {
                    let inner = (**inner).clone();
                    ops.push(Op::ReadRef(inner.clone()));
                    source = inner;
                }
                if let (Some(element), Ty::Class(class)) = (source.nonzero_element(), &source) {
                    ops.push(Op::Class(crate::record::ClassOp::Field(class.clone(), 0)));
                    source = element;
                }
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
                    return Err(casts::operand_error(
                        "try_from requires an integer value",
                        &source,
                        Ty::is_int,
                        &args[0],
                        &args[0].at,
                    ));
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
        if let Some(ty) = self.place_type(base).map(classes::unbox)
            && classes::inherent_receiver(&ty)
        {
            return self.class_method(
                base,
                classes::method_owner(&ty).unwrap(),
                name,
                None,
                args,
                ops,
            );
        }
        if self.place_type(base) == Some(Ty::File) {
            return self.file_method(base, name, args, ops);
        }
        if matches!(name, "__init__" | "__del__" | "__new__") {
            return Err(base.at.error("lifecycle methods cannot be called directly"));
        }
        if matches!(name, "reserve" | "append" | "add" | "insert") {
            if self.place_type(base).is_none() {
                let observed = self.observe_method_receiver(base, ops)?;
                if classes::inherent_receiver(&observed.0) {
                    return self.temporary_class_method(base, name, None, args, observed, ops);
                }
                return Err(base
                    .at
                    .error("mutation requires a named binding or class field"));
            }
            return self.fallible_mutation(base, name, args, ops);
        }
        if matches!(name, "into_values" | "into_keys") {
            let ty = self.value(base, ops)?;
            let class = classes::unbox(ty.clone());
            if classes::inherent_receiver(&class) {
                self.unbox_to(ty, &class, ops);
                return self.temporary_class_method(base, name, None, args, (class, vec![]), ops);
            }
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            if !matches!(ty, Ty::Dict(..)) {
                return Err(base
                    .at
                    .error(format!("{name} requires an owned dictionary")));
            }
            let operation = if name == "into_keys" {
                CollectionOp::TryKeys(ty)
            } else {
                CollectionOp::TryValues(ty)
            };
            let output = operation.signature().1;
            ops.push(Op::Collection(operation));
            return Ok(Some(output));
        }
        let collection = self.place_type(base).map(classes::unbox);
        if name == "pop" && matches!(collection, Some(Ty::Dict(..) | Ty::List(_))) {
            return self.collection_removal(base, args, ops);
        }
        if name == "get_mut" && matches!(collection, Some(Ty::Dict(..))) {
            return self.dict_get_mut(base, args, ops);
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
        if name == "keys" {
            let (reference, loan) = self.shared_argument(base, None, ops)?;
            let reference = self.unbox_reference(reference, ops);
            let Ty::Ref(owner, _) = reference else {
                unreachable!()
            };
            if classes::inherent_receiver(&owner) {
                ops.push(Op::ReadRef((*owner).clone()));
                return self.temporary_class_method(
                    base,
                    name,
                    None,
                    args,
                    ((*owner).clone(), vec![loan]),
                    ops,
                );
            }
            if !args.is_empty() {
                return Err(base.at.error("keys takes no arguments"));
            }
            let Ty::Dict(key, _) = &*owner else {
                return Err(base.at.error("keys requires a dictionary"));
            };
            let key = key.clone();
            self.loans[loan].precise = false;
            for op in ops.iter_mut() {
                if let Op::Loan(fact) = op
                    && fact.id == loan
                {
                    fact.precise = false;
                }
            }
            ops.push(Op::Collection(CollectionOp::KeyView((*owner).clone())));
            ops.push(Op::UseLoan(loan));
            return Ok(Some(Ty::KeyView(key)));
        }
        if let Some((receiver, root)) = self.hold_receiver_root(base, ops)? {
            let result = self.method(&receiver, name, args, ops);
            self.names.remove(&root);
            return result;
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
        if matches!(ty, Ty::KeyView(_))
            && !matches!(
                name,
                "to_set"
                    | "issubset"
                    | "issuperset"
                    | "isdisjoint"
                    | "union"
                    | "intersection"
                    | "difference"
                    | "symmetric_difference"
            )
        {
            return Err(base.at.error(format!("KeyView does not support `{name}`; use keys_snapshot() for owned list methods, or to_set() for an owned set")));
        }
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
        if ty.is_int() && matches!(name, "rotate_left" | "rotate_right") {
            let [count] = args else {
                return Err(base
                    .at
                    .error(format!("{name} takes one count of the receiver's type")));
            };
            let argument = self.expr_expected(count, Some(ty.clone()), ops)?;
            self.same(argument, Some(ty.clone()), &count.at)?;
            ops.push(Op::Integer(if name == "rotate_left" {
                IntegerOp::RotateLeft
            } else {
                IntegerOp::RotateRight
            }));
            Self::end_reads(loans, ops);
            return Ok(Some(ty));
        }
        if let Some(tag) = presence_tag(&ty, name) {
            let Ty::Enum(t) = ty else { unreachable!() };
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            ops.extend([
                Op::Enum(crate::sum::EnumOp::Tag(t)),
                Op::PushInt(Value::I64(tag)),
                Op::Eq,
            ]);
            Self::end_reads(loans, ops);
            return Ok(Some(Ty::Bool));
        }
        if let (Ty::Enum(t), "is_ok" | "is_err" | "is_some" | "is_none") = (&ty, name) {
            if t.is_result() {
                return Err(base.at.error(format!(
                    "{name} requires an Option, got {ty}; a Result provides `is_ok` and `is_err`"
                )));
            }
            if t.is_option() {
                return Err(base.at.error(format!(
                    "{name} requires a Result, got {ty}; an Option provides `is_some` and `is_none`"
                )));
            }
        }
        if ty == Ty::Str && name == "len" {
            return Err(base.at.error(
                "str has no `len` method; `len(text)` counts scalars and `text.byte_len()` counts UTF-8 bytes",
            ));
        }
        // `len` is the one method, so `bytes` satisfies std.bytes.ByteSource
        // as `Bytes` does; see protocols::builtin_methods.
        if ty == Ty::ByteStr && name == "len" {
            if !args.is_empty() {
                return Err(base.at.error("len takes no arguments"));
            }
            ops.push(Op::Collection(CollectionOp::Len(Ty::ByteStr)));
            Self::end_reads(loans, ops);
            return Ok(Some(Ty::I64));
        }
        if ty == Ty::ByteStr {
            return Err(base.at.error(format!(
                "bytes has no `{name}` method; `value.len()` counts its bytes, operations taking a `std.bytes.ByteSource` read it in place, and `std.bytes.from_bytes(value)?` copies it into a `Bytes` buffer"
            )));
        }
        if ty == Ty::Str && name == "byte_len" {
            if !args.is_empty() {
                return Err(base.at.error("byte_len takes no arguments"));
            }
            Self::push_query(CollectionOp::TextByteLen, ops);
            Self::end_reads(loans, ops);
            return Ok(Some(Ty::I64));
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
        if let Some(class) = crate::mutex::class(&ty).cloned() {
            return self.temporary_mutex_method(base, &class, name, args, &loans, ops);
        }
        if classes::inherent_receiver(&ty) {
            return self.temporary_class_method(base, name, None, args, (ty, loans), ops);
        }
        if matches!(ty, Ty::Executor | Ty::Future(_)) {
            return self.temporary_executor_method(base, name, args, (ty, loans), ops);
        }
        if ty == Ty::CancellationToken {
            return self.observed_control_method(base, name, args, (ty, loans), ops);
        }
        if let Ty::Shared(_) = &ty {
            return Err(base.at.error(
                "a temporary Shared owner has no place to borrow from; bind it to a name first",
            ));
        }
        if let Ty::Atomic(element) = &ty {
            // An owned temporary can only be unwrapped: an update would be lost.
            if name == "into_inner" && args.is_empty() && loans.is_empty() {
                let element = (**element).clone();
                ops.push(Op::Atomic(crate::atomic::AtomicOp::IntoInner(
                    element.clone(),
                )));
                return Ok(Some(element));
            }
            return Err(base.at.error(
                "atomic operations need a place to update; bind the atomic to a name first",
            ));
        }
        if let Some(need) = receiver_requirement(name, &ty) {
            return Err(base.at.error(format!("{name} requires {need}")));
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
            let operation = match name {
                "isascii" => CollectionOp::TextIsAscii,
                "isspace" => CollectionOp::TextIsSpace,
                "isdigit" => CollectionOp::TextIsDigit,
                _ => CollectionOp::TextIsAlpha,
            };
            Self::push_query(operation, ops);
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
        if let Ty::List(element) = &ty
            && matches!(name, "count" | "find" | "rfind")
        {
            if !(element.is_int() || element.is_float() || matches!(**element, Ty::Bool | Ty::Str))
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
            if name == "join" {
                let result = self.join_text(&args[0], ops)?;
                Self::end_reads(loans, ops);
                return Ok(Some(result));
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
                _ => unreachable!("join is lowered above"),
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
        if matches!(ty, Ty::KeyView(_)) && name == "to_set" {
            if !args.is_empty() {
                return Err(base.at.error("to_set takes no arguments"));
            }
            let operation = CollectionOp::KeyViewToSet(ty);
            let result = operation.signature().1;
            ops.push(Op::Collection(operation));
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
            if !matches!(ty, Ty::Set(_) | Ty::KeyView(_)) {
                return Err(base.at.error(format!("{name} requires a set receiver")));
            }
            if args.len() != 1 {
                return Err(base.at.error(format!("{name} requires one set argument")));
            }
            let argument = &args[0];
            let expected = if matches!(ty, Ty::KeyView(_)) {
                Ty::Set(Arc::new(ty.element().unwrap()))
            } else {
                ty.clone()
            };
            let (actual, reads) = self.observe_expected(argument, Some(expected), ops)?;
            if matches!(ty, Ty::KeyView(_)) {
                if !matches!(actual, Ty::Set(_) | Ty::KeyView(_))
                    || actual.element() != ty.element()
                {
                    return Err(argument
                        .at
                        .error("key view operations require &set[K] or &KeyView[K]"));
                }
                let operation = match name {
                    "issubset" => CollectionOp::key_set_relation(ty, actual, opcode::SET_IS_SUBSET),
                    "issuperset" => {
                        CollectionOp::key_set_relation(ty, actual, opcode::SET_IS_SUPERSET)
                    }
                    "isdisjoint" => {
                        CollectionOp::key_set_relation(ty, actual, opcode::SET_IS_DISJOINT)
                    }
                    "union" => CollectionOp::key_set_algebra(ty, actual, opcode::KEY_VIEW_UNION),
                    "intersection" => {
                        CollectionOp::key_set_algebra(ty, actual, opcode::KEY_VIEW_INTERSECTION)
                    }
                    "difference" => {
                        CollectionOp::key_set_algebra(ty, actual, opcode::KEY_VIEW_DIFFERENCE)
                    }
                    _ => CollectionOp::key_set_algebra(
                        ty,
                        actual,
                        opcode::KEY_VIEW_SYMMETRIC_DIFFERENCE,
                    ),
                };
                let result = operation.signature().1;
                ops.push(Op::Collection(operation));
                Self::end_reads(reads, ops);
                Self::end_reads(loans, ops);
                return Ok(Some(result));
            }
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
            return Err(self.value_demand_error(
                &base.at,
                base,
                format!("unsupported method `{name}`"),
                crate::ownership::Demand::Method(name.to_owned(), ty.clone()),
                true,
            ));
        };
        if name == "values" {
            if !args.is_empty() {
                return Err(base.at.error("values takes no arguments"));
            }
            return Err(base.at.error("values() is a borrowed loop/comprehension source; use values_snapshot() or into_values() for an owned list"));
        }
        if matches!(name, "keys_snapshot" | "values_snapshot") {
            if !args.is_empty() {
                return Err(base.at.error(format!("{name} takes no arguments")));
            }
            if name == "values_snapshot" && v.affine() && !loans.is_empty() {
                return Err(base.at.error(
                    "values_snapshot with owned payloads requires an owned temporary; use copy(dictionary)?.values_snapshot() for fallible duplication",
                ));
            }
            let operation = if name == "keys_snapshot" {
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

    /// `d.get_mut(k) -> Option[&mut V]`: the value for `k`, borrowed
    /// exclusively, or `Nothing`. The result holds an exclusive loan of the
    /// whole dictionary, as a reference-returning method's result would.
    fn dict_get_mut(&mut self, base: &Expr, args: &[Expr], ops: &mut Vec<Op>) -> Result<Type> {
        let [argument] = args else {
            return Err(base.at.error("get_mut requires one key argument"));
        };
        let ty = classes::unbox(self.place_type(base).expect("dictionary place"));
        let operation = CollectionOp::DictGetMut(ty);
        let (inputs, result) = operation.signature();
        let key = inputs[1].clone();
        // Observe the key before borrowing the receiver exclusively, as `pop`
        // does: a key read from the same dictionary stays valid.
        let (actual, reads) = self.observe_expected(argument, Some(key.clone()), ops)?;
        self.same(Some(actual), Some(key.clone()), &argument.at)?;
        let slot = self.slot(key, &argument.at)?;
        ops.push(Op::StoreLocal(slot));
        Self::end_reads(reads, ops);
        let (reference, loan) = self.borrow(base, true, ops)?;
        self.unbox_reference(reference, ops);
        ops.push(Op::MoveLocal(slot));
        ops.push(Op::Collection(operation));
        let id = self.new_loan(self.loans[loan].root, true, Some(loan), ops);
        self.loans[id].precise = false;
        if let Some(Op::Loan(fact)) = ops.last_mut() {
            *fact = self.loans[id].clone();
        }
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
                return Err(self.value_demand_error(
                    &base.at,
                    base,
                    format!("unsupported method `{name}` for {ty}"),
                    crate::ownership::Demand::Method(name.to_owned(), ty.clone()),
                    true,
                ));
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
        self.range_operands(args, &element, at, ops)?;
        ops.push(Op::Collection(CollectionOp::Range(element.clone())));
        Ok(Ty::Range(Arc::new(element)))
    }

    /// Push `start stop step` for `range(args)`, filling the defaults, and
    /// mark the call for parameter failures.
    fn range_operands(
        &mut self,
        args: &[Expr],
        element: &Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        if args.is_empty() || args.len() > 3 {
            return Err(at.error("range takes one to three arguments"));
        }
        if args.len() == 1 {
            ops.push(Op::PushInt(integer(&format!("0{element}"), false, at)?));
        }
        for (i, arg) in args.iter().enumerate() {
            if i == 2 {
                self.nonzero_step("range", arg, ops)?;
                continue;
            }
            let actual = self.expr_expected(arg, Some(element.clone()), ops)?;
            self.same(actual, Some(element.clone()), &arg.at)?;
        }
        if args.len() < 3 {
            ops.push(Op::PushInt(Value::I64(1)));
        }
        self.mark(at, ops);
        Ok(())
    }

    /// A range step is a `NonZero[i64]`, so a zero step cannot reach the
    /// range; the operand stack receives its `i64`.
    /// A `range` or slice step, which is a `NonZero[i64]`, as its integer.
    pub(super) fn nonzero_step(
        &mut self,
        what: &str,
        step: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let ty = self.nonzero_type(&Ty::I64, &step.at)?;
        let actual = self.expr_expected(step, Some(ty.clone()), ops)?;
        let actual = match actual {
            Some(Ty::Ref(inner, _)) if *inner == ty => {
                ops.push(Op::ReadRef(ty.clone()));
                Some(ty.clone())
            }
            other => other,
        };
        if actual.as_ref().is_some_and(Ty::is_int) {
            return Err(step.at.error(format!(
                "{what} step must be `NonZero[i64]`, not `{}`; write a nonzero literal, or convert a value with `NonZero.try_from(step)?`",
                actual.unwrap()
            )));
        }
        self.same(actual, Some(ty.clone()), &step.at)?;
        let Ty::Class(class) = ty else {
            unreachable!("NonZero is a class")
        };
        ops.push(Op::Class(crate::record::ClassOp::Field(class, 0)));
        Ok(())
    }

    /// Iterate `range(args)` as a counted loop, without a range value or a
    /// runtime call per element (issue #121). The arguments are evaluated
    /// once, before the loop, as for a range value.
    fn counted_range_iteration(
        &mut self,
        args: &[Expr],
        element: Ty,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let target = self.slot(element.clone(), at)?;
        let step = self.slot(Ty::I64, at)?;
        let remaining = self.slot(Ty::I64, at)?;
        self.range_operands(args, &element, at, ops)?;
        // Keep `start` as the first element while computing the count.
        ops.extend([
            Op::StoreLocal(step),
            Op::Swap,
            Op::Dup,
            Op::StoreLocal(target),
            Op::Swap,
            Op::LoadLocal(step),
            Op::RangeLength(element.clone()),
            Op::StoreLocal(remaining),
        ]);
        self.counted_iteration((target, step, remaining), element, reverse, at, ops)
    }

    /// Iterate a range value as the same counted loop as a direct `range`
    /// call (issue #371). Ranges are copyable inline values, so their first
    /// element, step, and validated count are read once before the loop.
    /// This covers ranges in locals, fields, and generator frames, which
    /// include a generator expression's outer source.
    fn range_value_iteration(
        &mut self,
        element: Ty,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let target = self.slot(element.clone(), at)?;
        let step = self.slot(Ty::I64, at)?;
        let remaining = self.slot(Ty::I64, at)?;
        ops.extend([
            Op::RangeParts(element.clone()),
            Op::StoreLocal(remaining),
            Op::StoreLocal(step),
            Op::StoreLocal(target),
        ]);
        self.counted_iteration((target, step, remaining), element, reverse, at, ops)
    }

    /// The loop over `remaining` elements from `target`, advancing by `step`.
    /// The loop variable is immutable, so its slot also holds the next
    /// element: each step adds the step with wrapping arithmetic, which the
    /// count keeps exact.
    fn counted_iteration(
        &mut self,
        (target, step, remaining): (u8, u8, u8),
        element: Ty,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        // The step at the element's width: modular addition of the
        // truncated step equals adding the full step and truncating.
        let advance = if element == Ty::I64 {
            step
        } else {
            let advance = self.slot(element.clone(), at)?;
            ops.extend([
                Op::LoadLocal(step),
                Op::Cast(element.clone()),
                Op::StoreLocal(advance),
            ]);
            advance
        };
        if reverse {
            // Start from `start + (count - 1) * step` and walk backwards.
            ops.extend([
                Op::LoadLocal(target),
                Op::LoadLocal(remaining),
                Op::PushInt(Value::I64(1)),
                Op::Integer(IntegerOp::WrappingSub),
                Op::Cast(element.clone()),
                Op::LoadLocal(advance),
                Op::Integer(IntegerOp::WrappingMul),
                Op::Integer(IntegerOp::WrappingAdd),
                Op::StoreLocal(target),
                Op::PushInt(integer(&format!("0{element}"), false, at)?),
                Op::LoadLocal(advance),
                Op::Integer(IntegerOp::WrappingSub),
                Op::StoreLocal(advance),
            ]);
        }
        Ok(Iteration {
            exact_len: Some(vec![Op::LoadLocal(remaining)]),
            cleanup: vec![],
            condition: vec![Op::LoadLocal(remaining), Op::PushInt(Value::I64(0)), Op::Gt],
            absent: vec![],
            body: vec![],
            step: vec![
                Op::LoadLocal(remaining),
                Op::PushInt(Value::I64(1)),
                Op::Integer(IntegerOp::WrappingSub),
                Op::StoreLocal(remaining),
                Op::LoadLocal(target),
                Op::LoadLocal(advance),
                Op::Integer(IntegerOp::WrappingAdd),
                Op::StoreLocal(target),
            ],
            target,
        })
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
            if let Some(ty @ Ty::Clist(_)) = self.place_type(&args[0]) {
                let (_, loan) = self.borrow(&args[0], false, ops)?;
                ops.push(Op::Collection(CollectionOp::Clist(
                    crate::clist::ClistOp::Len(ty),
                )));
                Self::end_reads(vec![loan], ops);
                return Ok(Ty::I64);
            }
            if let Some(ty @ Ty::List(_)) = self.place_type(&args[0]) {
                let (_, loan) = self.borrow(&args[0], false, ops)?;
                ops.push(Op::Collection(CollectionOp::ListLenRef(ty)));
                Self::end_reads(vec![loan], ops);
                return Ok(Ty::I64);
            }
            let (ty, loans) = self.observe(&args[0], ops)?;
            if let Ty::View(..) = ty {
                ops.push(Op::Collection(CollectionOp::ViewLen(ty)));
                Self::end_reads(loans, ops);
                return Ok(Ty::I64);
            }
            if ty.element().is_none() || ty.restricted_storage() && !matches!(ty, Ty::KeyView(_)) {
                return Err(at.error("len requires an iterable"));
            }
            Self::push_query(CollectionOp::Len(ty), ops);
            Self::end_reads(loans, ops);
            return Ok(Ty::I64);
        }
        if self.is_view(&args[0]) {
            return Err(args[0].at.error(format!(
                "{name}() does not take a slice view; copy the elements with a copying slice, `xs[a:b]?`"
            )));
        }
        let ty = self.value(&args[0], ops)?;
        if ty.key_view() {
            return Err(at.error(KEY_VIEW_COLLECTION));
        }
        let element = ty
            .element()
            .ok_or_else(|| at.error("collection constructor requires an iterable"))?;
        let out = match name {
            "list" => Ty::List(Arc::new(element)),
            "set" if element.hashable() => Ty::Set(Arc::new(element)),
            "dict" if matches!(ty, Ty::Dict(_, _)) => {
                let Ty::Enum(result) = crate::sum::result(ty, crate::sum::alloc_error()) else {
                    unreachable!()
                };
                ops.push(Op::Enum(crate::sum::EnumOp::New(result.clone(), 0)));
                return Ok(Ty::Enum(result));
            }
            "dict" => return Err(at.error("dict conversion requires a dictionary")),
            _ => return Err(at.error("set elements must be hashable")),
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
        if matches!(ty, Ty::Clist(_)) {
            if !args.is_empty() {
                return Err(
                    at.error("clist construction takes no arguments; use reserve for capacity")
                );
            }
            self.check_clist_access(&ty, at)?;
            ops.push(Op::Collection(CollectionOp::Clist(
                crate::clist::ClistOp::New(ty.clone()),
            )));
            return Ok(ty);
        }
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
                return Err(casts::operand_error(
                    "numeric casts require a number",
                    &source,
                    Ty::is_numeric,
                    &args[0],
                    at,
                ));
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
        if source.key_view() {
            return Err(at.error(KEY_VIEW_COLLECTION));
        }
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
        let start = self.locals.len();
        let mut iteration = self.iteration_on_stack(source, at, ops)?;
        // Preserve the existing collection-constructor allocation schedule.
        // Pipelines opt into their known-length reservation at their consumer.
        iteration.exact_len = None;
        let result = self.collect_iteration(iteration, target_ty, None, at, ops)?;
        self.cleanup(start, ops);
        Ok(result)
    }

    pub(super) fn collect_iteration(
        &mut self,
        iteration: Iteration,
        target_ty: Ty,
        item_error: Type,
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
            cleanup,
            exact_len,
            ..
        } = iteration;
        let alloc_error = crate::sum::alloc_error();
        let error = item_error.as_ref().map_or_else(
            || alloc_error.clone(),
            |item| crate::sum::anonymous(vec![item.clone(), alloc_error.clone()]),
        );
        let allocation_output = crate::sum::result(target_ty.clone(), alloc_error.clone());
        let output = crate::sum::result(target_ty.clone(), error.clone());
        let Ty::Enum(result_type) = &output else {
            unreachable!()
        };
        let mutation = crate::sum::allocation_result();
        let Ty::Enum(mutation_type) = &mutation else {
            unreachable!()
        };
        let result = self.slot(output.clone(), at)?;
        let pending = self.slot(mutation.clone(), at)?;
        ops.extend(exact_len.unwrap_or_else(|| vec![Op::PushInt(Value::I64(0))]));
        ops.push(Op::Collection(CollectionOp::TryNew(target_ty.clone())));
        if allocation_output != output {
            ops.push(Op::Enum(EnumOp::Widen(
                Box::new(allocation_output),
                Box::new(output.clone()),
            )));
        }
        ops.extend([
            Op::StoreLocal(result),
            Op::LoadLocal(result),
            Op::Enum(EnumOp::Tag(result_type.clone())),
            Op::PushInt(Value::I64(0)),
            Op::Eq,
        ]);
        let fallible = item_error.is_some();
        let target = if let Some(item_error) = item_error {
            let item_ty = self.locals[target as usize - self.parameters].clone();
            let Ty::Enum(item_result) = item_ty else {
                unreachable!()
            };
            let item_type = match &target_ty {
                Ty::Dict(k, v) => crate::sum::tuple(vec![(**k).clone(), (**v).clone()]),
                _ => target_ty.element().unwrap(),
            };
            let element = self.slot(item_type, at)?;
            let mut failed = vec![
                Op::MoveLocal(target),
                Op::Enum(EnumOp::Take(item_result.clone(), 1, 0)),
            ];
            if item_error != error {
                failed.push(Op::Enum(EnumOp::Widen(
                    Box::new(item_error),
                    Box::new(error.clone()),
                )));
            }
            failed.extend([
                Op::Enum(EnumOp::New(result_type.clone(), 1)),
                Op::StoreLocal(result),
                Op::Break,
            ]);
            body.extend([
                Op::LoadLocal(target),
                Op::Enum(EnumOp::Tag(item_result.clone())),
                Op::PushInt(Value::I64(1)),
                Op::Eq,
                branch(failed, vec![]),
                Op::MoveLocal(target),
                Op::Enum(EnumOp::Take(item_result, 0, 0)),
                Op::StoreLocal(element),
            ]);
            element
        } else {
            target
        };
        let mut failed = vec![
            Op::MoveLocal(pending),
            Op::Enum(EnumOp::Take(mutation_type.clone(), 1, 0)),
        ];
        if alloc_error != error {
            failed.push(Op::Enum(EnumOp::Widen(
                Box::new(alloc_error),
                Box::new(error),
            )));
        }
        failed.extend([
            Op::Enum(EnumOp::New(result_type.clone(), 1)),
            Op::StoreLocal(result),
            Op::Break,
        ]);
        body.extend([
            Op::LoadLocal(result),
            Op::Enum(EnumOp::Field(result_type.clone(), 0, 0)),
            if self.locals[target as usize - self.parameters].affine() {
                Op::MoveLocal(target)
            } else {
                Op::LoadLocal(target)
            },
        ]);
        if matches!(target_ty, Ty::Dict(..)) {
            let Ty::Enum(tuple) = &self.locals[target as usize - self.parameters] else {
                unreachable!()
            };
            body.push(Op::Split(tuple.clone(), 0));
        }
        body.extend([
            Op::Collection(CollectionOp::TryInsert(target_ty)),
            Op::StoreLocal(pending),
            Op::LoadLocal(pending),
            Op::Enum(EnumOp::Tag(mutation_type.clone())),
            Op::PushInt(Value::I64(1)),
            Op::Eq,
            branch(failed, vec![]),
            Op::DropLocal(pending),
        ]);
        if fallible {
            body.push(Op::DropLocal(target));
        }
        body.extend(step);
        ops.push(branch(
            vec![Op::Loop {
                condition: condition.into(),
                body: body.into(),
            }],
            vec![],
        ));
        ops.push(Op::LoadLocal(result));
        ops.extend(cleanup);
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

pub(super) fn dictionary_items(iterable: &Expr) -> Option<(&Expr, bool)> {
    let mut e = ungroup(iterable);
    let mut mutable = false;
    if let Expression::Unary(op, base) = &e.kind {
        if op != "&" && op != "&mut" {
            return None;
        }
        mutable = op == "&mut";
        e = ungroup(base);
    }
    let Expression::Method(base, method, args, _) = &e.kind else {
        return None;
    };
    if !matches!(method.as_str(), "items" | "values") || !args.is_empty() {
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

pub(super) fn decrement(index: u8, ops: &mut Vec<Op>) {
    ops.extend([
        Op::LoadLocal(index),
        Op::PushInt(Value::I64(1)),
        Op::Sub,
        Op::StoreLocal(index),
    ]);
}

/// Removes a just-emitted `ElementRef` into a list of scalars and returns the
/// list type. Its operands (list reference, index) stay on the stack for the
/// narrow `ListScalarGetRef`/`ListScalarSetRef` helpers, which trap at the
/// same mark with the same diagnostic (issue #122).
pub(super) fn scalar_element_ref(ops: &mut Vec<Op>) -> Option<Ty> {
    let Some(Op::Collection(CollectionOp::ElementRef(list @ Ty::List(element), _))) = ops.last()
    else {
        return None;
    };
    if !(element.is_numeric() || **element == Ty::Bool) {
        return None;
    }
    let list = list.clone();
    ops.pop();
    Some(list)
}

pub(super) fn increment(index: u8, ops: &mut Vec<Op>) {
    ops.extend([
        Op::LoadLocal(index),
        Op::PushInt(Value::I64(1)),
        Op::Add,
        Op::StoreLocal(index),
    ]);
}

/// The variant tag a `Result`/`Option` predicate named `name` tests for.
/// `Result` stores `Ok` first; `Option` stores `Nothing` first.
fn presence_tag(ty: &Ty, name: &str) -> Option<i64> {
    let Ty::Enum(t) = ty else { return None };
    match name {
        "is_ok" if t.is_result() => Some(0),
        "is_err" if t.is_result() => Some(1),
        "is_none" if t.is_option() => Some(0),
        "is_some" if t.is_option() => Some(1),
        _ => None,
    }
}

/// The receiver a mutating collection method needs, for the error reported
/// when it is called on a temporary or immutable value.
fn receiver_requirement(name: &str, ty: &Ty) -> Option<&'static str> {
    Some(match name {
        "pop" => "a mutable list or dictionary binding or class field",
        "discard" | "intersection_update" | "difference_update" => {
            "a mutable set binding or class field"
        }
        "reverse" | "extend" => "a mutable list binding or class field",
        // Of the collections, only lists have these methods.
        "sort" | "sort_by_key" | "swap" if matches!(ty, Ty::List(_)) => {
            "a mutable list binding or class field"
        }
        "update" => "a mutable dictionary or set binding or class field",
        "get_mut" if matches!(ty, Ty::Dict(..)) => "a mutable dictionary binding or class field",
        "clear" => "a mutable list, dictionary, or set binding or class field",
        _ => return None,
    })
}
