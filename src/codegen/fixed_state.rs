//! Bounded specialization of numerical loop state. Size hints select versions;
//! runtime guards, not hints, justify expansion and scalar replacement.
//!
//! The prefix scan is speculative and never rewrites executed operations. Each
//! integer fact imported into the fast loop gets an equality guard. Facts about
//! locals written by an iteration are discarded before expanding that iteration.
//! The strict evaluator then accepts only a closed, non-failing numerical body:
//! inner ranges become straight-line operations and constant-index cells become
//! scalar locals. Unsupported operations abandon the entire candidate.
//!
//! A length guard for every root proves all cell accesses, including hoisted
//! reads and deferred writes. One borrowed root and distinct affine owners avoid
//! aliasing between promoted roots. No calls, allocation, resizing or escaping
//! references can observe the delayed writes; all original pre-loop effects and
//! the fallback remain intact. Scalar assignments are retained even when only
//! code after the loop can observe them. No floating expression is reassociated.
// Ty equality and hashing exclude mutable layout caches; hints use only numeric lists.
#![allow(clippy::mutable_key_type)]

use super::*;
use crate::collection::CollectionOp as C;
use crate::hash::HashSet;
use crate::numeric::{Checked, NumericOp};
use crate::op::{CompiledFn, IntegerOp};
use crate::record::ClassOp;
use crate::value::Value as Literal;

const MAX_OPS: usize = 12000;
const MAX_ITERATIONS: usize = 128;
const MAX_CELLS: usize = 96;

fn marker(op: &Op) -> bool {
    matches!(
        op,
        Op::Site(_)
            | Op::CallerSite(_)
            | Op::Access(..)
            | Op::Loan(_)
            | Op::UseLoan(_)
            | Op::EndLoan(_)
    )
}
fn float(ty: &Ty) -> bool {
    matches!(ty, Ty::F32 | Ty::F64)
}
fn list(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::List(_) => Some(ty.clone()),
        Ty::Ref(t, _) if matches!(**t, Ty::List(_)) => Some((**t).clone()),
        _ => None,
    }
}
fn numerical(ty: &Ty) -> bool {
    match ty {
        Ty::List(t) => {
            float(t)
                || matches!(&**t, Ty::Class(c) if c.get().copy && c.get().fields.iter().all(|(_, t)| float(t)))
        }
        _ => false,
    }
}

/// Literal capacities are only speculative hints, scoped by the exact list type.
/// Ambiguous sizes are ignored. Every dependent scalar and accessed row is guarded.
pub(super) fn prepare(ops: &mut [Op], optimize: bool) {
    if !optimize {
        return;
    }
    let mut hints: HashMap<Ty, Option<i64>> = HashMap::default();
    fn collect(ops: &[Op], hints: &mut HashMap<Ty, Option<i64>>) {
        let mut previous = None;
        for op in ops {
            if marker(op) {
                continue;
            }
            match op {
                Op::DefineFn(_, f) => collect(&f.body, hints),
                Op::CheckedFunction { body, .. } => collect(body, hints),
                Op::Collection(C::TryNew(t)) if numerical(t) => {
                    if let Some(n @ 2..=8) = previous {
                        hints
                            .entry(t.clone())
                            .and_modify(|old| {
                                if *old != Some(n) {
                                    *old = None;
                                }
                            })
                            .or_insert(Some(n));
                    }
                }
                _ => {}
            }
            previous = if let Op::PushInt(Literal::I64(n)) = op {
                Some(*n)
            } else {
                None
            };
        }
    }
    collect(ops, &mut hints);
    if hints.is_empty() {
        return;
    }
    for op in ops {
        if let Op::DefineFn(_, f) = op {
            optimize_fn(f, &hints);
        }
    }
}

