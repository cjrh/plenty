//! Source places and explicit loans with statically disjoint field projections.
use super::*;

/// Whether `ty` may be the success payload of an `Option` or `Result`: a
/// reference to a value that holds no loan of its own, or a further
/// reference-carrying `Option`/`Result`. A reference to a reference-carrying
/// value would be a reference to a reference, which Plenty does not have.
pub(super) fn carriable(ty: &Ty) -> bool {
    match ty {
        Ty::Ref(inner, _) => !inner.contains_reference() && inner.storage_barrier().is_none(),
        ty => ty.carried_reference().is_some(),
    }
}

/// Whether `e` builds a reference-carrying value that holds no reference:
/// `Nothing`, an `Err`, or `Some`/`Ok` around one. Such a value borrows
/// nothing, so any function returning an `Option[&T]` may return it.
pub(super) fn loan_free(e: &Expr) -> bool {
    match &ungroup(e).kind {
        Expression::Name(name) => name == "Nothing",
        Expression::Call(name, args) if name == "Err" => args.len() == 1,
        Expression::Call(name, args) if name == "Some" || name == "Ok" => {
            matches!(args.as_slice(), [arg] if loan_free(arg))
        }
        _ => false,
    }
}

/// Summarize only a single direct returned projection. More complicated bodies
/// keep the conservative whole-argument footprint until flow summaries exist.
pub(super) fn returned_fields(body: &[Stmt], sig: &FnSig) -> Option<Vec<usize>> {
    if !matches!(sig.outputs.first(), Some(Ty::Ref(..))) {
        return None;
    }
    let (root, ty) = sig.inputs.iter().find(|(_, t)| matches!(t, Ty::Ref(..)))?;
    let [stmt] = body else {
        return None;
    };
    let expression = match &stmt.kind {
        Statement::Expr(e) | Statement::Return(Some(e)) => ungroup(e),
        _ => return None,
    };
    let place = match &expression.kind {
        Expression::Unary(op, e) if op == "&" || op == "&mut" => ungroup(e),
        Expression::Name(n) if n == root => return Some(vec![]),
        _ => return None,
    };
    fn path(e: &Expr, root: &str, names: &mut Vec<String>) -> Option<()> {
        match &ungroup(e).kind {
            Expression::Name(n) if n == root => Some(()),
            Expression::Member(base, name, _) => {
                path(base, root, names)?;
                names.push(name.clone());
                Some(())
            }
            _ => None,
        }
    }
    let mut names = Vec::new();
    path(place, root, &mut names)?;
    let Ty::Ref(inner, _) = ty else {
        unreachable!()
    };
    let mut ty = (**inner).clone();
    let mut fields = Vec::new();
    for name in names {
        let Ty::Class(class) = ty else {
            return None;
        };
        let index = class.get().fields.iter().position(|(n, _)| *n == name)?;
        fields.push(index);
        ty = class.get().fields[index].1.clone();
    }
    Some(fields)
}

/// The loan path step for a list element at a nonnegative integer literal.
fn literal_index(collection: &Ty, index: &Expr) -> Option<usize> {
    let Ty::List(_) = collection else {
        return None;
    };
    let Expression::Number(n) = &ungroup(index).kind else {
        return None;
    };
    let digits = n.strip_suffix("i64").unwrap_or(n);
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse::<i64>().ok()?;
    Some(crate::ownership::ELEMENT | value as usize)
}

