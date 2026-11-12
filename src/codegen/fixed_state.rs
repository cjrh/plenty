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
use crate::clist::ClistOp;
use crate::collection::CollectionOp as C;
use crate::hash::HashSet;
use crate::numeric::{Checked, NumericOp};
use crate::op::{CompiledFn, IntegerOp};
use crate::record::ClassOp;
use crate::sum::EnumOp;
use crate::value::Value as Literal;
use portable_simd::Numerical;

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
/// The list or clist a local owns or refers to.
fn list(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::List(_) | Ty::Clist(_) => Some(ty.clone()),
        Ty::Ref(t, _) if matches!(**t, Ty::List(_) | Ty::Clist(_)) => Some((**t).clone()),
        _ => None,
    }
}
fn numerical(ty: &Ty) -> bool {
    let floats =
        |row: &Ty| matches!(row, Ty::Class(c) if c.get().fields.iter().all(|(_, t)| float(t)));
    match ty {
        Ty::List(t) => float(t) || matches!(&**t, Ty::Class(c) if c.get().copy) && floats(t),
        // A clist row is a value in every column, whatever its class says.
        Ty::Clist(row) => floats(row),
        _ => false,
    }
}
/// Reads the row count of a list or clist from a reference to it.
fn length_of(ty: Ty) -> Op {
    Op::Collection(match ty {
        Ty::Clist(_) => C::Clist(ClistOp::Len(ty)),
        _ => C::ListLenRef(ty),
    })
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
                Op::Collection(C::TryNew(t) | C::Clist(ClistOp::WithCapacity(t)))
                    if numerical(t) =>
                {
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
    let mut simd = Simd::default();
    for op in ops.iter() {
        let Op::DefineFn(name, f) = op else {
            continue;
        };
        if !name.contains("std.simd.") {
            continue;
        }
        let operation = portable_simd::classify(name, &f.sig, f.origins.standard_library, optimize);
        let Some((kind, lanes)) = operation.and_then(|operation| operation.numerical()) else {
            continue;
        };
        let output = f.sig.outputs.first().cloned();
        if let (Numerical::Pure | Numerical::Get, Some(vector)) = (kind, &output) {
            simd.vectors.insert(vector.clone());
        }
        let function = Function {
            kind,
            lanes,
            inputs: f.sig.inputs.iter().map(|(_, ty)| ty.clone()).collect(),
            output,
        };
        simd.functions.insert(name.clone(), function);
    }
    for op in ops {
        if let Op::DefineFn(_, f) = op {
            optimize_fn(f, &hints, &simd);
        }
    }
}

/// The floating `std.simd` functions that the backend lowers to vector
/// instructions, by callee name, and their vector types. Such a call is no
/// call: lane arithmetic is a value of its operands, and a view access reads
/// or writes list rows.
#[derive(Default)]
struct Simd {
    functions: HashMap<String, Function>,
    vectors: HashSet<Ty>,
}
struct Function {
    kind: Numerical,
    lanes: i64,
    inputs: Vec<Ty>,
    output: Option<Ty>,
}

#[derive(Clone)]
enum Val {
    Int(i64),
    Bool(bool),
    Code(Vec<Op>, Ty),
    Root(u8),
    Rows(u8, bool),
    /// The address of one column of a clist root.
    Column(u8, usize),
    /// A root's length read before the candidate loop, with its size hint.
    Len(u8, i64),
    Cell(u8, i64, Option<usize>),
    /// A view of all of a root's rows.
    View(u8),
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
    simd: &'a Simd,
    /// The position of the next call.
    site: Option<Op>,
    /// Speculative lengths of roots, from counted loops of appends.
    lengths: HashMap<u8, i64>,
    /// Roots whose speculative length the expansion used.
    exact: HashSet<u8>,
    /// Nested loop bodies by address: those evaluated, and those whose loop
    /// ran no iteration at some entry.
    entered: HashSet<*const Op>,
    skipped: HashSet<*const Op>,
}
impl<'a> Eval<'a> {
    fn new(types: &'a [Ty], hints: &'a HashMap<Ty, Option<i64>>, simd: &'a Simd) -> Self {
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
            simd,
            site: None,
            lengths: HashMap::default(),
            exact: HashSet::default(),
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
    fn cell_ty(&self, root: u8, index: i64, field: Option<usize>) -> Option<Ty> {
        if !(0..64).contains(&index) {
            return None;
        }
        let ty = match (field, list(&self.types[root as usize])?) {
            (Some(f), Ty::Clist(row)) => match &*row {
                Ty::Class(c) => c.get().fields.get(f)?.1.clone(),
                _ => return None,
            },
            (None, Ty::List(t)) if float(&t) => (*t).clone(),
            (Some(f), Ty::List(t)) => match &*t {
                Ty::Class(c) if c.get().copy => c.get().fields.get(f)?.1.clone(),
                _ => return None,
            },
            _ => return None,
        };
        float(&ty).then_some(ty)
    }
    /// The promoted cells of vector `index` of a root, in lane order. A
    /// vector is assembled from and split into the same locals that scalar
    /// accesses use, so neither kind of access waits on the other's store.
    fn lanes(&mut self, root: u8, index: i64, lanes: i64, write: bool) -> Option<Vec<u8>> {
        let first = index.checked_mul(lanes)?;
        (0..lanes)
            .map(|lane| Some(self.cell(root, first.checked_add(lane)?, None, write)?.0))
            .collect()
    }
    /// A root's speculative length, which the guard then requires exactly.
    fn length(&mut self, root: u8) -> Option<i64> {
        let n = *self.lengths.get(&root)?;
        self.exact.insert(root);
        Some(n)
    }
    fn vector(&self, ty: &Ty) -> bool {
        self.simd.vectors.contains(ty)
    }
    /// A counted loop of appends before the candidate suggests each
    /// appended list's length. Only the guard makes a suggestion a fact.
    fn appended(&mut self, condition: &[Op], body: &[Op]) -> Option<()> {
        let range = induction::analyze_range(condition, body)?;
        let Val::Int(n @ 1..=64) = self.env[range.remaining as usize] else {
            return None;
        };
        let mut owner = None;
        let mut appends: HashMap<u8, usize> = HashMap::default();
        for op in body {
            match op {
                Op::BorrowLocal(root, true)
                    if matches!(self.types[*root as usize], Ty::List(_)) =>
                {
                    owner = Some(*root)
                }
                Op::Collection(C::TryInsert(Ty::List(_))) => {
                    *appends.entry(owner.take()?).or_default() += 1
                }
                _ => {}
            }
        }
        for (root, count) in appends {
            if count == 1 {
                *self.lengths.entry(root).or_default() += n;
            }
        }
        Some(())
    }
    /// A store may not change what a deferred stack value will read. A
    /// store to a named local leaves a value that does not mention that
    /// local unchanged. A cell store (`None`) keeps only the borrow of a
    /// vector local: no store of the fast loop writes that local.
    fn settled(&self, local: Option<u8>) -> Option<()> {
        self.stack
            .iter()
            .all(|value| match (value, local) {
                (Val::Code(ops, _), Some(local)) => {
                    let (mut read, mut written) = (HashSet::default(), HashSet::default());
                    induction::local_uses(ops, &mut read, &mut written);
                    !read.contains(&local) && !written.contains(&local)
                }
                (Val::Code(ops, _), None) => matches!(ops.as_slice(), [Op::BorrowLocal(_, false)]),
                _ => true,
            })
            .then_some(())
    }
    fn cell(
        &mut self,
        root: u8,
        index: i64,
        field: Option<usize>,
        write: bool,
    ) -> Option<(u8, Ty)> {
        let ty = self.cell_ty(root, index, field)?;
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
    /// The root whose cached row count and column address a clist view
    /// access names above its operands.
    fn column_root(&mut self, cached: bool, field: usize) -> Option<Option<u8>> {
        if !cached {
            return Some(None);
        }
        let Val::Column(column, of) = self.pop()? else {
            return None;
        };
        let (Val::Rows(length, false) | Val::Len(length, _)) = self.pop()? else {
            return None;
        };
        (column == length && of == field).then_some(Some(column))
    }
    fn step(&mut self, op: &Op) -> Option<()> {
        // A proved operation computes what its plain form does. Only an
        // integer operation is proved, and one on values this pass does not
        // fold ends the pass below, so no rebuilt code loses the proof.
        if let Op::Proved(inner, _) = op {
            return self.step(inner);
        }
        if let Op::CallerSite(_) = op {
            self.site = Some(op.clone());
        }
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
            Op::BorrowLocal(i, false) if self.vector(&self.types[*i as usize]) => {
                let ty = Ty::Ref(Arc::new(self.types[*i as usize].clone()), false);
                self.stack.push(Val::Code(vec![op.clone()], ty));
            }
            // A vector is a copy of lanes; nothing is released.
            Op::DropLocal(i) if self.vector(&self.types[*i as usize]) => {}
            Op::Reborrow(t) => match self.stack.last() {
                Some(Val::Root(_)) => {}
                Some(Val::View(_)) if matches!(t, Ty::View(..)) => {}
                _ => return None,
            },
            Op::StoreLocal(i) => {
                let value = self.pop()?;
                if self.strict {
                    self.settled(Some(*i))?;
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
                            self.out.push(if rows {
                                Op::Collection(C::ListRowsRef(t))
                            } else {
                                length_of(t)
                            });
                            self.out.push(op.clone());
                        }
                        Val::Rows(r, rows)
                    }
                    Val::Column(r, field) => {
                        if self.strict {
                            self.out.extend(root_ref(self.types, r, false));
                            let t = list(&self.types[r as usize])?;
                            self.out
                                .push(Op::Collection(C::Clist(ClistOp::Column(t, field))));
                            self.out.push(op.clone());
                        }
                        Val::Column(r, field)
                    }
                    // A moved owner now lives in this slot, not its emptied
                    // source. Local reference bindings are likewise kept as
                    // their own roots (and rejected by the alias eligibility
                    // check), rather than trusting a stale parameter slot.
                    Val::Root(_) if !self.strict => Val::Root(*i),
                    // The fast loop reads a view's rows from cells, so it
                    // never creates the view.
                    Val::View(root) if matches!(self.types[*i as usize], Ty::View(..)) => {
                        Val::View(root)
                    }
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
            Op::Collection(C::ListLenRef(t) | C::Clist(ClistOp::Len(t))) => {
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
            Op::Collection(C::Clist(ClistOp::Column(_, field))) => {
                let Val::Root(root) = self.pop()? else {
                    return None;
                };
                self.stack.push(Val::Column(root, *field));
            }
            Op::Collection(C::Clist(
                c @ (ClistOp::Get(t, field)
                | ClistOp::GetView(t, field)
                | ClistOp::Set(t, field)
                | ClistOp::SetView(t, field)),
            )) if self.strict => {
                let cached = matches!(c, ClistOp::GetView(..) | ClistOp::SetView(..));
                let column = self.column_root(cached, *field)?;
                let write = matches!(c, ClistOp::Set(..) | ClistOp::SetView(..));
                let value = if write {
                    Some(self.pop()?.code()?)
                } else {
                    None
                };
                let index = self.pop()?.int()?;
                let Val::Root(root) = self.pop()? else {
                    return None;
                };
                if column.is_some_and(|c| c != root)
                    || list(&self.types[root as usize]).as_ref() != Some(t)
                {
                    return None;
                }
                if write {
                    self.settled(None)?;
                }
                let (slot, ty) = self.cell(root, index, Some(*field), write)?;
                match value {
                    Some((mut code, vt)) => {
                        if vt != ty {
                            return None;
                        }
                        code.push(Op::StoreLocal(slot));
                        self.out.extend(code);
                        self.stack.push(Val::Unit);
                    }
                    None => self.stack.push(Val::Code(vec![Op::LoadLocal(slot)], ty)),
                }
            }
            Op::Collection(C::ViewNew(source @ Ty::List(_), _)) => {
                // No written bound: the view covers every row.
                for _ in 0..3 {
                    if !matches!(self.pop()?, Val::Int(0)) {
                        return None;
                    }
                }
                let Val::Root(root) = self.pop()? else {
                    return None;
                };
                if list(&self.types[root as usize]).as_ref() != Some(source) {
                    return None;
                }
                self.stack.push(Val::View(root));
            }
            Op::Collection(C::ViewLen(_)) if self.strict => {
                let Val::View(root) = self.pop()? else {
                    return None;
                };
                let n = self.length(root)?;
                self.stack.push(Val::Int(n));
            }
            Op::Collection(C::ViewRow(..)) if self.strict => {
                let index = self.pop()?.int()?;
                let Val::View(root) = self.pop()? else {
                    return None;
                };
                self.stack.push(Val::Cell(root, index, None));
            }
            Op::Call(name) => {
                let simd = self.simd.functions.get(name)?;
                let site = self.site.take();
                match simd.kind {
                    Numerical::Pure => {
                        let mut operands = Vec::new();
                        for input in simd.inputs.iter().rev() {
                            let (code, ty) = self.pop()?.code()?;
                            if &ty != input {
                                return None;
                            }
                            operands.push(code);
                        }
                        let mut code: Vec<Op> = operands.into_iter().rev().flatten().collect();
                        code.extend(site);
                        code.push(op.clone());
                        self.stack.push(Val::Code(code, simd.output.clone()?));
                    }
                    Numerical::Len | Numerical::Tail if self.strict => {
                        let Val::View(root) = self.pop()? else {
                            return None;
                        };
                        let whole = self.length(root)? / simd.lanes;
                        self.stack.push(Val::Int(if simd.kind == Numerical::Len {
                            whole
                        } else {
                            whole * simd.lanes
                        }));
                    }
                    // The length guard proves the vector's rows.
                    Numerical::Get if self.strict => {
                        let index = self.pop()?.int()?;
                        let Val::View(root) = self.pop()? else {
                            return None;
                        };
                        let Some(Ty::Enum(vector)) = &simd.output else {
                            return None;
                        };
                        let cells = self.lanes(root, index, simd.lanes, false)?;
                        let mut code: Vec<Op> = cells.into_iter().map(Op::LoadLocal).collect();
                        code.push(Op::Enum(EnumOp::New(vector.clone(), 0)));
                        self.stack.push(Val::Code(code, simd.output.clone()?));
                    }
                    Numerical::Set if self.strict && simd.output.is_none() => {
                        let index = self.pop()?.int()?;
                        let Val::View(root) = self.pop()? else {
                            return None;
                        };
                        let Val::Code(code, Ty::Ref(vector, false)) = self.pop()? else {
                            return None;
                        };
                        let ([Op::BorrowLocal(local, false)], Ty::Enum(t)) = (&code[..], &*vector)
                        else {
                            return None;
                        };
                        if simd.inputs.first() != Some(&Ty::Ref(vector.clone(), false)) {
                            return None;
                        }
                        self.settled(None)?;
                        let cells = self.lanes(root, index, simd.lanes, true)?;
                        self.out
                            .extend([Op::LoadLocal(*local), Op::Split(t.clone(), 0)]);
                        self.out.extend(cells.into_iter().rev().map(Op::StoreLocal));
                    }
                    _ => return None,
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
                self.settled(None)?;
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
                self.settled(None)?;
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
            Op::Loop { condition, body } => {
                self.appended(condition, body);
                return None;
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
/// Reads a cell into its local at entry, or writes it back at exit.
///
/// Every cell has a nonnegative constant index. The dominating fast-path
/// guard proves root.len > the maximum index for this root. No operation
/// between the guard and these entry/exit accesses can resize or retarget
/// a root, so these accesses cannot fail, including after deferred writes.
fn cell_access(types: &[Ty], c: &Cell, write: bool) -> Vec<Op> {
    let t = list(&types[c.root as usize]).unwrap();
    let mut ops = root_ref(types, c.root, write);
    ops.push(Op::PushInt(Literal::I64(c.index)));
    let assumed = |op| Op::Assumed(Box::new(Op::Collection(op)));
    match (&t, c.field) {
        (Ty::Clist(_), Some(field)) if write => ops.extend([
            Op::LoadLocal(c.slot),
            assumed(C::Clist(ClistOp::Set(t.clone(), field))),
            Op::Drop,
        ]),
        (Ty::Clist(_), Some(field)) => ops.extend([
            assumed(C::Clist(ClistOp::Get(t.clone(), field))),
            Op::StoreLocal(c.slot),
        ]),
        (Ty::List(element), field) => {
            ops.push(assumed(C::ElementRef(t.clone(), write)));
            if let Some(field) = field {
                let Ty::Class(class) = &**element else {
                    unreachable!()
                };
                ops.push(Op::Class(ClassOp::FieldRef(class.clone(), field, write)));
            }
            if write {
                ops.splice(0..0, [Op::LoadLocal(c.slot)]);
                ops.push(Op::WriteRef(c.ty.clone()));
            } else {
                ops.extend([Op::ReadRef(c.ty.clone()), Op::StoreLocal(c.slot)]);
            }
        }
        _ => unreachable!("a cell of a list or of a clist column"),
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
            Some(Op::Collection(
                part @ (C::ListLenRef(_)
                | C::ListRowsRef(_)
                | C::Clist(ClistOp::Len(_) | ClistOp::Column(..))),
            )) => {
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
    let mut eval = Eval::new(prefix.types, prefix.hints, prefix.simd);
    eval.env = prefix.env.clone();
    eval.lengths = prefix.lengths.clone();
    eval.strict = true;
    // Facts about locals written by an iteration cannot persist into the next.
    let mut written = HashSet::default();
    induction::local_uses(body, &mut HashSet::default(), &mut written);
    let mut guards = Vec::new();
    for (i, value) in eval.env.iter_mut().enumerate() {
        let root = match value {
            Val::Int(_) => None,
            Val::Len(root, _) => Some(*root),
            Val::View(..) if written.contains(&(i as u8)) => {
                *value = Val::Unknown;
                continue;
            }
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
    let roots: HashSet<u8> = eval
        .cells
        .iter()
        .map(|c| c.root)
        .chain(eval.exact.iter().copied())
        .collect();
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
        fast.extend(cell_access(prefix.types, c, false));
    }
    fast.push(Op::Loop {
        condition: condition.clone(),
        body: eval.out.into(),
    });
    for c in eval.cells.iter().filter(|c| c.written) {
        fast.extend(cell_access(prefix.types, c, true));
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
            .max();
        let mut guard = root_ref(prefix.types, root, false);
        guard.push(length_of(list(&prefix.types[root as usize])?));
        // A length the expansion used as a fact is required exactly.
        if eval.exact.contains(&root) {
            let n = eval.lengths[&root];
            if max.is_some_and(|max| max >= n) {
                return None;
            }
            guard.extend([Op::PushInt(Literal::I64(n)), Op::Eq]);
        } else {
            guard.extend([Op::PushInt(Literal::I64(max?)), Op::Gt]);
        }
        checks.push(guard);
    }
    for (slot, n, root) in guards.into_iter().rev() {
        // A cached length is not read yet on this path; read the list's.
        let mut check = match root.filter(|_| cached.contains(&slot)) {
            Some(root) => {
                let mut read = root_ref(prefix.types, root, false);
                read.push(length_of(list(&prefix.types[root as usize])?));
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

fn optimize_fn(f: &mut CompiledFn, hints: &HashMap<Ty, Option<i64>>, simd: &Simd) {
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
    let mut prefix = Eval::new(&types, hints, simd);
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