#[derive(Clone)]
enum Val {
    Int(i64),
    Bool(bool),
    Code(Vec<Op>, Ty),
    Root(u8),
    Rows(u8, bool),
    /// A root's length read before the candidate loop, with its size hint.
    Len(u8, i64),
    Cell(u8, i64, Option<usize>),
    Unit,
    Unknown,
}
impl Val {
    fn int(&self) -> Option<i64> {
        if let Self::Int(n) | Self::Len(_, n) = self {
            Some(*n)
        } else {
            None
        }
    }
    fn code(self) -> Option<(Vec<Op>, Ty)> {
        match self {
            Self::Int(n) | Self::Len(_, n) => Some((vec![Op::PushInt(Literal::I64(n))], Ty::I64)),
            Self::Bool(b) => Some((vec![Op::PushBool(b)], Ty::Bool)),
            Self::Code(ops, ty) => Some((ops, ty)),
            Self::Unit => Some((vec![Op::PushUnit], Ty::Unit)),
            _ => None,
        }
    }
}
#[derive(Clone)]
struct Cell {
    root: u8,
    index: i64,
    field: Option<usize>,
    slot: u8,
    ty: Ty,
    written: bool,
}
struct Eval<'a> {
    types: &'a [Ty],
    hints: &'a HashMap<Ty, Option<i64>>,
    env: Vec<Val>,
    stack: Vec<Val>,
    out: Vec<Op>,
    cells: Vec<Cell>,
    strict: bool,
    iterations: usize,
    /// Nested loop bodies by address: those evaluated, and those whose loop
    /// ran no iteration at some entry.
    entered: HashSet<*const Op>,
    skipped: HashSet<*const Op>,
}
impl<'a> Eval<'a> {
    fn new(types: &'a [Ty], hints: &'a HashMap<Ty, Option<i64>>) -> Self {
        let env = types
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if list(t).is_some() {
                    Val::Root(i as u8)
                } else if float(t) || *t == Ty::I64 {
                    Val::Code(vec![Op::LoadLocal(i as u8)], t.clone())
                } else {
                    Val::Unknown
                }
            })
            .collect();
        Self {
            types,
            hints,
            env,
            stack: vec![],
            out: vec![],
            cells: vec![],
            strict: false,
            iterations: 0,
            entered: HashSet::default(),
            skipped: HashSet::default(),
        }
    }
    fn pop(&mut self) -> Option<Val> {
        self.stack.pop()
    }
    fn invalidate(&mut self, ops: &[Op]) {
        let mut written = HashSet::default();
        induction::local_uses(ops, &mut HashSet::default(), &mut written);
        for i in written {
            // List roots retain their identity as slots; no length is assumed here.
            self.env[i as usize] = if list(&self.types[i as usize]).is_some() {
                Val::Root(i)
            } else {
                Val::Code(vec![Op::LoadLocal(i)], self.types[i as usize].clone())
            };
        }
        self.stack.clear();
    }
    fn cell(
        &mut self,
        root: u8,
        index: i64,
        field: Option<usize>,
        write: bool,
    ) -> Option<(u8, Ty)> {
        if !(0..64).contains(&index) {
            return None;
        }
        let lt = list(&self.types[root as usize])?;
        let Ty::List(element) = lt else {
            return None;
        };
        let ty = match (field, &*element) {
            (None, t) if float(t) => t.clone(),
            (Some(f), Ty::Class(c)) if c.get().copy => c.get().fields.get(f)?.1.clone(),
            _ => return None,
        };
        if !float(&ty) {
            return None;
        }
        if let Some(c) = self
            .cells
            .iter_mut()
            .find(|c| (c.root, c.index, c.field) == (root, index, field))
        {
            c.written |= write;
            return Some((c.slot, c.ty.clone()));
        }
        if self.cells.len() >= MAX_CELLS {
            return None;
        }
        let slot = u8::try_from(self.types.len() + self.cells.len()).ok()?;
        self.cells.push(Cell {
            root,
            index,
            field,
            slot,
            ty: ty.clone(),
            written: write,
        });
        Some((slot, ty))
    }
    fn run(&mut self, ops: &[Op]) -> Option<()> {
        for op in ops {
            if self.out.len() > MAX_OPS {
                return None;
            }
            self.step(op)?;
        }
        Some(())
    }
    fn view_root(&mut self, view: bool) -> Option<Option<u8>> {
        if !view {
            return Some(None);
        }
        let Val::Rows(rows, true) = self.pop()? else {
            return None;
        };
        let (Val::Rows(length, false) | Val::Len(length, _)) = self.pop()? else {
            return None;
        };
        (rows == length).then_some(Some(rows))
    }
    fn no_pending_reads(&self) -> Option<()> {
        (!self.stack.iter().any(|v| matches!(v, Val::Code(..)))).then_some(())
    }
    fn step(&mut self, op: &Op) -> Option<()> {
        if marker(op) {
            return Some(());
        }
        match op {
            Op::PushInt(Literal::I64(n)) => self.stack.push(Val::Int(*n)),
            Op::PushFloat { ty, .. } => self.stack.push(Val::Code(vec![op.clone()], ty.clone())),
            Op::PushBool(b) => self.stack.push(Val::Bool(*b)),
            Op::PushUnit => self.stack.push(Val::Unit),
            Op::LoadLocal(i) | Op::MoveLocal(i) => self.stack.push(self.env[*i as usize].clone()),
            Op::BorrowLocal(i, _) if list(&self.types[*i as usize]).is_some() => {
                self.stack.push(Val::Root(*i))
            }
            Op::Reborrow(_) => {
                if !matches!(self.stack.last(), Some(Val::Root(_))) {
                    return None;
                }
            }
            Op::StoreLocal(i) => {
                let value = self.pop()?;
                if self.strict && self.stack.iter().any(|v| matches!(v, Val::Code(..))) {
                    return None;
                }
                if self.strict && list(&self.types[*i as usize]).is_some() {
                    return None;
                }
                self.env[*i as usize] = match value {
                    Val::Int(n) => {
                        if self.strict {
                            self.out.extend([Op::PushInt(Literal::I64(n)), op.clone()]);
                        }
                        Val::Int(n)
                    }
                    // A loop nest's cached length, read before the candidate
                    // loop (issue #943): later accesses name it as an operand.
                    Val::Len(r, n) if !self.strict => Val::Len(r, n),
                    Val::Rows(r, rows) => {
                        if self.strict {
                            self.out.extend(root_ref(self.types, r, false));
                            let t = list(&self.types[r as usize])?;
                            self.out.push(Op::Collection(if rows {
                                C::ListRowsRef(t)
                            } else {
                                C::ListLenRef(t)
                            }));
                            self.out.push(op.clone());
                        }
                        Val::Rows(r, rows)
                    }
                    // A moved owner now lives in this slot, not its emptied
                    // source. Local reference bindings are likewise kept as
                    // their own roots (and rejected by the alias eligibility
                    // check), rather than trusting a stale parameter slot.
                    Val::Root(_) if !self.strict => Val::Root(*i),
                    Val::Unknown if !self.strict => Val::Unknown,
                    value => {
                        let (mut code, ty) = value.code()?;
                        code.push(op.clone());
                        if self.strict {
                            self.out.extend(code);
                        }
                        Val::Code(vec![Op::LoadLocal(*i)], ty)
                    }
                };
            }
            Op::Swap => {
                let n = self.stack.len();
                if n < 2 {
                    return None;
                }
                self.stack.swap(n - 1, n - 2);
            }
            Op::Dup => self.stack.push(self.stack.last()?.clone()),
            Op::Drop => {
                let v = self.pop()?;
                if !matches!(v, Val::Unit) && self.strict {
                    let (mut code, _) = v.code()?;
                    code.push(Op::Drop);
                    self.out.extend(code);
                }
            }
            Op::Collection(C::ListLenRef(t)) => {
                let Val::Root(root) = self.pop()? else {
                    return None;
                };
                if self.strict {
                    self.stack.push(Val::Rows(root, false));
                } else {
                    self.stack.push(match self.hints.get(t).and_then(|n| *n) {
                        Some(n) => Val::Len(root, n),
                        None => Val::Rows(root, false),
                    });
                }
            }
            Op::Collection(C::ListRowsRef(_)) => {
                let Val::Root(root) = self.pop()? else {
                    return None;
                };
                self.stack.push(Val::Rows(root, true));
            }
            Op::Collection(
                c @ (C::ListScalarGetView(_)
                | C::ListScalarGetRef(_)
                | C::ElementRefView(_, _)
                | C::ElementRef(_, _)),
            ) if self.strict => {
                let view =
                    self.view_root(matches!(c, C::ListScalarGetView(_) | C::ElementRefView(..)))?;
                let index = self.pop()?.int()?;
                let Val::Root(root) = self.pop()? else {
                    return None;
                };
                if view.is_some_and(|v| v != root) {
                    return None;
                }
                if matches!(c, C::ElementRefView(..) | C::ElementRef(..)) {
                    self.stack.push(Val::Cell(root, index, None));
                } else {
                    let (slot, ty) = self.cell(root, index, None, false)?;
                    self.stack.push(Val::Code(vec![Op::LoadLocal(slot)], ty));
                }
            }
            Op::Collection(c @ (C::ListScalarSetView(_) | C::ListScalarSetRef(_)))
                if self.strict =>
            {
                let view = self.view_root(matches!(c, C::ListScalarSetView(_)))?;
                let (mut code, ty) = self.pop()?.code()?;
                let index = self.pop()?.int()?;
                let Val::Root(root) = self.pop()? else {
                    return None;
                };
                if view.is_some_and(|v| v != root) {
                    return None;
                }
                self.no_pending_reads()?;
                let (slot, ct) = self.cell(root, index, None, true)?;
                if ct != ty {
                    return None;
                }
                code.push(Op::StoreLocal(slot));
                self.out.extend(code);
                self.stack.push(Val::Unit);
            }
            Op::Class(ClassOp::FieldRef(_, field, _)) if self.strict => {
                let Val::Cell(root, index, None) = self.pop()? else {
                    return None;
                };
                self.stack.push(Val::Cell(root, index, Some(*field)));
            }
            Op::ReadRef(ty) if self.strict && float(ty) => {
                let Val::Cell(root, index, field) = self.pop()? else {
                    return None;
                };
                let (slot, ct) = self.cell(root, index, field, false)?;
                if &ct != ty {
                    return None;
                }
                self.stack.push(Val::Code(vec![Op::LoadLocal(slot)], ct));
            }
            Op::WriteRef(ty) if self.strict && float(ty) => {
                let Val::Cell(root, index, field) = self.pop()? else {
                    return None;
                };
                let (mut code, vt) = self.pop()?.code()?;
                if &vt != ty {
                    return None;
                }
                self.no_pending_reads()?;
                let (slot, ct) = self.cell(root, index, field, true)?;
                if &ct != ty {
                    return None;
                }
                code.push(Op::StoreLocal(slot));
                self.out.extend(code);
            }
            Op::RangeLength(Ty::I64) => {
                let step = self.pop()?.int()?;
                let stop = self.pop()?.int()?;
                let start = self.pop()?.int()?;
                if step == 0 {
                    return None;
                }
                let distance = if step > 0 {
                    stop as i128 - start as i128
                } else {
                    start as i128 - stop as i128
                };
                let width = (step as i128).abs();
                let n = if distance <= 0 {
                    0
                } else {
                    i64::try_from((distance + width - 1) / width).ok()?
                };
                self.stack.push(Val::Int(n));
            }
            Op::Loop { condition, body } if self.strict => {
                induction::analyze_range(condition, body)?;
                let before = self.iterations;
                loop {
                    self.run(condition)?;
                    let Val::Bool(take) = self.pop()? else {
                        return None;
                    };
                    if !take {
                        if self.iterations == before {
                            self.skipped.insert(body.as_ptr());
                        }
                        break;
                    }
                    self.iterations += 1;
                    if self.iterations > MAX_ITERATIONS {
                        return None;
                    }
                    self.entered.insert(body.as_ptr());
                    self.run(body)?;
                }
            }
            Op::Numeric(NumericOp::Sqrt) | Op::FloatNeg => {
                let (mut code, ty) = self.pop()?.code()?;
                if !float(&ty) {
                    return None;
                }
                code.push(op.clone());
                self.stack.push(Val::Code(code, ty));
            }
            Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::FloorDiv(_)
            | Op::Gt
            | Op::Eq
            | Op::Integer(IntegerOp::WrappingAdd | IntegerOp::WrappingSub)
            | Op::CheckedArith { .. } => {
                let b = self.pop()?;
                let a = self.pop()?;
                if let (Some(a), Some(b)) = (a.int(), b.int()) {
                    let n = match op {
                        Op::Add
                        | Op::CheckedArith {
                            operation: Checked::Add,
                            ty: Ty::I64,
                            ..
                        } => a.checked_add(b)?,
                        Op::Sub
                        | Op::CheckedArith {
                            operation: Checked::Sub,
                            ty: Ty::I64,
                            ..
                        } => a.checked_sub(b)?,
                        Op::Mul
                        | Op::CheckedArith {
                            operation: Checked::Mul,
                            ty: Ty::I64,
                            ..
                        } => a.checked_mul(b)?,
                        Op::FloorDiv(_) if a >= 0 && b > 0 => a.checked_div(b)?,
                        Op::Integer(IntegerOp::WrappingAdd) => a.wrapping_add(b),
                        Op::Integer(IntegerOp::WrappingSub) => a.wrapping_sub(b),
                        Op::Gt => {
                            self.stack.push(Val::Bool(a > b));
                            return Some(());
                        }
                        Op::Eq => {
                            self.stack.push(Val::Bool(a == b));
                            return Some(());
                        }
                        _ => return None,
                    };
                    self.stack.push(Val::Int(n));
                } else {
                    let (mut code, ty) = a.code()?;
                    let (bcode, bt) = b.code()?;
                    if ty != bt {
                        return None;
                    }
                    let wrapping = matches!(
                        op,
                        Op::Integer(IntegerOp::WrappingAdd | IntegerOp::WrappingSub)
                    );
                    if !(float(&ty) && matches!(op, Op::Add | Op::Sub | Op::Mul | Op::Div)
                        || ty == Ty::I64 && wrapping)
                    {
                        return None;
                    }
                    code.extend(bcode);
                    code.push(op.clone());
                    self.stack.push(Val::Code(code, ty));
                }
            }
            _ => return None,
        }
        Some(())
    }
}

