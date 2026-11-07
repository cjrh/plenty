//! Range facts about the integer counter of a `while` loop (issue #369).
//!
//! A loop such as `while i < n: ...; i = i + 1` keeps checked arithmetic, but
//! two of its checks are provably redundant:
//!
//! - The step runs only after the condition held. If `i` has not changed since,
//!   `i < n` bounds `i` below the type's maximum, so `i + 1` cannot overflow.
//!   [`Counter::proven_step`] names that step; it lowers to a plain add.
//! - A signed counter that starts at a nonnegative constant and is only ever
//!   increased by checked steps stays nonnegative. Floor modulo and floor
//!   division by a nonnegative divisor then need no sign correction; see
//!   [`Facts`].
//!
//! A counted `for i in range(...)` loop gets the same two facts by a different
//! argument; see [`RangeLoop`].
//!
//! A step is a trapping `Add`/`Sub` or, in a function with a Result
//! contract, the same `CheckedArith`; the proofs hold for both.
//!
//! The analysis matches operations structurally and gives up on anything it
//! does not recognize. It applies only to counters held in CLIF variables, so
//! no reference can write them behind the operations' back.
use cranelift_codegen::ir::{DataFlowGraph, Value};

use crate::hash::{HashMap, HashSet};
use crate::numeric::Checked as Arith;
use crate::op::{Op, Ty};

/// What the analysis proved about one loop's counter.
pub(super) struct Counter {
    pub local: u8,
    /// Body index of the first step's `Add` or `Sub`, when that step cannot
    /// overflow.
    pub proven_step: Option<usize>,
    /// Every write to the counter inside the loop is a checked step that adds
    /// a nonnegative constant.
    pub only_grows: bool,
    /// The bound's local and its first step's body index, when the bound is
    /// also a counter whose step cannot overflow: in `while lo < hi`, with
    /// `lo` the counter, `hi -= 1` cannot pass `lo` (issue #720).
    pub bound_step: Option<(u8, usize)>,
}

/// Facts the lowerer holds while it lowers a function.
#[derive(Default)]
pub(super) struct Facts {
    /// Signed counter locals of the loops being lowered whose every value is
    /// nonnegative, one entry per loop that established it.
    pub nonnegative_locals: Vec<u8>,
    /// The length, first element, step and stop of each 64-bit `range`
    /// whose length this function computed; see [`RangeLoop`].
    pub ranges: Vec<(Value, Value, Value, Value)>,
    /// SSA values known to be nonnegative. A fact about an SSA value holds
    /// wherever the value is used.
    pub nonnegative: HashSet<Value>,
    /// Row bounds checks done on the current path; see [`Checked`].
    pub checked: Checked,
    /// Facts that prove a row index below a list's length; see [`Bounds`].
    pub bounds: Bounds,
}

/// Facts that prove a scalar list index in bounds, so its check is dead
/// (issue #728).
///
/// The referent of a shared list parameter cannot change while the function
/// runs: the caller's loan outlives the call. Every length read through the
/// parameter's entry value, or a copy of it, is therefore the same number.
/// Other shared references are not stable this way. A reborrow of an
/// exclusive reference has the same native value as its source, and once the
/// reborrow ends the source may clear or replace the list (issue #736). An
/// inlined body lowers with its own facts, so its parameters' stability ends
/// with the call. A slice view's count is part of the view's value. Two
/// length values read through the same stable reference or view are
/// therefore equal, and an index known to be nonnegative and below one of
/// them is in bounds for the other.
///
/// Values are compared by their canonical value: inside a loop, a local the
/// loop never writes has a fresh SSA value at each read (its header block is
/// unsealed while the body lowers), so each read maps to the value the local
/// held on entry.
#[derive(Default)]
pub(super) struct Bounds {
    /// Locals the loops being lowered never write, with their entry values.
    pub invariant_locals: Vec<(u8, Value)>,
    pub canonical: HashMap<Value, Value>,
    /// Entry values of the shared list parameters.
    pub stable_lists: HashSet<Value>,
    /// Canonical length values and the reference or view each was read from.
    pub lengths: HashMap<Value, Value>,
    /// Counter locals of the loops being lowered and the value every one of
    /// their reads in the body is below.
    pub below_locals: Vec<(u8, Value)>,
    /// Values and a value each is below.
    pub below: HashMap<Value, Value>,
    /// The versioned loop copy being lowered, if any; see [`Version`].
    pub version: Option<Version>,
}

