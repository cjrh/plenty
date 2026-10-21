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
//! The analysis matches operations structurally and gives up on anything it
//! does not recognize. It applies only to counters held in CLIF variables, so
//! no reference can write them behind the operations' back.
use cranelift_codegen::ir::{DataFlowGraph, Value};

use crate::hash::HashSet;
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
}

/// Facts the lowerer holds while it lowers a function.
#[derive(Default)]
pub(super) struct Facts {
    /// Signed counter locals of the loops being lowered whose every value is
    /// nonnegative, one entry per loop that established it.
    pub nonnegative_locals: Vec<u8>,
    /// SSA values known to be nonnegative. A fact about an SSA value holds
    /// wherever the value is used.
    pub nonnegative: HashSet<Value>,
}

/// Operations that emit no code and so cannot separate a matched sequence.
fn transparent(op: &Op) -> bool {
    matches!(
        op,
        Op::Site(_) | Op::Access(..) | Op::Loan(_) | Op::UseLoan(_)
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
    left_counter.or_else(|| match right {
        Op::LoadLocal(i) => analyze_counter(*i, left, side.mirrored(), body, locals),
        _ => None,
    })
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
    })
}

/// Visit every write to `local` in `ops` and their nested bodies. Returns
/// false for a write other than a store, after which no fact holds; clears
/// `only_grows` for a store that is not a nonnegative step.
fn writes_are_steps(ops: &[Op], local: u8, only_grows: &mut bool) -> bool {
    for (index, op) in ops.iter().enumerate() {
        let nested_ok = match op {
            Op::BorrowLocal(i, _) | Op::MoveLocal(i) | Op::DropLocal(i) | Op::Next(i)
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
            Op::Try { cleanup, .. } => writes_are_steps(cleanup, local, only_grows),
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
        Op::Try { cleanup, .. } => any(cleanup),
        _ => false,
    }
}

fn writes(op: &Op, local: u8) -> bool {
    matches!(op, Op::StoreLocal(i) if *i == local) || writes_nested(op, local)
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
    let delta = match (operation, first, second) {
        (Op::Add, Op::LoadLocal(i), Op::PushInt(k))
        | (Op::Add, Op::PushInt(k), Op::LoadLocal(i))
            if *i == local =>
        {
            literal(*k)
        }
        (Op::Sub, Op::LoadLocal(i), Op::PushInt(k)) if *i == local => -literal(*k),
        _ => return None,
    };
    Some((arith, delta))
}

/// Whether `value` is computed from constants and nonnegative at its width.
pub(super) fn nonnegative_constant(dfg: &DataFlowGraph, value: Value) -> bool {
    let sign = dfg.value_type(value).bits() - 1;
    super::known::known_int(dfg, value).is_some_and(|bits| bits >> sign & 1 == 0)
}