fn root_ref(types: &[Ty], root: u8, mutable: bool) -> Vec<Op> {
    match &types[root as usize] {
        Ty::Ref(t, _) => vec![
            Op::LoadLocal(root),
            Op::Reborrow(Ty::Ref(t.clone(), mutable)),
        ],
        _ => vec![Op::BorrowLocal(root, mutable)],
    }
}
fn cell_ref(types: &[Ty], c: &Cell, mutable: bool) -> Vec<Op> {
    let t = list(&types[c.root as usize]).unwrap();
    let mut ops = root_ref(types, c.root, mutable);
    ops.push(Op::PushInt(Literal::I64(c.index)));
    // Every cell has a nonnegative constant index. The dominating fast-path
    // guard proves root.len > the maximum index for this root. No operation
    // between the guard and these entry/exit accesses can resize or retarget
    // a root, so these accesses cannot fail, including after deferred writes.
    ops.push(Op::Assumed(Box::new(Op::Collection(C::ElementRef(
        t.clone(),
        mutable,
    )))));
    if let Some(field) = c.field {
        let Ty::List(element) = t else { unreachable!() };
        let Ty::Class(class) = &*element else {
            unreachable!()
        };
        ops.push(Op::Class(ClassOp::FieldRef(class.clone(), field, mutable)));
    }
    ops
}
fn guarded(condition: Vec<Op>, then: Vec<Op>, otherwise: Vec<Op>) -> Vec<Op> {
    let mut ops = condition;
    ops.push(Op::If {
        then: then.into(),
        elifs: Arc::from([]),
        otherwise: otherwise.into(),
    });
    ops
}

