//! Lending a pipeline to a `&mut Iter[T]` parameter.
//!
//! A plan stays owner-local: its operations are copied into each borrowing
//! function, specialized per lent plan like a generator consumer frame. The
//! lender passes a pointer to each owned state slot and the value of each
//! reference held as state. The borrower copies owned state into its own
//! locals at entry and writes it back at every exit, so the lender resumes
//! where the borrower stopped.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Lent plan identities, as the id of `Ty::Iter`, start here: far above any
/// local plan index, so a lent type never names a plan of the function that
/// holds it.
pub(in crate::frontend) const LENT_BASE: usize = usize::MAX / 2;
/// State slots one lent plan may pass. Each is a hidden parameter and a local
/// of the borrower, which has at most 256 slots in all.
const MAX_LENT_STATE: usize = 64;

/// A plan as its lender numbered it, and how a borrower receives it.
pub(in crate::frontend) struct Lent {
    plan: Pipeline,
    types: BTreeMap<u8, Ty>,
    /// State the borrower copies in and writes back, passed by `&mut`.
    owned: Vec<u8>,
    /// References held as state, passed by value. The plan never reassigns
    /// them, and the lender keeps their loans live across the call.
    shared: Vec<u8>,
    /// Slots rewritten for each item; the borrower allocates its own.
    scratch: Vec<u8>,
    /// Item slots holding references, with the lender's loan each derives
    /// from. The borrower gives each a loan of its own.
    references: Vec<(u8, usize, bool)>,
}

impl Lent {
    /// The hidden parameters of a borrower, after its declared ones.
    pub(in crate::frontend) fn hidden(&self) -> impl Iterator<Item = Ty> + '_ {
        self.owned
            .iter()
            .map(|slot| Ty::Ref(Arc::new(self.types[slot].clone()), true))
            .chain(self.shared.iter().map(|slot| self.types[slot].clone()))
    }
}

/// A lent plan bound to a borrower's parameter.
#[derive(Clone)]
pub(in crate::frontend) struct LentParameter {
    /// The plan's index in `pipelines`.
    plan: usize,
    id: usize,
    /// The borrower's copies of owned state and its hidden reference
    /// parameters, in the order a further lend passes them.
    owned: Vec<u8>,
    shared: Vec<u8>,
}

/// Whether `ty` is the declared `&mut Iter[T]` of a borrower template.
pub(in crate::frontend) fn declared(ty: &Ty) -> bool {
    matches!(ty, Ty::Ref(inner, true) if matches!(**inner, Ty::Iter(_, usize::MAX)))
}

/// The lent plan identity of a borrower's parameter type, if it has one.
pub(in crate::frontend) fn identity(ty: &Ty) -> Option<usize> {
    match ty {
        Ty::Ref(inner, true) => match **inner {
            Ty::Iter(_, id) if id != usize::MAX && id >= LENT_BASE => Some(id - LENT_BASE),
            _ => None,
        },
        _ => None,
    }
}

fn slots(ops: &[Op], out: &mut BTreeSet<u8>) {
    for op in ops {
        match op {
            Op::LoadLocal(slot)
            | Op::StoreLocal(slot)
            | Op::MoveLocal(slot)
            | Op::DropLocal(slot)
            | Op::EmptyLocal(slot)
            | Op::BorrowLocal(slot, _)
            | Op::Next(slot)
            | Op::Access(slot, ..)
            | Op::Tokens(crate::op::TokenMark::Guard(slot))
            | Op::Thread(
                crate::threading::ThreadOp::Start(_, slot)
                | crate::threading::ThreadOp::Finish(_, slot),
            )
            | Op::Mutex(crate::mutex::MutexOp::Lock(slot) | crate::mutex::MutexOp::Unlock(slot)) => {
                out.insert(*slot);
            }
            Op::Loan(loan) => {
                out.insert(loan.root);
            }
            Op::FieldMove(fact) => {
                out.insert(fact.root());
            }
            Op::Loop { condition, body } => {
                slots(condition, out);
                slots(body, out);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                slots(then, out);
                for arm in elifs.iter() {
                    slots(&arm.condition, out);
                    slots(&arm.body, out);
                }
                slots(otherwise, out);
            }
            Op::Match(arms) => arms.iter().for_each(|arm| slots(&arm.body, out)),
            Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => slots(cleanup, out),
            Op::CheckedBlock { body, .. } => slots(body, out),
            _ => {}
        }
    }
}