impl Bounds {
    pub(super) fn canonical(&self, dfg: &DataFlowGraph, value: Value) -> Value {
        let value = dfg.resolve_aliases(value);
        self.canonical.get(&value).copied().unwrap_or(value)
    }

    /// Whether `index` is below `length`; the caller still proves it
    /// nonnegative.
    pub(super) fn below(&self, dfg: &DataFlowGraph, index: Value, length: Value) -> bool {
        let index = dfg.resolve_aliases(index);
        let Some(&bound) = self.below.get(&index) else {
            return false;
        };
        let (bound, length) = (self.canonical(dfg, bound), self.canonical(dfg, length));
        bound == length
            || self
                .lengths
                .get(&bound)
                .is_some_and(|list| self.lengths.get(&length) == Some(list))
    }
}

/// The locals `ops` and their nested bodies read and those they may write.
pub(super) fn local_uses(ops: &[Op], read: &mut HashSet<u8>, written: &mut HashSet<u8>) {
    for op in ops {
        match op {
            Op::LoadLocal(i) => {
                read.insert(*i);
            }
            Op::StoreLocal(i)
            | Op::BorrowLocal(i, _)
            | Op::MoveLocal(i)
            | Op::DropLocal(i)
            | Op::EmptyLocal(i)
            | Op::Next(i) => {
                written.insert(*i);
            }
            Op::Loop { condition, body } => {
                local_uses(condition, read, written);
                local_uses(body, read, written);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                local_uses(then, read, written);
                for arm in elifs.iter() {
                    local_uses(&arm.condition, read, written);
                    local_uses(&arm.body, read, written);
                }
                local_uses(otherwise, read, written);
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    local_uses(&arm.body, read, written);
                }
            }
            Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => {
                local_uses(cleanup, read, written);
            }
            Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
                local_uses(body, read, written);
            }
            _ => {}
        }
    }
}

/// Row bounds checks already done, for reuse by a later access of the same
/// row (issue #721). Each check, like each trap guard, ends by switching to
/// a fresh block that only its own block reaches, so every check recorded
/// along that chain of blocks dominates its end, `block`. Lowering in any
/// other block forgets them.
#[derive(Default)]
pub(super) struct Checked {
    block: Option<cranelift_codegen::ir::Block>,
    /// Row keys, row sizes, and the rows' addresses.
    rows: Vec<(RowKey, i64, Value)>,
    /// Views split into their rows and count since `block`, so repeated
    /// accesses through one view compare the same values.
    splits: Vec<(Value, (Value, Value))>,
}

/// The rows and length values of a checked access, and its index: a
/// constant by value, since each literal is a separate SSA value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct RowKey {
    pub rows: Value,
    pub length: Value,
    pub index: Result<i64, Value>,
}

/// Enough for one statement's accesses; older checks are dropped first.
const CHECKED_ROWS: usize = 16;

impl Checked {
    pub(super) fn find(
        &mut self,
        block: Option<cranelift_codegen::ir::Block>,
        key: RowKey,
        row_bytes: i64,
    ) -> Option<Value> {
        if self.block != block {
            self.forget();
            return None;
        }
        self.rows
            .iter()
            .find(|(k, bytes, _)| *k == key && *bytes == row_bytes)
            .map(|&(_, _, address)| address)
    }

    /// Records a check made in `from` that continues in `valid`.
    pub(super) fn record(
        &mut self,
        from: Option<cranelift_codegen::ir::Block>,
        valid: cranelift_codegen::ir::Block,
        key: RowKey,
        row_bytes: i64,
        address: Value,
    ) {
        if self.block != from {
            self.forget();
        }
        if self.rows.len() == CHECKED_ROWS {
            self.rows.remove(0);
        }
        self.rows.push((key, row_bytes, address));
        self.block = Some(valid);
    }

    /// Extends the chain when lowering continues from `from` into `to`,
    /// a block only `from` reaches.
    pub(super) fn continue_to(
        &mut self,
        from: Option<cranelift_codegen::ir::Block>,
        to: cranelift_codegen::ir::Block,
    ) {
        if self.block.is_some() && self.block == from {
            self.block = Some(to);
        }
    }

    fn forget(&mut self) {
        self.rows.clear();
        self.splits.clear();
        self.block = None;
    }

    /// The rows and count of `view` split earlier on this path, if any.
    pub(super) fn split(
        &mut self,
        block: Option<cranelift_codegen::ir::Block>,
        view: Value,
    ) -> Option<(Value, Value)> {
        if self.block != block {
            self.forget();
            self.block = block;
            return None;
        }
        self.splits
            .iter()
            .find(|(v, _)| *v == view)
            .map(|&(_, split)| split)
    }