/// A list header read cached in a local before a loop.
#[derive(Clone)]
struct Read {
    slot: u8,
    root: u8,
    part: C,
}

impl Read {
    fn ops(&self, types: &[Ty]) -> Vec<Op> {
        let mut ops = root_ref(types, self.root, false);
        ops.extend([Op::Collection(self.part.clone()), Op::StoreLocal(self.slot)]);
        ops
    }
}

/// The cached header reads that end `ops`, immediately before a candidate
/// loop, and the index they start at. Each stores a list's length or row
/// address, or a copy of an earlier one, which is the same read. They
/// cannot fail and nothing follows them, so they can run on either side of
/// the guard, and a fresh length read at the guard equals a stored one.
fn cache_reads(ops: &[Op], types: &[Ty]) -> (usize, Vec<Read>) {
    let back = |mut at: usize| {
        while at > 0 && marker(&ops[at - 1]) {
            at -= 1;
        }
        at
    };
    let previous = |at: usize| at.checked_sub(1).map(|i| &ops[i]);
    let mut units = Vec::new();
    let mut start = ops.len();
    loop {
        let at = back(start);
        let Some(Op::StoreLocal(slot)) = previous(at) else {
            break;
        };
        let at = back(at - 1);
        let (at, source) = match previous(at) {
            Some(Op::LoadLocal(from)) => (at - 1, Err(*from)),
            Some(Op::Collection(part @ (C::ListLenRef(_) | C::ListRowsRef(_)))) => {
                let at = back(at - 1);
                let root = match previous(at) {
                    Some(Op::BorrowLocal(root, false)) => Some((at - 1, *root)),
                    Some(Op::Reborrow(_)) => match previous(back(at - 1)) {
                        Some(Op::LoadLocal(root)) => Some((back(at - 1) - 1, *root)),
                        _ => None,
                    },
                    _ => None,
                };
                match root {
                    Some((at, root)) if list(&types[root as usize]).is_some() => {
                        (at, Ok((root, part.clone())))
                    }
                    _ => break,
                }
            }
            _ => break,
        };
        units.push((at, *slot, source));
        start = at;
    }
    // A copy is a cache read only when its source is one.
    let mut reads: Vec<Read> = Vec::new();
    let mut start = ops.len();
    for (at, slot, source) in units.into_iter().rev() {
        let read = match source {
            Ok((root, part)) => Some(Read { slot, root, part }),
            Err(from) => reads
                .iter()
                .find(|read| read.slot == from)
                .map(|read| Read {
                    slot,
                    ..read.clone()
                }),
        };
        let Some(read) = read else {
            reads.clear();
            start = ops.len();
            continue;
        };
        if reads.is_empty() {
            start = at;
        }
        reads.push(read);
    }
    (start, reads)
}

