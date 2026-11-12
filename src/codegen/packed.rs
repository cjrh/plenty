//! Two-lane lowering of adjacent `f64` statements of one shape.
//!
//! A loop body often holds statements that differ only in the locals they
//! name, such as the magnitudes guarded numerical state expands for
//! successive pairs. Where they take a square root or divide, one `f64x2`
//! instruction computes both. Each lane of a vector `fadd`, `fsub`, `fmul`,
//! `fdiv` or `sqrt` is the IEEE 754 operation of the scalar instruction, so
//! every stored value has the bits the scalar statements store. No operation
//! is reordered within a lane, and none can fail.
//!
//! [`find`] is the only judge of what may be packed: it accepts plain `f64`
//! locals, constants and those five operations, and requires that running
//! the second lane's statements beside the first's, rather than after them,
//! reads the same values. An expanded loop also assigns its counters between
//! the statements. Such an integer constant assignment reads nothing and
//! writes no `f64` local, so it runs after the lanes, in its original order.
use super::*;
use crate::numeric::NumericOp;
use cranelift_codegen::ir::Value as NativeValue;

/// The statements tried as one lane, and the longest one read.
const MAX_STATEMENTS: usize = 8;
const MAX_STATEMENT_OPS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    /// The two lanes' values of locals written before the statements.
    Read(u8, u8),
    Constant(u64),
    /// The result of an earlier statement of the same lanes.
    Result(usize),
    Sqrt,
    Add,
    Sub,
    Mul,
    Div,
    /// Ends a statement: its value is the next result.
    Store,
    /// An integer constant, and its assignment to a local that is no `f64`.
    Integer,
    Assign,
}

#[derive(Debug, PartialEq)]
pub(super) struct Plan {
    /// The operations of one lane.
    pub(super) ops: usize,
    steps: Vec<Step>,
    /// The locals that hold a result afterwards: local, result and lane, in
    /// the order the scalar statements write them.
    stores: Vec<(u8, usize, u8)>,
    /// Offsets in a lane of its integer constant assignments.
    integers: Vec<usize>,
}

fn simple(op: &Op) -> bool {
    matches!(
        op,
        Op::LoadLocal(_)
            | Op::PushInt(_)
            | Op::PushFloat { ty: Ty::F64, .. }
            | Op::Numeric(NumericOp::Sqrt)
            | Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
    )
}

/// The first `2 * n` statements of `ops` as two lanes of `n`, for the
/// smallest `n` that packs. A statement is an expression and the
/// `StoreLocal` that ends it.
pub(super) fn find(ops: &[Op], float: impl Fn(u8) -> bool) -> Option<Plan> {
    let mut ends = Vec::with_capacity(2 * MAX_STATEMENTS);
    let mut start = 0;
    let mut costly = false;
    while ends.len() < 2 * MAX_STATEMENTS {
        let window = &ops[start..ops.len().min(start + MAX_STATEMENT_OPS)];
        let Some(length) = window.iter().position(|op| !simple(op)) else {
            break;
        };
        if length == 0 || !matches!(window[length], Op::StoreLocal(_)) {
            break;
        }
        if ends.len() < MAX_STATEMENTS {
            costly |= window[..length]
                .iter()
                .any(|op| matches!(op, Op::Div | Op::Numeric(NumericOp::Sqrt)));
        }
        start += length + 1;
        ends.push(start);
    }
    if !costly {
        return None;
    }
    (1..=ends.len() / 2).find_map(|n| {
        let (middle, end) = (ends[n - 1], ends[2 * n - 1]);
        plan(&ops[..middle], &ops[middle..end], &float)
    })
}