    pub(super) fn record_split(&mut self, view: Value, split: (Value, Value)) {
        if self.splits.len() == CHECKED_ROWS {
            self.splits.remove(0);
        }
        self.splits.push((view, split));
    }
}

/// The slots of a counted `range` loop, as `counted_iteration` in
/// `frontend/collections.rs` builds it: the loop runs while `remaining > 0`,
/// and each pass ends by decrementing `remaining` and adding `advance` to
/// `target`, the loop variable.
///
/// When the loop starts at a range's first element with `remaining` its
/// length and `advance` its step, and that step is 1, every value of `target`
/// in the body is an element of the range, so `target + 1` is at most the
/// range's stop and cannot overflow; the value after the last pass is the
/// stop. A nonnegative start then keeps `target` nonnegative. Both hold only
/// if nothing else writes these slots: [`analyze_range`] accepts no write to
/// them except the step itself, at the end of the body or followed by
/// `continue`.
pub(super) struct RangeLoop {
    pub target: u8,
    pub remaining: u8,
    pub advance: u8,
    /// Body indices of top-level `target + 1` additions.
    pub unit_steps: Vec<usize>,
}

/// Match a counted `range` loop; [`RangeLoop`] states what the caller must
/// still check about the slots' values on entry.
pub(super) fn analyze_range(condition: &[Op], body: &[Op]) -> Option<RangeLoop> {
    use crate::op::IntegerOp;
    use crate::value::Value as V;
    let [Op::LoadLocal(remaining), Op::PushInt(V::I64(0)), Op::Gt] = condition else {
        return None;
    };
    let remaining = *remaining;
    let [
        Op::LoadLocal(r),
        Op::PushInt(V::I64(1)),
        Op::Integer(IntegerOp::WrappingSub),
        Op::StoreLocal(r2),
        Op::LoadLocal(target),
        Op::LoadLocal(advance),
        Op::Integer(IntegerOp::WrappingAdd),
        Op::StoreLocal(t2),
    ] = body.get(body.len().checked_sub(STEP_OPS)?..)?
    else {
        return None;
    };
    let (target, advance) = (*target, *advance);
    if *r != remaining || *r2 != remaining || *t2 != target {
        return None;
    }
    let slots = [target, remaining, advance];
    if target == advance || target == remaining || advance == remaining {
        return None;
    }
    let step = &body[body.len() - STEP_OPS..];
    if !only_steps(&body[..body.len() - STEP_OPS], &slots, step) {
        return None;
    }
    let unit_steps = (0..body.len() - STEP_OPS)
        .filter(|&index| {
            let mut before = body[..index].iter().rev().filter(|op| !transparent(op));
            matches!(
                (arith_kind(&body[index]), before.next(), before.next()),
                (Some(Arith::Add), Some(Op::PushInt(k)), Some(Op::LoadLocal(i)))
                    | (Some(Arith::Add), Some(Op::LoadLocal(i)), Some(Op::PushInt(k)))
                    if *i == target && literal(*k) == 1
            )
        })
        .collect();
    Some(RangeLoop {
        target,
        remaining,
        advance,
        unit_steps,
    })
}

const STEP_OPS: usize = 8;

/// A forward cursor loop, as list and view iteration lower it:
/// `while index < length: ...; index = index wrapping+ 1`, where nothing but
/// that final step writes `index` and nothing writes `length`. Every body read
/// of `index` precedes the step, so it is below `length`; the step then
/// cannot wrap, and an `index` that starts nonnegative stays nonnegative.
/// `enumerate` binds its counter to this index (issue #727).
pub(super) struct CursorLoop {
    pub index: u8,
    pub length: u8,
}

/// Match a forward cursor loop; [`CursorLoop`] states what the caller must
/// still check about the slots' values on entry.
pub(super) fn analyze_cursor(condition: &[Op], body: &[Op]) -> Option<CursorLoop> {
    use crate::op::IntegerOp;
    use crate::value::Value as V;
    let condition: Vec<&Op> = condition.iter().filter(|op| !transparent(op)).collect();
    let [Op::LoadLocal(index), Op::LoadLocal(length), Op::Lt] = condition[..] else {
        return None;
    };
    let (index, length) = (*index, *length);
    let step = body.get(body.len().checked_sub(CURSOR_STEP_OPS)?..)?;
    let [
        Op::LoadLocal(i),
        Op::PushInt(V::I64(1)),
        Op::Integer(IntegerOp::WrappingAdd),
        Op::StoreLocal(i2),
    ] = step
    else {
        return None;
    };
    if index == length || *i != index || *i2 != index {
        return None;
    }
    only_steps(
        &body[..body.len() - CURSOR_STEP_OPS],
        &[index, length],
        step,
    )
    .then_some(CursorLoop { index, length })
}