fn reads_local(ops: &[Op], slot: u8) -> bool {
    let mut read = HashSet::default();
    induction::local_uses(ops, &mut read, &mut HashSet::default());
    read.contains(&slot)
}

/// Whether `ops` read `slot` outside their directly nested loops' bodies.
fn reads_here(ops: &[Op], slot: u8) -> bool {
    ops.iter().any(|op| match op {
        Op::Loop { condition, .. } => reads_local(condition, slot),
        op => reads_local(std::slice::from_ref(op), slot),
    })
}

/// `ops` with `read` placed before each outermost loop whose own level
/// uses it, so no enclosing loop keeps the value across code that does not.
fn sink(ops: &[Op], read: &Read, types: &[Ty]) -> Vec<Op> {
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        match op {
            Op::Loop { condition, body }
                if reads_local(condition, read.slot) || reads_local(body, read.slot) =>
            {
                if reads_local(condition, read.slot) || reads_here(body, read.slot) {
                    out.extend(read.ops(types));
                    out.push(op.clone());
                } else {
                    out.push(Op::Loop {
                        condition: condition.clone(),
                        body: sink(body, read, types).into(),
                    });
                }
            }
            op => out.push(op.clone()),
        }
    }
    out
}

/// `setup` holds the cache reads before the loop that only the original
/// loop uses. They go on the fallback path, each before the loops that use
/// it, so neither path keeps a value across code that does not use it
/// (issue #943).
fn specialize(
    original: &Op,
    prefix: &Eval<'_>,
    params: usize,
    setup: &[Read],
) -> Option<(Vec<Op>, Vec<Ty>)> {
    let cached: HashSet<u8> = setup.iter().map(|read| read.slot).collect();
    let Op::Loop { condition, body } = original else {
        return None;
    };
    let range = induction::analyze_range(condition, body)?;
    if !matches!(prefix.env[range.remaining as usize], Val::Code(..)) {
        return None;
    }
    let mut eval = Eval::new(prefix.types, prefix.hints);
    eval.env = prefix.env.clone();
    eval.strict = true;
    // Facts about locals written by an iteration cannot persist into the next.
    let mut written = HashSet::default();
    induction::local_uses(body, &mut HashSet::default(), &mut written);
    let mut guards = Vec::new();
    for (i, value) in eval.env.iter_mut().enumerate() {
        let root = match value {
            Val::Int(_) => None,
            Val::Len(root, _) => Some(*root),
            _ => continue,
        };
        let n = value.int()?;
        if written.contains(&(i as u8)) {
            *value = Val::Code(vec![Op::LoadLocal(i as u8)], Ty::I64);
        } else {
            guards.push((i as u8, n, root));
        }
    }
    eval.run(body)?;
    let complete = eval.skipped.is_subset(&eval.entered);
    if !eval.stack.is_empty() || eval.cells.len() < 4 || eval.iterations == 0 {
        return None;
    }
    // One borrowed root, plus distinct owned lists: no two promoted roots alias.
    let roots: HashSet<u8> = eval.cells.iter().map(|c| c.root).collect();
    if roots
        .iter()
        .any(|r| *r as usize >= params && matches!(prefix.types[*r as usize], Ty::Ref(..)))
    {
        return None;
    }
    if roots
        .iter()
        .filter(|r| matches!(prefix.types[**r as usize], Ty::Ref(..)))
        .count()
        > 1
    {
        return None;
    }
    let mut fast = Vec::new();
    for c in &eval.cells {
        fast.extend(cell_ref(prefix.types, c, false));
        fast.push(Op::ReadRef(c.ty.clone()));
        fast.push(Op::StoreLocal(c.slot));
    }
    fast.push(Op::Loop {
        condition: condition.clone(),
        body: eval.out.into(),
    });
    for c in eval.cells.iter().filter(|c| c.written) {
        fast.push(Op::LoadLocal(c.slot));
        fast.extend(cell_ref(prefix.types, c, true));
        fast.push(Op::WriteRef(c.ty.clone()));
    }
    // Guard lengths before any hoisted field access; no list changes in the fast loop.
    let mut checks = Vec::new();
    let mut ordered: Vec<u8> = roots.into_iter().collect();
    ordered.sort_unstable();
    for root in ordered.into_iter().rev() {
        let max = eval
            .cells
            .iter()
            .filter(|c| c.root == root)
            .map(|c| c.index)
            .max()?;
        let mut guard = root_ref(prefix.types, root, false);
        guard.extend([
            Op::Collection(C::ListLenRef(list(&prefix.types[root as usize])?)),
            Op::PushInt(Literal::I64(max)),
            Op::Gt,
        ]);
        checks.push(guard);
    }
    for (slot, n, root) in guards.into_iter().rev() {
        // A cached length is not read yet on this path; read the list's.
        let mut check = match root.filter(|_| cached.contains(&slot)) {
            Some(root) => {
                let mut read = root_ref(prefix.types, root, false);
                read.push(Op::Collection(C::ListLenRef(list(
                    &prefix.types[root as usize],
                )?)));
                read
            }
            None => vec![Op::LoadLocal(slot)],
        };
        check.extend([Op::PushInt(Literal::I64(n)), Op::Eq]);
        checks.push(check);
    }
    // Empty ranges never read/write cells, including at loop entry.
    let mut test = vec![Op::PushBool(true)];
    for check in checks {
        test = guarded(check, test, vec![Op::PushBool(false)]);
    }
    test = guarded(condition.to_vec(), test, vec![Op::PushBool(false)]);
    let mut read = HashSet::default();
    induction::local_uses(&test, &mut read, &mut HashSet::default());
    induction::local_uses(&fast, &mut read, &mut HashSet::default());
    if read.iter().any(|slot| cached.contains(slot)) {
        return None;
    }
    // Strict evaluation accepts no op that resizes or rebinds a list, so
    // when it saw every op of the loop, a header read made anywhere inside
    // equals the one made before it. The cleanup of checked arithmetic is
    // not evaluated; it leaves the function.
    let mut fallback = vec![original.clone()];
    for read in setup {
        let at = fallback.len() - 1;
        if complete && !written.contains(&read.slot) {
            let sunk = sink(&fallback[at..], read, prefix.types);
            fallback.splice(at.., sunk);
        } else {
            fallback.splice(at..at, read.ops(prefix.types));
        }
    }
    let fast = guarded(test, fast, fallback);
    Some((fast, eval.cells.iter().map(|c| c.ty.clone()).collect()))
}

