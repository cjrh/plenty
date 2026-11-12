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
//!
//! Statements without a square root or a division pack only where every pair
//! of locals they read is a row: two locals [`Pairs`] keeps as the lanes of
//! one `f64x2` variable, such as the `x` and `y` of a body. A row is read and
//! written whole, with no lane assembled or taken apart, and it stays a
//! vector across the loop. A scalar use of either local extracts or inserts
//! its lane, so a row is only a different home for the same two values.
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
    /// Whether a lane takes a square root or divides.
    costly: bool,
    /// The arithmetic instructions of one lane.
    arithmetic: usize,
}

impl Plan {
    /// The pairs of distinct locals read, each once.
    fn reads(&self) -> Vec<(u8, u8)> {
        let mut reads = Vec::new();
        for step in &self.steps {
            if let Step::Read(x, y) = *step
                && x != y
                && !reads.contains(&(x, y))
            {
                reads.push((x, y));
            }
        }
        reads
    }
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

/// Decides whether the pairs of distinct locals two lanes read are all rows.
pub(super) type Rows<'a> = &'a mut dyn FnMut(&[(u8, u8)]) -> bool;

/// The first `2 * n` statements of `ops` as two lanes of `n`, for the
/// smallest `n` that packs. A statement is an expression and the
/// `StoreLocal` that ends it. Lanes without a square root or a division pack
/// only where `rows` accepts the pairs of distinct locals they read, of which
/// there must be one; `None` accepts none.
pub(super) fn find(
    ops: &[Op],
    float: impl Fn(u8) -> bool,
    mut rows: Option<Rows<'_>>,
) -> Option<Plan> {
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
    if !costly && rows.is_none() {
        return None;
    }
    (1..=ends.len() / 2).find_map(|n| {
        let (middle, end) = (ends[n - 1], ends[2 * n - 1]);
        let plan = plan(&ops[..middle], &ops[middle..end], &float)?;
        let packs = plan.costly
            || plan.arithmetic > 0 && {
                let reads = plan.reads();
                !reads.is_empty() && rows.as_mut().is_some_and(|rows| rows(&reads))
            };
        packs.then_some(plan)
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
    let mut arithmetic = 0;
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
                arithmetic += 1;
                Step::Sqrt
            }
            (Op::Add, Op::Add) | (Op::Sub, Op::Sub) | (Op::Mul, Op::Mul) | (Op::Div, Op::Div)
                if depth >= 2 =>
            {
                depth -= 1;
                arithmetic += 1;
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
        costly,
        arithmetic,
    })
}

/// How much more often a loop's body runs than the code around it, as
/// [`rows`] weighs it, and the deepest nesting it distinguishes.
const LOOP_WEIGHT: u64 = 8;
const MAX_DEPTH: u32 = 6;

/// What [`rows`] has seen of a function's `f64` locals.
struct Survey<'a, F, C> {
    float: &'a F,
    /// The locals that may be in a row.
    candidate: &'a C,
    /// The candidate rows, and by local the row it belongs to.
    pairs: Vec<(u8, u8)>,
    row: Vec<Option<usize>>,
    /// The rows each run of cheap packed statements reads, and the
    /// instructions packing it saves.
    groups: Vec<(Vec<usize>, u64)>,
    /// By local, the weighted scalar reads and writes outside those runs.
    reads: Vec<u64>,
    writes: Vec<u64>,
    borrowed: Vec<bool>,
}