const CURSOR_STEP_OPS: usize = 4;

/// Whether `ops` and their nested bodies write none of `slots`, except by a
/// copy of `step` that `continue` follows.
fn only_steps(ops: &[Op], slots: &[u8], step: &[Op]) -> bool {
    let mut index = 0;
    while index < ops.len() {
        if ops[index..].starts_with(step) {
            index += step.len();
            if !matches!(ops.get(index), Some(Op::Continue)) {
                return false;
            }
            continue;
        }
        let nested = |ops: &[Op]| only_steps(ops, slots, step);
        let ok = match &ops[index] {
            Op::StoreLocal(i)
            | Op::BorrowLocal(i, _)
            | Op::MoveLocal(i)
            | Op::DropLocal(i)
            | Op::EmptyLocal(i)
            | Op::Next(i) => !slots.contains(i),
            Op::Loop { condition, body } => nested(condition) && nested(body),
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                nested(then)
                    && elifs
                        .iter()
                        .all(|arm| nested(&arm.condition) && nested(&arm.body))
                    && nested(otherwise)
            }
            Op::Match(arms) => arms.iter().all(|arm| nested(&arm.body)),
            Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => nested(cleanup),
            Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => nested(body),
            _ => true,
        };
        if !ok {
            return false;
        }
        index += 1;
    }
    true
}

/// Operations that emit no code and so cannot separate a matched sequence.
fn transparent(op: &Op) -> bool {
    matches!(
        op,
        Op::Site(_) | Op::Access(..) | Op::Loan(_) | Op::UseLoan(_) | Op::EndLoan(_)
    )
}

fn literal(value: crate::value::Value) -> i128 {
    use crate::value::Value as V;
    match value {
        V::I8(n) => n.into(),
        V::I16(n) => n.into(),
        V::I32(n) => n.into(),
        V::I64(n) => n.into(),
        V::U8(n) => n.into(),
        V::U16(n) => n.into(),
        V::U32(n) => n.into(),
        V::U64(n) => n.into(),
    }
}

/// Which side of its bound the counter is on whenever the body runs.
#[derive(Clone, Copy)]
enum Side {
    Below { inclusive: bool },
    Above { inclusive: bool },
}

impl Side {
    fn of(compare: &Op) -> Option<Side> {
        Some(match compare {
            Op::Lt => Side::Below { inclusive: false },
            Op::Le => Side::Below { inclusive: true },
            Op::Gt => Side::Above { inclusive: false },
            Op::Ge => Side::Above { inclusive: true },
            _ => return None,
        })
    }
    fn mirrored(self) -> Side {
        match self {
            Side::Below { inclusive } => Side::Above { inclusive },
            Side::Above { inclusive } => Side::Below { inclusive },
        }
    }
}

/// Analyze a loop whose condition compares a counter local with another
/// local or a literal.
pub(super) fn analyze(condition: &[Op], body: &[Op], locals: &[Ty]) -> Option<Counter> {
    let condition_ops: Vec<&Op> = condition.iter().filter(|op| !transparent(op)).collect();
    let [left, right, compare] = condition_ops[..] else {
        return None;
    };
    let side = Side::of(compare)?;
    let left_counter = match left {
        Op::LoadLocal(i) => analyze_counter(*i, right, side, body, locals),
        _ => None,
    };
    let right_counter = match right {
        Op::LoadLocal(i) => analyze_counter(*i, left, side.mirrored(), body, locals),
        _ => None,
    };
    match left_counter {
        Some(mut counter) => {
            counter.bound_step =
                right_counter.and_then(|bound| Some((bound.local, bound.proven_step?)));
            Some(counter)
        }
        None => right_counter,
    }
}