fn optimize_fn(f: &mut CompiledFn, hints: &HashMap<Ty, Option<i64>>) {
    if f.generator.is_some() {
        return;
    }
    let types: Vec<Ty> = f
        .sig
        .inputs
        .iter()
        .map(|(_, t)| t.clone())
        .chain(f.locals.iter().cloned())
        .collect();
    if types.len() > 220
        || !types
            .iter()
            .any(|t| list(t).is_some_and(|t| hints.get(&t).is_some_and(Option::is_some)))
    {
        return;
    }
    let (body, result) = match f.body.as_ref() {
        [Op::CheckedFunction { body, result }] => (body.as_ref(), Some(result)),
        body => (body, None),
    };
    let mut prefix = Eval::new(&types, hints);
    let mut output = Vec::new();
    let mut extra = Vec::new();
    for (i, op) in body.iter().enumerate() {
        if extra.is_empty() && matches!(op, Op::Loop { .. }) {
            let params = f.sig.inputs.len();
            let (mut start, mut reads) = cache_reads(&output, &types);
            let (mut read, mut written) = (HashSet::default(), HashSet::default());
            induction::local_uses(&output[..start], &mut read, &mut written);
            induction::local_uses(&body[i + 1..], &mut read, &mut written);
            if reads
                .iter()
                .any(|r| read.contains(&r.slot) || written.contains(&r.slot))
            {
                (start, reads) = (output.len(), Vec::new());
            }
            let moved = specialize(op, &prefix, params, &reads);
            if moved.is_some() {
                // A source position after the last read belongs to the loop.
                let tail = output
                    .iter()
                    .rposition(|op| !matches!(op, Op::Site(_)))
                    .map_or(0, |i| i + 1);
                let tail = output.split_off(tail.max(start));
                output.truncate(start);
                output.extend(tail);
            }
            if let Some((replacement, locals)) =
                moved.or_else(|| specialize(op, &prefix, params, &[]))
            {
                output.extend(replacement);
                extra = locals;
            } else {
                output.push(op.clone());
            }
        } else {
            output.push(op.clone());
        }
        if prefix.step(op).is_none() {
            prefix.invalidate(std::slice::from_ref(op));
        }
    }
    if extra.is_empty() {
        return;
    }
    let mut locals = f.locals.to_vec();
    locals.extend(extra);
    f.locals = locals.into();
    f.body = if let Some(result) = result {
        vec![Op::CheckedFunction {
            body: output.into(),
            result: result.clone(),
        }]
        .into()
    } else {
        output.into()
    };
}

#[cfg(test)]
mod tests;
