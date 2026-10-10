//! Source places and explicit loans with statically disjoint field projections.
use super::*;

/// Summarize only a single direct returned projection. More complicated bodies
/// keep the conservative whole-argument footprint until flow summaries exist.
pub(super) fn returned_fields(function: &Function, sig: &FnSig) -> Option<Vec<usize>> {
    if !matches!(sig.outputs.first(), Some(Ty::Ref(..))) {
        return None;
    }
    let (root, ty) = sig.inputs.iter().find(|(_, t)| matches!(t, Ty::Ref(..)))?;
    let [stmt] = function.body.as_slice() else {
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
            Expression::Member(base, name) => {
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

impl Lower<'_> {
    /// Signature-directed shared arguments lend places, forward references, or
    /// materialize an owner until the end of the containing full expression.
    pub(super) fn shared_argument(
        &mut self,
        e: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        if self.place_type(e).is_some() {
            return self.borrow(e, false, ops);
        }
        let ty = self
            .expr_expected(e, expected, ops)?
            .ok_or_else(|| e.at.error("expected a borrowed value, got ()"))?;
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
        Ok((Ty::Ref(Rc::new(ty), false), loan))
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

    /// A reassigned reference binding keeps the loan it was declared with, so
    /// the new reference must be borrowed through that loan: it then stays
    /// inside the target the loan already protects.
    pub(super) fn check_retargeted_reference(
        &self,
        name: &str,
        slot: u8,
        mut loan: usize,
        at: &Token,
    ) -> Result<()> {
        let held = self.reference_locals[&slot];
        // Only a loan widened by `bind_reference` protects every later target.
        if self.loans[held].precise {
            return Err(at.error(format!(
                "reference `{name}` was not declared as a `mut` reference binding and cannot be reassigned"
            )));
        }
        loop {
            if loan == held {
                return Ok(());
            }
            match self.loans[loan].parent {
                Some(parent) => loan = parent,
                None => {
                    return Err(at.error(format!(
                        "reference binding `{name}` can be reassigned only to a reference borrowed from `{name}` itself"
                    )))
                }
            }
        }
    }

    pub(super) fn reference_origin(&self, e: &Expr, ops: &[Op]) -> Result<usize> {
        if let Expression::Name(name) = &ungroup(e).kind {
            if let Some(local) = self.names.get(name) {
                if let Some(id) = self.reference_locals.get(&local.slot) {
                    return Ok(*id);
                }
            }
        }
        if matches!(&ungroup(e).kind, Expression::Unary(op, _) if op == "&" || op == "&mut")
            || matches!(
                &ungroup(e).kind,
                Expression::Call(..)
                    | Expression::GenericCall(..)
                    | Expression::Method(..)
                    | Expression::GenericMethod(..)
                    | Expression::Invoke(..)
            )
        {
            if let Some(id) = ops.iter().rev().find_map(|op| {
                if let Op::Loan(l) = op {
                    Some(l.id)
                } else {
                    None
                }
            }) {
                return Ok(id);
            }
        }
        Err(e.at.error("reference result requires a named reference, direct borrow, or reference-returning call"))
    }

    pub(super) fn check_return_reference(&self, e: &Expr, ops: &[Op]) -> Result<usize> {
        let id = self.reference_origin(e, ops)?;
        if Some(self.loans[id].root) != self.return_origin {
            return Err(e
                .at
                .error("returned reference must originate from the reference parameter"));
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
        if let Some(Ty::Ref(_, mutable)) = sig.outputs.first() {
            let parent = loans[0];
            let id = self.new_loan(self.loans[parent].root, *mutable, Some(parent), ops);
            if let Some(fields) = self
                .returned_fields
                .get(name)
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
    fn named_place(&self, e: &Expr) -> Result<Local> {
        match &e.kind {
            Expression::Group(e) => self.named_place(e),
            Expression::Name(name) => {
                let local = self.names.get(name).cloned().ok_or_else(|| e.at.error(format!("unknown binding `{name}`")))?;
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
        match &ungroup(e).kind {
            Expression::Member(base, _) => self.assignment_indices(base, slots, ops),
            Expression::Index(base, index) => {
                self.assignment_indices(base, slots, ops)?;
                let key = match self.place_type(base) {
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
            let collection = self.place_type(base).ok_or_else(|| {
                e.at.error("element borrowing requires a named collection or field")
            })?;
            if let Ty::Enum(t) = &collection {
                if t.tuple() {
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
                    if mutable && !collection.affine() {
                        return Err(e.at.error("copyable tuple storage is immutable"));
                    }
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
                    return Ok((Ty::Ref(Rc::new(field), mutable), loan));
                }
            }
            let key = match &collection {
                Ty::List(_) => Ty::I64,
                Ty::Dict(key, _) => (**key).clone(),
                _ => {
                    return Err(e
                        .at
                        .error("element references require a list or dictionary"))
                }
            };
            let (_, loan) = self.borrow_with_indices(base, mutable, indices, ops)?;
            // Index identities are not proven disjoint. Descendant projections
            // must therefore retain the whole collection's loan footprint.
            self.loans[loan].precise = false;
            for op in ops.iter_mut().rev() {
                if let Op::Loan(fact) = op {
                    if fact.id == loan {
                        *fact = self.loans[loan].clone();
                        break;
                    }
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
            ops.push(Op::Collection(operation));
            return Ok((output, loan));
        }
        if let Expression::Unary(op, base) = &ungroup(e).kind {
            if op == "*" {
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
                    _ if reference_binding => Ok((reference, loan)),
                    _ => Err(e.at.error("`*` requires a reference or a Box")),
                };
            }
        }
        if let Expression::Member(base, name) = &ungroup(e).kind {
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
            if self.loans[loan].precise {
                self.loans[loan].fields.push(index);
            }
            // Recursive projections refine the one loan created at the root.
            // Update its emitted fact before any checker sees the final place.
            for op in ops.iter_mut().rev() {
                if let Op::Loan(fact) = op {
                    if fact.id == loan {
                        *fact = self.loans[loan].clone();
                        break;
                    }
                }
            }
            let result = Ty::Ref(Rc::new(class.get().fields[index].1.clone()), mutable);
            ops.push(Op::Class(crate::record::ClassOp::FieldRef(
                class.clone(),
                index,
                mutable,
            )));
            return Ok((result, loan));
        }
        let local = self.named_place(e)?;
        self.mark(&e.at, ops);
        let (ty, root, parent) = if let Ty::Ref(ty, writable) = &local.ty {
            if mutable && !writable {
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
            ops.push(Op::Reborrow(Ty::Ref(Rc::new(ty.clone()), mutable)));
        } else {
            ops.push(Op::BorrowLocal(local.slot, mutable));
        }
        Ok((Ty::Ref(Rc::new(ty), mutable), id))
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
        let result = self.observe_inner(e, expected, ops)?;
        // A temporary owner survives all observations in the containing full
        // expression, including projections through a temporary collection.
        // An element read is a view of its collection, which is held already.
        if result.1.is_empty()
            && result.0.has_destructor()
            && !matches!(ungroup(e).kind, Expression::Index(..))
        {
            let slot = self.slot(result.0.clone(), &e.at)?;
            ops.push(Op::StoreLocal(slot));
            ops.push(Op::LoadLocal(slot));
            self.expression_temps.push(slot);
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
                if let Ty::Ref(ty, _) = &local.ty {
                    let parent = self.reference_locals[&local.slot];
                    if !ty.affine() {
                        ops.push(Op::Access(self.loans[parent].root, false, Some(parent)));
                        ops.push(Op::LoadLocal(local.slot));
                        ops.push(Op::ReadRef((**ty).clone()));
                        ops.push(Op::UseLoan(parent));
                        return Ok(((**ty).clone(), vec![]));
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
            Expression::Member(base, _) if self.qualified_type(base)?.is_none() => {
                self.field(e, ops)
            }
            Expression::Index(base, index) => {
                if matches!(self.place_type(base), Some(Ty::List(element))
                    if element.is_numeric() || *element == Ty::Bool)
                {
                    return self.index(base, index, ops).map(|ty| (ty, vec![]));
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
                        if value == Ty::Unit {
                            return Err(e.at.error("expected a value, got ()"));
                        }
                        ops.push(Op::Enum(crate::sum::EnumOp::Field(t.clone(), 0, i)));
                        return Ok((value, loans));
                    }
                }
                let (key, value) = match &ty {
                    Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
                    Ty::List(_) | Ty::Str | Ty::Range(_) => (Ty::I64, ty.element().unwrap()),
                    _ => return Err(e.at.error("indexing requires list, dict, str, or range")),
                };
                let actual = self.expr_expected(index, Some(key.clone()), ops)?;
                self.same(actual, Some(key), &index.at)?;
                ops.push(Op::Collection(CollectionOp::Get(ty)));
                Ok((value, loans))
            }
            _ => {
                let ty = self
                    .expr_expected(e, expected, ops)?
                    .ok_or_else(|| e.at.error("expected a value, got ()"))?;
                if let Ty::Ref(inner, _) = ty {
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
                if matches!(local.ty, Ty::Ref(..)) {
                    return Err(at.error("drop requires an owned value, not a reference"));
                }
                self.mark(&arg.at, ops);
                ops.push(Op::MoveLocal(local.slot));
            } else {
                if matches!(self.value(arg, ops)?, Ty::Ref(..)) {
                    return Err(at.error("drop requires an owned value, not a reference"));
                }
            }
            ops.push(Op::Drop);
            return Ok(None);
        }
        let (ty, loans) = self.observe(arg, ops)?;
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