fn analyze_counter(
    local: u8,
    bound: &Op,
    side: Side,
    body: &[Op],
    locals: &[Ty],
) -> Option<Counter> {
    let ty = locals.get(local as usize)?;
    let (min, end) = ty.int_range()?;
    let max = end - 1;
    let (bound_min, bound_max) = match bound {
        Op::LoadLocal(b) if *b != local && locals.get(*b as usize) == Some(ty) => (min, max),
        Op::PushInt(value) => {
            let value = literal(*value);
            if !(min..=max).contains(&value) {
                return None;
            }
            (value, value)
        }
        _ => return None,
    };
    let mut only_grows = true;
    if !writes_are_steps(body, local, &mut only_grows) {
        return None;
    }
    let proven_step = first_step(body, local).filter(|&(_, delta)| match side {
        Side::Below { inclusive } if delta > 0 => bound_max - i128::from(!inclusive) + delta <= max,
        Side::Above { inclusive } if delta < 0 => bound_min + i128::from(!inclusive) + delta >= min,
        _ => false,
    });
    Some(Counter {
        local,
        proven_step: proven_step.map(|(index, _)| index),
        only_grows,
        bound_step: None,
    })
}

/// Visit every write to `local` in `ops` and their nested bodies. Returns
/// false for a write other than a store, after which no fact holds; clears
/// `only_grows` for a store that is not a nonnegative step.
fn writes_are_steps(ops: &[Op], local: u8, only_grows: &mut bool) -> bool {
    for (index, op) in ops.iter().enumerate() {
        let nested_ok = match op {
            Op::BorrowLocal(i, _)
            | Op::MoveLocal(i)
            | Op::DropLocal(i)
            | Op::EmptyLocal(i)
            | Op::Next(i)
                if *i == local =>
            {
                return false;
            }
            Op::StoreLocal(i) if *i == local => {
                if !step(ops, index, local).is_some_and(|(_, delta)| delta >= 0) {
                    *only_grows = false;
                }
                true
            }
            Op::Loop { condition, body } => {
                writes_are_steps(condition, local, only_grows)
                    && writes_are_steps(body, local, only_grows)
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                writes_are_steps(then, local, only_grows)
                    && elifs.iter().all(|arm| {
                        writes_are_steps(&arm.condition, local, only_grows)
                            && writes_are_steps(&arm.body, local, only_grows)
                    })
                    && writes_are_steps(otherwise, local, only_grows)
            }
            Op::Match(arms) => arms
                .iter()
                .all(|arm| writes_are_steps(&arm.body, local, only_grows)),
            Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => {
                writes_are_steps(cleanup, local, only_grows)
            }
            Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
                writes_are_steps(body, local, only_grows)
            }
            _ => true,
        };
        if !nested_ok {
            return false;
        }
    }
    true
}

/// The first write to `local` in the body, when it is a step at the body's
/// top level: it then runs at most once per pass, with the counter unchanged
/// since the condition held. Returns the step's operation index and amount.
fn first_step(body: &[Op], local: u8) -> Option<(usize, i128)> {
    for (index, op) in body.iter().enumerate() {
        match op {
            Op::StoreLocal(i) if *i == local => return step(body, index, local),
            _ if writes_nested(op, local) => return None,
            _ => {}
        }
    }
    None
}

fn writes_nested(op: &Op, local: u8) -> bool {
    let any = |ops: &[Op]| ops.iter().any(|op| writes(op, local));
    match op {
        Op::Loop { condition, body } => any(condition) || any(body),
        Op::If {
            then,
            elifs,
            otherwise,
        } => {
            any(then)
                || any(otherwise)
                || elifs
                    .iter()
                    .any(|arm| any(&arm.condition) || any(&arm.body))
        }
        Op::Match(arms) => arms.iter().any(|arm| any(&arm.body)),
        Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => any(cleanup),
        Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => any(body),
        _ => false,
    }
}

fn writes(op: &Op, local: u8) -> bool {
    matches!(op, Op::StoreLocal(i) | Op::EmptyLocal(i) if *i == local) || writes_nested(op, local)
}

/// Match `local = local + k`, `local = k + local` or `local = local - k`
/// ending with the store at `store`. Returns the index of the arithmetic
/// operation and the signed amount added.
fn step(ops: &[Op], store: usize, local: u8) -> Option<(usize, i128)> {
    let mut before = ops[..store]
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, op)| !transparent(op));
    let (arith, operation) = before.next()?;
    let (_, second) = before.next()?;
    let (_, first) = before.next()?;
    let delta = match (arith_kind(operation)?, first, second) {
        (Arith::Add, Op::LoadLocal(i), Op::PushInt(k))
        | (Arith::Add, Op::PushInt(k), Op::LoadLocal(i))
            if *i == local =>
        {
            literal(*k)
        }
        (Arith::Sub, Op::LoadLocal(i), Op::PushInt(k)) if *i == local => -literal(*k),
        _ => return None,
    };
    Some((arith, delta))
}