fn loans(ops: &[Op], out: &mut BTreeMap<usize, crate::ownership::Loan>) {
    for op in ops {
        match op {
            Op::Loan(loan) => {
                out.entry(loan.id).or_insert_with(|| loan.clone());
            }
            Op::Loop { condition, body } => {
                loans(condition, out);
                loans(body, out);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                loans(then, out);
                for arm in elifs.iter() {
                    loans(&arm.condition, out);
                    loans(&arm.body, out);
                }
                loans(otherwise, out);
            }
            Op::Match(arms) => arms.iter().for_each(|arm| loans(&arm.body, out)),
            Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => loans(cleanup, out),
            Op::CheckedBlock { body, .. } => loans(body, out),
            _ => {}
        }
    }
}

fn iteration_ops(plan: &Pipeline) -> impl Iterator<Item = &[Op]> {
    let iteration = &plan.iteration;
    [
        iteration.condition.as_slice(),
        iteration.body.as_slice(),
        iteration.step.as_slice(),
        iteration.absent.as_slice(),
    ]
    .into_iter()
    .chain(iteration.exact_len.as_deref())
}

/// Slot and loan renumbering from a lender's plan into one borrower.
struct Relocation<'a> {
    slots: HashMap<u8, u8>,
    /// Reference state, which the plan may read but never reassign.
    shared: BTreeSet<u8>,
    /// The lender's slots, which the borrower cannot reach: checking
    /// operations on them are dropped, and the lender keeps their loans
    /// live across the call.
    foreign: BTreeSet<u8>,
    loans: HashMap<usize, usize>,
    names: &'a [Option<Arc<str>>],
    at: &'a Token,
}

