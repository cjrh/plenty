//! The intervals the checker derives while it lowers integer expressions.
//! The rules are in `book/src/design/language/03-types/intervals.md`.
//!
//! An interval is recorded by the code that lowers the operation producing
//! the value, under the address of that expression's token, and read by the
//! code that lowers the expression as an operand. A reader clears the record
//! before lowering its operand, so it only ever sees what that lowering
//! recorded, and an expression nothing records has the full range of its
//! type.
use super::*;
use crate::interval::{Interval, Operator, Outcome};
use crate::numeric::Divisor;
use crate::op::IntegerDomain;

/// The operand intervals of one binary operation, and its own token.
#[derive(Clone, Copy, Default)]
pub(super) struct Operands {
    pub(super) node: usize,
    pub(super) left: Option<Interval>,
    pub(super) right: Option<Interval>,
}

/// The identity of the expression whose token is `at`.
pub(super) fn node(at: &Token) -> usize {
    std::ptr::from_ref(at) as usize
}

impl Lower<'_> {
    /// Record that the value just lowered for the expression at `at`, of
    /// type `ty`, lies in `interval`.
    pub(super) fn note_interval(&mut self, at: usize, ty: &Ty, interval: Interval) {
        if at != 0 && Interval::of(ty).is_some_and(|full| interval.within(full)) {
            self.interval = Some((at, ty.clone(), interval));
        }
    }

    /// The interval of `operand`, lowered just now with type `ty` after
    /// `self.interval` was cleared: a literal or module constant, an
    /// immutable local, or what the lowering recorded. `None` when `ty` is
    /// not an integer; otherwise at most the type's range.
    pub(super) fn operand_interval(&self, operand: &Expr, ty: &Ty) -> Option<Interval> {
        let full = Interval::of(ty)?;
        let operand = ungroup(operand);
        let known = if let Some(value) = self.known_integer(operand) {
            Some(Interval::point(value))
        } else if let Expression::Name(name) = &operand.kind {
            self.names
                .get(name)
                .filter(|local| !local.mutable && local.ty == *ty)
                .and_then(|local| self.slot_intervals.get(&local.slot).copied())
        } else {
            self.interval
                .as_ref()
                .filter(|(at, recorded, _)| *at == node(&operand.at) && recorded == ty)
                .map(|(_, _, interval)| *interval)
        };
        Some(known.and_then(|known| known.meet(full)).unwrap_or(full))
    }

    /// What the operand intervals prove about the integer operator `op`.
    pub(super) fn outcome(&self, op: &str, ty: &Ty, operands: Operands) -> Option<Outcome> {
        let full = Interval::of(ty)?;
        let operator = Operator::named(op)?;
        Some(Interval::operation(
            operator,
            ty,
            operands.left.unwrap_or(full),
            operands.right.unwrap_or(full),
        ))
    }

    /// Record the result of an operation that cannot fail. One that can
    /// fail records nothing, so its result keeps the full range, also in an
    /// `unchecked:` suite, where the failure traps.
    pub(super) fn note_outcome(&mut self, at: usize, ty: &Ty, outcome: Option<Outcome>) {
        if let Some(outcome) = outcome.filter(|outcome| outcome.proved()) {
            self.note_interval(at, ty, outcome.value);
        }
    }

    /// The proof the operand intervals give `//` or `%`.
    pub(super) fn divisor_proof(&self, op: &str, ty: &Ty, operands: Operands) -> Divisor {
        let Some(full) = Interval::of(ty).filter(|_| matches!(op, "//" | "%")) else {
            return Divisor::Any;
        };
        operands
            .left
            .unwrap_or(full)
            .divisor(ty, operands.right.unwrap_or(full))
    }

    /// The interval `.get()` returns on a receiver of type `receiver`, with
    /// the integer type it returns: the bounds of a `Bounded`, and the
    /// lower bound of `NonNegative`, `Positive` and an unsigned `NonZero`.
    pub(super) fn domain_interval(&self, receiver: &Ty) -> Option<(Ty, Interval)> {
        let receiver = match receiver {
            Ty::Ref(inner, _) => inner,
            other => other,
        };
        if let Some((element, lo, hi)) = self.aliases.bounded(receiver) {
            return Some((element, Interval::new(lo.value, hi.value)?));
        }
        let (domain, element) = receiver.integer_domain()?;
        let full = Interval::of(&element)?;
        let lo = match domain {
            IntegerDomain::NonNegative => 0,
            IntegerDomain::Positive => 1,
            IntegerDomain::NonZero if full.lo == 0 => 1,
            IntegerDomain::NonZero => return None,
        };
        Some((element, Interval::new(lo, full.hi)?))
    }
}
