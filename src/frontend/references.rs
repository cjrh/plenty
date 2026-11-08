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

/// Whether a parameter of type `ty` lends a borrow the call's result may
/// keep: a reference, a slice view, or an `Option` or `Result` carrying a
/// reference. The first such parameter is the origin of a returned reference.
pub(super) fn lends(ty: &Ty) -> bool {
    matches!(ty, Ty::Ref(..) | Ty::View(..)) || ty.carried_reference().is_some()
}

/// Whether a lending parameter of type `ty` can be the origin of an
/// exclusive returned reference.
pub(super) fn lends_mutably(ty: &Ty) -> bool {
    matches!(ty, Ty::View(_, true)) || ty.carries_mutable_loan()
}

/// Whether an argument of type `ty` holds a borrow other than through a
/// reference, a view, or a reference-carrying `Option` or `Result`: a
/// closure environment that captures one.
pub(super) fn borrowing_closure(ty: &Ty) -> bool {
    !matches!(ty, Ty::Ref(..) | Ty::View(..))
        && ty.carried_reference().is_none()
        && ty.contains_reference()
}

/// Summarize only a single direct returned projection. More complicated bodies
/// keep the conservative whole-argument footprint until flow summaries exist.
pub(super) fn returned_fields(body: &[Stmt], sig: &FnSig) -> Option<Vec<usize>> {
    if !matches!(sig.outputs.first(), Some(Ty::Ref(..))) {
        return None;
    }
    let (root, ty) = sig.inputs.iter().find(|(_, t)| lends(t))?;
    if !matches!(ty, Ty::Ref(..)) {
        return None;
    }
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

/// Dictionary indexing never writes: a missing key would trap at run time,
/// which `--check` cannot report (issue #580). These name the forms that
/// add, replace, or update without trapping.
pub(super) const DICT_INDEX_ASSIGNMENT: &str =
    "dictionaries have no indexed assignment; use `d.insert(k, v)?`, which adds or replaces";
pub(super) const SHARED_VIEW_MUTATION: &str =
    "cannot borrow a shared slice view as mutable; take an exclusive view with `&mut xs[a:b]`";
pub(super) const VIEW_PROJECTION: &str =
    "a slice view has no fields and is not dereferenced; index its elements, as `v[i]`";
pub(super) const DICT_INDEX_BORROW: &str = "dictionary values cannot be borrowed mutably by index; use `d.get_mut(k)`, which returns `Option[&mut V]`, or loop over `(&mut d).values()` or `(&mut d).items()` to change every value";
pub(super) fn dict_index_augment(op: &str) -> String {
    format!(
        "dictionaries have no indexed assignment; use `d.insert(k, d.get(k, 0) {op} v)?`, or update the value in place through `d.get_mut(k)`, which returns `Option[&mut V]`"
    )
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
    let value = digits.parse::<usize>().ok()?;
    (value < crate::ownership::ELEMENT_LIMIT).then_some(crate::ownership::ELEMENT | value)
}

/// An `if` or `match` being lowered as a value: see [`Lower::branch_value`].
pub(super) struct BranchJoin {
    /// The id of the loan the value takes.
    join: usize,
    /// One loan id per arm, taken by the arm's value if it borrows.
    reserved: Vec<usize>,
    /// The next arm whose value is recorded.
    next: usize,
    /// The loans of the arm values that borrow.
    assigned: Vec<usize>,
    /// The sum method this branch lowers, if any; see
    /// `Lower::branch_adapter`.
    adapter: Option<crate::ownership::Adapter>,
}

impl BranchJoin {
    /// Whether an arm value of type `ty` joins this branch's loan: a plain
    /// reference or one inside an Option or Result, as `unwrap_or` joins its
    /// operands (issue #897).
    pub(super) fn carries(ty: &Ty) -> bool {
        ty.carries_loan()
    }
}

/// A `mut` holder of a reference-carrying type that started empty.
#[derive(Default)]
pub(super) struct Holder {
    /// The loans of every borrowing assignment lowered so far.
    pub(super) assigned: Vec<usize>,
    /// Whether some `return` returns the holder.
    pub(super) returned: bool,
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
        // Text-view indexing produces an owned one-scalar string, rather than
        // an addressable element slot. Lend that result as a full-expression
        // temporary, just as an owned string returned by a call is lent.
        let scalar_text = matches!(
            &ungroup(e).kind,
            Expression::Index(base, _) if self.slice_view_type(base, false) == Some(text_view())
        );
        if !scalar_text && (self.place_type(e).is_some() || self.is_function_place(e)) {
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
        if self.text_bytes_receiver(value).is_some() {
            let (view, loan) = self.text_bytes_view(value, true, ops)?;
            return Ok((view, Some(loan)));
        }
        if self.is_pipeline(value) && !self.is_dictionary_projection(value) {
            let outer = std::mem::replace(&mut self.pipeline.binding, true);
            let plan = self.pipeline(value, ops);
            self.pipeline.binding = outer;
            let plan = plan?;
            let item = Arc::new(plan.item.ty.clone());
            let id = self.pipeline.plans.len();
            self.pipeline.plans.push(plan);
            ops.push(Op::PipelineToken(item.clone(), id));
            return Ok((Ty::Iter(item, id), None));
        }
        let place = match &ungroup(value).kind {
            Expression::Unary(op, place) if op == "&" || op == "&mut" => {
                Some((&**place, op == "&mut"))
            }
            Expression::Name(name) if !self.captures.contains(name) => match self.names.get(name) {
                Some(Local {
                    ty: Ty::Ref(_, mutable) | Ty::View(_, mutable),
                    ..
                }) => Some((value, *mutable)),
                _ => None,
            },
            _ => None,
        };
        if let Some((place, mutable)) = place {
            let (ty, loan) = self.borrow(place, mutable, ops)?;
            if matches!(ty, Ty::View(..)) {
                self.check_bound_view_origin(loan, &value.at)?;
            }
            let ty = match &expected {
                Some(expected) => self.unbox_to(ty, expected, ops),
                None => ty,
            };
            return Ok((ty, Some(loan)));
        }
        // A unit value is stored as the unit word (issue #870).
        let Some(ty) = self.expr_expected(value, expected, ops)? else {
            ops.push(Op::PushUnit);
            return Ok((Ty::Unit, None));
        };
        if let Ty::KeyView(_) | Ty::DictView(..) = ty {
            let parent = self.reference_origin(value, ops)?;
            let loan = self.new_loan(self.loans[parent].root, false, Some(parent), ops);
            return Ok((ty, Some(loan)));
        }
        if matches!(ty, Ty::View(..)) {
            return Err(value.at.error(
                "a view binding requires a direct borrow, named view, or text as_bytes() borrow",
            ));
        }
        if ty.carried_reference().is_some() {
            if loan_free(value) {
                let loan = self.empty_holder_loan(&ty, &value.at, ops)?;
                return Ok((ty, Some(loan)));
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
                | Expression::If(..)
                | Expression::Match(..)
                | Expression::Conditional { .. }
                | Expression::Invoke(..)
                | Expression::GenericCall(..)
                | Expression::Method(..)
                | Expression::GenericMethod(..)
                | Expression::Try(..)
        ) {
            return Err(value.at.error(
                "reference bindings require a named reference, a direct borrow, a reference-returning call, or an `if`, `match` or conditional value",
            ));
        }
        let loan = self.reference_origin(value, ops)?;
        Ok((ty, Some(loan)))
    }

    /// The loan of a binding whose reference-carrying value holds no
    /// reference: `Nothing`, `Err(e)`, or `Some`/`Ok` around one. Its payload
    /// has no referent, so the loan borrows no place in the program. Its root
    /// is a unit slot of its own that nothing reads, writes or borrows, so the
    /// loan never conflicts, and returning it is not an escape. Rooting it
    /// there instead of on a parameter keeps every holder and payload
    /// reference on the ordinary loan rules without restricting any real
    /// place.
    pub(super) fn empty_holder_loan(
        &mut self,
        ty: &Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<usize> {
        let referent = self.absent_referent(at)?;
        Ok(self.new_loan(referent, ty.carries_mutable_loan(), None, ops))
    }

    /// A unit slot that roots loans no real place backs. Reads through a
    /// payload reference check that their root is initialized, although such
    /// a read can never run, so the slot is initialized once at function
    /// entry: a store at each evaluation, as in a loop, would conflict with a
    /// copy of an earlier evaluation's value that is still live.
    pub(super) fn absent_referent(&mut self, at: &Token) -> Result<u8> {
        let referent = self.slot(Ty::Unit, at)?;
        self.function_initializers
            .extend([Op::PushUnit, Op::StoreLocal(referent)]);
        self.absent_referents.insert(referent);
        Ok(referent)
    }

    /// The loan an argument `arg` of reference-carrying type `ty` lends a
    /// call: a reborrow of the holder's loan, as a reference argument has, so
    /// a later use of the holder ends every borrow the call's result keeps.
    /// `Nothing` or `Err` lends one that borrows nothing.
    pub(super) fn carried_argument(
        &mut self,
        arg: &Expr,
        ty: &Ty,
        ops: &mut Vec<Op>,
    ) -> Result<usize> {
        if loan_free(arg) {
            return self.empty_holder_loan(ty, &arg.at, ops);
        }
        let parent = self.reference_origin(arg, ops)?;
        let root = self.loans[parent].root;
        Ok(self.new_loan(root, ty.carries_mutable_loan(), Some(parent), ops))
    }

    /// Whether `loan` descends from an [`Self::empty_holder_loan`], so the
    /// reference it covers can never exist at run time.
    pub(super) fn absent_loan(&self, loan: usize) -> bool {
        self.absent_referents.contains(&self.loans[loan].root)
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
            self.loans[loan].retargetable = true;
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

    /// Records assigning a value holding loan `origin` to the existing `mut`
    /// reference binding or reference-carrying holder `name` in `slot`.
    /// A `Nothing` or `Err` borrows nothing and keeps the held place. A
    /// holder that started empty keeps its declaration loan, so every use
    /// names it wherever it lies relative to an assignment. Each assignment
    /// adds a loan of its own to the holder, which every such use keeps
    /// live; see [`crate::ownership::Loan::holder`], so values may borrow
    /// different places and owners. A value of another started-empty holder
    /// borrows whatever that holder's values borrow, so it is not treated as
    /// `Nothing`.
    pub(super) fn retarget_reference(
        &mut self,
        name: &str,
        slot: u8,
        origin: usize,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let held = self.reference_locals[&slot];
        let source = self.holder_of(origin);
        if self.absent_loan(origin) && source.is_none() {
            return Ok(());
        }
        if self.absent_loan(held) && !self.loans[held].precise {
            return self.assign_holder(held, origin, at, ops);
        }
        if self.check_retargeted_reference(name, slot, origin, at)? {
            ops.push(Op::EndLoan(held));
        }
        Ok(())
    }

    fn assign_holder(
        &mut self,
        held: usize,
        origin: usize,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let holder = self.holders.entry(held).or_default();
        if holder.returned {
            // A `return` of the holder, possibly earlier in a loop, can
            // return this value, so it must have the returned origin.
            self.check_returned_value(origin, at)?;
        }
        // A loan of its own, emitted here, so the checker sees it marked.
        let mutable = self.loans[origin].mutable;
        let loan = self.new_loan(self.loans[origin].root, mutable, Some(origin), ops);
        self.loans[loan].holder = Some(crate::ownership::Assigned {
            holder: held,
            kind: crate::ownership::Joined::Assignment,
        });
        if let Some(Op::Loan(fact)) = ops
            .iter_mut()
            .rev()
            .find(|op| matches!(op, Op::Loan(l) if l.id == loan))
        {
            *fact = self.loans[loan].clone();
        }
        // Earlier values stay live: a copy of the holder shares its
        // declaration loan and can still hold one of them.
        self.holders
            .get_mut(&held)
            .expect("holder")
            .assigned
            .push(loan);
        Ok(())
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
            // Only an `Option` or `Result` holder can start without a borrow.
            let choose = match self.names.get(name).map(|local| &local.ty) {
                Some(ty @ Ty::Enum(t)) if t.is_option() => format!(
                    "; to choose between owners, start it empty, as in `mut {name}: {ty} = Nothing`"
                ),
                Some(ty @ Ty::Enum(t)) if t.is_result() => format!(
                    "; to choose between owners, start it empty, as in `mut {name}: {ty} = Err(error)`"
                ),
                _ => String::new(),
            };
            return Err(at.error(format!(
                "reference binding `{name}` can be reassigned only to a reference into the owner it first borrowed{choose}"
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
                    | Expression::If(..)
                    | Expression::Match(..)
                    | Expression::Conditional { .. }
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

    /// Lowers an `if`, `match` or conditional expression with `arms` arms used as a value with
    /// `lower`. A value of a reference-carrying type gets a join loan,
    /// created after the arms and rooted at a hidden referent like an empty
    /// holder's. It is a holder that started empty and was assigned each
    /// borrowing arm value: a use keeps every one of them borrowed, and a
    /// return checks each against the origin parameter. Arms may borrow
    /// different owners.
    ///
    /// The ids of the join and of one loan per arm are reserved first, so
    /// each arm can end the other arms' loans after its value: the value an
    /// evaluation produces holds only the loan of the arm that ran, and an
    /// earlier evaluation's loans are not carried into a later one (see
    /// [`Loan::holder`](crate::ownership::Loan::holder) for when such an end
    /// is ignored).
    pub(super) fn branch_value(
        &mut self,
        at: &Token,
        arms: usize,
        ops: &mut Vec<Op>,
        lower: impl FnOnce(&mut Self, &mut Vec<Op>) -> Result<super::BlockResult>,
    ) -> Result<super::BlockResult> {
        let join = self.new_loan(0, false, None, &mut Vec::new());
        let reserved = (0..arms)
            .map(|_| self.new_loan(0, false, None, &mut Vec::new()))
            .collect();
        self.branch_joins.push(BranchJoin {
            join,
            reserved,
            next: 0,
            assigned: Vec::new(),
            adapter: std::mem::take(&mut self.branch_adapter),
        });
        let result = lower(self, ops);
        let branch = self.branch_joins.pop().expect("pushed above");
        let result = result?;
        if let super::BlockResult::Continues(Some(ty)) = &result
            && BranchJoin::carries(ty)
        {
            let referent = self.absent_referent(at)?;
            let loan = &mut self.loans[join];
            loan.root = referent;
            loan.mutable = ty.carries_mutable_loan();
            loan.dependencies.clear();
            ops.push(Op::Loan(loan.clone()));
            if !branch.assigned.is_empty() {
                self.holders.insert(
                    join,
                    Holder {
                        assigned: branch.assigned,
                        returned: false,
                    },
                );
            }
        }
        Ok(result)
    }

    /// Emits the conditional expression `e`, whose operands `yes` and `no`
    /// were lowered into `a` and `b` and joined as `ty`. A reference joins
    /// both operands' owners as an `if` block's arms do (issue #897).
    pub(super) fn conditional_value(
        &mut self,
        e: &Expr,
        (yes, no): (&Expr, &Expr),
        (mut a, mut b): (Vec<Op>, Vec<Op>),
        ty: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if matches!(ty, Some(Ty::View(..))) {
            return Err(e.at.error("conditional expressions cannot produce slice views; select an owning value or branch around the view's use"));
        }
        // A dictionary view has no join loan to keep its owners borrowed.
        // A borrowing closure needs none: its capture loans follow the move.
        if matches!(ty, Some(Ty::KeyView(_) | Ty::DictView(..))) {
            return Err(e.at.error("a conditional expression cannot produce a dictionary view; produce an owned value, or choose with an `if` statement that assigns or returns each value"));
        }
        if !ty.as_ref().is_some_and(BranchJoin::carries) {
            ops.push(super::branch(a, b));
            return Ok(ty);
        }
        let joined = self.branch_value(&e.at, 2, ops, |lower, ops| {
            lower.branch_arm_value(yes, &mut a)?;
            lower.branch_arm_value(no, &mut b)?;
            ops.push(super::branch(a, b));
            Ok(super::BlockResult::Continues(ty))
        })?;
        let super::BlockResult::Continues(ty) = joined else {
            unreachable!("a conditional expression continues")
        };
        Ok(ty)
    }

    /// Records the final value `e`, of a reference-carrying type, of an
    /// arm of the innermost [`Self::branch_value`], then ends the loans of
    /// its other arms.
    pub(super) fn branch_arm_value(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<()> {
        let branch = self.branch_joins.last_mut().expect("inside a branch value");
        let arm = branch.next;
        branch.next += 1;
        let own = branch.reserved.get(arm).copied();
        if !loan_free(e) {
            let origin = self.reference_origin(e, ops)?;
            let join = self
                .branch_joins
                .last()
                .expect("inside a branch value")
                .join;
            let mutable = self.loans[origin].mutable;
            let fresh = self.new_loan(self.loans[origin].root, mutable, Some(origin), ops);
            // An arm past the reserved count keeps its fresh id, and the
            // other arms do not end it.
            let id = own.unwrap_or(fresh);
            let mut loan = self.loans[fresh].clone();
            loan.id = id;
            let method = self.branch_joins.last().and_then(|branch| branch.adapter);
            loan.holder = Some(crate::ownership::Assigned {
                holder: join,
                kind: method.map_or(
                    crate::ownership::Joined::Arm,
                    crate::ownership::Joined::Method,
                ),
            });
            if let Some(Op::Loan(fact)) = ops.last_mut() {
                *fact = loan.clone();
            }
            self.loans[id] = loan;
            self.branch_joins
                .last_mut()
                .expect("inside a branch value")
                .assigned
                .push(id);
        }
        let branch = self.branch_joins.last().expect("inside a branch value");
        ops.extend(
            branch
                .reserved
                .iter()
                .filter(|&&id| Some(id) != own)
                .map(|&id| Op::EndLoan(id)),
        );
        Ok(())
    }

    /// Gives the value just produced from the borrows `origins`, which may
    /// borrow different owners, a join loan like [`Self::branch_value`]'s:
    /// a use of the value keeps every origin borrowed.
    pub(super) fn fallback_value(
        &mut self,
        at: &Token,
        origins: &[usize],
        mutable: bool,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let join = self.new_loan(0, false, None, &mut Vec::new());
        let mut assigned = Vec::with_capacity(origins.len());
        for &origin in origins {
            let id = self.new_loan(self.loans[origin].root, mutable, Some(origin), ops);
            self.loans[id].holder = Some(crate::ownership::Assigned {
                holder: join,
                kind: crate::ownership::Joined::Fallback,
            });
            if let Some(Op::Loan(fact)) = ops.last_mut() {
                *fact = self.loans[id].clone();
            }
            assigned.push(id);
        }
        let referent = self.absent_referent(at)?;
        let loan = &mut self.loans[join];
        loan.root = referent;
        loan.mutable = mutable;
        loan.dependencies.clear();
        ops.push(Op::Loan(loan.clone()));
        self.holders.insert(
            join,
            Holder {
                assigned,
                returned: false,
            },
        );
        Ok(())
    }

    pub(super) fn check_return_reference(&mut self, e: &Expr, ops: &[Op]) -> Result<usize> {
        let id = self.reference_origin(e, ops)?;
        self.check_returned_value(id, &e.at)?;
        Ok(id)
    }

    /// The declaration loan of the started-empty `mut` holder that `loan`
    /// is a use of, or derives from through calls and copies.
    pub(super) fn holder_of(&self, loan: usize) -> Option<usize> {
        let mut ancestor = Some(loan);
        while let Some(id) = ancestor {
            if self.holders.contains_key(&id) {
                return Some(id);
            }
            ancestor = self.loans[id].parent;
        }
        None
    }

    /// Checks that a value with loan `id`, returned at `at`, borrows from
    /// the function's origin parameter. A value from a started-empty holder
    /// can be any value assigned to it, before or after the `return` in a
    /// loop, so each is checked, and later ones as they are assigned.
    fn check_returned_value(&mut self, id: usize, at: &Token) -> Result<()> {
        let Some(decl) = self.holder_of(id) else {
            if self.absent_loan(id) {
                // `Nothing` or `Err`: no reference exists.
                return Ok(());
            }
            return self.check_returned_loan(id, at);
        };
        let holder = self.holders.get_mut(&decl).expect("holder");
        if std::mem::replace(&mut holder.returned, true) {
            return Ok(());
        }
        for (index, assigned) in holder.assigned.clone().into_iter().enumerate() {
            use crate::ownership::Joined;
            let kind = self.loans[assigned].holder.map(|joined| joined.kind);
            // An `unwrap_or` join holds the receiver's loan, then the default's.
            let operand = match kind {
                Some(Joined::Fallback) if index == 0 => "value of `unwrap_or`".to_string(),
                Some(Joined::Fallback) => "default of `unwrap_or`".to_string(),
                // Its `match` lists the payload's arm first.
                Some(Joined::Method(method)) if index == 0 => {
                    format!("mapped payload of `{}`", method.name())
                }
                Some(Joined::Method(crate::ownership::Adapter::MapOr)) => {
                    "default of `map_or`".to_string()
                }
                Some(Joined::Method(method)) => format!("fallback of `{}`", method.name()),
                _ => String::new(),
            };
            let checked = self.check_returned_value(assigned, at);
            if !operand.is_empty() && checked.is_err() {
                let origin = self
                    .return_origin
                    .and_then(|slot| self.slot_names.get(slot as usize).cloned().flatten());
                return Err(at.error(match origin {
                    Some(origin) => format!(
                        "returned reference must originate from the first reference parameter, `{origin}`; the {operand} must also borrow from `{origin}`"
                    ),
                    None => format!("returned reference must originate from the first reference parameter; the {operand} must also borrow from it"),
                }));
            }
            checked?;
        }
        // A holder declared with another holder's value also returns it.
        match self.loans[decl].parent {
            Some(parent) => self.check_returned_value(parent, at),
            None => Ok(()),
        }
    }

    /// Checks that loan `id`, of a reference returned at `at`, borrows from
    /// the function's origin parameter.
    pub(super) fn check_returned_loan(&self, id: usize, at: &Token) -> Result<()> {
        let mut ancestor = Some(id);
        while let Some(loan) = ancestor {
            if self.pipeline.lent_item_loans.contains(&loan) {
                return Err(at.error(
                    "an item of a lent pipeline cannot be returned: it borrows the caller's source only for this call; return an owned copy",
                ));
            }
            ancestor = self.loans[loan].parent;
        }
        let root = self.loans[id].root;
        if let Some(origin) = self.return_origin.filter(|origin| *origin != root) {
            let name = |slot: u8| self.slot_names.get(slot as usize).cloned().flatten();
            let mut message = String::from(
                "returned reference must originate from the first reference parameter",
            );
            if let Some(origin) = name(origin) {
                message.push_str(&format!(", `{origin}`"));
            }
            let parameter = self.parameter_referents.contains(&root)
                || self.sigs.get(&self.function_name).is_some_and(|sig| {
                    sig.inputs
                        .get(root as usize)
                        .is_some_and(|(_, ty)| lends(ty))
                });
            if let Some(source) = name(root).filter(|_| parameter) {
                message.push_str(&format!(
                    "; this one borrows `{source}`: make `{source}` the first reference parameter"
                ));
            }
            return Err(at.error(message));
        }
        Ok(())
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
            retargetable: false,
            holder: None,
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
    /// Reading a reference-carrying local reads through the loan it holds,
    /// so it conflicts with a live exclusive reborrow of that loan, such as
    /// a `&mut` payload matched out of it (issue #901).
    pub(super) fn use_carried(&self, local: &Local, ops: &mut Vec<Op>) {
        if local.ty.carried_reference().is_some()
            && let Some(&loan) = self.reference_locals.get(&local.slot)
        {
            ops.push(Op::Access(self.loans[loan].root, false, Some(loan)));
            ops.push(Op::UseLoan(loan));
        }
    }

    fn named_place(&self, e: &Expr) -> Result<Local> {
        match &e.kind {
            Expression::Group(e) => self.named_place(e),
            Expression::Name(name) => {
                let local = self.names.get(name).cloned().ok_or_else(|| e.at.error(self.unknown_binding(name)))?;
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
        if self.is_pipeline(e) && !self.is_dictionary_projection(e) {
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
                    Some(Ty::List(_) | Ty::View(..)) => Ty::I64,
                    Some(Ty::Dict(key, _)) => (*key).clone(),
                    Some(Ty::Enum(t)) if t.tuple() => {
                        return Err(e.at.error("tuples have no indexed assignment; assign through a component reference: `item = &mut pair[0]`, then `*item = value`"));
                    }
                    _ => return Err(e.at.error("indexed assignment requires a list")),
                };
                let (actual, reads) = self.observe_expected(index, Some(key.clone()), ops)?;
                self.same(Some(actual), Some(key.clone()), &index.at)?;
                let slot = self.slot(key, &index.at)?;
                ops.push(Op::StoreLocal(slot));
                Self::end_reads(reads, ops);
                slots.push(slot);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Whether `target` is `d[k]` on a dictionary place.
    pub(super) fn dict_index_target(&self, target: &Expr) -> bool {
        let Expression::Index(base, _) = &ungroup(target).kind else {
            return false;
        };
        matches!(
            self.place_type(base).map(classes::unbox),
            Some(Ty::Dict(..))
        )
    }

    pub(super) fn borrow_with_indices(
        &mut self,
        e: &Expr,
        mutable: bool,
        indices: &mut Option<std::slice::Iter<'_, u8>>,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        if self.text_bytes_receiver(e).is_some() {
            if mutable {
                return Err(e
                    .at
                    .error("string bytes are immutable; as_bytes() yields a shared byte view"));
            }
            return self.text_bytes_view(e, false, ops);
        }
        if let Expression::Slice(base, bounds) = &ungroup(e).kind {
            return self.slice_view(base, bounds, mutable, ops);
        }
        if let Expression::Index(base, index) = &ungroup(e).kind {
            if matches!(self.place_type(base), Some(Ty::Shared(_))) {
                let element = Expr {
                    at: e.at.clone(),
                    kind: Expression::Index(Box::new(shared::content(base)), index.clone()),
                };
                return self.borrow_with_indices(&element, mutable, indices, ops);
            }
            let collection = self
                .place_type(base)
                .or_else(|| self.expression_type_hint(base))
                .ok_or_else(|| {
                    e.at.error("element borrowing requires a named collection or field")
                })?;
            if let Ty::Enum(t) = &collection
                && t.tuple()
            {
                let fields = &t.get().variants[0].fields;
                let index = slices::tuple_position(index, fields.len())?;
                let field = fields[index].clone();
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
            if collection == text_view() {
                return Err(e.at.error(
                    "a `&strview` has no element slots to borrow; `v[i]` reads the scalar as a one-scalar `str`, and `&v[i:i + 1]` views it",
                ));
            }
            if let Ty::View(element, _) = &collection {
                // A row's loan path extends its view's, so rows at distinct
                // literal indices are disjoint, as are distinct fields
                // borrowed through one row reference.
                let (view, loan) = self.borrow_with_indices(base, mutable, indices, ops)?;
                let step = literal_index(&Ty::List(element.clone()), index)
                    .unwrap_or(crate::ownership::ANY_ELEMENT);
                self.refine_loan(loan, step, ops);
                if let Some(indices) = indices {
                    let slot = *indices.next().expect("prepared assignment index");
                    ops.push(Op::MoveLocal(slot));
                } else {
                    let actual = self.expr_expected(index, Some(Ty::I64), ops)?;
                    self.same(actual, Some(Ty::I64), &index.at)?;
                }
                self.mark(&e.at, ops);
                ops.push(self.index_op(CollectionOp::ViewRow(view, mutable)));
                return Ok((Ty::Ref(element.clone(), mutable), loan));
            }
            let key = match &collection {
                Ty::List(_) => Ty::I64,
                Ty::Dict(_, _) if mutable => return Err(e.at.error(DICT_INDEX_BORROW)),
                Ty::Dict(key, _) => (**key).clone(),
                _ => {
                    return Err(e
                        .at
                        .error("element references require a list or dictionary"));
                }
            };
            // Scalar rows are read and written by their own operations; see
            // `scalar_element_ref`. Caching borrows the list, so it comes first.
            let view = match &collection {
                Ty::List(element) if !(element.is_numeric() || **element == Ty::Bool) => {
                    self.indexed_list_view(base, &collection)?
                }
                _ => None,
            };
            let (_, loan) = self.borrow_with_indices(base, mutable, indices, ops)?;
            // Only distinct literal list indices are proven disjoint. Other
            // element loans, and descendant projections, keep the whole
            // collection's footprint. Dictionary keys never refine a loan.
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
            let reads = if let Some(indices) = indices {
                let slot = *indices.next().expect("prepared assignment index");
                ops.push(Op::MoveLocal(slot));
                vec![]
            } else {
                let (actual, reads) = self.observe_expected(index, Some(key.clone()), ops)?;
                self.same(Some(actual), Some(key), &index.at)?;
                reads
            };
            self.mark(&e.at, ops);
            let operation = match view {
                // `uncache` relies on this exact sequence.
                Some([length, rows]) => {
                    ops.extend([Op::LoadLocal(length), Op::LoadLocal(rows)]);
                    CollectionOp::ElementRefView(collection, mutable)
                }
                None => CollectionOp::ElementRef(collection, mutable),
            };
            let output = operation.signature().1;
            ops.push(self.index_op(operation));
            Self::end_reads(reads, ops);
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
            if matches!(reference, Ty::View(..)) {
                return Err(e.at.error(VIEW_PROJECTION));
            }
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
        if let Expression::Member(base, name, _) = &ungroup(e).kind
            && !self.is_function_place(e)
        {
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
            if matches!(reference, Ty::View(..)) {
                return Err(e.at.error(VIEW_PROJECTION));
            }
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
                // A named function has no state, so `&mut f` borrows a fresh
                // copy of its code address; distinct borrows never conflict.
                let ty = self.value(ungroup(e), ops)?;
                let slot = self.slot(ty.clone(), &e.at)?;
                ops.push(Op::StoreLocal(slot));
                Local {
                    slot,
                    ty,
                    mutable: true,
                }
            } else {
                self.function_place(e)?
            }
        } else {
            self.named_place(e)?
        };
        self.mark(&e.at, ops);
        self.use_carried(&local, ops);
        if let Ty::View(element, writable) = &local.ty {
            // Borrowing a view reborrows it rather than referring to it, so
            // `&v[i]` and `&v[a:b]` work through the view's own loan.
            if mutable && !writable {
                return Err(e.at.error(SHARED_VIEW_MUTATION));
            }
            let parent = self.reference_locals[&local.slot];
            let loan = self.new_loan(self.loans[parent].root, mutable, Some(parent), ops);
            let view = Ty::View(element.clone(), mutable);
            ops.extend([Op::LoadLocal(local.slot), Op::Reborrow(view.clone())]);
            return Ok((view, loan));
        }
        if matches!(local.ty, Ty::KeyView(_) | Ty::DictView(..)) {
            if mutable {
                return Err(e.at.error("dictionary views are shared borrows"));
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
                if self.shared_dictionary_values.contains(&local.slot) {
                    return Err(e.at.error(
                        "cannot borrow shared reference as mutable; `values()` and `items()` lend shared values; loop over `(&mut d).values()` or `(&mut d).items()` to change them",
                    ));
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
                !self.names.contains_key(name)
                    && (self.sigs.contains_key(name)
                        || matches!(lookup_type(name, self.aliases), Some(Some(Ty::Class(_)))))
            }
            Expression::GenericValue(name, _) => {
                !self.names.contains_key(name)
                    && (self.generics.templates.contains_key(name)
                        || self.aliases.data.is_class(name))
            }
            Expression::Member(base, variant, _) => {
                matches!(self.qualified_type(base), Ok(Some(Ty::Enum(t)))
                    if t.get().tag(variant).is_some_and(|tag| !t.get().variants[tag].fields.is_empty()))
            }
            Expression::Type(ty) => matches!(ty.resolve(self.aliases), Ok(Some(Ty::Class(_)))),
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
        let (actual, loans) = self.observe_with_context(e, expected.clone(), false, ops)?;
        let actual = match expected {
            Some(expected) => self.inject_observed_sum(actual, &expected, ops),
            None => actual,
        };
        Ok((actual, loans))
    }

    /// Retag an observed value while its source's loans/temporary remain live.
    /// This creates no owning copy and does not widen reference types.
    pub(super) fn inject_observed_sum(&self, actual: Ty, expected: &Ty, ops: &mut Vec<Op>) -> Ty {
        if actual != *expected
            && matches!(expected, Ty::Enum(sum) if sum.anonymous())
            && crate::sum::accepts(expected, &actual)
        {
            ops.push(Op::Enum(crate::sum::EnumOp::Widen(
                Box::new(actual),
                Box::new(expected.clone()),
            )));
            expected.clone()
        } else {
            actual
        }
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
        if self.is_pipeline(e) && !self.is_dictionary_projection(e) {
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
    /// Every operand of a method chain or arithmetic chain recurses through
    /// this frame, so arms with many locals are outlined (issue #849).
    fn observe_inner(
        &mut self,
        e: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, Vec<usize>)> {
        if self.text_bytes_receiver(e).is_some() {
            let (view, loan) = self.text_bytes_view(e, false, ops)?;
            return Ok((view, vec![loan]));
        }
        match &e.kind {
            Expression::Group(inner) => self.observe_inner(inner, expected, ops),
            Expression::Unary(op, inner)
                if (op == "&" || op == "&mut")
                    && matches!(ungroup(inner).kind, Expression::Slice(..)) =>
            {
                let (view, loan) = self.borrow(inner, op == "&mut", ops)?;
                Ok((view, vec![loan]))
            }
            Expression::Unary(op, inner) if op == "&" => {
                let (ty, loan) = self.shared_argument(inner, expected, ops)?;
                let Ty::Ref(inner, _) = ty else {
                    unreachable!()
                };
                ops.push(Op::ReadRef((*inner).clone()));
                Ok(((*inner).clone(), vec![loan]))
            }
            Expression::Name(_) => self.observe_name(e, ops),
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
            Expression::Index(..) => self.observe_index(e, expected, ops),
            _ => {
                let ty = self.expr_expected(e, expected, ops)?;
                let ty = self.stored(ty, ops);
                if matches!(ty, Ty::KeyView(_) | Ty::DictView(..)) {
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
    /// Observes a name. Outlined from `observe_inner`, which every operand
    /// recurses through, so its locals stay off the recursion (issue #849).
    #[inline(never)]
    fn observe_name(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<(Ty, Vec<usize>)> {
        let Expression::Name(name) = &e.kind else {
            unreachable!("observe_name observes names")
        };
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
        if matches!(local.ty, Ty::KeyView(_) | Ty::DictView(..) | Ty::View(..)) {
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
                    if matches!(**ty, Ty::KeyView(_) | Ty::DictView(..)) {
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
    /// Observes an index expression. Outlined from `observe_inner` for the
    /// same reason as `observe_name`.
    #[inline(never)]
    fn observe_index(
        &mut self,
        e: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, Vec<usize>)> {
        let Expression::Index(base, index) = &e.kind else {
            unreachable!("observe_index observes index expressions")
        };
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
            let fields = &t.get().variants[0].fields;
            let i = slices::tuple_position(index, fields.len())?;
            let value = fields[i].clone();
            ops.push(Op::Enum(crate::sum::EnumOp::Field(t.clone(), 0, i)));
            return Ok((value, loans));
        }
        if let Some(class) = byte_buffers::byte_buffer(&ty) {
            let reads = self.byte_value_item(&class, index, &e.at, ops)?;
            let mut loans = loans;
            loans.extend(reads);
            return Ok((Ty::U8, loans));
        }
        if let Ty::View(..) = ty {
            let (value, reads) = self.view_get(&ty, index, &e.at, ops)?;
            let mut loans = loans;
            loans.extend(reads);
            return Ok((value, loans));
        }
        let (key, value) = match &ty {
            Ty::Dict(k, v) => ((**k).clone(), (**v).clone()),
            Ty::List(_) | Ty::Str | Ty::ByteStr | Ty::Range(_) => (Ty::I64, ty.element().unwrap()),
            _ => {
                return Err(e
                    .at
                    .error("indexing requires list, dict, str, bytes, range, or Bytes"));
            }
        };
        let (actual, reads) = self.observe_expected(index, Some(key.clone()), ops)?;
        self.same(Some(actual), Some(key), &index.at)?;
        self.mark(&e.at, ops);
        ops.push(self.index_op(CollectionOp::Get(ty)));
        let mut loans = loans;
        loans.extend(reads);
        Ok((value, loans))
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
                if matches!(
                    local.ty,
                    Ty::Ref(..) | Ty::KeyView(_) | Ty::DictView(..) | Ty::View(..)
                ) {
                    return Err(at.error("drop requires an owned value, not a reference"));
                }
                self.mark(&arg.at, ops);
                ops.push(Op::MoveLocal(local.slot));
            } else {
                if matches!(
                    self.value(arg, ops)?,
                    Ty::Ref(..) | Ty::KeyView(_) | Ty::DictView(..) | Ty::View(..)
                ) {
                    return Err(at.error("drop requires an owned value, not a reference"));
                }
            }
            ops.push(Op::Drop);
            return Ok(None);
        }
        let (ty, loans) = self.observe(arg, ops)?;
        if matches!(ty, Ty::View(..)) {
            return Err(at.error(
                "copy() does not take a slice view; copy its elements into a list with `list(v)?` or `v[a:b]?`",
            ));
        }
        // A nested handle is shared, as `share()` would; a bare handle is
        // rejected so that taking another owner is written as `share()`.
        if matches!(ty, Ty::Shared(_)) {
            return Err(at.error(
                "copy() does not take a Shared owner; call handle.share() for another owner of the same value",
            ));
        }
        let facts = ty.copy_facts();
        if facts.recursive {
            return Err(at.error(
                "automatic copy is not supported for recursive data; rebuild it explicitly",
            ));
        }
        if !facts.copyable {
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
        if reference_call(base) {
            return self.write_returned_reference(target, base, value, ops);
        }
        let local = self.named_place(base)?;
        let ty = writable_referent(&local.ty, target)?;
        let expected = (*ty != Ty::Unit).then(|| ty.clone());
        let got = self.expr_expected(value, expected.clone(), ops)?;
        self.same(got, expected, &value.at)?;
        if *ty == Ty::Unit {
            ops.push(Op::PushUnit);
        }
        let loan = self.reference_locals[&local.slot];
        ops.push(Op::Access(self.loans[loan].root, true, Some(loan)));
        ops.push(Op::LoadLocal(local.slot));
        ops.push(Op::WriteRef(ty.clone()));
        ops.push(Op::UseLoan(loan));
        Ok(())
    }

    /// `*f(...) = value` writes through the reference a call returns (issue
    /// #598). The call runs first, as `r = f(...); *r = value` would, and its
    /// loan stays live through the value and the write.
    fn write_returned_reference(
        &mut self,
        target: &Expr,
        call: &Expr,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let (ty, loan) = self.returned_reference(target, call, ops)?;
        let expected = (ty != Ty::Unit).then(|| ty.clone());
        let got = self.expr_expected(value, expected.clone(), ops)?;
        self.same(got, expected, &value.at)?;
        if ty == Ty::Unit {
            ops.push(Op::PushUnit);
        }
        ops.push(Op::Access(self.loans[loan].root, true, Some(loan)));
        ops.push(Op::Swap);
        ops.push(Op::WriteRef(ty));
        ops.push(Op::UseLoan(loan));
        Ok(())
    }

    /// Evaluates `call`, which must return `&mut T`, leaving the reference on
    /// the stack. Returns `T` and the loan the result holds; the caller ends
    /// it after the write.
    pub(super) fn returned_reference(
        &mut self,
        target: &Expr,
        call: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        let result = self
            .expr(call, ops)?
            .ok_or_else(|| call.at.error("expected a value, got ()"))?;
        let ty = writable_referent(&result, target)?.clone();
        let loan = self.reference_origin(call, ops)?;
        Ok((ty, loan))
    }
}

/// Whether `e` is a call, whose result may be a reference to write through.
pub(super) fn reference_call(e: &Expr) -> bool {
    matches!(
        &ungroup(e).kind,
        Expression::Call(..)
            | Expression::Invoke(..)
            | Expression::GenericCall(..)
            | Expression::Method(..)
            | Expression::GenericMethod(..)
            | Expression::Try(..)
    )
}

/// The referent of `reference` that `*reference = value` may replace.
fn writable_referent<'a>(reference: &'a Ty, target: &Expr) -> Result<&'a Ty> {
    let Ty::Ref(ty, true) = reference else {
        return Err(target
            .at
            .error("assignment requires an exclusive reference"));
    };
    // Quantity is a reserved builtin with copyable numeric storage. Replacing
    // it preserves the referent's layout and never owns a destructor.
    if matches!(&**ty, Ty::Class(t) if !t.name.starts_with("Quantity[")) {
        return Err(target
            .at
            .error("cannot replace a whole class through a reference; assign its fields instead"));
    }
    if matches!(&**ty, Ty::Atomic(_)) {
        return Err(target
            .at
            .error("an atomic is written with store(), not through `*`"));
    }
    Ok(ty)
}