/// The addition or subtraction `op` performs, trapping or Result-checked.
/// A proven step cannot fail, so both lower to a plain add or subtract.
fn arith_kind(op: &Op) -> Option<Arith> {
    match op {
        Op::Add => Some(Arith::Add),
        Op::Sub => Some(Arith::Sub),
        Op::CheckedArith { operation, .. } if matches!(operation, Arith::Add | Arith::Sub) => {
            Some(*operation)
        }
        _ => None,
    }
}

/// Whether `value` is computed from constants and nonnegative at its width.
pub(super) fn nonnegative_constant(dfg: &DataFlowGraph, value: Value) -> bool {
    let sign = dfg.value_type(value).bits() - 1;
    super::known::known_int(dfg, value).is_some_and(|bits| bits >> sign & 1 == 0)
}

/// A `while low < high` loop whose body begins with a checked access to a
/// view local, indexed by `high` and possibly `low` (issue #728), as in an
/// in-place reversal:
///
/// ```text
/// while lo < hi:
///     s.swap(lo, hi)
///     lo += 1
///     hi -= 1
/// ```
///
/// If every write to `high` subtracts a constant with checked arithmetic,
/// each pass's `high` is at most its value on entry, so a check of the entry
/// value done once before the loop bounds every pass's `high`; with `low`
/// nonnegative, `low < high` bounds `low` too. The caller makes that check
/// only when the loop runs, `low < high` on entry, and makes it exactly as
/// the first pass's access would: the access is reached from the condition
/// by reads and moves of locals alone, so nothing observable happens between
/// the two. The facts hold for reads in the first `prefix` body operations,
/// before any write to `low` or `high`.
pub(super) struct Relation {
    pub low: u8,
    pub high: u8,
    pub view: u8,
    pub ty: Ty,
    /// The access's index locals, in the order it checks them.
    pub indices: Vec<u8>,
    pub site: Option<crate::ownership::Site>,
    pub prefix: usize,
}

/// Match a loop that [`Relation`] describes; the caller still checks that
/// `low` is a nonnegative counter, that both are `i64`, and that the view is
/// loop-invariant.
pub(super) fn analyze_relation(condition: &[Op], body: &[Op]) -> Option<Relation> {
    let condition_ops: Vec<&Op> = condition.iter().filter(|op| !transparent(op)).collect();
    let [Op::LoadLocal(low), Op::LoadLocal(high), Op::Lt] = condition_ops[..] else {
        return None;
    };
    let (low, high) = (*low, *high);
    if low == high || !writes_only_shrink(body, high) {
        return None;
    }
    let prefix = body
        .iter()
        .position(|op| {
            let (mut read, mut written) = (HashSet::default(), HashSet::default());
            local_uses(std::slice::from_ref(op), &mut read, &mut written);
            written.contains(&low) || written.contains(&high)
        })
        .unwrap_or(body.len());
    // A small stack machine over the operations before the access: each
    // stack entry is the local whose value it holds.
    let mut stack: Vec<u8> = Vec::new();
    let mut temporaries: HashMap<u8, u8> = HashMap::default();
    let mut site = None;
    for op in &body[..prefix] {
        match op {
            Op::Site(at) => site = Some(*at),
            Op::Access(..) | Op::Loan(_) | Op::UseLoan(_) | Op::Reborrow(Ty::View(..)) => {}
            Op::LoadLocal(local) => stack.push(*local),
            Op::StoreLocal(local) => {
                temporaries.insert(*local, stack.pop()?);
            }
            Op::MoveLocal(local) => stack.push(temporaries.remove(local)?),
            Op::Collection(operation) => {
                let (ty, count) = match operation {
                    crate::collection::CollectionOp::ViewSwap(ty) => (ty, 2),
                    crate::collection::CollectionOp::ViewRow(ty, _) => (ty, 1),
                    _ => return None,
                };
                let indices = stack.split_off(stack.len().checked_sub(count)?);
                let view = stack.pop()?;
                let known = indices.iter().all(|&index| index == low || index == high);
                if !known || !indices.contains(&high) {
                    return None;
                }
                return Some(Relation {
                    low,
                    high,
                    view,
                    ty: ty.clone(),
                    indices,
                    site,
                    prefix,
                });
            }
            _ => return None,
        }
    }
    None
}