impl Relocation<'_> {
    fn slot(&self, old: u8) -> Result<u8> {
        self.slots.get(&old).copied().ok_or_else(|| {
            let name = self
                .names
                .get(old as usize)
                .cloned()
                .flatten()
                .map_or_else(String::new, |name| format!(" `{name}`"));
            self.at.error(format!(
                "this pipeline cannot be lent: it uses{name} of its creating function directly; consume it there"
            ))
        })
    }

    fn written(&self, old: u8) -> Result<u8> {
        if self.shared.contains(&old) {
            return Err(self.at.error(
                "this pipeline cannot be lent: it reassigns a reference it holds; consume it in its creating function",
            ));
        }
        self.slot(old)
    }

    fn list(&self, values: &[u8]) -> Arc<[u8]> {
        values
            .iter()
            .filter_map(|slot| self.slots.get(slot).copied())
            .collect()
    }

    fn ops(&self, ops: &[Op]) -> Result<Vec<Op>> {
        let mut result = Vec::with_capacity(ops.len());
        for op in ops {
            result.push(match op {
                // Positions are the lender's source; the borrower's marker for
                // the consuming expression stands for every operation of the plan.
                Op::Site(_) | Op::CallerSite(_) => continue,
                Op::LoadLocal(i) => Op::LoadLocal(self.slot(*i)?),
                Op::BorrowLocal(i, mutable) => Op::BorrowLocal(self.slot(*i)?, *mutable),
                Op::StoreLocal(i) => Op::StoreLocal(self.written(*i)?),
                Op::MoveLocal(i) => Op::MoveLocal(self.written(*i)?),
                Op::DropLocal(i) => Op::DropLocal(self.written(*i)?),
                Op::EmptyLocal(i) => Op::EmptyLocal(self.written(*i)?),
                Op::Next(i) => Op::Next(self.written(*i)?),
                Op::FieldMove(fact) => Op::FieldMove(fact.with_root(self.written(fact.root())?)),
                Op::Access(i, _, _) if self.foreign.contains(i) => continue,
                Op::Access(i, write, via) => Op::Access(
                    self.slot(*i)?,
                    *write,
                    via.and_then(|id| self.loans.get(&id).copied()),
                ),
                Op::Loan(loan) if self.foreign.contains(&loan.root) => continue,
                Op::Loan(loan) => {
                    let mut loan = loan.clone();
                    loan.id = self.loans[&loan.id];
                    loan.root = self.slot(loan.root)?;
                    loan.parent = loan.parent.and_then(|id| self.loans.get(&id).copied());
                    loan.through = loan.through.and_then(|i| self.slots.get(&i).copied());
                    loan.binding = loan.binding.and_then(|i| self.slots.get(&i).copied());
                    loan.dependencies = loan
                        .dependencies
                        .iter()
                        .filter_map(|id| self.loans.get(id).copied())
                        .collect();
                    Op::Loan(loan)
                }
                Op::UseLoan(id) => match self.loans.get(id) {
                    Some(id) => Op::UseLoan(*id),
                    None => continue,
                },
                Op::EndLoan(id) => match self.loans.get(id) {
                    Some(id) => Op::EndLoan(*id),
                    None => continue,
                },
                Op::Passes(values) => Op::Passes(self.list(values)),
                Op::Returning(site, values) => Op::Returning(*site, self.list(values)),
                Op::Building(Building::Open(site, values)) => {
                    Op::Building(Building::Open(*site, self.list(values)))
                }
                Op::Building(Building::Hold(site, values)) => {
                    Op::Building(Building::Hold(*site, self.list(values)))
                }
                Op::Tokens(crate::op::TokenMark::Holds(values)) => {
                    Op::Tokens(crate::op::TokenMark::Holds(self.list(values)))
                }
                Op::Tokens(crate::op::TokenMark::Guard(i)) => {
                    Op::Tokens(crate::op::TokenMark::Guard(self.slot(*i)?))
                }
                Op::Thread(crate::threading::ThreadOp::Start(task, i)) => Op::Thread(
                    crate::threading::ThreadOp::Start(task.clone(), self.slot(*i)?),
                ),
                Op::Thread(crate::threading::ThreadOp::Finish(task, i)) => Op::Thread(
                    crate::threading::ThreadOp::Finish(task.clone(), self.slot(*i)?),
                ),
                Op::Mutex(crate::mutex::MutexOp::Lock(i)) => {
                    Op::Mutex(crate::mutex::MutexOp::Lock(self.slot(*i)?))
                }
                Op::Mutex(crate::mutex::MutexOp::Unlock(i)) => {
                    Op::Mutex(crate::mutex::MutexOp::Unlock(self.slot(*i)?))
                }
                Op::Loop { condition, body } => Op::Loop {
                    condition: self.ops(condition)?.into(),
                    body: self.ops(body)?.into(),
                },
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => Op::If {
                    then: self.ops(then)?.into(),
                    elifs: elifs
                        .iter()
                        .map(|arm| {
                            Ok(ElifArm {
                                condition: self.ops(&arm.condition)?.into(),
                                body: self.ops(&arm.body)?.into(),
                            })
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into(),
                    otherwise: self.ops(otherwise)?.into(),
                },
                Op::Match(arms) => Op::Match(
                    arms.iter()
                        .map(|arm| {
                            Ok(MatchArm {
                                pattern: arm.pattern,
                                body: self.ops(&arm.body)?.into(),
                            })
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into(),
                ),
                Op::Try {
                    source,
                    target,
                    cleanup,
                } => Op::Try {
                    source: source.clone(),
                    target: target.clone(),
                    cleanup: self.ops(cleanup)?.into(),
                },
                Op::CheckedBlock { body, result } => Op::CheckedBlock {
                    body: self.ops(body)?.into(),
                    result: result.clone(),
                },
                Op::CheckedArith {
                    operation,
                    ty,
                    cleanup,
                } => Op::CheckedArith {
                    operation: *operation,
                    ty: ty.clone(),
                    cleanup: self.ops(cleanup)?.into(),
                },
                _ => op.clone(),
            });
        }
        Ok(result)
    }

    fn local(&self, local: &Local) -> Result<Local> {
        Ok(Local {
            slot: self.slot(local.slot)?,
            ty: local.ty.clone(),
            mutable: local.mutable,
        })
    }
}

impl Lower<'_> {
    /// Whether `e` names a pipeline this function may lend: its own `mut`
    /// binding or a lent parameter.
    pub(in crate::frontend) fn lendable(&self, e: &Expr) -> bool {
        let mut place = ungroup(e);
        if let Expression::Unary(op, base) = &place.kind
            && op == "&mut"
        {
            place = ungroup(base);
        }
        let Expression::Name(name) = &place.kind else {
            return false;
        };
        match self.names.get(name) {
            Some(Local {
                ty: Ty::Iter(..), ..
            }) => true,
            Some(local) => self.lent_parameters.contains_key(&local.slot),
            None => false,
        }
    }

    /// The `&mut Iter[T]` type a lendable `arg` would pass, for inferring a
    /// generic borrower's type parameters before the argument is lent.
    pub(in crate::frontend) fn lendable_type(&self, e: &Expr) -> Option<Ty> {
        if !self.lendable(e) {
            return None;
        }
        let mut place = ungroup(e);
        if let Expression::Unary(_, base) = &place.kind {
            place = ungroup(base);
        }
        let Expression::Name(name) = &place.kind else {
            return None;
        };
        match &self.names[name].ty {
            ty @ Ty::Iter(..) => Some(Ty::Ref(Arc::new(ty.clone()), true)),
            ty => Some(ty.clone()),
        }
    }

    /// Lends the pipeline `arg` names to a `&mut Iter[T]` parameter. Pushes
    /// the reference argument and returns its type, which identifies the
    /// lent plan, and its loan. `hidden` collects the state arguments, which
    /// follow the declared ones, and their loans.
    pub(in crate::frontend) fn lend_argument(
        &mut self,
        arg: &Expr,
        expected: &Ty,
        hidden: &mut (Vec<Op>, Vec<usize>),
        after: &mut Vec<usize>,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        if !self.lendable(arg) {
            return Err(arg.at.error(
                "expected `&mut` and an iterator pipeline binding; a pipeline parameter borrows a pipeline its caller created",
            ));
        }
        let mut place = ungroup(arg);
        let explicit = matches!(&place.kind, Expression::Unary(op, _) if op == "&mut");
        if let Expression::Unary(_, base) = &place.kind {
            place = ungroup(base);
        }
        let Expression::Name(name) = &place.kind else {
            return Err(arg.at.error("expected `&mut` and a pipeline binding"));
        };
        let local = self.names[name].clone();
        let (index, reused) = match &local.ty {
            Ty::Iter(_, id) => {
                if !explicit {
                    return Err(arg.at.error("lend a pipeline with `&mut`"));
                }
                if !local.mutable {
                    return Err(arg.at.error(
                        "lending a pipeline requires a mutable binding; declare it with `mut`",
                    ));
                }
                (*id, None)
            }
            _ => {
                let parameter = self.lent_parameters[&local.slot].clone();
                (parameter.plan, Some(parameter))
            }
        };
        let plan = self
            .pipelines
            .get(index)
            .cloned()
            .ok_or_else(|| arg.at.error("iterator pipeline has no local source plan"))?;
        self.lent_item_references(&plan, &arg.at)?;
        self.resumed_ends.extend(
            plan.resources
                .iter()
                .filter(|slot| self.pipeline_ends.contains(slot)),
        );
        self.mark(&arg.at, ops);
        // A pipeline that a loop or terminal consumed has no initialized
        // token to borrow.
        let (ty, loan) = self.borrow_with_indices(place, true, &mut None, ops)?;
        let Ty::Ref(inner, _) = &ty else {
            unreachable!()
        };
        let Ty::Iter(item, _) = &**inner else {
            unreachable!()
        };
        self.same(
            Some(Ty::Iter(item.clone(), usize::MAX)),
            match expected {
                Ty::Ref(inner, _) => Some((**inner).clone()),
                _ => None,
            },
            &arg.at,
        )?;
        let (id, owned, shared) = match reused {
            // Lending a parameter's plan on unchanged passes the same
            // identity, so recursion reuses one specialization.
            Some(parameter) => (parameter.id, parameter.owned, parameter.shared),
            None => self.register_lent(index, &plan, &arg.at)?,
        };
        let (pushes, loans) = hidden;
        for slot in owned {
            let loan = self.new_loan(slot, true, None, pushes);
            pushes.push(Op::BorrowLocal(slot, true));
            loans.push(loan);
        }
        for slot in shared {
            pushes.push(Op::LoadLocal(slot));
        }
        after.extend(plan.loans.iter().copied());
        Ok((
            Ty::Ref(Arc::new(Ty::Iter(item.clone(), LENT_BASE + id)), true),
            loan,
        ))
    }

    /// Records the plan at `index` for borrowers, returning its identity and
    /// the slots whose state it passes.
    fn register_lent(
        &mut self,
        index: usize,
        plan: &Pipeline,
        at: &Token,
    ) -> Result<(usize, Vec<u8>, Vec<u8>)> {
        let mut referenced = BTreeSet::new();
        for ops in iteration_ops(plan) {
            slots(ops, &mut referenced);
        }
        referenced.insert(plan.item.slot);
        referenced.insert(plan.iteration.target);
        referenced.extend(plan.pair.iter().flatten().map(|local| local.slot));
        referenced.extend(plan.split);
        let state: BTreeSet<u8> = plan
            .resources
            .iter()
            .chain(&plan.borrowed)
            .copied()
            .filter(|slot| plan.initialized.contains(slot) || plan.empty.contains(slot))
            .chain(plan.shared.iter().copied())
            .collect();
        let mut owned = Vec::new();
        let mut shared = Vec::new();
        let mut scratch = Vec::new();
        let mut types = BTreeMap::new();
        for &slot in &referenced {
            let ty = self
                .slot_type(slot)
                .expect("a plan slot has a type")
                .0
                .clone();
            if state.contains(&slot) {
                if matches!(ty, Ty::Ref(..)) {
                    shared.push(slot);
                } else if ty.contains_reference() {
                    return Err(self.borrowing_state_error(slot, &ty, at));
                } else {
                    owned.push(slot);
                }
            } else if plan.resources.contains(&slot) {
                scratch.push(slot);
            } else {
                continue;
            }
            types.insert(slot, ty);
        }
        if owned.len() + shared.len() > MAX_LENT_STATE {
            return Err(at.error(format!(
                "this pipeline cannot be lent: its state needs {} slots, above the implementation limit of {MAX_LENT_STATE}",
                owned.len() + shared.len()
            )));
        }
        // A reference held as state is passed as it is. Should it point into
        // owned state, the borrower's copy would leave it pointing at the
        // lender's emptied slot.
        if !shared.is_empty()
            && plan
                .loans
                .iter()
                .any(|&id| owned.contains(&self.loans[id].root))
        {
            return Err(at.error(
                "this pipeline cannot be lent: it holds a reference into its own state; consume it in its creating function",
            ));
        }
        let key = (self.symbol, index);
        let references = self.lent_item_references(plan, at)?;
        let lent = Arc::new(Lent {
            plan: plan.clone(),
            types,
            owned: owned.clone(),
            shared: shared.clone(),
            scratch,
            references,
        });
        let id = match self.generics.lent_ids.get(&key) {
            Some(&id) => {
                self.generics.lent[id] = lent;
                id
            }
            None => {
                self.generics.lent.push(lent);
                let id = self.generics.lent.len() - 1;
                self.generics.lent_ids.insert(key, id);
                id
            }
        };
        Ok((id, owned, shared))
    }

    /// The item slots of `plan` that hold references, each with the loan it
    /// derives from. An item is a reference or an owned value; a pair item
    /// has one of these per component. A reference nested in another value
    /// has no loan of its own to pass on, so it keeps the pipeline local.
    fn lent_item_references(&self, plan: &Pipeline, at: &Token) -> Result<Vec<(u8, usize, bool)>> {
        let items = match &plan.pair {
            Some(pair) => pair.to_vec(),
            None => vec![plan.item.clone()],
        };
        let mut references = Vec::new();
        for item in items {
            let barrier = match &item.ty {
                Ty::Ref(inner, _) => inner.storage_barrier(),
                ty => ty.storage_barrier(),
            };
            if let Some(barrier) = barrier {
                return Err(
                    at.error(barrier.message("a lent pipeline item; map to an owned value first"))
                );
            }
            if let Ty::Ref(_, mutable) = item.ty {
                let Some(&loan) = self.reference_locals.get(&item.slot) else {
                    return Err(at.error(
                        "this pipeline cannot be lent: the source of its reference items is not known here; map to an owned value first",
                    ));
                };
                references.push((item.slot, loan, mutable));
            }
        }
        Ok(references)
    }

    /// Why state `slot`, which holds references inside another value, keeps
    /// its pipeline from being lent. A closure value is reported where the
    /// adapter received it, with the literal form that can be lent.
    fn borrowing_state_error(&self, slot: u8, ty: &Ty, at: &Token) -> Box<dyn Error> {
        match self.pipeline_callbacks.get(&slot) {
            Some((site, source)) if matches!(ty, Ty::Closure(_)) => {
                let what = match source {
                    Some(name) => format!(
                        "closure `{name}`, which is stored in a variable and borrows its captures"
                    ),
                    None => "a closure value that borrows its captures".into(),
                };
                site.error(format!(
                    "this pipeline cannot be lent (at line {}): it holds {what}; write the closure literal inside the adapter call instead, as in `.filter(def [&limit](x: &i64) -> bool: *x > limit)`",
                    at.line
                ))
            }
            _ => at.error(format!(
                "this pipeline cannot be lent: it holds {ty}, which contains a reference, as state; consume it in its creating function"
            )),
        }
    }

    /// Binds each lent parameter of this borrower to its own copy of the
    /// plan. Returns the write-back for the function's normal end; every
    /// other exit runs it as the exit of a scope context.
    pub(in crate::frontend) fn receive_lent(
        &mut self,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Vec<Op>> {
        let inputs = self.sigs[&self.function_name].inputs.clone();
        let mut next = inputs
            .iter()
            .position(|(name, _)| name.starts_with("<lent "))
            .unwrap_or(inputs.len());
        let mut exit = Vec::new();
        let mut resumes = Vec::new();
        for (parameter, (_, ty)) in inputs.iter().enumerate() {
            let Some(id) = identity(ty) else {
                continue;
            };
            let lent = self.generics.lent[id].clone();
            // This plan's hidden parameters start here, owned state first.
            let hidden_start = next as u8;
            let mut map = HashMap::default();
            let mut copies = Vec::new();
            for &slot in &lent.owned {
                let ty = lent.types[&slot].clone();
                let hidden = next as u8;
                next += 1;
                let copy = self.slot(ty.clone(), at)?;
                if ty.managed() {
                    ops.extend([
                        Op::EmptyLocal(copy),
                        Op::MoveLocal(copy),
                        Op::LoadLocal(hidden),
                        Op::ReplaceRef(ty.clone()),
                        Op::StoreLocal(copy),
                    ]);
                    exit.extend([
                        Op::MoveLocal(copy),
                        Op::LoadLocal(hidden),
                        Op::ReplaceRef(ty),
                        Op::Drop,
                    ]);
                } else {
                    ops.extend([
                        Op::LoadLocal(hidden),
                        Op::ReadRef(ty.clone()),
                        Op::StoreLocal(copy),
                    ]);
                    exit.extend([Op::LoadLocal(copy), Op::LoadLocal(hidden), Op::WriteRef(ty)]);
                }
                map.insert(slot, copy);
                copies.push(copy);
            }
            let mut shared = Vec::new();
            for &slot in &lent.shared {
                map.insert(slot, next as u8);
                shared.push(next as u8);
                next += 1;
            }
            let mut scratch = Vec::new();
            for &slot in &lent.scratch {
                let local = self.slot(lent.types[&slot].clone(), at)?;
                map.insert(slot, local);
                scratch.push(local);
            }
            let plan = &lent.plan;
            let mut internal = BTreeMap::new();
            for ops in iteration_ops(plan) {
                loans(ops, &mut internal);
            }
            let mut referenced = BTreeSet::new();
            for ops in iteration_ops(plan) {
                slots(ops, &mut referenced);
            }
            let foreign: BTreeSet<u8> = referenced
                .into_iter()
                .filter(|slot| !map.contains_key(slot))
                .collect();
            let mut renumbered = HashMap::default();
            let first = self.loans.len();
            for (old, loan) in &internal {
                if !foreign.contains(&loan.root) {
                    renumbered.insert(*old, first + renumbered.len());
                }
            }
            let relocation = Relocation {
                slots: map,
                shared: lent.shared.iter().copied().collect(),
                foreign,
                loans: renumbered,
                names: &[],
                at,
            };
            // Loan facts live in the lender's numbering until relocated.
            let mut facts: Vec<_> = internal
                .values()
                .filter(|loan| relocation.loans.contains_key(&loan.id))
                .map(|loan| Op::Loan(loan.clone()))
                .collect::<Vec<_>>();
            facts = relocation.ops(&facts)?;
            let mut facts: Vec<_> = facts
                .into_iter()
                .filter_map(|op| match op {
                    Op::Loan(loan) => Some(loan),
                    _ => None,
                })
                .collect();
            facts.sort_by_key(|loan| loan.id);
            self.loans.extend(facts);
            // An item reference points into a source the lender keeps borrowed
            // for the whole call, which the borrower reaches only through
            // this plan's hidden parameters. Rooting the item's loan at one of
            // them keeps it inside the call: it cannot be returned, and the
            // hidden parameters are only read.
            let mut item_loans = Vec::new();
            for &(slot, old, mutable) in &lent.references {
                let loan = match relocation.loans.get(&old) {
                    Some(&loan) => loan,
                    None => {
                        if lent.owned.is_empty() && lent.shared.is_empty() {
                            return Err(at.error(
                                "this pipeline cannot be lent: the source of its reference items is not known here; map to an owned value first",
                            ));
                        }
                        let loan = self.new_loan(hidden_start, mutable, None, ops);
                        self.loans[loan].precise = false;
                        if let Some(Op::Loan(fact)) = ops.last_mut() {
                            *fact = self.loans[loan].clone();
                        }
                        loan
                    }
                };
                self.lent_item_loans.insert(loan);
                item_loans.push((relocation.slot(slot)?, loan));
            }
            let mut iteration = Iteration {
                exact_len: plan
                    .iteration
                    .exact_len
                    .as_deref()
                    .map(|ops| relocation.ops(ops))
                    .transpose()?,
                cleanup: Vec::new(),
                condition: relocation.ops(&plan.iteration.condition)?,
                absent: relocation.ops(&plan.iteration.absent)?,
                body: relocation.ops(&plan.iteration.body)?,
                step: relocation.ops(&plan.iteration.step)?,
                target: relocation.slot(plan.iteration.target)?,
            };
            // Every copy of the step, including one an adapter runs for an
            // item it rejects, clears `pending`; a consumer that stops with
            // it set took an item without stepping over it.
            let pending = self.slot(Ty::Bool, at)?;
            ops.extend([Op::PushBool(false), Op::StoreLocal(pending)]);
            let resume = vec![
                Op::LoadLocal(pending),
                branch(
                    [
                        iteration.step.as_slice(),
                        &[Op::PushBool(false), Op::StoreLocal(pending)],
                    ]
                    .concat(),
                    vec![],
                ),
            ];
            iteration
                .body
                .splice(0..0, [Op::PushBool(true), Op::StoreLocal(pending)]);
            iteration
                .step
                .splice(0..0, [Op::PushBool(false), Op::StoreLocal(pending)]);
            resumes.extend(resume.iter().cloned());
            let relocated = Pipeline {
                iteration,
                item: relocation.local(&plan.item)?,
                pair: plan
                    .pair
                    .as_ref()
                    .map(|pair| -> Result<[Local; 2]> {
                        Ok([relocation.local(&pair[0])?, relocation.local(&pair[1])?])
                    })
                    .transpose()?,
                resources: scratch,
                initialized: copies.iter().copied().collect(),
                empty: Vec::new(),
                split: plan.split.map(|slot| relocation.slot(slot)).transpose()?,
                owned: plan.owned,
                loans: Vec::new(),
                depth: plan.depth,
                lent: true,
                borrowed: copies.clone(),
                shared: shared.clone(),
                resume,
            };
            self.pipelines.push(relocated);
            self.reference_locals.extend(item_loans);
            self.lent_parameters.insert(
                parameter as u8,
                LentParameter {
                    plan: self.pipelines.len() - 1,
                    id,
                    owned: copies,
                    shared,
                },
            );
        }
        resumes.extend(exit);
        let exit = resumes;
        if !exit.is_empty() {
            let marker = self.slot(Ty::Bool, at)?;
            self.contexts.push(contexts::Context {
                slot: marker,
                exit: exit.clone(),
                loan: None,
                kind: contexts::Kind::Lent,
                at: site(at, false),
            });
        }
        Ok(exit)
    }

    /// The plan of a lent parameter, held by a loan of the parameter for as
    /// long as the consumer or binding built from it lives.
    pub(in crate::frontend) fn lent_plan(
        &mut self,
        e: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Pipeline> {
        let Expression::Name(name) = &ungroup(e).kind else {
            unreachable!()
        };
        let slot = self.names[name].slot;
        let index = self.lent_parameters[&slot].plan;
        self.mark(&e.at, ops);
        self.borrow_with_indices(e, true, &mut None, ops)?;
        ops.push(Op::Drop);
        Ok(self.pipelines[index].clone())
    }
}