/// How to compute lanes `a` and `b` together, when that stores what running
/// `a` and then `b` stores.
fn plan(a: &[Op], b: &[Op], float: &impl Fn(u8) -> bool) -> Option<Plan> {
    if a.len() != b.len() {
        return None;
    }
    let mut steps = Vec::with_capacity(a.len());
    // Each lane's locals written so far, with the result written last.
    let mut first: Vec<(u8, usize)> = Vec::new();
    let mut second: Vec<(u8, usize)> = Vec::new();
    let written = |lane: &[(u8, usize)], local: u8| {
        lane.iter()
            .rev()
            .find(|(written, _)| *written == local)
            .map(|(_, result)| *result)
    };
    let mut entry_reads = Vec::new();
    let mut integers = Vec::new();
    let (mut depth, mut results, mut costly) = (0usize, 0usize, false);
    let mut integer = false;
    for (offset, pair) in a.iter().zip(b).enumerate() {
        let step = match pair {
            (Op::PushInt(_), Op::PushInt(_)) if depth == 0 && !integer => {
                integer = true;
                integers.push(offset);
                Step::Integer
            }
            (Op::StoreLocal(x), Op::StoreLocal(y)) if integer && !float(*x) && !float(*y) => {
                integer = false;
                Step::Assign
            }
            _ if integer => return None,
            (Op::LoadLocal(x), Op::LoadLocal(y)) if float(*x) && float(*y) => {
                depth += 1;
                match (written(&first, *x), written(&second, *y)) {
                    (Some(left), Some(right)) if left == right => Step::Result(left),
                    (None, None) => {
                        entry_reads.push(*y);
                        Step::Read(*x, *y)
                    }
                    _ => return None,
                }
            }
            (
                Op::PushFloat { bits, ty: Ty::F64 },
                Op::PushFloat {
                    bits: other,
                    ty: Ty::F64,
                },
            ) if bits == other => {
                depth += 1;
                Step::Constant(*bits)
            }
            (Op::Numeric(NumericOp::Sqrt), Op::Numeric(NumericOp::Sqrt)) if depth >= 1 => {
                costly = true;
                Step::Sqrt
            }
            (Op::Add, Op::Add) | (Op::Sub, Op::Sub) | (Op::Mul, Op::Mul) | (Op::Div, Op::Div)
                if depth >= 2 =>
            {
                depth -= 1;
                match pair.0 {
                    Op::Add => Step::Add,
                    Op::Sub => Step::Sub,
                    Op::Mul => Step::Mul,
                    _ => {
                        costly = true;
                        Step::Div
                    }
                }
            }
            (Op::StoreLocal(x), Op::StoreLocal(y)) if depth == 1 && float(*x) && float(*y) => {
                depth = 0;
                first.push((*x, results));
                second.push((*y, results));
                results += 1;
                Step::Store
            }
            _ => return None,
        };
        steps.push(step);
    }
    // The second lane's statements run after all of the first's: a value
    // they read from before the statements must be one the first lane left
    // alone. The first lane never sees a write of the second.
    if depth != 0
        || integer
        || !costly
        || entry_reads
            .iter()
            .any(|local| written(&first, *local).is_some())
    {
        return None;
    }
    let mut stores = Vec::new();
    for (lane, writes) in [(0, &first), (1, &second)] {
        for (index, (local, result)) in writes.iter().enumerate() {
            let rewritten = writes[index + 1..].iter().any(|(later, _)| later == local)
                || lane == 0 && written(&second, *local).is_some();
            if !rewritten {
                stores.push((*local, *result, lane));
            }
        }
    }
    Some(Plan {
        ops: a.len(),
        steps,
        stores,
        integers,
    })
}

impl Lowerer<'_, '_> {
    /// Lower the statements that start `ops` as two vector lanes, and
    /// return how many operations that covered. `None` leaves them to the
    /// scalar lowering.
    pub(super) fn lower_packed(&mut self, ops: &[Op]) -> Result<Option<usize>> {
        let Some(Op::LoadLocal(local)) = ops.first() else {
            return Ok(None);
        };
        let locals = &self.locals;
        let float = |local: u8| {
            locals
                .get(local as usize)
                .is_some_and(|(_, ty)| *ty == Ty::F64)
        };
        if !float(*local)
            || !cfg!(target_arch = "x86_64")
            || !self.module.optimizes()
            || self.gathering()
            || !self.moves.pending.is_empty()
        {
            return Ok(None);
        }
        let Some(plan) = find(ops, float) else {
            return Ok(None);
        };
        if self.joins.pending() {
            self.seal_joins();
        }
        self.fresh_load = None;
        self.slot_copies.clear();
        let mut read: Vec<((u8, u8), NativeValue)> = Vec::new();
        let mut stack = Vec::new();
        let mut results = Vec::new();
        for step in &plan.steps {
            let value = match *step {
                Step::Read(x, y) => match read.iter().find(|(pair, _)| *pair == (x, y)) {
                    Some((_, value)) => *value,
                    None => {
                        let left = self.read_local(x);
                        let value = if x == y {
                            self.bcx.ins().splat(types::F64X2, left)
                        } else {
                            let right = self.read_local(y);
                            let low = self.bcx.ins().scalar_to_vector(types::F64X2, left);
                            self.bcx.ins().insertlane(low, right, 1)
                        };
                        read.push(((x, y), value));
                        value
                    }
                },
                Step::Constant(bits) => {
                    let lane = self
                        .bcx
                        .ins()
                        .f64const(cranelift_codegen::ir::immediates::Ieee64::with_bits(bits));
                    self.bcx.ins().splat(types::F64X2, lane)
                }
                Step::Result(index) => results[index],
                Step::Sqrt => {
                    let value = stack.pop().expect("planned operand");
                    self.bcx.ins().sqrt(value)
                }
                Step::Add | Step::Sub | Step::Mul | Step::Div => {
                    let right = stack.pop().expect("planned operand");
                    let left = stack.pop().expect("planned operand");
                    match step {
                        Step::Add => self.bcx.ins().fadd(left, right),
                        Step::Sub => self.bcx.ins().fsub(left, right),
                        Step::Mul => self.bcx.ins().fmul(left, right),
                        _ => self.bcx.ins().fdiv(left, right),
                    }
                }
                Step::Store => {
                    results.push(stack.pop().expect("planned operand"));
                    continue;
                }
                Step::Integer | Step::Assign => continue,
            };
            stack.push(value);
        }
        for &(local, result, lane) in &plan.stores {
            let value = self.bcx.ins().extractlane(results[result], lane);
            self.write_local(local, value);
        }
        for lane in [0, plan.ops] {
            for offset in &plan.integers {
                self.lower(&ops[lane + offset])?;
                self.lower(&ops[lane + offset + 1])?;
            }
        }
        Ok(Some(2 * plan.ops))
    }
}

#[cfg(test)]
mod tests;
