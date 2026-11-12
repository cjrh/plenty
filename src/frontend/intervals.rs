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

/// What the conditions of an `if` statement being lowered prove.
pub(super) struct IfNarrowing {
    /// The narrowing outside the statement.
    outer: HashMap<u8, Interval>,
    /// The facts of the suite being lowered.
    suite: Vec<(u8, Interval)>,
    /// What the failure of every condition so far proves.
    earlier: Vec<(u8, Interval)>,
    /// The facts of each suite that continues past the statement.
    continuing: Vec<Vec<(u8, Interval)>>,
}

/// The comparison that holds exactly when `op` does not.
fn negated(op: &str) -> &'static str {
    match op {
        "<" => ">=",
        "<=" => ">",
        ">" => "<=",
        ">=" => "<",
        "==" => "!=",
        "!=" => "==",
        _ => "",
    }
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
                .and_then(|local| self.local_interval(local.slot))
        } else {
            self.interval
                .as_ref()
                .filter(|(at, recorded, _)| *at == node(&operand.at) && recorded == ty)
                .map(|(_, _, interval)| *interval)
        };
        Some(known.and_then(|known| known.meet(full)).unwrap_or(full))
    }

    /// What the binding and the dominating comparisons prove about the
    /// immutable integer local in `slot`.
    fn local_interval(&self, slot: u8) -> Option<Interval> {
        let bound = self.slot_intervals.get(&slot).copied();
        match (bound, self.narrowed.get(&slot).copied()) {
            (Some(bound), Some(narrowed)) => Some(bound.meet(narrowed).unwrap_or(bound)),
            (bound, narrowed) => bound.or(narrowed),
        }
    }

    /// Record the operand intervals of the integer comparison `op` at
    /// `node`, which [`Lower::condition_facts`] reads.
    pub(super) fn note_comparison(&mut self, op: &str, ty: &Ty, operands: Operands) {
        if let (true, Some(full)) = (is_comparison(op), Interval::of(ty)) {
            let interval = |side: Option<Interval>| side.unwrap_or(full);
            self.comparisons.insert(
                operands.node,
                (
                    ty.clone(),
                    interval(operands.left),
                    interval(operands.right),
                ),
            );
        }
    }

    /// Forget the comparisons and connectives that `condition_facts` would
    /// read in `condition`, before it is lowered, so only this lowering's
    /// records count.
    pub(super) fn forget_condition(&mut self, condition: &Expr) {
        let condition = ungroup(condition);
        match &condition.kind {
            Expression::Unary(op, inner) if op == "not" => self.forget_condition(inner),
            Expression::Binary(op, left, right) => {
                let node = node(&condition.at);
                self.comparisons.remove(&node);
                if op == "and" || op == "or" {
                    self.connectives.remove(&node);
                    self.forget_condition(left);
                    self.forget_condition(right);
                }
            }
            _ => {}
        }
    }

    /// The intervals that `condition`, just lowered, proves for immutable
    /// integer locals when it evaluates to `truth`. A comparison of such a
    /// local with any integer operand bounds the local by that operand's
    /// interval (`!=` bounds nothing); `not` flips `truth`; the operands of
    /// a true `and` and a false `or` all hold. Nothing else proves anything.
    pub(super) fn condition_facts(&self, condition: &Expr, truth: bool) -> Vec<(u8, Interval)> {
        let mut facts = Vec::new();
        self.collect_facts(condition, truth, &mut facts);
        facts
    }

    fn collect_facts(&self, condition: &Expr, truth: bool, facts: &mut Vec<(u8, Interval)>) {
        let condition = ungroup(condition);
        match &condition.kind {
            Expression::Unary(op, inner) if op == "not" => self.collect_facts(inner, !truth, facts),
            Expression::Binary(op, left, right)
                if (op == "and" && truth) || (op == "or" && !truth) =>
            {
                if self.connectives.contains(&node(&condition.at)) {
                    self.collect_facts(left, truth, facts);
                    self.collect_facts(right, truth, facts);
                }
            }
            Expression::Binary(op, left, right) => {
                let Some((ty, l, r)) = self.comparisons.get(&node(&condition.at)) else {
                    return;
                };
                let op = if truth { op.as_str() } else { negated(op) };
                let Some(full) = Interval::of(ty) else {
                    return;
                };
                let (left_bound, right_bound) = match op {
                    "<" => (
                        Interval::new(full.lo, r.hi - 1),
                        Interval::new(l.lo + 1, full.hi),
                    ),
                    "<=" => (Interval::new(full.lo, r.hi), Interval::new(l.lo, full.hi)),
                    ">" => (
                        Interval::new(r.lo + 1, full.hi),
                        Interval::new(full.lo, l.hi - 1),
                    ),
                    ">=" => (Interval::new(r.lo, full.hi), Interval::new(full.lo, l.hi)),
                    "==" => (Some(*r), Some(*l)),
                    _ => (None, None),
                };
                for (side, bound) in [(left, left_bound), (right, right_bound)] {
                    if let (Some(slot), Some(bound)) = (self.subject(side, ty), bound) {
                        facts.push((slot, bound));
                    }
                }
            }
            _ => {}
        }
    }

    /// The slot of `operand` when it names an immutable local of the
    /// integer type `ty`: a parameter, a loop variable or an immutable
    /// binding.
    fn subject(&self, operand: &Expr, ty: &Ty) -> Option<u8> {
        let Expression::Name(name) = &ungroup(operand).kind else {
            return None;
        };
        self.names
            .get(name)
            .filter(|local| !local.mutable && local.ty == *ty)
            .map(|local| local.slot)
    }

    /// Narrow each local in `facts` to its bound. A bound that excludes
    /// everything the local can hold marks code that cannot run, and keeps
    /// the local's interval.
    pub(super) fn narrow(&mut self, facts: &[(u8, Interval)]) {
        for &(slot, bound) in facts {
            let narrowed = match self.local_interval(slot) {
                Some(current) => current.meet(bound).unwrap_or(current),
                None => bound,
            };
            self.narrowed.insert(slot, narrowed);
        }
    }

    // The `if` statement's narrowing lives in these outlined methods and a
    // boxed state, so the statement loop's frame stays small enough for the
    // nesting limit on a worker stack (tests/test_nesting.rs).

    /// Start narrowing the `if` statement whose `condition` was just
    /// lowered, and narrow its first suite.
    #[inline(never)]
    pub(super) fn if_narrowing(&mut self, condition: &Expr) -> Box<IfNarrowing> {
        let narrowing = Box::new(IfNarrowing {
            outer: self.narrowed.clone(),
            suite: self.condition_facts(condition, true),
            earlier: self.condition_facts(condition, false),
            continuing: Vec::new(),
        });
        self.narrow(&narrowing.suite);
        narrowing
    }

    /// Lower an `elif` condition under the earlier conditions' failure.
    #[inline(never)]
    pub(super) fn elif_condition_narrowing(&mut self, narrowing: &IfNarrowing, condition: &Expr) {
        self.narrow(&narrowing.earlier);
        self.forget_condition(condition);
    }

    /// Record what the `elif` `condition`, just lowered, proves for its
    /// suite and for the later ones.
    #[inline(never)]
    pub(super) fn elif_narrowing(&mut self, narrowing: &mut IfNarrowing, condition: &Expr) {
        let mut suite = narrowing.earlier.clone();
        suite.extend(self.condition_facts(condition, true));
        narrowing
            .earlier
            .extend(self.condition_facts(condition, false));
        narrowing.suite = suite;
        self.narrowed = narrowing.outer.clone();
    }

    /// Narrow the suite of the `elif` most recently recorded.
    #[inline(never)]
    pub(super) fn enter_suite_narrowing(&mut self, narrowing: &IfNarrowing) {
        self.narrow(&narrowing.suite);
    }

    /// Narrow the `else` suite, which runs when every condition failed.
    #[inline(never)]
    pub(super) fn else_narrowing(&mut self, narrowing: &mut IfNarrowing) {
        narrowing.suite = narrowing.earlier.clone();
        self.narrow(&narrowing.suite);
    }

    /// Restore the narrowing outside the suite just lowered, noting its
    /// facts when it continues (`result` is `None` after an error).
    #[inline(never)]
    pub(super) fn suite_narrowed(
        &mut self,
        narrowing: &mut IfNarrowing,
        result: Option<&BlockResult>,
    ) {
        self.narrowed = narrowing.outer.clone();
        if matches!(result, Some(BlockResult::Continues(_))) {
            let suite = std::mem::take(&mut narrowing.suite);
            narrowing.continuing.push(suite);
        }
    }

    /// When exactly one suite continues, what follows the `if` in the
    /// enclosing suite runs under that suite's facts.
    #[inline(never)]
    pub(super) fn after_if_narrowing(&mut self, narrowing: &IfNarrowing) {
        if let [only] = &narrowing.continuing[..] {
            self.narrow(only);
        }
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
