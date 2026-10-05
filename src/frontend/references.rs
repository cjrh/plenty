//! Source places and explicit loans. Field loans conservatively protect their root.
use super::*;

impl Lower<'_> {
    pub(super) fn new_loan(
        &mut self,
        root: u8,
        mutable: bool,
        parent: Option<usize>,
        ops: &mut Vec<Op>,
    ) -> usize {
        let id = self.loans.len();
        let loan = crate::ownership::Loan {
            id,
            root,
            mutable,
            parent,
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
        if let Expression::Member(base, name) = &ungroup(e).kind {
            let (reference, loan) = self.borrow(base, mutable, ops)?;
            let Ty::Ref(inner, _) = reference else {
                unreachable!()
            };
            let Ty::Class(class) = &*inner else {
                return Err(e.at.error("field borrowing requires a class"));
            };
            let index = classes::field_index(class, name, &e.at)?;
            let result = Ty::Ref(Rc::new(class.fields[index].1.clone()), mutable);
            ops.push(Op::Class(crate::record::ClassOp::FieldRef(
                class.clone(),
                index,
                mutable,
            )));
            return Ok((result, loan));
        }
        let local = self.named_place(e)?;
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
        let result = self.observe_inner(e, ops)?;
        // A temporary owner survives all observations in the containing full
        // expression, including projections through a temporary collection.
        if result.1.is_empty() && result.0.has_destructor() {
            let slot = self.slot(result.0.clone(), &e.at)?;
            ops.push(Op::StoreLocal(slot));
            ops.push(Op::LoadLocal(slot));
            self.expression_temps.push(slot);
        }
        Ok(result)
    }
    fn observe_inner(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<(Ty, Vec<usize>)> {
        match &e.kind {
            Expression::Group(inner) => self.observe(inner, ops),
            Expression::Name(name) => {
                if enums::prelude_variant(name) && !self.names.contains_key(name) {
                    return self.value(e, ops).map(|ty| (ty, vec![]));
                }
                let local = self.named_place(e)?;
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
                let (ty, loans) = self.observe(base, ops)?;
                let (key, value) = match &ty {
                    Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
                    Ty::List(_) | Ty::Str | Ty::Range => (Ty::I64, ty.element().unwrap()),
                    _ => return Err(e.at.error("indexing requires list, dict, str, or range")),
                };
                let actual = self.expr_expected(index, Some(key.clone()), ops)?;
                self.same(actual, Some(key), &index.at)?;
                ops.push(Op::Collection(CollectionOp::Get(ty)));
                Ok((value, loans))
            }
            _ => {
                let ty = self.value(e, ops)?;
                if let Ty::Ref(inner, _) = ty {
                    let id = match ops.iter().rev().find_map(|op| {
                        if let Op::Loan(l) = op {
                            Some(l.id)
                        } else {
                            None
                        }
                    }) {
                        Some(id) => id,
                        None => return Err(e.at.error("reference origin unavailable")),
                    };
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
                ops.push(Op::MoveLocal(
                    local.slot,
                    format!("{}:{}", at.line, at.column),
                ));
            } else {
                if matches!(self.value(arg, ops)?, Ty::Ref(..)) {
                    return Err(at.error("drop requires an owned value, not a reference"));
                }
            }
            ops.push(Op::Drop);
            return Ok(None);
        }
        let (ty, loans) = self.observe(arg, ops)?;
        if !ty.can_copy() {
            return Err(at.error("this resource cannot be copied"));
        }
        if ty.affine() {
            ops.push(Op::Collection(CollectionOp::Copy(ty.clone())));
        }
        Self::end_reads(loans, ops);
        Ok(Some(ty))
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
        let got = self.expr_expected(value, Some((**ty).clone()), ops)?;
        self.same(got, Some((**ty).clone()), &value.at)?;
        let loan = self.reference_locals[&local.slot];
        ops.push(Op::Access(self.loans[loan].root, true, Some(loan)));
        ops.push(Op::LoadLocal(local.slot));
        ops.push(Op::WriteRef((**ty).clone()));
        ops.push(Op::UseLoan(loan));
        Ok(())
    }
}
