//! Last-use moves of copyable owned locals (issue #388).
//!
//! The frontend reads a copyable local with `LoadLocal`, which counts a new
//! owner, and the slot's own owner is released later, by a `StoreLocal`, a
//! `DropLocal` or the function's exit. When no later operation can read the
//! slot, the read can take the slot's value instead: `MoveLocal` leaves zero
//! behind, which every later release already treats as an empty slot. A
//! `match` on an owned parameter and a payload binding read once are the
//! common cases.
//!
//! Only copyable types qualify. They have no destructor, so ending their last
//! owner earlier changes no observable cleanup, and the ownership checker has
//! already accepted the frontend's operations, so diagnostics are unchanged.
//! Borrowed locals are left alone, because a reference reads the slot without
//! naming it. Generator bodies are left alone too.
use std::sync::Arc;

use crate::hash::HashMap;
use crate::op::{CompiledFn, ElifArm, MatchArm, Op, Ty};

/// A set of local slots, which are numbered by `u8`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
struct Slots([u64; 4]);

impl Slots {
    fn contains(&self, slot: u8) -> bool {
        self.0[slot as usize >> 6] & (1 << (slot & 63)) != 0
    }
    fn insert(&mut self, slot: u8) {
        self.0[slot as usize >> 6] |= 1 << (slot & 63);
    }
    fn remove(&mut self, slot: u8) {
        self.0[slot as usize >> 6] &= !(1 << (slot & 63));
    }
    fn union(mut self, other: Slots) -> Slots {
        for (word, other) in self.0.iter_mut().zip(other.0) {
            *word |= other;
        }
        self
    }
}

/// Rewrite every function defined in `ops`, nested definitions included.
pub(super) fn prepare(ops: &mut [Op], workers: usize) {
    let rewrites: Vec<Option<Op>> =
        crate::parallel::map(ops.len(), workers, |index| match &ops[index] {
            Op::DefineFn(name, f) => function(f).map(|f| Op::DefineFn(name.clone(), f)),
            _ => None,
        });
    for (op, rewrite) in ops.iter_mut().zip(rewrites) {
        if let Some(rewrite) = rewrite {
            *op = rewrite;
        }
    }
}

/// `f` with its last-use loads turned into moves, or `None` when nothing
/// changes.
fn function(f: &CompiledFn) -> Option<Box<CompiledFn>> {
    let types: Vec<&Ty> = f
        .sig
        .inputs
        .iter()
        .map(|(_, ty)| ty)
        .chain(f.locals.iter())
        .collect();
    let mut borrowed = vec![false; types.len()];
    super::mark_borrowed_locals(&f.body, &mut borrowed);
    let mut liveness = Liveness::default();
    if f.generator.is_none() {
        for (slot, ty) in types.iter().enumerate() {
            if !borrowed[slot] && ty.managed() && ty.can_copy() && !ty.has_destructor() {
                liveness.eligible.insert(slot as u8);
                if !ty.wide() && !matches!(ty, Ty::Closure(_)) {
                    liveness.one_word.insert(slot as u8);
                }
            }
        }
    }
    let (_, body) = liveness.sequence(&f.body, Slots::default(), true);
    let mut f = Box::new(f.clone());
    f.body = body?.into();
    Some(f)
}

#[derive(Default)]
struct Liveness {
    /// Copyable, managed, never borrowed slots.
    eligible: Slots,
    /// Eligible slots whose copy is one counted word; see [`Liveness::op`].
    one_word: Slots,
    /// For each enclosing loop, innermost last: the slots live at its
    /// condition, where `continue` goes, and after it, where `break` goes.
    loops: Vec<(Slots, Slots)>,
    /// A loop's slots live at its condition and at its body's start, by the
    /// addresses of its condition and body and the slots live after it. The rewrite reuses
    /// what the analysis computed, so nested loops are not solved again.
    solved: HashMap<(usize, usize, Slots), (Slots, Slots)>,
}

impl Liveness {
    /// The slots `ops` may read before writing them, given the slots `live`
    /// after it. With `rewrite`, also the rewritten operations, when a load
    /// in them changes.
    fn sequence(&mut self, ops: &[Op], mut live: Slots, rewrite: bool) -> (Slots, Option<Vec<Op>>) {
        let mut changes = Vec::new();
        for (index, op) in ops.iter().enumerate().rev() {
            let (before, changed) = self.op(op, ops.get(index + 1), live, rewrite);
            live = before;
            if let Some(changed) = changed {
                changes.push((index, changed));
            }
        }
        if changes.is_empty() {
            return (live, None);
        }
        let mut rewritten = ops.to_vec();
        for (index, op) in changes {
            rewritten[index] = op;
        }
        (live, Some(rewritten))
    }

    fn nested(
        &mut self,
        ops: &Arc<[Op]>,
        live: Slots,
        rewrite: bool,
        changed: &mut bool,
    ) -> (Slots, Arc<[Op]>) {
        match self.sequence(ops, live, rewrite) {
            (before, Some(rewritten)) => {
                *changed = true;
                (before, rewritten.into())
            }
            (before, None) => (before, Arc::clone(ops)),
        }
    }