fn moves(op: &Op, local: u8) -> bool {
    matches!(
        op,
        Op::BorrowLocal(i, _) | Op::MoveLocal(i) | Op::DropLocal(i) | Op::Next(i) if *i == local
    )
}

/// Whether every write to `local` in `ops` and their nested bodies is a
/// checked subtraction of a nonnegative constant, which cannot wrap.
fn writes_only_shrink(ops: &[Op], local: u8) -> bool {
    ops.iter().enumerate().all(|(index, op)| match op {
        _ if moves(op, local) => false,
        Op::EmptyLocal(i) => *i != local,
        Op::StoreLocal(i) if *i == local => {
            step(ops, index, local).is_some_and(|(arith, delta)| {
                delta <= 0 && matches!(ops[arith], Op::CheckedArith { .. })
            })
        }
        Op::Loop { condition, body } => {
            writes_only_shrink(condition, local) && writes_only_shrink(body, local)
        }
        Op::If {
            then,
            elifs,
            otherwise,
        } => {
            writes_only_shrink(then, local)
                && elifs.iter().all(|arm| {
                    writes_only_shrink(&arm.condition, local)
                        && writes_only_shrink(&arm.body, local)
                })
                && writes_only_shrink(otherwise, local)
        }
        Op::Match(arms) => arms.iter().all(|arm| writes_only_shrink(&arm.body, local)),
        Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => {
            writes_only_shrink(cleanup, local)
        }
        Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
            writes_only_shrink(body, local)
        }
        _ => true,
    })
}

/// Loop versioning for a counted `range` loop whose stop has no visible
/// relation to the lengths its body indexes (issue #728), as in
/// `for i in range(r): xs[i] = xs[i + 1]`.
///
/// The loop is lowered twice. The first copy's accesses `xs[b + k]`, with
/// `0 <= b < stop` and a constant `k >= 0`, skip their checks when the length
/// they compare against is a value the loop reads but never writes; the copy
/// records each such length with its largest `k`. One test before the loop,
/// `stop <= length - k` for every recorded length, then chooses that copy or
/// the second, an ordinary checked copy. If it holds, every pass's `b + k` is
/// below its length, so the skipped checks could not fail; if not, the
/// checked copy runs the same passes with the same traps, messages and
/// positions, including any `break`, `return` or error before the bad index.
///
/// A `while` loop, or a range loop's access through an index no pass
/// changes, relies instead on that index, or a constant, being below a
/// length the loop never changes: one unsigned test before the loop covers
/// every pass (issue #766), as for `perm[0]` in fannkuch's flip loop.
pub(super) struct Version {
    /// The range's stop, canonical; `None` for a `while` loop, whose
    /// accesses can rely only on indices that no pass changes.
    pub stop: Option<Value>,
    /// Canonical entry values of the enclosing loops' invariant locals, each
    /// marked when it is a view: each was read before the loop, so it is
    /// available to the test.
    pub entries: HashMap<Value, bool>,
    /// Accesses the first copy relied on, each with its largest offset.
    pub assumed: Vec<(Assumed, i64)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Assumed {
    /// The counter plus an offset, below a length on every pass.
    Counter(LengthOf),
    /// A loop-invariant index below a length: one unsigned test covers every
    /// pass, including a negative index.
    Index(Value, LengthOf),
    /// A nonnegative constant index below a length; the constant is
    /// materialized again before the loop, where the body's value is not
    /// available.
    Constant(i64, LengthOf),
}

/// A length local, or a view whose count is the length.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LengthOf {
    Length(Value),
    View(Value),
}

/// Largest constant offset a versioned access may add to the counter.
pub(super) const VERSION_OFFSET: i64 = 64;

/// Largest value range of a zero-extended index a versioned access may
/// assume below a length, `0..=u16::MAX`. A wider source, such as a `u32`,
/// would need a table so large that the test before the loop would almost
/// always choose the checked copy.
pub(super) const VERSION_EXTENT: u64 = u16::MAX as u64;

/// Largest body, in operations, that is lowered twice; it bounds the code
/// size and compile time versioning adds.
const VERSION_OPS: usize = 256;