impl<F: Fn(u8) -> bool, C: Fn(u8) -> bool> Survey<'_, F, C> {
    /// Make rows of `reads`, when no local among them is in another row.
    fn propose(&mut self, reads: &[(u8, u8)]) -> bool {
        let free = reads.iter().enumerate().all(|(index, &(x, y))| {
            let row = self.row[x as usize];
            match row {
                Some(row) => self.pairs[row] == (x, y),
                None => {
                    (self.candidate)(x)
                        && (self.candidate)(y)
                        && self.row[y as usize].is_none()
                        && reads[..index]
                            .iter()
                            .all(|&(a, b)| a != x && a != y && b != x && b != y)
                }
            }
        });
        if free {
            for &(x, y) in reads {
                if self.row[x as usize].is_none() {
                    self.row[x as usize] = Some(self.pairs.len());
                    self.row[y as usize] = Some(self.pairs.len());
                    self.pairs.push((x, y));
                }
            }
        }
        free
    }

    /// Walk `ops` as the lowering does: only a loop body packs statements.
    fn scan(&mut self, ops: &[Op], depth: u32, body: bool) {
        let weight = LOOP_WEIGHT.pow(depth.min(MAX_DEPTH));
        let mut statement = true;
        // After `Op::Leaving` the lowering gathers a scope's cleanup up to
        // its exit and lowers it there, one operation at a time.
        let mut gathering = false;
        let mut index = 0;
        while let Some(op) = ops.get(index) {
            let starts = std::mem::replace(&mut statement, matches!(op, Op::StoreLocal(_)));
            match op {
                Op::Leaving => gathering = true,
                Op::Return | Op::Break | Op::Continue => gathering = false,
                _ => {}
            }
            if body
                && starts
                && !gathering
                && matches!(op, Op::LoadLocal(local) if (self.float)(*local))
                && let Some(plan) = find(
                    &ops[index..],
                    self.float,
                    Some(&mut |reads| self.propose(reads)),
                )
            {
                if !plan.costly {
                    let rows = plan.reads().into_iter().map(|(x, _)| self.row[x as usize]);
                    let rows = rows.collect::<Option<Vec<_>>>().expect("proposed rows");
                    self.groups.push((rows, plan.arithmetic as u64 * weight));
                }
                index += 2 * plan.ops;
                statement = true;
                continue;
            }
            match op {
                Op::LoadLocal(local) => self.reads[*local as usize] += weight,
                Op::StoreLocal(local) | Op::EmptyLocal(local) => {
                    self.writes[*local as usize] += weight
                }
                Op::MoveLocal(local) => {
                    self.reads[*local as usize] += weight;
                    self.writes[*local as usize] += weight;
                }
                Op::BorrowLocal(local, _) => self.borrowed[*local as usize] = true,
                Op::Assumed(inner) | Op::Proved(inner, _) => {
                    self.scan(std::slice::from_ref(inner), depth, false)
                }
                Op::Loop { condition, body } => {
                    self.scan(condition, depth + 1, false);
                    self.scan(body, depth + 1, true);
                }
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    self.scan(then, depth, false);
                    for arm in elifs.iter() {
                        self.scan(&arm.condition, depth, false);
                        self.scan(&arm.body, depth, false);
                    }
                    self.scan(otherwise, depth, false);
                }
                Op::Match(arms) => {
                    for arm in arms.iter() {
                        self.scan(&arm.body, depth, false);
                    }
                }
                Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => {
                    self.scan(cleanup, depth, false)
                }
                Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
                    self.scan(body, depth, false)
                }
                _ => {}
            }
            index += 1;
        }
    }

    /// The rows worth keeping. A row earns the instructions saved by the
    /// runs that read it while every row they read is kept. It costs an
    /// insertion for each scalar write of either local and an extraction for
    /// each scalar read of the second; the first is read in place.
    fn settle(self) -> Vec<(u8, u8)> {
        let mut kept: Vec<bool> = self
            .pairs
            .iter()
            .map(|&(x, y)| !self.borrowed[x as usize] && !self.borrowed[y as usize])
            .collect();
        loop {
            let mut gain = vec![0u64; kept.len()];
            for (rows, saved) in &self.groups {
                if rows.iter().all(|row| kept[*row]) {
                    rows.iter().for_each(|row| gain[*row] += saved);
                }
            }
            let mut changed = false;
            for (row, &(x, y)) in self.pairs.iter().enumerate() {
                let (x, y) = (x as usize, y as usize);
                let cost = 2 * (self.writes[x] + self.writes[y]) + self.reads[y];
                if kept[row] && gain[row] <= cost {
                    kept[row] = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let pairs = self.pairs.into_iter().zip(kept);
        pairs
            .filter_map(|(pair, kept)| kept.then_some(pair))
            .collect()
    }
}

/// The pairs of `f64` locals of `body` to keep as rows: those whose packed
/// reads in loops save more than their scalar uses cost. `float` names the
/// `f64` locals, and `candidate` those that may be in a row.
pub(super) fn rows(
    body: &[Op],
    float: impl Fn(u8) -> bool,
    candidate: impl Fn(u8) -> bool,
) -> Vec<(u8, u8)> {
    let mut survey = Survey {
        float: &float,
        candidate: &candidate,
        pairs: Vec::new(),
        row: vec![None; 256],
        groups: Vec::new(),
        reads: vec![0; 256],
        writes: vec![0; 256],
        borrowed: vec![false; 256],
    };
    survey.scan(body, 0, false);
    survey.settle()
}

/// The rows of a function: by local, the `f64x2` variable that holds it and
/// its lane there.
#[derive(Default)]
pub(super) struct Pairs {
    lanes: Vec<Option<(Variable, u8)>>,
}

impl Pairs {
    /// Choose the rows of `body` among the locals after its `inputs`, and
    /// declare a variable for each. A row starts as zero, as its locals do.
    /// `enabled` is [`symbols::Symbols::packs_statements`].
    pub(super) fn new(
        body: &[Op],
        locals: &[(Variable, Ty)],
        inputs: usize,
        builder: &mut FunctionBuilder<'_>,
        enabled: bool,
    ) -> Self {
        let float = |local: u8| {
            locals
                .get(local as usize)
                .is_some_and(|(_, ty)| *ty == Ty::F64)
        };
        // A parameter keeps its own variable, defined at entry.
        let candidate = |local: u8| local as usize >= inputs && float(local);
        if !enabled || (inputs..locals.len()).filter(|i| float(*i as u8)).count() < 2 {
            return Self::default();
        }
        let rows = rows(body, float, candidate);
        if rows.is_empty() {
            return Self::default();
        }
        let mut lanes = vec![None; locals.len()];
        let zero = builder.ins().f64const(0.0);
        let zero = builder.ins().splat(types::F64X2, zero);
        for (x, y) in rows {
            let row = builder.declare_var(types::F64X2);
            builder.def_var(row, zero);
            lanes[x as usize] = Some((row, 0));
            lanes[y as usize] = Some((row, 1));
        }
        Self { lanes }
    }

    fn lane(&self, local: u8) -> Option<(Variable, u8)> {
        *self.lanes.get(local as usize)?
    }

    /// The variable whose lanes are `x` and `y`, in that order.
    fn row(&self, x: u8, y: u8) -> Option<Variable> {
        match (self.lane(x)?, self.lane(y)?) {
            ((row, 0), (other, 1)) if row == other => Some(row),
            _ => None,
        }
    }
}

impl Lowerer<'_, '_> {
    /// The value of a local kept in a row.
    pub(super) fn read_row_local(&mut self, local: u8) -> Option<NativeValue> {
        let (row, lane) = self.pairs.lane(local)?;
        let row = self.bcx.use_var(row);
        Some(self.bcx.ins().extractlane(row, lane))
    }

    /// Write a local kept in a row; false when `local` is in none.
    pub(super) fn write_row_local(&mut self, local: u8, value: NativeValue) -> bool {
        let Some((row, lane)) = self.pairs.lane(local) else {
            return false;
        };
        let old = self.bcx.use_var(row);
        let new = self.bcx.ins().insertlane(old, value, lane);
        self.bcx.def_var(row, new);
        true
    }

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
        // A gathered cleanup is lowered at its exit, which [`Survey::scan`]
        // mirrors. No in-place move is pending where a statement starts: the
        // `StoreLocal` or loop test before it has settled them.
        if !float(*local)
            || !self.module.packs_statements()
            || self.gathering()
            || !self.moves.pending.is_empty()
        {
            return Ok(None);
        }
        let pairs = &self.pairs;
        let mut rows = |reads: &[(u8, u8)]| reads.iter().all(|&(x, y)| pairs.row(x, y).is_some());
        let rows: Option<Rows<'_>> = if pairs.lanes.is_empty() {
            None
        } else {
            Some(&mut rows)
        };
        let Some(plan) = find(ops, float, rows) else {
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
                        let value = if let Some(row) = self.pairs.row(x, y) {
                            self.bcx.use_var(row)
                        } else if let (Some((left, from)), Some((right, at))) =
                            (self.pairs.lane(x), self.pairs.lane(y))
                            && x != y
                        {
                            // A lane of each of two rows: one unpack, shuffle
                            // or blend instruction at every CPU level.
                            let mask: Vec<u8> = (8 * from..8 * from + 8)
                                .chain(16 + 8 * at..24 + 8 * at)
                                .collect();
                            let mask = self
                                .bcx
                                .func
                                .dfg
                                .immediates
                                .push(cranelift_codegen::ir::ConstantData::from(&mask[..]));
                            let little = cranelift_codegen::ir::MemFlagsData::new()
                                .with_endianness(cranelift_codegen::ir::Endianness::Little);
                            let left = self.bcx.use_var(left);
                            let right = self.bcx.use_var(right);
                            let left = self.bcx.ins().bitcast(types::I8X16, little, left);
                            let right = self.bcx.ins().bitcast(types::I8X16, little, right);
                            let lanes = self.bcx.ins().shuffle(left, right, mask);
                            self.bcx.ins().bitcast(types::F64X2, little, lanes)
                        } else if x == y {
                            let left = self.read_local(x);
                            self.bcx.ins().splat(types::F64X2, left)
                        } else {
                            let left = self.read_local(x);
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
            // A row stored whole is written with its first lane.
            let whole = plan.stores.iter().any(|&(other, from, at)| {
                from == result
                    && match (lane, at) {
                        (0, 1) => self.pairs.row(local, other).is_some(),
                        (1, 0) => self.pairs.row(other, local).is_some(),
                        _ => false,
                    }
            });
            if whole {
                if lane == 0 {
                    let (row, _) = self.pairs.lane(local).expect("row");
                    self.bcx.def_var(row, results[result]);
                }
                continue;
            }
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