impl Lower<'_> {
    /// Keep a method's already-evaluated place stable while arguments run.
    /// Runtime reference construction stays in source order; only exclusivity
    /// starts later, so argument reads may finish before the call.
    pub(super) fn reserve_receiver(&mut self, loan: usize, ops: &mut [Op]) {
        self.loans[loan].mutable = false;
        for op in ops.iter_mut().rev() {
            if let Op::Loan(fact) = op
                && fact.id == loan
            {
                fact.mutable = false;
                return;
            }
        }
        unreachable!("receiver loan was just emitted")
    }

    pub(super) fn activate_receiver(&mut self, loan: usize, ops: &mut Vec<Op>) -> usize {
        self.new_loan(self.loans[loan].root, true, Some(loan), ops)
    }

    /// Signature-directed shared arguments lend places, forward references, or
    /// materialize an owner until the end of the containing full expression.
    pub(super) fn shared_argument(
        &mut self,
        e: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        if self.place_type(e).is_some() || self.is_function_place(e) {
            return self.borrow(e, false, ops);
        }
        // A unit argument still needs an addressable temporary to lend.
        let ty = match self.expr_expected(e, expected, ops)? {
            Some(ty) => ty,
            None => {
                ops.push(Op::PushUnit);
                Ty::Unit
            }
        };
        if let Ty::Ref(inner, _) = ty {
            let parent = self.reference_origin(e, ops)?;
            let loan = self.new_loan(self.loans[parent].root, false, Some(parent), ops);
            let ty = Ty::Ref(inner, false);
            ops.push(Op::Reborrow(ty.clone()));
            return Ok((ty, loan));
        }
        let slot = self.slot(ty.clone(), &e.at)?;
        ops.push(Op::StoreLocal(slot));
        self.expression_temps.push(slot);
        let loan = self.new_loan(slot, false, None, ops);
        ops.push(Op::BorrowLocal(slot, false));
        Ok((Ty::Ref(Arc::new(ty), false), loan))
    }

    pub(super) fn check_stored_reference(&self, loan: usize, at: &Token) -> Result<()> {
        if self.expression_temps.contains(&self.loans[loan].root) {
            return Err(
                at.error("a reference to a temporary cannot be stored beyond its full expression")
            );
        }
        Ok(())
    }

    /// Lowers the value of a binding, returning the loan it holds when it is a
    /// reference. A named reference is reborrowed, as a call argument is, so
    /// each reference binding has a loan of its own.
    pub(super) fn binding_value(
        &mut self,
        value: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, Option<usize>)> {
        if self.is_pipeline(value) {
            let outer = std::mem::replace(&mut self.binding_pipeline, true);
            let plan = self.pipeline(value, ops);
            self.binding_pipeline = outer;
            let plan = plan?;
            let item = Arc::new(plan.item.ty.clone());
            let id = self.pipelines.len();
            self.pipelines.push(plan);
            ops.push(Op::PipelineToken(item.clone(), id));
            return Ok((Ty::Iter(item, id), None));
        }
        let place = match &ungroup(value).kind {
            Expression::Unary(op, place) if op == "&" || op == "&mut" => {
                Some((&**place, op == "&mut"))
            }
            Expression::Name(name) if !self.captures.contains(name) => match self.names.get(name) {
                Some(Local {
                    ty: Ty::Ref(_, mutable),
                    ..
                }) => Some((value, *mutable)),
                _ => None,
            },
            _ => None,
        };
        if let Some((place, mutable)) = place {
            let (ty, loan) = self.borrow(place, mutable, ops)?;
            let ty = match &expected {
                Some(expected) => self.unbox_to(ty, expected, ops),
                None => ty,
            };
            return Ok((ty, Some(loan)));
        }
        let ty = self
            .expr_expected(value, expected, ops)?
            .ok_or_else(|| value.at.error("expected a value, got ()"))?;
        if let Ty::KeyView(_) = ty {
            let parent = self.reference_origin(value, ops)?;
            let loan = self.new_loan(self.loans[parent].root, false, Some(parent), ops);
            return Ok((ty, Some(loan)));
        }
        if ty.carried_reference().is_some() {
            if loan_free(value) {
                return Err(value.at.error(format!(
                    "a binding of type {ty} must hold a borrow, and this value holds none; bind a lookup result or `Some(&place)`"
                )));
            }
            let parent = self.reference_origin(value, ops)?;
            // A copied binding gets a loan of its own, as a reborrow does.
            let loan = if matches!(&ungroup(value).kind, Expression::Name(_)) {
                let mutable = self.loans[parent].mutable;
                self.new_loan(self.loans[parent].root, mutable, Some(parent), ops)
            } else {
                parent
            };
            return Ok((ty, Some(loan)));
        }
        if !matches!(ty, Ty::Ref(..)) {
            return Ok((ty, None));
        }
        if !matches!(
            &ungroup(value).kind,
            Expression::Call(..)
                | Expression::Invoke(..)
                | Expression::GenericCall(..)
                | Expression::Method(..)
                | Expression::GenericMethod(..)
                | Expression::Try(..)
        ) {
            return Err(value.at.error(
                "reference bindings require a named reference, a direct borrow, or a reference-returning call",
            ));
        }
        let loan = self.reference_origin(value, ops)?;
        Ok((ty, Some(loan)))
    }

    /// Records the loan a new reference binding holds. A `mut` binding can
    /// later point anywhere inside its first target, so its loan and every
    /// loan derived from it cover that whole target instead of a field path.
    pub(super) fn bind_reference(&mut self, slot: u8, loan: usize, mutable: bool, ops: &mut [Op]) {
        self.loans[loan].binding = Some(slot);
        if mutable {
            // A list element target widens to the whole list, so the binding
            // can later be retargeted to any element of it.
            let fields = &mut self.loans[loan].fields;
            if let Some(element) = fields
                .iter()
                .position(|step| step & crate::ownership::ELEMENT != 0)
            {
                fields.truncate(element);
            }
            self.loans[loan].precise = false;
        }
        if let Some(Op::Loan(fact)) = ops
            .iter_mut()
            .rev()
            .find(|op| matches!(op, Op::Loan(l) if l.id == loan))
        {
            *fact = self.loans[loan].clone();
        }
        self.reference_locals.insert(slot, loan);
    }

    /// A reassigned reference binding keeps the loan it was declared with,
    /// which covers the place the binding first borrowed. The new reference
    /// must lie within that place. One borrowed through the binding extends
    /// its loan. Any other borrow of that place ends the held loan here; the
    /// new loan was already checked against every loan live at its creation.
    /// Returns whether the held loan ends.
    pub(super) fn check_retargeted_reference(
        &self,
        name: &str,
        slot: u8,
        loan: usize,
        at: &Token,
    ) -> Result<bool> {
        let held = self.reference_locals[&slot];
        // Only a loan widened by `bind_reference` protects every later target.
        if self.loans[held].precise {
            return Err(at.error(format!(
                "reference `{name}` was not declared as a `mut` reference binding and cannot be reassigned"
            )));
        }
        let mut ancestor = Some(loan);
        while let Some(id) = ancestor {
            if id == held {
                return Ok(false);
            }
            ancestor = self.loans[id].parent;
        }
        let (old, new) = (&self.loans[held], &self.loans[loan]);
        if new.root != old.root {
            return Err(at.error(format!(
                "reference binding `{name}` can be reassigned only to a reference into the owner it first borrowed"
            )));
        }
        if !new.fields.starts_with(&old.fields) {
            return Err(at.error(format!(
                "reference binding `{name}` can be reassigned only to a reference within the place it first borrowed"
            )));
        }
        Ok(true)
    }

    /// Gives a just-lowered success payload `arg` of type `ty` its own loan
    /// when it carries a reference, so the enclosing `Some(...)` or `Ok(...)`
    /// holds the loan as a reference binding would. The new loan is the last
    /// one emitted, which is where [`Self::reference_origin`] finds it.
    pub(super) fn carried_payload(&mut self, arg: &Expr, ty: &Ty, ops: &mut Vec<Op>) -> Result<()> {
        if !ty.carries_loan() || loan_free(arg) {
            return Ok(());
        }
        let parent = self.reference_origin(arg, ops)?;
        self.new_loan(
            self.loans[parent].root,
            ty.carries_mutable_loan(),
            Some(parent),
            ops,
        );
        Ok(())
    }

    pub(super) fn reference_origin(&self, e: &Expr, ops: &[Op]) -> Result<usize> {
        if let Expression::Name(name) = &ungroup(e).kind
            && let Some(local) = self.names.get(name)
            && let Some(id) = self.reference_locals.get(&local.slot)
        {
            return Ok(*id);
        }
        if (matches!(&ungroup(e).kind, Expression::Unary(op, _) if op == "&" || op == "&mut")
            || matches!(
                &ungroup(e).kind,
                Expression::Call(..)
                    | Expression::GenericCall(..)
                    | Expression::Method(..)
                    | Expression::GenericMethod(..)
                    | Expression::Invoke(..)
                    | Expression::Try(..)
            ))
            && let Some(id) = ops.iter().rev().find_map(|op| {
                if let Op::Loan(l) = op {
                    Some(l.id)
                } else {
                    None
                }
            })
        {
            return Ok(id);
        }
        Err(e.at.error("reference result requires a named reference, direct borrow, or reference-returning call"))
    }

    pub(super) fn check_return_reference(&self, e: &Expr, ops: &[Op]) -> Result<usize> {
        let id = self.reference_origin(e, ops)?;
        let mut ancestor = Some(id);
        while let Some(loan) = ancestor {
            if self.lent_item_loans.contains(&loan) {
                return Err(e.at.error(
                    "an item of a lent pipeline cannot be returned: it borrows the caller's source only for this call; return an owned copy",
                ));
            }
            ancestor = self.loans[loan].parent;
        }
        if Some(self.loans[id].root) != self.return_origin {
            return Err(e
                .at
                .error("returned reference must originate from the first reference parameter"));
        }
        Ok(id)
    }

    pub(super) fn call_reference_result(
        &mut self,
        name: &str,
        sig: &FnSig,
        loans: &[usize],
        ops: &mut Vec<Op>,
    ) {
        if let Some(output) = sig.outputs.first().filter(|ty| ty.carries_loan()) {
            let parent = loans[0];
            let mutable = output.carries_mutable_loan();
            let id = self.new_loan(self.loans[parent].root, mutable, Some(parent), ops);
            if let Some(fields) = self
                .sigs
                .returned_fields(name)
                .filter(|_| self.loans[id].precise)
            {
                self.loans[id].fields.extend(fields);
            } else {
                self.loans[id].precise = false;
            }
            if let Some(Op::Loan(fact)) = ops.last_mut() {
                *fact = self.loans[id].clone();
            }
        }
    }

    pub(super) fn new_loan(
        &mut self,
        root: u8,
        mutable: bool,
        parent: Option<usize>,
        ops: &mut Vec<Op>,
    ) -> usize {
        let id = self.loans.len();
        let dependencies = if let Some(parent) = parent {
            self.loans[parent].dependencies.clone()
        } else {
            self.names
                .values()
                .find(|local| local.slot == root)
                .map(|local| &local.ty)
                .or_else(|| {
                    (root as usize)
                        .checked_sub(self.parameters)
                        .and_then(|i| self.locals.get(i))
                })
                .and_then(|ty| {
                    if let Ty::Closure(t) = ty {
                        self.closure_loans.get(&t.name).cloned()
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        };
        let loan = crate::ownership::Loan {
            dependencies,
            id,
            root,
            fields: parent
                .map(|id| self.loans[id].fields.clone())
                .unwrap_or_default(),
            precise: parent.is_none_or(|id| self.loans[id].precise),
            mutable,
            parent,
            through: parent.and_then(|id| self.loans[id].binding.or(self.loans[id].through)),
            binding: None,
        };
        self.loans.push(loan.clone());
        ops.push(Op::Loan(loan));
        id
    }
    pub(super) fn end_reads(loans: Vec<usize>, ops: &mut Vec<Op>) {
        ops.extend(loans.into_iter().map(Op::UseLoan));
    }
    /// Push `operation`, asking a borrowed [`Query`] instead when its operand
    /// is a copy just read through a reference (issue #364). The reference
    /// then reaches the query, and the loans that ended after the read move
    /// past it, so the query observes the value in place: no retain or release.
    ///
    /// [`Query`]: crate::collection::Query
    pub(super) fn push_query(operation: CollectionOp, ops: &mut Vec<Op>) {
        if let Some(query) = crate::collection::Query::of(&operation) {
            let read = ops.len()
                - ops
                    .iter()
                    .rev()
                    .take_while(|op| matches!(op, Op::UseLoan(_)))
                    .count();
            if read > 0 && ops[read - 1] == Op::ReadRef(query.operand()) {
                let loans = ops.split_off(read);
                ops.pop();
                // The reference may be writable; the query only reads it.
                ops.push(Op::Reborrow(Ty::Ref(Arc::new(query.operand()), false)));
                ops.push(Op::Collection(CollectionOp::QueryRef(query)));
                ops.extend(loans);
                return;
            }
        }
        ops.push(Op::Collection(operation));
    }
    /// Reading a reference-carrying local uses the loan it holds.
    fn use_carried(&self, local: &Local, ops: &mut Vec<Op>) {
        if local.ty.carried_reference().is_some()
            && let Some(&loan) = self.reference_locals.get(&local.slot)
        {
            ops.push(Op::UseLoan(loan));
        }
    }

    fn named_place(&self, e: &Expr) -> Result<Local> {
        match &e.kind {
            Expression::Group(e) => self.named_place(e),
            Expression::Name(name) => {
                let local = self.names.get(name).cloned().ok_or_else(|| e.at.error(self.unknown_binding(name)))?;
                if local.ty == Ty::Unit {
                    return Err(e.at.error("expected a value, got ()"));
                }
                if matches!(local.ty, Ty::Task(_)) {
                    return Err(e.at.error("scoped tasks cannot be borrowed or stored; use task.join()"));
                }
                Ok(local)
            }
            _ => Err(e.at.error("borrowing requires a named binding; element and temporary references are not supported yet")),
        }
    }
    pub(super) fn borrow(
        &mut self,
        e: &Expr,
        mutable: bool,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        if self.clist_projection(e)?.is_some() {
            return Err(e.at.error("packed clist fields cannot be borrowed; read the scalar value or use column iteration"));
        }
        if self.is_pipeline(e) {
            return Err(e.at.error(
                "an iterator pipeline can only be lent, as `&mut p` to a `&mut Iter[T]` parameter",
            ));
        }
        self.ide_member(e);
        self.borrow_with_indices(e, mutable, &mut None, ops)
    }

    /// Save index values before taking an assignment loan. No element address
    /// survives evaluation of user code that could resize its enclosing owner.
    pub(super) fn assignment_indices(
        &mut self,
        e: &Expr,
        slots: &mut Vec<u8>,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        self.access_indices(e, slots, ops, true)
    }

    /// Save computed keys without borrowing their owners. Ordinary indexed
    /// assignment has additional collection-only restrictions; clist owner
    /// projections can pass through tuples and check mutability when borrowed.
    pub(super) fn access_indices(
        &mut self,
        e: &Expr,
        slots: &mut Vec<u8>,
        ops: &mut Vec<Op>,
        mutating: bool,
    ) -> Result<()> {
        match &ungroup(e).kind {
            Expression::Member(base, _, _) => self.access_indices(base, slots, ops, mutating),
            Expression::Unary(op, base) if op == "*" => {
                self.access_indices(base, slots, ops, mutating)
            }
            Expression::Index(base, index) => {
                if mutating && matches!(self.place_type(base), Some(Ty::Shared(_))) {
                    return Err(e.at.error(shared::MUTATION));
                }
                self.access_indices(base, slots, ops, mutating)?;
                let container = self.place_type(base).map(|ty| match ty {
                    Ty::Shared(content) => (*content).clone(),
                    other => other,
                });
                let key = match container {
                    // Tuple indices are compile-time projections, not consumed
                    // runtime keys. borrow_with_indices validates the literal.
                    Some(Ty::Enum(t)) if !mutating && t.tuple() => return Ok(()),
                    Some(Ty::List(_)) => Ty::I64,
                    Some(Ty::Dict(key, _)) => (*key).clone(),
                    _ => return Err(e.at.error("indexed assignment requires list or dict")),
                };
                let actual = self.expr_expected(index, Some(key.clone()), ops)?;
                self.same(actual, Some(key.clone()), &index.at)?;
                let slot = self.slot(key, &index.at)?;
                ops.push(Op::StoreLocal(slot));
                slots.push(slot);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn borrow_with_indices(
        &mut self,
        e: &Expr,
        mutable: bool,
        indices: &mut Option<std::slice::Iter<'_, u8>>,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        if let Expression::Index(base, index) = &ungroup(e).kind {
            if matches!(self.place_type(base), Some(Ty::Shared(_))) {
                let element = Expr {
                    at: e.at.clone(),
                    kind: Expression::Index(Box::new(shared::content(base)), index.clone()),
                };
                return self.borrow_with_indices(&element, mutable, indices, ops);
            }
            let collection = self.place_type(base).ok_or_else(|| {
                e.at.error("element borrowing requires a named collection or field")
            })?;
            if let Ty::Enum(t) = &collection
                && t.tuple()
            {
                let Expression::Number(n) = &ungroup(index).kind else {
                    return Err(index
                        .at
                        .error("tuple index must be a nonnegative integer literal"));
                };
                let index = n
                    .parse::<usize>()
                    .map_err(|_| index.at.error("invalid tuple index"))?;
                let field = t.get().variants[0]
                    .fields
                    .get(index)
                    .cloned()
                    .ok_or_else(|| e.at.error("tuple index out of bounds"))?;
                let (_, loan) = self.borrow_with_indices(base, mutable, indices, ops)?;
                if self.loans[loan].precise {
                    self.loans[loan].fields.push(index);
                    if let Some(Op::Loan(fact)) = ops
                        .iter_mut()
                        .rev()
                        .find(|op| matches!(op, Op::Loan(l) if l.id == loan))
                    {
                        *fact = self.loans[loan].clone();
                    }
                }
                ops.push(Op::Enum(crate::sum::EnumOp::FieldRef(
                    t.clone(),
                    0,
                    index,
                    mutable,
                )));
                return Ok((Ty::Ref(Arc::new(field), mutable), loan));
            }
            let key = match &collection {
                Ty::List(_) => Ty::I64,
                Ty::Dict(key, _) => (**key).clone(),
                _ => {
                    return Err(e
                        .at
                        .error("element references require a list or dictionary"));
                }
            };
            let (_, loan) = self.borrow_with_indices(base, mutable, indices, ops)?;
            // Only distinct literal list indices are proven disjoint. Other
            // element loans, and descendant projections, keep the whole
            // collection's footprint. Dictionary assignment may insert, so
            // dictionary keys never refine a loan.
            match literal_index(&collection, index) {
                Some(step) if self.loans[loan].precise => self.loans[loan].fields.push(step),
                _ => self.loans[loan].precise = false,
            }
            for op in ops.iter_mut().rev() {
                if let Op::Loan(fact) = op
                    && fact.id == loan
                {
                    *fact = self.loans[loan].clone();
                    break;
                }
            }
            if let Some(indices) = indices {
                let slot = *indices.next().expect("prepared assignment index");
                ops.push(Op::MoveLocal(slot));
            } else {
                let actual = self.expr_expected(index, Some(key.clone()), ops)?;
                self.same(actual, Some(key), &index.at)?;
            }
            let operation = CollectionOp::ElementRef(collection, mutable);
            let output = operation.signature().1;
            self.mark(&e.at, ops);
            ops.push(Op::Collection(operation));
            return Ok((output, loan));
        }
        if let Expression::Unary(op, base) = &ungroup(e).kind
            && op == "*"
        {
            if mutable && matches!(self.place_type(base), Some(Ty::Shared(_))) {
                return Err(e.at.error(shared::MUTATION));
            }
            let reference_binding = self.reference_binding(base);
            let (reference, loan) = self.borrow_with_indices(base, mutable, indices, ops)?;
            let Ty::Ref(inner, _) = &reference else {
                unreachable!()
            };
            return match &**inner {
                Ty::Box(content) => {
                    ops.push(Op::Box(crate::boxed::BoxOp::Ref(
                        (**content).clone(),
                        mutable,
                    )));
                    Ok((Ty::Ref(content.clone(), mutable), loan))
                }
                Ty::Shared(content) => {
                    ops.push(Op::Box(crate::boxed::BoxOp::SharedRef((**content).clone())));
                    Ok((Ty::Ref(content.clone(), false), loan))
                }
                _ if reference_binding => Ok((reference, loan)),
                _ => Err(e
                    .at
                    .error("`*` requires a reference, a Box, or a Shared handle")),
            };
        }
        if let Expression::Member(base, name, _) = &ungroup(e).kind {
            if matches!(self.place_type(base), Some(Ty::Shared(_))) {
                return self.borrow_with_indices(
                    &Expr {
                        at: e.at.clone(),
                        kind: Expression::Member(
                            Box::new(shared::content(base)),
                            name.clone(),
                            Pos::default(),
                        ),
                    },
                    mutable,
                    indices,
                    ops,
                );
            }
            let (reference, loan) = self.borrow_with_indices(base, mutable, indices, ops)?;
            let reference = self.unbox_reference(reference, ops);
            let Ty::Ref(inner, _) = reference else {
                unreachable!()
            };
            let Ty::Class(class) = &*inner else {
                return Err(e.at.error("field borrowing requires a class"));
            };
            let index = classes::field_index(class, name, &e.at)?;
            modules::check_member(self.access, &class.name, name, &e.at)?;
            self.aliases.data.note_field(&class.name, name);
            if self.loans[loan].precise {
                self.loans[loan].fields.push(index);
            }
            // Recursive projections refine the one loan created at the root.
            // Update its emitted fact before any checker sees the final place.
            for op in ops.iter_mut().rev() {
                if let Op::Loan(fact) = op
                    && fact.id == loan
                {
                    *fact = self.loans[loan].clone();
                    break;
                }
            }
            let result = Ty::Ref(Arc::new(class.get().fields[index].1.clone()), mutable);
            ops.push(Op::Class(crate::record::ClassOp::FieldRef(
                class.clone(),
                index,
                mutable,
            )));
            return Ok((result, loan));
        }
        let local = if self.is_function_place(e) {
            if mutable {
                return Err(e.at.error("named function declarations are immutable; assign the function to a mut binding before borrowing it with &mut"));
            }
            self.function_place(e)?
        } else {
            self.named_place(e)?
        };
        self.mark(&e.at, ops);
        self.use_carried(&local, ops);
        if matches!(local.ty, Ty::KeyView(_)) {
            if mutable {
                return Err(e.at.error("dictionary key views are shared borrows"));
            }
            let parent = self.reference_locals[&local.slot];
            let loan = self.new_loan(self.loans[parent].root, false, Some(parent), ops);
            ops.push(Op::BorrowLocal(local.slot, false));
            return Ok((Ty::Ref(Arc::new(local.ty), false), loan));
        }
        let (ty, root, parent) = if let Ty::Ref(ty, writable) = &local.ty {
            if mutable && !writable {
                if self.bare_self_receiver
                    && local.slot == 0
                    && matches!(&ungroup(e).kind, Expression::Name(name) if name == "self")
                {
                    let hint = if matches!(&**ty, Ty::Class(class) if class.name.contains('[')) {
                        "declare an explicit mutable receiver (`self: &mut ...`) using the class's type parameters".into()
                    } else {
                        format!("declare `self: &mut {ty}` to mutate the receiver")
                    };
                    return Err(e.at.error(format!(
                        "cannot borrow shared reference as mutable; bare `self` is read-only in ordinary methods; {hint}"
                    )));
                }
                return Err(e.at.error("cannot borrow shared reference as mutable"));
            }
            let parent = *self
                .reference_locals
                .get(&local.slot)
                .ok_or_else(|| e.at.error("reference origin unavailable"))?;
            ((**ty).clone(), self.loans[parent].root, Some(parent))
        } else {
            if mutable && !local.mutable {
                return Err(e
                    .at
                    .error("immutable binding: mutable borrowing requires a mut binding"));
            }
            (local.ty.clone(), local.slot, None)
        };
        let id = self.new_loan(root, mutable, parent, ops);
        if parent.is_some() {
            ops.push(Op::LoadLocal(local.slot));
            ops.push(Op::Reborrow(Ty::Ref(Arc::new(ty.clone()), mutable)));
        } else {
            ops.push(Op::BorrowLocal(local.slot, mutable));
        }
        Ok((Ty::Ref(Arc::new(ty), mutable), id))
    }

    fn is_function_place(&self, e: &Expr) -> bool {
        match &ungroup(e).kind {
            Expression::Name(name) => {
                !self.names.contains_key(name) && self.sigs.contains_key(name)
            }
            Expression::GenericValue(name, _) => {
                !self.names.contains_key(name) && self.generics.templates.contains_key(name)
            }
            _ => false,
        }
    }

    fn function_place(&mut self, e: &Expr) -> Result<Local> {
        let mut initializer = Vec::new();
        let ty = self.value(ungroup(e), &mut initializer)?;
        let Some(Op::FunctionAddress(symbol, _)) = initializer.last() else {
            unreachable!("named function value lowers to a code address")
        };
        if let Some(local) = self.function_places.get(symbol) {
            return Ok(local.clone());
        }
        let symbol = symbol.clone();
        let slot = self.slot(ty.clone(), &e.at)?;
        initializer.push(Op::StoreLocal(slot));
        self.function_initializers.extend(initializer);
        let local = Local {
            slot,
            ty,
            mutable: false,
        };
        self.function_places.insert(symbol, local.clone());
        Ok(local)
    }
    /// Load for observation, preserving ownership and extending loans through the consumer.
    pub(super) fn observe(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<(Ty, Vec<usize>)> {
        self.observe_expected(e, None, ops)
    }
    pub(super) fn observe_expected(
        &mut self,
        e: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, Vec<usize>)> {
        self.observe_with_context(e, expected, false, ops)
    }

    pub(super) fn observe_method_receiver(
        &mut self,
        e: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, Vec<usize>)> {
        self.observe_with_context(e, None, true, ops)
    }

    fn observe_with_context(
        &mut self,
        e: &Expr,
        expected: Type,
        unbox_owned: bool,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, Vec<usize>)> {
        if self.is_pipeline(e) {
            return Err(e.at.error("iterator pipelines cannot be passed as values; consume them with a loop or terminal"));
        }
        let mut result = self.observe_inner(e, expected, ops)?;
        if unbox_owned
            && result.1.is_empty()
            && matches!(result.0, Ty::Box(_))
            && !matches!(
                ungroup(e).kind,
                Expression::Index(..) | Expression::Member(..)
            )
        {
            // Take temporary box contents before recording their owner, so the
            // full-expression temporary owns the content rather than a freed box.
            let content = classes::unbox(result.0.clone());
            result.0 = self.unbox_to(result.0, &content, ops);
        }
        // A temporary owner survives all observations in the containing full
        // expression, including projections through a temporary collection.
        // An element view inherits its held collection's loan. A tuple index
        // can instead produce an owned collection, which still needs holding.
        // Method dispatch separately distinguishes consumed temporary receivers
        // (for example affine values_snapshot) from borrowed collections.
        let collection_owner = !unbox_owned
            && matches!(&result.0, Ty::List(value) | Ty::Dict(_, value) if value.affine());
        if result.1.is_empty()
            && (result.0.has_destructor() || collection_owner)
            && (collection_owner || !matches!(ungroup(e).kind, Expression::Index(..)))
        {
            let slot = self.slot(result.0.clone(), &e.at)?;
            ops.push(Op::StoreLocal(slot));
            self.expression_temps.push(slot);
            if collection_owner {
                // Indexed affine elements are views into collection storage,
                // even when that storage contains no custom destructor. Lend
                // the saved owner so projections keep its provenance, and so
                // last-use lowering cannot move it out before the view is used.
                let loan = self.new_loan(slot, false, None, ops);
                ops.push(Op::BorrowLocal(slot, false));
                ops.push(Op::ReadRef(result.0.clone()));
                result.1.push(loan);
            } else {
                ops.push(Op::LoadLocal(slot));
            }
        }
        Ok(result)
    }
    fn observe_inner(
        &mut self,
        e: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, Vec<usize>)> {
        match &e.kind {
            Expression::Group(inner) => self.observe_inner(inner, expected, ops),
            Expression::Unary(op, inner) if op == "&" => {
                let (ty, loan) = self.shared_argument(inner, expected, ops)?;
                let Ty::Ref(inner, _) = ty else {
                    unreachable!()
                };
                ops.push(Op::ReadRef((*inner).clone()));
                Ok(((*inner).clone(), vec![loan]))
            }
            Expression::Name(name) => {
                if !self.names.contains_key(name)
                    && (enums::prelude_variant(name)
                        || self.sigs.contains_key(name)
                        || self.generics.templates.contains_key(name))
                {
                    return self.value(e, ops).map(|ty| (ty, vec![]));
                }
                let local = self.named_place(e)?;
                self.mark(&e.at, ops);
                self.use_carried(&local, ops);
                if matches!(local.ty, Ty::KeyView(_)) {
                    let loan = self.reference_locals[&local.slot];
                    ops.push(Op::LoadLocal(local.slot));
                    return Ok((local.ty, vec![loan]));
                }
                if let Ty::Ref(ty, _) = &local.ty {
                    let parent = self.reference_locals[&local.slot];
                    if !ty.affine() {
                        ops.push(Op::Access(self.loans[parent].root, false, Some(parent)));
                        ops.push(Op::LoadLocal(local.slot));
                        ops.push(Op::ReadRef((**ty).clone()));
                        ops.push(Op::UseLoan(parent));
                        return Ok((
                            (**ty).clone(),
                            if matches!(**ty, Ty::KeyView(_)) {
                                vec![parent]
                            } else {
                                vec![]
                            },
                        ));
                    }
                    let loan = self.new_loan(self.loans[parent].root, false, Some(parent), ops);
                    ops.push(Op::LoadLocal(local.slot));
                    ops.push(Op::ReadRef((**ty).clone()));
                    Ok(((**ty).clone(), vec![loan]))
                } else {
                    if !local.ty.affine() {
                        ops.push(Op::Access(local.slot, false, None));
                        ops.push(Op::LoadLocal(local.slot));
                        return Ok((local.ty, vec![]));
                    }
                    let loan = self.new_loan(local.slot, false, None, ops);
                    ops.push(Op::LoadLocal(local.slot));
                    Ok((local.ty, vec![loan]))
                }
            }
            Expression::Member(base, _, _) if self.qualified_type(base)?.is_none() => {
                self.field(e, ops)
            }
            Expression::Unary(op, base)
                if op == "*" && matches!(self.place_type(base), Some(Ty::Shared(_))) =>
            {
                let (reference, loan) = self.borrow(e, false, ops)?;
                let Ty::Ref(content, _) = reference else {
                    unreachable!()
                };
                ops.push(Op::ReadRef((*content).clone()));
                Ok(((*content).clone(), vec![loan]))
            }
            Expression::Index(base, index) => {
                if matches!(self.place_type(base), Some(Ty::Shared(_))) {
                    let element = Expr {
                        at: e.at.clone(),
                        kind: Expression::Index(Box::new(shared::content(base)), index.clone()),
                    };
                    return self.observe_inner(&element, expected, ops);
                }
                // A literal list index reads through an element loan, which
                // stays disjoint from loans of other literal indices.
                if matches!(self.place_type(base), Some(Ty::Enum(t)) if t.tuple())
                    || self
                        .place_type(base)
                        .and_then(|ty| literal_index(&ty, index))
                        .is_some()
                {
                    let (reference, loan) = self.borrow(e, false, ops)?;
                    let Ty::Ref(value, _) = reference else {
                        unreachable!()
                    };
                    if *value == Ty::Unit {
                        return Err(e.at.error("expected a value, got ()"));
                    }
                    ops.push(Op::ReadRef((*value).clone()));
                    return Ok(((*value).clone(), vec![loan]));
                }
                if self.byte_place(base).is_some()
                    || matches!(self.place_type(base), Some(Ty::List(element))
                    if element.is_numeric() || *element == Ty::Bool)
                    || self.place_type(base) == Some(Ty::Str)
                {
                    return self.index(e, ops).map(|ty| (ty, vec![]));
                }
                let (ty, loans) = self.observe(base, ops)?;
                if let Ty::Enum(t) = &ty
                    && t.tuple()
                {
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
                    if value == Ty::Unit {
                        return Err(e.at.error("expected a value, got ()"));
                    }
                    ops.push(Op::Enum(crate::sum::EnumOp::Field(t.clone(), 0, i)));
                    return Ok((value, loans));
                }
                if let Some(class) = byte_buffers::byte_buffer(&ty) {
                    let reads = self.byte_value_item(&class, index, &e.at, ops)?;
                    let mut loans = loans;
                    loans.extend(reads);
                    return Ok((Ty::U8, loans));
                }
                let (key, value) = match &ty {
                    Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
                    Ty::List(_) | Ty::Str | Ty::ByteStr | Ty::Range(_) => {
                        (Ty::I64, ty.element().unwrap())
                    }
                    _ => {
                        return Err(e
                            .at
                            .error("indexing requires list, dict, str, bytes, range, or Bytes"));
                    }
                };
                let (actual, reads) = self.observe_expected(index, Some(key.clone()), ops)?;
                self.same(Some(actual), Some(key), &index.at)?;
                self.mark(&e.at, ops);
                ops.push(Op::Collection(CollectionOp::Get(ty)));
                let mut loans = loans;
                loans.extend(reads);
                Ok((value, loans))
            }
            _ => {
                let ty = self
                    .expr_expected(e, expected, ops)?
                    .ok_or_else(|| e.at.error("expected a value, got ()"))?;
                if matches!(ty, Ty::KeyView(_)) {
                    let loan = self.reference_origin(e, ops)?;
                    Ok((ty, vec![loan]))
                } else if let Ty::Ref(inner, _) = ty {
                    let id = self.reference_origin(e, ops)?;
                    ops.push(Op::ReadRef((*inner).clone()));
                    Ok(((*inner).clone(), vec![id]))
                } else {
                    Ok((ty, vec![]))
                }
            }
        }
    }
    pub(super) fn copy_or_drop(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let [arg] = args else {
            return Err(at.error(format!("{name} takes one argument")));
        };
        let arg = ungroup(arg);
        if name == "drop" {
            if let Expression::Name(_) = &arg.kind {
                let local = self.named_place(arg)?;
                if matches!(local.ty, Ty::Ref(..) | Ty::KeyView(_)) {
                    return Err(at.error("drop requires an owned value, not a reference"));
                }
                self.mark(&arg.at, ops);
                ops.push(Op::MoveLocal(local.slot));
            } else {
                if matches!(self.value(arg, ops)?, Ty::Ref(..) | Ty::KeyView(_)) {
                    return Err(at.error("drop requires an owned value, not a reference"));
                }
            }
            ops.push(Op::Drop);
            return Ok(None);
        }
        let (ty, loans) = self.observe(arg, ops)?;
        if ty.facts().shared {
            return Err(at.error(
                "values containing Shared owners cannot be copied; call share() for another owner of the same value",
            ));
        }
        if ty.recursive_data() {
            return Err(at.error(
                "automatic copy is not supported for recursive data; rebuild it explicitly",
            ));
        }
        if !ty.can_copy() {
            return Err(at.error("this resource cannot be copied"));
        }
        if ty.layout_depth() >= 64 {
            return Err(at.error("type nesting exceeds the implementation limit of 64"));
        }
        ops.push(Op::Collection(CollectionOp::TryCopy(ty.clone())));
        Self::end_reads(loans, ops);
        Ok(Some(crate::sum::result(ty, crate::sum::alloc_error())))
    }

    pub(super) fn read_place(&mut self, local: &Local, ops: &mut Vec<Op>) -> (Ty, usize) {
        if let Ty::Ref(ty, _) = &local.ty {
            let parent = self.reference_locals[&local.slot];
            let loan = self.new_loan(self.loans[parent].root, true, Some(parent), ops);
            ops.push(Op::LoadLocal(local.slot));
            ops.push(Op::ReadRef((**ty).clone()));
            ((**ty).clone(), loan)
        } else {
            let loan = self.new_loan(local.slot, true, None, ops);
            ops.push(Op::LoadLocal(local.slot));
            (local.ty.clone(), loan)
        }
    }
    pub(super) fn write_reference(
        &mut self,
        target: &Expr,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let Expression::Unary(op, base) = &target.kind else {
            unreachable!()
        };
        if op != "*" {
            return Err(target.at.error("invalid assignment target"));
        }
        let local = self.named_place(base)?;
        let Ty::Ref(ty, true) = &local.ty else {
            return Err(target
                .at
                .error("assignment requires an exclusive reference"));
        };
        if matches!(&**ty, Ty::Class(_)) {
            return Err(target.at.error(
                "cannot replace a whole class through a reference; assign its fields instead",
            ));
        }
        if matches!(&**ty, Ty::Atomic(_)) {
            return Err(target
                .at
                .error("an atomic is written with store(), not through `*`"));
        }
        let expected = (**ty != Ty::Unit).then(|| (**ty).clone());
        let got = self.expr_expected(value, expected.clone(), ops)?;
        self.same(got, expected, &value.at)?;
        if **ty == Ty::Unit {
            ops.push(Op::PushUnit);
        }
        let loan = self.reference_locals[&local.slot];
        ops.push(Op::Access(self.loans[loan].root, true, Some(loan)));
        ops.push(Op::LoadLocal(local.slot));
        ops.push(Op::WriteRef((**ty).clone()));
        ops.push(Op::UseLoan(loan));
        Ok(())
    }
}