    fn op(
        &mut self,
        op: &Op,
        next: Option<&Op>,
        mut live: Slots,
        rewrite: bool,
    ) -> (Slots, Option<Op>) {
        let mut changed = false;
        let rewritten = match op {
            Op::LoadLocal(slot) => {
                let last = self.eligible.contains(*slot) && !live.contains(*slot)
                    // A collection query right after a load of one counted
                    // word borrows the slot's value instead of counting a
                    // copy (issue #379), which costs less than a move.
                    && !(self.one_word.contains(*slot) && matches!(next, Some(Op::Collection(_))));
                live.insert(*slot);
                return (live, (rewrite && last).then_some(Op::MoveLocal(*slot)));
            }
            Op::MoveLocal(slot)
            | Op::BorrowLocal(slot, _)
            | Op::Next(slot)
            | Op::Mutex(crate::mutex::MutexOp::Lock(slot) | crate::mutex::MutexOp::Unlock(slot))
            | Op::Thread(
                crate::threading::ThreadOp::Start(_, slot)
                | crate::threading::ThreadOp::Finish(_, slot),
            ) => {
                live.insert(*slot);
                return (live, None);
            }
            // A release of an emptied slot does nothing.
            Op::StoreLocal(slot) | Op::DropLocal(slot) => {
                live.remove(*slot);
                return (live, None);
            }
            // Exits release every local; an emptied slot owns nothing.
            Op::Return | Op::TailCall(_) | Op::TailCallIndirect(_) | Op::Unreachable => {
                return (Slots::default(), None);
            }
            Op::Break => return (self.loops.last().map_or(live, |loop_| loop_.1), None),
            Op::Continue => return (self.loops.last().map_or(live, |loop_| loop_.0), None),
            // The error path runs `cleanup` and returns.
            Op::Try {
                source,
                target,
                cleanup,
            } => {
                let (failing, cleanup) =
                    self.nested(cleanup, Slots::default(), rewrite, &mut changed);
                live = live.union(failing);
                changed.then(|| Op::Try {
                    source: source.clone(),
                    target: target.clone(),
                    cleanup,
                })
            }
            Op::Match(arms) => {
                let after = live;
                let arms: Arc<[MatchArm]> = arms
                    .iter()
                    .map(|arm| {
                        let (before, body) = self.nested(&arm.body, after, rewrite, &mut changed);
                        live = live.union(before);
                        MatchArm {
                            pattern: arm.pattern,
                            body,
                        }
                    })
                    .collect();
                changed.then_some(Op::Match(arms))
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                let after = live;
                let (mut rest, otherwise) = self.nested(otherwise, after, rewrite, &mut changed);
                let mut rewritten = Vec::with_capacity(elifs.len());
                for arm in elifs.iter().rev() {
                    let (body_start, body) = self.nested(&arm.body, after, rewrite, &mut changed);
                    let (start, condition) = self.nested(
                        &arm.condition,
                        body_start.union(rest),
                        rewrite,
                        &mut changed,
                    );
                    rest = start;
                    rewritten.push(ElifArm { condition, body });
                }
                rewritten.reverse();
                let (then_start, then) = self.nested(then, after, rewrite, &mut changed);
                live = then_start.union(rest);
                changed.then(|| Op::If {
                    then,
                    elifs: rewritten.into(),
                    otherwise,
                })
            }
            Op::Loop { condition, body } => {
                let exit = live;
                let (header, body_start) = self.solve(condition, body, exit);
                live = header;
                if !rewrite {
                    return (live, None);
                }
                self.loops.push((header, exit));
                let (_, body) = self.nested(body, header, true, &mut changed);
                self.loops.pop();
                let (_, condition) =
                    self.nested(condition, exit.union(body_start), true, &mut changed);
                changed.then_some(Op::Loop { condition, body })
            }
            Op::DefineFn(name, f) if rewrite => function(f).map(|f| Op::DefineFn(name.clone(), f)),
            _ => None,
        };
        (live, rewritten)
    }

    /// The slots live at a loop's condition and at its body's start, given
    /// the slots live after it. Liveness only grows with each pass, so
    /// iterating from the empty set reaches the least fixed point.
    fn solve(&mut self, condition: &Arc<[Op]>, body: &Arc<[Op]>, exit: Slots) -> (Slots, Slots) {
        let key = (condition.as_ptr() as usize, body.as_ptr() as usize, exit);
        if let Some(&solved) = self.solved.get(&key) {
            return solved;
        }
        let mut header = Slots::default();
        loop {
            self.loops.push((header, exit));
            let (body_start, _) = self.sequence(body, header, false);
            self.loops.pop();
            let (start, _) = self.sequence(condition, exit.union(body_start), false);
            if start == header {
                self.solved.insert(key, (header, body_start));
                return (header, body_start);
            }
            header = start;
        }
    }
}