/// Whether a loop's condition and body may be lowered twice: they are small
/// and hold only operations without effects on other code's lowering. A
/// nested `while` loop counts with its body, which may itself be versioned.
/// A nested range loop is not duplicated (`RangeLength` is excluded): its
/// own versioning already covers its accesses, and four copies of it gained
/// nothing measurable. A call is not duplicated either: an inlined callee
/// lowers with its own state, and a single-site callee's body is lowered
/// only at its call (issue #766).
pub(super) fn versionable(condition: &[Op], body: &[Op]) -> bool {
    let mut count = 0;
    let mut allowed = true;
    crate::threading::walk(condition, &mut |op| allowed &= allowed_op(op));
    crate::threading::walk(body, &mut |op| {
        // Markers for the checker and positions emit no code.
        count += usize::from(!matches!(
            op,
            Op::Assumed(_)
                | Op::Site(_)
                | Op::Loan(_)
                | Op::UseLoan(_)
                | Op::EndLoan(_)
                | Op::Access(..)
        ));
        allowed &= allowed_op(op);
    });
    allowed && count <= VERSION_OPS
}

fn allowed_op(op: &Op) -> bool {
    use crate::collection::CollectionOp as C;
    if let Op::Class(crate::record::ClassOp::Field(class, _)) = op {
        // A copy class's field read releases nothing.
        return Ty::Class(class.clone()).can_copy();
    }
    if let Op::Collection(C::ViewSwap(view)) = op {
        // A scalar swap is inline checks, loads and stores; others call
        // the runtime.
        let element = super::views::view_element(view);
        return element.is_numeric() || element == Ty::Bool;
    }
    if let Op::Enum(crate::sum::EnumOp::New(sum, _)) = op {
        // Building a copy value, such as a returned tuple or error, is
        // stores to a stack slot; it retains and releases nothing.
        return Ty::Enum(sum.clone()).can_copy();
    }
    matches!(
        op,
        Op::Collection(
            C::ListScalarGetRef(_)
                | C::ListScalarGetView(_)
                | C::ListScalarSetRef(_)
                | C::ListScalarSetView(_)
                | C::ListLenRef(_)
                | C::ViewLen(_)
                | C::ViewRow(..)
                | C::ViewItem(..)
                | C::ElementRefView(..)
                // Inline checks, loads and stores through a cached column.
                | C::Clist(crate::clist::ClistOp::GetView(..) | crate::clist::ClistOp::SetView(..))
        ) | Op::Class(crate::record::ClassOp::FieldRef(..))
            | Op::WriteRef(_)
            | Op::Integer(_)
            | Op::Numeric(_)
            | Op::CheckedArith { .. }
            | Op::Cast(_)
            // A range check traps as a checked arithmetic operation does.
            | Op::CheckedCast(_)
            | Op::PushInt(_)
            | Op::PushFloat { .. }
            | Op::PushBool(_)
            | Op::PushUnit
            | Op::FloatNeg
            | Op::Add
            | Op::Sub
            | Op::Mul
            // A division's own checks run in both copies, in pass order.
            | Op::Div
            | Op::FloorDiv(_)
            | Op::Modulo(_)
            | Op::Assumed(_)
            | Op::Eq
            | Op::Ne
            | Op::Lt
            | Op::Le
            | Op::Gt
            | Op::Ge
            | Op::Not
            | Op::Drop
            | Op::Dup
            | Op::Swap
            | Op::LoadLocal(_)
            | Op::StoreLocal(_)
            | Op::MoveLocal(_)
            // Releases a local, as an error path's cleanup does.
            | Op::DropLocal(_)
            | Op::BorrowLocal(..)
            | Op::ReadRef(_)
            | Op::Reborrow(_)
            | Op::If { .. }
            | Op::Loop { .. }
            | Op::Break
            | Op::Continue
            | Op::Loan(_)
            | Op::UseLoan(_)
            | Op::EndLoan(_)
            | Op::Access(..)
            | Op::Site(_)
            | Op::Leaving
            | Op::Returning(..)
            | Op::Return
    )
}

/// `(base, k)` when `value` is `base + k` for a constant `k`, or `value`
/// itself with `k = 0`. A checked add's sum counts: its overflow check
/// still runs before any use of the sum.
pub(super) fn offset(dfg: &DataFlowGraph, value: Value) -> (Value, i64) {
    use cranelift_codegen::ir::{InstructionData, Opcode};
    let value = dfg.resolve_aliases(value);
    if let cranelift_codegen::ir::ValueDef::Result(inst, 0) = dfg.value_def(value)
        && let InstructionData::Binary {
            opcode: Opcode::Iadd | Opcode::SaddOverflow,
            args: [a, b],
        } = dfg.insts[inst]
    {
        for (base, constant) in [(a, b), (b, a)] {
            if let Some(k) = super::known::known_signed(dfg, constant) {
                return (dfg.resolve_aliases(base), k);
            }
        }
    }
    (value, 0)
}
