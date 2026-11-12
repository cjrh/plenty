//! Independent type checking of lowered operations (§11.6). It reads only
//! the IR's public surface, the contract a second backend would also consume.

use super::*;
use crate::interval::{Interval, Operator};
use crate::numeric::{Checked, Divisor};

/// Independently check a complete module's operations and function signatures.
/// Abstract operand stacks track concrete types without executing the program.
#[cfg_attr(feature = "profile", hotpath::measure)]
pub fn check(ops: &[Op]) -> Result<()> {
    check_with(ops, crate::parallel::workers())
}

/// Functions below which another checking thread costs more to start than it
/// saves: a typical body takes microseconds to check.
const FUNCTIONS_PER_WORKER: usize = 16;

/// Function bodies are checked on `workers` threads: a body reads only the
/// signature table. The error reported is the first in program order.
fn check_with(ops: &[Op], workers: usize) -> Result<()> {
    // Nearly every top-level operation defines a function.
    let mut sigs = HashMap::with_capacity_and_hasher(ops.len(), Default::default());
    collect_sigs(ops, &mut sigs);
    let functions: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, f) => Some((name, f)),
            _ => None,
        })
        .collect();
    let workers = workers.min(functions.len() / FUNCTIONS_PER_WORKER + 1);
    let program = crate::ownership::Program::new(ops);
    let mut checked = crate::parallel::map(functions.len(), workers, |i| {
        let (name, f) = functions[i];
        check_function(name, f, &sigs, &program).map_err(crate::diagnostic::Diagnostic::from_boxed)
    })
    .into_iter();
    // Top-level: locals are empty (the compiler will never have emitted a
    // `LoadLocal` here either), and there is no end-of-stream invariant.
    let mut stack = Vec::new();
    let mut flow = Flow::Continues;
    for op in ops {
        if flow == Flow::Exits {
            return Err("unreachable operation after a control-flow exit".into());
        }
        flow = match op {
            Op::DefineFn(..) => {
                // Coerce rather than `?`, which would box the box.
                checked
                    .next()
                    .expect("one result per function")
                    .map_err(|e| e as Box<dyn Error>)?;
                Flow::Continues
            }
            _ => step(op, &mut stack, &[], &sigs, None, None, None)?,
        };
    }
    crate::threading::check(ops)?;
    Ok(())
}

/// Every function's signature by name. Keys and signatures borrow from the
/// program: a second owned table costs a name copy and a reference-count
/// write per function, each a cache miss in a large program (issue #57).
type Sigs<'a> = HashMap<&'a str, &'a FnSig>;

/// Only paths that continue participate in a branch's stack-shape join.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continues,
    Exits,
}

fn check_sequence(
    ops: &[Op],
    stack: &mut Vec<Ty>,
    locals: &[Ty],
    sigs: &Sigs,
    returns: Option<&[Ty]>,
    loop_stack: Option<&[Ty]>,
    yield_ty: Option<&Ty>,
) -> Result<Flow> {
    let mut flow = Flow::Continues;
    let mut known = Known::new();
    for op in ops {
        if flow == Flow::Exits {
            return Err("unreachable operation after a control-flow exit".into());
        }
        let result = check_proof(op, stack, &known)?;
        flow = step(op, stack, locals, sigs, returns, loop_stack, yield_ty)?;
        track_known(op, stack.len(), result, &mut known);
    }
    Ok(flow)
}

/// The stack positions of one straight-line sequence whose interval the
/// operations themselves determine: an integer literal, the result of a
/// `Proved` operation, and a cast, `//`, `%` or `&` of such values. A local
/// read, a call and every other operation leave a value whose interval
/// only the frontend knew.
type Known = Vec<(usize, Interval)>;

fn known_at(known: &Known, index: Option<usize>) -> Option<Interval> {
    let index = index?;
    known
        .iter()
        .find(|(position, _)| *position == index)
        .map(|(_, interval)| *interval)
}

fn literal(value: Value) -> i128 {
    match value {
        Value::I8(n) => i128::from(n),
        Value::I16(n) => i128::from(n),
        Value::I32(n) => i128::from(n),
        Value::I64(n) => i128::from(n),
        Value::U8(n) => i128::from(n),
        Value::U16(n) => i128::from(n),
        Value::U32(n) => i128::from(n),
        Value::U64(n) => i128::from(n),
    }
}

/// Update `known` for `op`, which left the stack `after` values deep with
/// a top value in `result` when `check_proof` determined one.
fn track_known(op: &Op, after: usize, result: Option<Interval>, known: &mut Known) {
    match (op, result) {
        // Pushes one value and pops none.
        (Op::PushInt(value), _) => known.push((after - 1, Interval::point(literal(*value)))),
        (Op::LoadLocal(_) | Op::Site(_) | Op::CallerSite(_), _) => {}
        // Replaces its operands with one result.
        (_, Some(result)) => {
            known.retain(|(position, _)| *position + 1 < after);
            known.push((after - 1, result));
        }
        // Any other operation may replace values at any depth.
        _ => known.clear(),
    }
}

/// Recompute what the frontend claimed about `op` from the operand
/// intervals in `known`, before `op` is applied to `stack`, and return the
/// interval of its result when the operations determine one.
///
/// A `Proved` operation is recomputed when every operand is known. A `//`
/// or `%` divisor proof is checked against a known divisor, and its claim
/// about `MIN // -1` when the dividend is known too. An operand that is a
/// local, a `.get()` or a `len` has an interval only in the frontend, so a
/// proof that depends on one is checked for its shape alone, in `step`.
fn check_proof(op: &Op, stack: &[Ty], known: &Known) -> Result<Option<Interval>> {
    if !matches!(
        op,
        Op::Proved(..)
            | Op::CheckedCast(_)
            | Op::FloorDiv(_)
            | Op::Modulo(_)
            | Op::Integer(IntegerOp::And)
            | Op::CheckedArith { .. }
    ) {
        return Ok(None);
    }
    let right = known_at(known, stack.len().checked_sub(1));
    let left = known_at(known, stack.len().checked_sub(2));
    let integer = stack.last().and_then(|ty| Some((ty, Interval::of(ty)?)));
    // Both operands, when the operations determine them and they have the
    // type on top of the stack; `step` reports operands of two types.
    let pair = match (left, right, integer) {
        (Some(a), Some(b), Some((ty, full))) if a.within(full) && b.within(full) => {
            Some((a, b, ty))
        }
        _ => None,
    };
    match op {
        Op::Proved(inner, claimed) => {
            if let Op::CheckedCast(target) = &**inner {
                if let Some(operand) = right.filter(|_| Interval::of(target).is_some()) {
                    let (value, fits) = operand.cast(target);
                    if !fits || !value.within(*claimed) {
                        return Err(format!(
                            "`Proved` {inner:?} {claimed} does not follow from its operand {operand}"
                        )
                        .into());
                    }
                }
                return Ok(Some(*claimed));
            }
            let operator = match &**inner {
                Op::Add => Operator::Add,
                Op::Sub => Operator::Sub,
                Op::Mul => Operator::Mul,
                Op::Integer(IntegerOp::ShiftLeft) => Operator::ShiftLeft,
                Op::Integer(IntegerOp::ShiftRight) => Operator::ShiftRight,
                // `step` rejects any other operation.
                _ => return Ok(None),
            };
            if let Some((a, b, ty)) = pair {
                let outcome = Interval::operation(operator, ty, a, b);
                // A shift discards bits without failing; only its count can.
                let shift = matches!(operator, Operator::ShiftLeft | Operator::ShiftRight);
                let fails = if shift {
                    outcome.bad_count
                } else {
                    !outcome.proved()
                };
                if fails || !outcome.value.within(*claimed) {
                    return Err(format!(
                        "`Proved` {inner:?} {claimed} does not follow from its operands {a} and {b}"
                    )
                    .into());
                }
            }
            Ok(Some(*claimed))
        }
        Op::CheckedCast(target) => Ok(right
            .filter(|_| Interval::of(target).is_some())
            .map(|operand| operand.cast(target).0)),
        Op::Integer(IntegerOp::And) => {
            Ok(pair.map(|(a, b, ty)| Interval::operation(Operator::And, ty, a, b).value))
        }
        Op::FloorDiv(proof)
        | Op::Modulo(proof)
        | Op::CheckedArith {
            operation: Checked::FloorDiv(proof) | Checked::Modulo(proof),
            ..
        } => {
            let (Some(b), Some((_, full))) = (right, integer) else {
                return Ok(None);
            };
            let nonzero = proof.may_be_zero() || !b.contains(0);
            let sign = match proof {
                Divisor::Positive => b.lo > 0,
                Divisor::BelowMinusOne => b.hi < -1,
                _ => true,
            };
            // `MIN // -1` needs both operands; an unknown dividend may be
            // one the frontend bounded.
            let fits = proof.may_be_minus_one()
                || !b.contains(-1)
                || left.is_none_or(|a| !a.within(full) || !(full.lo < 0 && a.contains(full.lo)));
            if !(nonzero && sign && fits) {
                return Err(format!(
                    "the divisor proof {proof:?} does not follow from the divisor {b}"
                )
                .into());
            }
            let operator = match op {
                Op::FloorDiv(_) => Operator::FloorDiv,
                Op::Modulo(_) => Operator::Modulo,
                // A checked operation leaves a `Result`.
                _ => return Ok(None),
            };
            Ok(pair.map(|(a, b, ty)| Interval::operation(operator, ty, a, b).value))
        }
        _ => Ok(None),
    }
}

fn check_return(stack: &[Ty], returns: Option<&[Ty]>) -> Result<Flow> {
    let expected = returns.ok_or("return outside a function")?;
    if stack != expected {
        return Err(format!(
            "function exit leaves [{}], but signature declares outputs [{}]",
            fmt_types(stack),
            fmt_types(expected),
        )
        .into());
    }
    Ok(Flow::Exits)
}

/// Add the sig of every `DefineFn` reachable from `ops` — top-level and
/// nested — to `out`. Walking recursively makes the resulting table a
/// safe over-approximation of "what's callable somewhere in this source":
/// it allows forward references at the cost of accepting calls to a
/// nested function before its enclosing definition has run. The latter is
/// caught at runtime as an "undefined function" error, which is fine —
/// the checker's job is to catch *type* mismatches, not to police call
/// ordering.
fn collect_sigs<'a>(ops: &'a [Op], out: &mut Sigs<'a>) {
    for op in ops {
        match op {
            Op::DefineFn(name, f) => {
                out.insert(name, &f.sig);
                collect_sigs(&f.body, out);
            }
            Op::Loop { condition, body } => {
                collect_sigs(condition, out);
                collect_sigs(body, out);
            }
            Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
                collect_sigs(body, out)
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    collect_sigs(&arm.body, out);
                }
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                collect_sigs(then, out);
                for arm in elifs.iter() {
                    collect_sigs(&arm.condition, out);
                    collect_sigs(&arm.body, out);
                }
                collect_sigs(otherwise, out);
            }
            _ => {}
        }
    }
}

/// Apply one op to the abstract stack.
///
/// `locals` types the active function's input names by index — empty at
/// the top level, non-empty inside a body. `sigs` is the resolved table
/// of every function callable in this source.
fn step(
    op: &Op,
    stack: &mut Vec<Ty>,
    locals: &[Ty],
    sigs: &Sigs,
    returns: Option<&[Ty]>,
    loop_stack: Option<&[Ty]>,
    yield_ty: Option<&Ty>,
) -> Result<Flow> {
    match op {
        Op::Control(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid cancellation token argument".into());
                }
            }
            stack.push(output);
        }
        Op::Atomic(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid atomic argument".into());
                }
            }
            stack.push(output);
        }
        Op::Mutex(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid mutex argument".into());
                }
            }
            stack.push(output);
        }
        Op::Executor(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid executor argument".into());
                }
            }
            stack.push(output);
        }
        Op::Channel(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid channel argument".into());
                }
            }
            stack.push(output);
        }
        Op::Thread(operation) => match operation {
            crate::threading::ThreadOp::Start(task, slot, discard_job) => {
                if locals.get(*slot as usize) != Some(&Ty::Task(task.clone())) {
                    return Err("invalid scoped thread slot".into());
                }
                // A lent pipeline enters the worker as a call argument does
                // (issue #911): the worker's input names the lent plan.
                for ty in task.inputs.iter().rev() {
                    if !stack
                        .pop()
                        .is_some_and(|actual| argument_matches(&actual, ty))
                    {
                        return Err("invalid scoped thread argument".into());
                    }
                }
                stack.push(task.start_result(*discard_job));
            }
            crate::threading::ThreadOp::Join(task) => {
                if stack.pop() != Some(Ty::Task(task.clone())) {
                    return Err("invalid scoped join token".into());
                }
                if task.output != Ty::Unit {
                    stack.push(task.output.clone());
                }
            }
            crate::threading::ThreadOp::Finish(task, slot) => {
                if locals.get(*slot as usize) != Some(&Ty::Task(task.clone())) {
                    return Err("invalid scoped thread cleanup".into());
                }
            }
            crate::threading::ThreadOp::Run(task) => {
                for ty in task.inputs.iter().rev() {
                    if stack.pop().as_ref() != Some(ty) {
                        return Err("invalid __actor_run argument".into());
                    }
                }
                if stack.pop() != Some(Ty::I64) {
                    return Err("invalid __actor_run heap".into());
                }
                stack.push(task.run_result());
            }
            crate::threading::ThreadOp::Forget(ty) => {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid forgotten value".into());
                }
            }
            crate::threading::ThreadOp::Rows(..) => {}
        },
        Op::Try {
            source,
            target,
            cleanup,
        } => {
            let mut empty = Vec::new();
            check_sequence(
                cleanup, &mut empty, locals, sigs, returns, loop_stack, yield_ty,
            )?;
            if !empty.is_empty() {
                return Err("propagation cleanup must return unit".into());
            }
            if !source.propagatable()
                || !target.propagatable()
                || source.is_option() != target.is_option()
                || (!source.is_option() && !target.accepts_result_error(source))
                || yield_ty.is_some()
                || returns != Some(&[Ty::Enum((**target).clone())][..])
                || stack.pop() != Some(Ty::Enum((**source).clone()))
            {
                return Err("invalid Result/Option propagation".into());
            }
            stack.push(source.get().variants[usize::from(source.is_option())].fields[0].clone());
        }
        Op::Loan(_)
        | Op::UseLoan(_)
        | Op::EndLoan(_)
        | Op::Access(..)
        | Op::Site(_)
        | Op::CallerSite(_)
        | Op::Returning(..)
        | Op::Leaving
        | Op::Building(_)
        | Op::Passes(_)
        | Op::Forwards(_)
        | Op::Tokens(_)
        | Op::FieldMove(_)
        | Op::ForwardsReferences
        | Op::CallerReferent(_) => {}
        Op::BorrowLocal(i, mutable) => {
            let ty = locals.get(*i as usize).ok_or("invalid borrowed place")?;
            stack.push(Ty::Ref(Arc::new(ty.clone()), *mutable));
        }
        Op::ReadRef(ty) => {
            if !matches!(stack.pop(), Some(Ty::Ref(t, _)) if t.as_ref() == ty) {
                return Err("invalid reference read".into());
            }
            stack.push(ty.clone());
        }
        Op::Reborrow(target) => {
            let source = match stack.pop() {
                Some(Ty::DictView(owner, _)) => Some(Ty::Ref(owner, false)),
                source => source,
            };
            let ((Some(Ty::Ref(source, writable)), Ty::Ref(inner, mutable))
            | (Some(Ty::View(source, writable)), Ty::View(inner, mutable))) = (source, target)
            else {
                return Err("invalid reborrow".into());
            };
            if source != *inner || (*mutable && !writable) {
                return Err("invalid reborrow permissions".into());
            }
            stack.push(target.clone());
        }
        Op::WriteRef(ty) | Op::FillRef(ty) | Op::ReplaceRef(ty) => {
            if stack.pop() != Some(Ty::Ref(Arc::new(ty.clone()), true))
                || stack.pop().as_ref() != Some(ty)
            {
                return Err("invalid reference write".into());
            }
            if matches!(op, Op::ReplaceRef(_)) {
                stack.push(ty.clone());
            }
        }
        Op::Collection(operation) => {
            match operation {
                crate::collection::CollectionOp::ListLenRef(ty)
                | crate::collection::CollectionOp::ListRowsRef(ty)
                | crate::collection::CollectionOp::ListRowBytesRef(ty)
                | crate::collection::CollectionOp::ListRowRef(ty, _)
                | crate::collection::CollectionOp::ListRowTake(ty)
                | crate::collection::CollectionOp::ElementRefView(ty, _)
                    if !matches!(ty, Ty::List(_)) =>
                {
                    return Err("list length and rows require a list reference".into());
                }
                crate::collection::CollectionOp::ViewNew(
                    Ty::List(_) | Ty::ByteStorage | Ty::ByteStr | Ty::View(..),
                    _,
                ) => {}
                crate::collection::CollectionOp::ViewNew(..) => {
                    return Err("a slice view requires a list, byte, or view owner".into());
                }
                crate::collection::CollectionOp::ViewLen(ty)
                | crate::collection::CollectionOp::ViewRow(ty, _)
                | crate::collection::CollectionOp::ViewItem(ty, _)
                | crate::collection::CollectionOp::ViewGetRef(ty, _)
                | crate::collection::CollectionOp::ViewEqual(ty)
                | crate::collection::CollectionOp::ViewSwap(ty)
                | crate::collection::CollectionOp::ViewSort(ty)
                    if !matches!(ty, Ty::View(..)) =>
                {
                    return Err("view operations require a slice view".into());
                }
                crate::collection::CollectionOp::ViewRow(Ty::View(_, false), true)
                | crate::collection::CollectionOp::ViewItem(Ty::View(_, false), true)
                | crate::collection::CollectionOp::ViewGetRef(Ty::View(_, false), true) => {
                    return Err("a shared slice view cannot lend a mutable row".into());
                }
                crate::collection::CollectionOp::ViewSwap(Ty::View(_, false)) => {
                    return Err("swapping view rows requires an exclusive view".into());
                }
                crate::collection::CollectionOp::ViewSort(Ty::View(element, true))
                    if !element.is_orderable() =>
                {
                    return Err("sorting a view requires orderable elements".into());
                }
                crate::collection::CollectionOp::ViewSort(Ty::View(_, false)) => {
                    return Err("sorting view rows requires an exclusive view".into());
                }
                crate::collection::CollectionOp::ListScalarGetRef(ty)
                | crate::collection::CollectionOp::ListScalarGetView(ty)
                | crate::collection::CollectionOp::ListScalarSetRef(ty)
                | crate::collection::CollectionOp::ListScalarSetView(ty)
                | crate::collection::CollectionOp::ListScalarSwapRef(ty)
                    if !matches!(ty, Ty::List(element) if element.is_numeric() || **element == Ty::Bool) =>
                {
                    return Err("scalar list access requires a scalar list reference".into());
                }
                // Codegen calls the method for an index outside the cached
                // length, so the view must name a `Bytes` method that exists.
                view @ (crate::collection::CollectionOp::BytesGetView(ty)
                | crate::collection::CollectionOp::BytesSetView(ty)) => {
                    let is_bytes = matches!(
                        ty,
                        Ty::Class(class) if &*class.name == crate::frontend::BYTES_CLASS
                    );
                    if !is_bytes
                        || !view
                            .bytes_view_method()
                            .is_some_and(|method| sigs.contains_key(method.as_str()))
                    {
                        return Err("byte view access requires a Bytes receiver".into());
                    }
                }
                crate::collection::CollectionOp::ListSortBy(sort) => {
                    let Some(element) = sort.element() else {
                        return Err("key sorting requires a list or an exclusive view".into());
                    };
                    let item = Ty::Ref(element.clone(), false);
                    let valid = sigs.get(sort.key_of.as_str()).is_some_and(|key_of| {
                        key_of
                            .inputs
                            .iter()
                            .map(|(_, t)| t)
                            .eq([&sort.callback, &item])
                            && key_of.outputs
                                == [sort.error.as_ref().map_or_else(
                                    || sort.key.clone(),
                                    |error| crate::sum::result(sort.key.clone(), error.clone()),
                                )]
                    });
                    if !valid || !sort.key.is_orderable() {
                        return Err("key sort helper signature mismatch".into());
                    }
                }
                _ => {}
            }
            let (inputs, output) = operation.signature();
            if stack.len() < inputs.len() || stack[stack.len() - inputs.len()..] != inputs {
                return Err("collection operation type mismatch".into());
            }
            stack.truncate(stack.len() - inputs.len());
            stack.push(output);
        }
        Op::Class(operation) => {
            let (inputs, output) = operation.signature().ok_or("invalid class operation")?;
            if stack.len() < inputs.len() || stack[stack.len() - inputs.len()..] != inputs {
                return Err("class operation type mismatch".into());
            }
            stack.truncate(stack.len() - inputs.len());
            stack.push(output);
        }
        Op::Enum(operation) => {
            let (inputs, output) = operation.signature().ok_or("invalid enum operation")?;
            if stack.len() < inputs.len() || stack[stack.len() - inputs.len()..] != inputs {
                return Err("enum operation type mismatch".into());
            }
            stack.truncate(stack.len() - inputs.len());
            stack.push(output);
        }
        Op::Box(operation) => {
            let (inputs, output) = operation.signature();
            if stack.len() < inputs.len() || stack[stack.len() - inputs.len()..] != inputs {
                return Err("box operation type mismatch".into());
            }
            stack.truncate(stack.len() - inputs.len());
            stack.push(output);
        }
        Op::Split(t, tag) => {
            let definition = t.get();
            let fields = &definition
                .variants
                .get(*tag)
                .ok_or("invalid split variant")?
                .fields;
            if fields.len() < 2 || stack.pop() != Some(Ty::Enum(t.clone())) {
                return Err("invalid split".into());
            }
            stack.extend(fields.iter().cloned());
        }
        Op::SplitClass(t) => {
            let definition = t.get();
            if definition.fields.len() < 2
                || definition.destructor.is_some()
                || stack.pop() != Some(Ty::Class(t.clone()))
            {
                return Err("invalid class split".into());
            }
            stack.extend(definition.fields.iter().map(|(_, ty)| ty.clone()));
        }
        Op::Unreachable => return Ok(Flow::Exits),
        Op::Yield(ty) => {
            if yield_ty != Some(ty) || stack.pop().as_ref() != Some(ty) || !stack.is_empty() {
                return Err(
                    "yield requires its declared element type and an empty residual operand stack"
                        .into(),
                );
            }
        }
        Op::JoinFrame(join) => {
            let (from, to) = &**join;
            if !crate::generator::joins_into(from, to) || stack.pop().as_ref() != Some(from) {
                return Err("frame join requires a member producer of the joined type".into());
            }
            stack.push(to.clone());
        }
        Op::Next(slot) => {
            let Some(Ty::Generator(element)) = locals.get(*slot as usize) else {
                return Err("next requires a generator local".into());
            };
            stack.push(crate::sum::option(element.element.clone()));
        }
        Op::DropLocal(i) => {
            locals.get(*i as usize).ok_or("invalid drop local")?;
        }
        Op::EmptyLocal(i) => {
            locals.get(*i as usize).ok_or("invalid empty local")?;
        }
        Op::Loop { condition, body } => {
            let initial = stack.clone();
            let mut cond = initial.clone();
            if check_sequence(condition, &mut cond, locals, sigs, returns, None, yield_ty)?
                != Flow::Continues
                || cond.pop() != Some(Ty::Bool)
                || cond != initial
            {
                return Err("loop condition must produce bool".into());
            }
            let mut iter = initial.clone();
            if check_sequence(
                body,
                &mut iter,
                locals,
                sigs,
                returns,
                Some(&initial),
                yield_ty,
            )? == Flow::Continues
                && iter != initial
            {
                return Err("loop body must preserve operand types".into());
            }
            if crate::op::loop_diverges(condition, body) {
                return Ok(Flow::Exits);
            }
        }
        // Unsuffixed integer literals are `i64`; a suffix records its chosen
        // width directly in the `Value` carried by the operation.
        Op::PushInt(value) => stack.push(Ty::from(*value)),
        Op::PushFloat { ty, .. } => {
            if !ty.is_float() {
                return Err("float literal requires a float type".into());
            }
            stack.push(ty.clone());
        }
        Op::PushUnit => stack.push(Ty::Unit),
        Op::PipelineToken(item, id) => stack.push(Ty::Iter(item.clone(), *id)),
        Op::FloatNeg => {
            let ty = stack.last().ok_or("stack underflow on float negation")?;
            if !ty.is_float() {
                return Err("float negation requires a float".into());
            }
        }
        Op::PushStr(_) => stack.push(Ty::Str),
        Op::PushBytes(_) => stack.push(Ty::ByteStr),
        Op::PushBool(_) => stack.push(Ty::Bool),
        Op::ForeignNull(ty) => {
            if !matches!(ty, Ty::ForeignPtr(..)) {
                return Err("foreign null requires an opaque pointer".into());
            }
            stack.push(ty.clone());
        }
        Op::ForeignCall { sig, .. } => {
            for (_, expected) in sig.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(expected) {
                    return Err("foreign call argument type mismatch".into());
                }
            }
            stack.extend(sig.outputs.iter().cloned());
        }
        Op::Proved(inner, interval) => {
            let (before, ty) = match &**inner {
                Op::Add
                | Op::Sub
                | Op::Mul
                | Op::Integer(IntegerOp::ShiftLeft | IntegerOp::ShiftRight) => {
                    (stack.last().cloned(), stack.last().cloned())
                }
                Op::CheckedCast(target) => (stack.last().cloned(), Some(target.clone())),
                _ => return Err(format!("`Proved` cannot hold {inner:?}").into()),
            };
            let fits = |ty: &Option<Ty>| {
                ty.as_ref()
                    .and_then(crate::interval::Interval::of)
                    .is_some_and(|full| interval.within(full))
            };
            if before.as_ref().is_none_or(|ty| !ty.is_int()) || !fits(&ty) {
                return Err(format!("`Proved` {inner:?} does not fit {interval}").into());
            }
            return step(inner, stack, locals, sigs, returns, loop_stack, yield_ty);
        }
        Op::Assumed(inner) => {
            let operand = matches!(
                **inner,
                Op::Add
                    | Op::Sub
                    | Op::Mul
                    | Op::FloorDiv(_)
                    | Op::Modulo(_)
                    | Op::Numeric(crate::numeric::NumericOp::Pow)
                    | Op::Integer(IntegerOp::ShiftLeft | IntegerOp::ShiftRight)
                    | Op::CheckedCast(_)
            );
            let indexing =
                matches!(&**inner, Op::Collection(operation) if operation.assumable_index());
            if !operand && !indexing {
                return Err(format!("`Assumed` cannot hold {inner:?}").into());
            }
            return step(inner, stack, locals, sigs, returns, loop_stack, yield_ty);
        }
        Op::Add => {
            let (a, b) = pop2(stack, "+")?;
            let out = match (a.clone(), b.clone()) {
                (Ty::Str, Ty::Str) => Ty::Str,
                (a, b) if a == b && a.is_numeric() => a,
                _ => {
                    return Err(format!(
                        "`+` requires same-typed numbers or (Str Str), got ({a} {b})"
                    )
                    .into());
                }
            };
            stack.push(out);
        }
        Op::Sub => arith(stack, "-")?,
        Op::Mul => arith(stack, "*")?,
        Op::Div => arith(stack, "/")?,
        Op::FloorDiv(_) | Op::Modulo(_) => {
            if !stack.last().is_some_and(Ty::is_int) {
                return Err("floor division and modulo require integers".into());
            }
            arith(stack, "// or %")?;
        }
        Op::Integer(kind) => {
            let ty = stack.pop().ok_or("integer operation stack underflow")?;
            if !ty.is_int() {
                return Err("bitwise and wrapping operations require integers".into());
            }
            if *kind != IntegerOp::Not && stack.pop().as_ref() != Some(&ty) {
                return Err("integer operations require same-typed operands".into());
            }
            stack.push(ty);
        }
        Op::Numeric(operation) => operation.check(stack)?,
        Op::CheckedArith {
            operation,
            ty,
            cleanup,
        } => {
            let mut empty = Vec::new();
            check_sequence(
                cleanup, &mut empty, locals, sigs, returns, loop_stack, yield_ty,
            )?;
            if !empty.is_empty() {
                return Err("checked arithmetic cleanup must leave no value".into());
            }
            operation.check(ty, stack)?;
        }
        Op::CheckedFunction { body, result } => {
            let Ty::Enum(sum) = result else {
                return Err("checked function must return Result".into());
            };
            if !sum.is_result() {
                return Err("checked function must return Result".into());
            }
            let logical = vec![result.clone()];
            let base = stack.len();
            let flow = check_sequence(
                body,
                stack,
                locals,
                sigs,
                Some(&logical),
                loop_stack,
                yield_ty,
            )?;
            if flow == Flow::Exits {
                stack.truncate(base);
            }
            if flow == Flow::Continues && stack.pop().as_ref() != Some(result) {
                return Err("checked function must leave its declared Result".into());
            }
            stack.push(result.clone());
        }
        Op::CheckedBlock { body, result } => {
            let Ty::Enum(sum) = result else {
                return Err("a checked block must produce a Result".into());
            };
            if !sum.is_result() {
                return Err("a checked block must produce a Result".into());
            }
            let definition = sum.get();
            let success = definition
                .variants
                .first()
                .and_then(|variant| variant.fields.first())
                .ok_or("a checked block must leave its success value")?;
            // Unit expressions leave no operand; codegen materializes Unit
            // only when wrapping Ok. Preserve every pending entry operand.
            let entry = (*success == Ty::Unit).then(|| stack.clone());
            if check_sequence(body, stack, locals, sigs, returns, loop_stack, yield_ty)?
                != Flow::Continues
                || match entry {
                    Some(entry) => *stack != entry,
                    None => stack.pop().as_ref() != Some(success),
                }
            {
                return Err("a checked block must leave its success value".into());
            }
            stack.push(result.clone());
        }
        Op::Eq => {
            let (a, b) = pop2(stack, "=")?;
            if a != b {
                return Err(
                    format!("`=` requires both operands of the same type, got ({a} {b})").into(),
                );
            }
            stack.push(Ty::Bool);
        }
        Op::Lt => cmp_int(stack, "<")?,
        Op::Gt => cmp_int(stack, ">")?,
        Op::Not => {
            let top = stack.pop().ok_or("stack underflow on `not`")?;
            if top != Ty::Bool {
                return Err(format!("`not` requires Bool, got {top}").into());
            }
            stack.push(Ty::Bool);
        }
        Op::Ne => {
            let (a, b) = pop2(stack, "!=")?;
            if a != b {
                return Err(
                    format!("`!=` requires both operands of the same type, got ({a} {b})").into(),
                );
            }
            stack.push(Ty::Bool);
        }
        Op::Le => cmp_int(stack, "<=")?,
        Op::Ge => cmp_int(stack, ">=")?,
        Op::Drop => {
            stack.pop().ok_or("stack underflow on `drop`")?;
        }
        Op::Dup => {
            let top = stack.last().ok_or("stack underflow on `dup`")?.clone();
            stack.push(top);
        }
        Op::Swap => {
            if stack.len() < 2 {
                return Err(format!(
                    "stack underflow on `swap` (need 2 values, have {})",
                    stack.len()
                )
                .into());
            }
            let len = stack.len();
            stack.swap(len - 1, len - 2);
        }
        Op::LoadLocal(i) | Op::MoveLocal(i) => {
            let ty = locals.get(*i as usize).cloned().ok_or_else(|| {
                format!("LoadLocal({i}) has no matching input in the enclosing function")
            })?;
            stack.push(ty);
        }
        Op::StoreLocal(i) => {
            let expected = locals.get(*i as usize).ok_or("invalid local slot")?;
            if stack.pop().as_ref() != Some(expected) {
                return Err("local assignment type mismatch".into());
            }
        }
        // Without the enclosing program, every call this body makes is unknown.
        Op::DefineFn(name, f) => {
            check_function(name, f, sigs, &crate::ownership::Program::new(&[]))?
        }
        Op::Call(name) => check_call(name, stack, sigs)?,
        Op::ClosureNew(t) => {
            for (_, ty) in t.captures.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("closure capture type mismatch".into());
                }
            }
            let expected = FnSig {
                inputs: t
                    .captures
                    .iter()
                    .enumerate()
                    .map(|(i, (n, _))| (n.clone(), t.parameter(i)))
                    .chain(t.signature.function().inputs)
                    .collect(),
                outputs: t.signature.output.clone().into_iter().collect(),
            };
            let actual = sigs.get(t.name.as_str()).ok_or("undefined closure body")?;
            if actual
                .inputs
                .iter()
                .map(|(_, t)| t)
                .ne(expected.inputs.iter().map(|(_, t)| t))
                || actual.outputs != expected.outputs
            {
                return Err("closure body signature mismatch".into());
            }
            stack.push(Ty::Closure(t.clone()));
        }
        Op::ClosureCall(t) => {
            for ty in t.signature.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("closure argument type mismatch".into());
                }
            }
            let expected = if t.once {
                Ty::Closure(t.clone())
            } else {
                Ty::Ref(Arc::new(Ty::Closure(t.clone())), t.mutable)
            };
            if stack.pop() != Some(expected) {
                return Err("closure call environment ownership mismatch".into());
            }
            stack.extend(t.signature.output.clone());
        }
        Op::ProtocolBoxNew(source, adapters, protocol) => {
            if (!matches!(source.as_ref(), Ty::Class(_) | Ty::ByteStr)
                && !matches!(source.as_ref(), Ty::Enum(t) if !t.anonymous()))
                || protocol.methods.is_empty()
                || source.storage_barrier().is_some()
                || crate::boxed::erasure_barrier(source)
                || stack.pop().as_ref() != Some(source.as_ref())
                || adapters.len() != protocol.methods.len()
            {
                return Err("invalid protocol box source or method table".into());
            }
            for (adapter, method) in adapters.iter().zip(&protocol.methods) {
                if method
                    .signature
                    .output
                    .as_ref()
                    .is_some_and(Ty::contains_reference)
                    || method.signature.inputs.len() >= 256
                {
                    return Err("invalid protocol method signature".into());
                }
                let mut expected = method.signature.function();
                expected.inputs.insert(
                    0,
                    (
                        "self".into(),
                        Ty::Ref(Arc::new((**source).clone()), method.mutable),
                    ),
                );
                if sigs.get(adapter.as_str()).copied() != Some(&expected) {
                    return Err("protocol adapter signature mismatch".into());
                }
            }
            stack.push(crate::sum::result(
                Ty::BoxedProtocol(protocol.clone()),
                crate::sum::alloc_error(),
            ));
        }
        Op::ProtocolBoxCall(protocol, index) => {
            let method = protocol
                .methods
                .get(*index)
                .ok_or("invalid protocol method index")?;
            if method
                .signature
                .output
                .as_ref()
                .is_some_and(Ty::contains_reference)
            {
                return Err("protocol methods cannot return borrows".into());
            }
            for expected in method.signature.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(expected) {
                    return Err("protocol argument mismatch".into());
                }
            }
            if stack.pop()
                != Some(Ty::Ref(
                    Arc::new(Ty::BoxedProtocol(protocol.clone())),
                    method.mutable,
                ))
            {
                return Err("protocol receiver ownership mismatch".into());
            }
            stack.extend(method.signature.output.iter().cloned());
        }
        Op::BoxedCallableNew(source, adapter, signature, mode) => {
            if signature.inputs.len() >= 256 || matches!(signature.output, Some(Ty::Ref(..))) {
                return Err("invalid boxed callable signature".into());
            }
            let source: &Ty = source;
            let Some(expected) = mode
                .accepts(source)
                .filter(|_| source.storage_barrier().is_none())
            else {
                return Err("invalid boxed callable environment".into());
            };
            if expected != signature.as_ref() || stack.pop().as_ref() != Some(source) {
                return Err("boxed callable construction signature mismatch".into());
            }
            let mut adapted = signature.function();
            adapted.inputs.insert(
                0,
                ("environment".into(), mode.environment(source, signature)),
            );
            if sigs.get(adapter.as_str()).copied() != Some(&adapted) {
                return Err("boxed callable adapter signature mismatch".into());
            }
            stack.push(crate::sum::result(
                Ty::BoxedCallable(signature.clone(), *mode),
                crate::sum::alloc_error(),
            ));
        }
        Op::BoxedCallableCall(signature, mode) => {
            if matches!(signature.output, Some(Ty::Ref(..))) {
                return Err("boxed callable cannot return references".into());
            }
            for expected in signature.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(expected) {
                    return Err("boxed callable argument mismatch".into());
                }
            }
            let owner = Ty::BoxedCallable(signature.clone(), *mode);
            let expected = match mode {
                crate::op::BoxedMode::Shared => Ty::Ref(Arc::new(owner), false),
                crate::op::BoxedMode::Mut => Ty::Ref(Arc::new(owner), true),
                crate::op::BoxedMode::Once => owner,
            };
            if stack.pop() != Some(expected) {
                return Err("boxed callable call owner mismatch".into());
            }
            stack.extend(signature.output.iter().cloned());
        }
        Op::BoxedCallableTake(source, signature) => {
            let once = crate::op::BoxedMode::Once;
            if once.accepts(source) != Some(signature.as_ref())
                || stack.pop() != Some(Ty::BoxedCallable(signature.clone(), once))
            {
                return Err("boxed callable take mismatch".into());
            }
            stack.push((**source).clone());
        }
        Op::FunctionAddress(name, signature) => {
            let actual = sigs.get(name.as_str()).ok_or("undefined function value")?;
            if actual.outputs.len() > 1 || CallableSig::from_function(actual) != **signature {
                return Err("function value signature mismatch".into());
            }
            stack.push(Ty::Callable(signature.clone()));
        }
        Op::CallIndirect(signature) | Op::TailCallIndirect(signature) => {
            for expected in signature.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(expected) {
                    return Err("indirect call argument mismatch".into());
                }
            }
            if stack.pop() != Some(Ty::Callable(signature.clone())) {
                return Err("indirect call requires a matching callable".into());
            }
            stack.extend(signature.output.iter().cloned());
            if matches!(op, Op::TailCallIndirect(_)) {
                return check_return(stack, returns);
            }
        }
        Op::TailCall(name) => {
            check_call(name, stack, sigs)?;
            return check_return(stack, returns);
        }
        Op::Return => return check_return(stack, returns),
        Op::Break | Op::Continue => {
            let expected = loop_stack.ok_or("loop control outside a loop")?;
            if stack != expected {
                return Err("loop control must preserve operand types".into());
            }
            return Ok(Flow::Exits);
        }
        Op::Match(arms) => {
            return check_match(arms, stack, locals, sigs, returns, loop_stack, yield_ty);
        }
        Op::If {
            then,
            elifs,
            otherwise,
        } => {
            if stack.pop() != Some(Ty::Bool) {
                return Err("`if` condition must produce bool".into());
            }
            let snapshot = stack.clone();
            for arm in elifs.iter() {
                let mut cond = snapshot.clone();
                if check_sequence(
                    &arm.condition,
                    &mut cond,
                    locals,
                    sigs,
                    returns,
                    loop_stack,
                    yield_ty,
                )? != Flow::Continues
                    || cond.pop() != Some(Ty::Bool)
                    || cond != snapshot
                {
                    return Err("`elif` condition must produce bool".into());
                }
            }
            // Bodies join as match arms do: every continuing one leaves the
            // same stack.
            let bodies = std::iter::once(then)
                .chain(elifs.iter().map(|arm| &arm.body))
                .chain(std::iter::once(otherwise));
            let mut joined: Option<Vec<Ty>> = None;
            for (i, body) in bodies.enumerate() {
                let mut branch = snapshot.clone();
                if check_sequence(
                    body,
                    &mut branch,
                    locals,
                    sigs,
                    returns,
                    loop_stack,
                    yield_ty,
                )? == Flow::Exits
                {
                    continue;
                }
                match &joined {
                    None => joined = Some(branch),
                    Some(expected) if &branch != expected => {
                        return Err(format!(
                            "`if` branch {i} leaves [{}], but the first continuing branch \
                             leaves [{}] (every continuing branch must produce the same \
                             stack effect)",
                            fmt_types(&branch),
                            fmt_types(expected),
                        )
                        .into());
                    }
                    Some(_) => {}
                }
            }
            let Some(joined) = joined else {
                return Ok(Flow::Exits);
            };
            *stack = joined;
        }
        Op::Cast(target) => {
            let top = stack.pop().ok_or("stack underflow on cast")?;
            if !top.is_numeric() || !target.is_numeric() {
                return Err(format!(
                    "cast `:as-{target}` requires numeric source and target types, got {top}"
                )
                .into());
            }
            stack.push(target.clone());
        }
        Op::RangeLength(element) => {
            let step = stack.pop();
            let stop = stack.pop();
            let start = stack.pop();
            if !element.is_int()
                || step != Some(Ty::I64)
                || stop.as_ref() != Some(element)
                || start.as_ref() != Some(element)
            {
                return Err(format!("range length requires ({element} {element} i64)").into());
            }
            stack.push(Ty::I64);
        }
        Op::RangeParts(element) => {
            let range = stack.pop();
            if !element.is_int() || range != Some(Ty::Range(Arc::new(element.clone()))) {
                return Err(format!("range parts requires range[{element}]").into());
            }
            stack.extend([element.clone(), Ty::I64, Ty::I64]);
        }
        Op::CheckedCast(target) => {
            let top = stack.pop().ok_or("stack underflow on cast")?;
            if !top.is_numeric() || !target.is_numeric() {
                return Err(format!(
                    "cast to {target} requires numeric source and target types, got {top}"
                )
                .into());
            }
            stack.push(target.clone());
        }
        Op::TryCast(target) => {
            let top = stack.pop().ok_or("stack underflow on checked cast")?;
            if !top.is_numeric() || !target.is_int() {
                return Err(format!(
                    "checked cast to {target} requires a numeric source and an integer target, got {top}"
                )
                .into());
            }
            stack.push(crate::sum::result(
                target.clone(),
                crate::sum::range_error(),
            ));
        }
        Op::Contains => {
            let (a, b) = pop2(stack, ":contains")?;
            if a != Ty::Str || b != Ty::Str {
                return Err(format!("`:contains` requires (Str Str), got ({a} {b})").into());
            }
            stack.push(Ty::Bool);
        }
    }
    Ok(Flow::Continues)
}

/// Pop two values off the abstract stack; produce a uniform underflow
/// error message that names the operator.
fn pop2(stack: &mut Vec<Ty>, op_label: &str) -> Result<(Ty, Ty)> {
    if stack.len() < 2 {
        return Err(format!(
            "stack underflow on `{op_label}` (need 2 values, have {})",
            stack.len()
        )
        .into());
    }
    let b = stack.pop().expect("length checked");
    let a = stack.pop().expect("length checked");
    Ok((a, b))
}

/// Stack effect for `-`, `*`, `/`: same-typed numbers in, same type out.
/// No implicit widening — the operands' types must match exactly, which is
/// the hard rule §11.2 commits to over the convenience of mixed-width
/// arithmetic.
fn arith(stack: &mut Vec<Ty>, op_label: &str) -> Result<()> {
    let (a, b) = pop2(stack, op_label)?;
    if !a.is_numeric() || a != b {
        return Err(format!("`{op_label}` requires same-typed numbers, got ({a} {b})").into());
    }
    stack.push(a);
    Ok(())
}

/// Stack effect for ordering: matching orderable operands in, Bool out.
fn cmp_int(stack: &mut Vec<Ty>, op_label: &str) -> Result<()> {
    let (a, b) = pop2(stack, op_label)?;
    if !a.is_orderable() || a != b {
        return Err(format!(
            "`{op_label}` requires same-typed numbers, strings, or orderable tuples, got ({a} {b})"
        )
        .into());
    }
    stack.push(Ty::Bool);
    Ok(())
}

/// Whether `actual` may be passed where a callee declares `expected`. A
/// pipeline's identity names the plan its holder lowers; the borrower of a
/// lent pipeline knows it by another. At calls and thread starts alike, the
/// frontend owns plan identity: it specializes the borrower for the lent plan,
/// and this check only compares item types. The hidden state arguments that
/// follow are still compared exactly.
fn argument_matches(actual: &Ty, expected: &Ty) -> bool {
    actual == expected
        || matches!((actual, expected), (Ty::Ref(a, true), Ty::Ref(b, true))
            if matches!((&**a, &**b), (Ty::Iter(a, _), Ty::Iter(b, _)) if a == b))
}

/// Stack effect for a `Call(name)`: verify the top of the stack matches
/// the function's declared inputs in declaration order, then replace them
/// with the declared outputs.
fn check_call(name: &str, stack: &mut Vec<Ty>, sigs: &Sigs) -> Result<()> {
    let sig = sigs
        .get(name)
        .ok_or_else(|| format!("call to undefined function `{name}`"))?;
    let n = sig.inputs.len();
    if stack.len() < n {
        return Err(format!(
            "calling `{name}`: needs {n} value(s) on the stack, have {}",
            stack.len()
        )
        .into());
    }
    // Inputs appear in declaration order, deepest operand first.
    let split = stack.len() - n;
    for (i, (param, expected)) in sig.inputs.iter().enumerate() {
        let actual = stack[split + i].clone();
        if !argument_matches(&actual, expected) {
            return Err(format!(
                "calling `{name}`: argument `{param}` (position {i}) \
                 expects {expected}, got {actual}"
            )
            .into());
        }
    }
    stack.truncate(split);
    for out in &sig.outputs {
        stack.push(out.clone());
    }
    Ok(())
}

/// Stack effect for `match`: pop the matched value's type, type-check
/// every arm body against a copy of the abstract stack, require continuing
/// arms to agree pointwise, and require exhaustiveness (§11.8). Exiting arms
/// are checked against the enclosing function's return signature instead.
///
/// The agreed-on shape becomes the post-match stack.
fn check_match(
    arms: &[MatchArm],
    stack: &mut Vec<Ty>,
    locals: &[Ty],
    sigs: &Sigs,
    returns: Option<&[Ty]>,
    loop_stack: Option<&[Ty]>,
    yield_ty: Option<&Ty>,
) -> Result<Flow> {
    let matched_ty = stack
        .pop()
        .ok_or("stack underflow on `match` (no value to match against)")?;
    if arms.is_empty() {
        return Err("`match` requires at least one arm".into());
    }

    // Pattern compatibility — each pattern must be reachable on the
    // matched type. Wildcards are always reachable; integer patterns are
    // legal against any integer width but their value must fit (otherwise
    // the arm could never fire after the runtime narrowing in
    // `pattern_matches`).
    for arm in arms {
        let compatible = match (matched_ty.clone(), arm.pattern) {
            (_, Pattern::Wildcard) => true,
            (Ty::Str, Pattern::Str(_)) => true,
            (Ty::Bool, Pattern::Bool(_)) => true,
            (t, Pattern::Int { value, explicit_ty }) if t.is_int() => {
                let pattern_ty = Ty::from(value);
                if explicit_ty {
                    if pattern_ty != t {
                        return Err(format!(
                            "pattern literal has type {pattern_ty}, but the matched type is {matched_ty}"
                        )
                        .into());
                    }
                } else {
                    let Value::I64(n) = value else {
                        unreachable!("unsuffixed integer patterns are i64")
                    };
                    let (lo, hi) = t.int_range().expect("integer types have a range");
                    let n = n as i128;
                    if n < lo || n >= hi {
                        return Err(format!(
                            "pattern literal {n} is out of range for {matched_ty}"
                        )
                        .into());
                    }
                }
                true
            }
            _ => false,
        };
        if !compatible {
            return Err(format!(
                "match-arm pattern is incompatible with the matched type {matched_ty}"
            )
            .into());
        }
    }

    // Exhaustiveness — Bool requires both literals (or a wildcard);
    // every other type (integers and Str) is treated as unbounded and
    // requires a wildcard arm. We deliberately do not special-case `u8`
    // (256 values, technically exhaustible by listing); that would be a
    // soft rule and §11.2 chose the hard one.
    let has_wildcard = arms.iter().any(|a| matches!(a.pattern, Pattern::Wildcard));
    let exhaustive = match matched_ty {
        Ty::Bool => {
            has_wildcard
                || (arms
                    .iter()
                    .any(|a| matches!(a.pattern, Pattern::Bool(true)))
                    && arms
                        .iter()
                        .any(|a| matches!(a.pattern, Pattern::Bool(false))))
        }
        _ => has_wildcard,
    };
    if !exhaustive {
        return Err(
            format!("non-exhaustive `match` on {matched_ty} (add the missing arm or `_`)").into(),
        );
    }

    // Check every arm body against a fresh copy of the abstract stack;
    // require all continuing arms to leave the stack in the same shape.
    let snapshot = stack.clone();
    let mut joined: Option<Vec<Ty>> = None;
    for (i, arm) in arms.iter().enumerate() {
        let mut arm_stack = snapshot.clone();
        if check_sequence(
            &arm.body,
            &mut arm_stack,
            locals,
            sigs,
            returns,
            loop_stack,
            yield_ty,
        )? == Flow::Exits
        {
            continue;
        }
        match &joined {
            None => joined = Some(arm_stack),
            Some(expected) => {
                if &arm_stack != expected {
                    return Err(format!(
                        "match arm {i} leaves [{}], but the first continuing arm leaves [{}] \
                         (every continuing arm must produce the same stack effect)",
                        fmt_types(&arm_stack),
                        fmt_types(expected),
                    )
                    .into());
                }
            }
        }
    }
    match joined {
        Some(joined) => {
            *stack = joined;
            Ok(Flow::Continues)
        }
        None => Ok(Flow::Exits),
    }
}

/// Check one function definition, naming its file in the error.
fn check_function(
    name: &str,
    f: &CompiledFn,
    sigs: &Sigs,
    program: &crate::ownership::Program,
) -> Result<()> {
    check_body(name, f, sigs, program).map_err(|e| -> Box<dyn std::error::Error> {
        // An ownership error that knows its own position already names
        // the file; the header is only a fallback. One in a generic
        // instance is positioned at the use that required the instance.
        let located = e
            .downcast_ref::<crate::ownership::Error>()
            .is_some_and(|e| e.primary.is_some() || e.required.is_some());
        match &f.location {
            Some(at) if !located => Box::new(crate::diagnostic::Diagnostic::new(
                Some(at.clone()),
                e.to_string(),
            )),
            _ => e,
        }
    })
}

/// Check one function body against its declared sig.
///
/// The body's abstract data stack starts **empty** — inputs are drained
/// into the locals frame by `Op::Call`, not left on the stack — and the
/// inputs become the body's `locals` for `LoadLocal` to resolve against.
/// At end of body the abstract stack must equal the declared outputs
/// exactly; anything else is a type error.
fn check_body(
    fn_name: &str,
    f: &CompiledFn,
    sigs: &Sigs,
    program: &crate::ownership::Program,
) -> Result<()> {
    let (sig, body, yield_ty) = (&*f.sig, &*f.body, f.generator.as_ref());
    let locals: Vec<Ty> = sig
        .inputs
        .iter()
        .map(|(_, t)| t.clone())
        .chain(f.locals.iter().cloned())
        .collect();
    let mut stack: Vec<Ty> = Vec::new();
    let returns = if yield_ty.is_some() {
        &[][..]
    } else {
        &sig.outputs
    };
    crate::ownership::check(body, &locals, sig.inputs.len(), &f.origins, program)
        .map_err(|e| -> Box<dyn std::error::Error> { e })?;
    let flow = check_sequence(
        body,
        &mut stack,
        &locals,
        sigs,
        Some(returns),
        None,
        yield_ty,
    )
    .map_err(|e| -> Box<dyn Error> { format!("in `{fn_name}`: {e}").into() })?;
    if flow == Flow::Continues && stack != returns {
        return Err(format!(
            "function `{fn_name}` body leaves [{}], but signature declares outputs [{}]",
            fmt_types(&stack),
            fmt_types(&sig.outputs),
        )
        .into());
    }
    Ok(())
}

/// Render a sequence of types for a human, space-separated, the same
/// orientation as the runtime `stack_repr` (deepest on the left).
fn fmt_types(tys: &[Ty]) -> String {
    tys.iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_block_step(body: Vec<Op>, result: Ty, stack: &mut Vec<Ty>) -> Result<Flow> {
        step(
            &Op::CheckedBlock {
                body: body.into(),
                result,
            },
            stack,
            &[],
            &Sigs::default(),
            Some(&[]),
            Some(&[]),
            None,
        )
    }

    #[test]
    fn checked_unit_preserves_empty_and_pending_entry_operands() {
        let result = crate::sum::result(Ty::Unit, Ty::I64);
        for entry in [vec![], vec![Ty::I64, Ty::Bool], vec![Ty::Unit]] {
            let mut stack = entry.clone();
            assert!(matches!(
                checked_block_step(vec![], result.clone(), &mut stack).unwrap(),
                Flow::Continues
            ));
            assert_eq!(stack, [entry, vec![result.clone()]].concat());
        }
    }

    #[test]
    fn checked_unit_rejects_extra_missing_and_replaced_entry_operands() {
        for (entry, body) in [
            (vec![], vec![Op::PushUnit]),
            (vec![], vec![Op::PushBool(true)]),
            (vec![Ty::I64], vec![Op::Drop]),
            (vec![Ty::I64], vec![Op::Drop, Op::PushBool(true)]),
            (vec![Ty::I64, Ty::Bool], vec![Op::Swap]),
        ] {
            let error = checked_block_step(
                body,
                crate::sum::result(Ty::Unit, Ty::I64),
                &mut entry.clone(),
            )
            .err()
            .expect("malformed Unit success must fail");
            assert_eq!(
                error.to_string(),
                "a checked block must leave its success value"
            );
        }
    }

    #[test]
    fn checked_blocks_reject_terminating_and_unreachable_success_paths() {
        for success in [Ty::Unit, Ty::I64] {
            for body in [
                vec![Op::Return],
                vec![Op::Break],
                vec![Op::Continue],
                vec![Op::Unreachable],
                vec![Op::Return, Op::PushUnit],
            ] {
                assert!(
                    checked_block_step(
                        body,
                        crate::sum::result(success.clone(), Ty::I64),
                        &mut vec![],
                    )
                    .is_err()
                );
            }
        }
    }

    #[test]
    fn checked_nonunit_still_requires_its_success_type() {
        let result = crate::sum::result(Ty::I64, Ty::Bool);
        for body in [vec![], vec![Op::PushUnit], vec![Op::PushBool(true)]] {
            let error = checked_block_step(body, result.clone(), &mut vec![])
                .err()
                .expect("missing or mistyped nonunit success must fail");
            assert_eq!(
                error.to_string(),
                "a checked block must leave its success value"
            );
        }
        let mut stack = vec![Ty::Bool, Ty::I64];
        assert!(checked_block_step(vec![], result.clone(), &mut stack).is_ok());
        assert_eq!(stack, vec![Ty::Bool, result]);
    }

    #[test]
    fn checked_blocks_reject_empty_success_fields() {
        let Ty::Enum(sum) = crate::sum::result(Ty::Unit, Ty::I64) else {
            unreachable!()
        };
        let mut malformed = (*sum.get()).clone();
        malformed.variants[0].fields.clear();
        let result = Ty::Enum(crate::nominal::Nominal::new(malformed));
        assert!(checked_block_step(vec![], result, &mut vec![]).is_err());
    }

    #[test]
    fn byte_views_require_a_bytes_class_and_its_method() {
        use crate::collection::CollectionOp;
        fn class(name: &str) -> Ty {
            Ty::Class(crate::nominal::Nominal::new(crate::record::ClassType {
                name: name.into(),
                fields: vec![("storage".into(), Ty::ByteStorage)],
                destructor: None,
                fallible_init: false,
                copy: false,
                forwards_operators: false,
                redacted: false,
                quantity: None,
                bounds: None,
                bytes: std::sync::OnceLock::new(),
                managed: std::sync::OnceLock::new(),
            }))
        }
        let get = FnSig {
            inputs: vec![],
            outputs: vec![],
        };
        let set = get.clone();
        let get_name = crate::record::method(crate::frontend::BYTES_CLASS, "get");
        let set_name = crate::record::method(crate::frontend::BYTES_CLASS, "set");
        let sigs: Sigs = [(get_name.as_str(), &get), (set_name.as_str(), &set)]
            .into_iter()
            .collect();
        let run = |operation: CollectionOp, sigs: &Sigs| {
            let (inputs, _) = operation.signature();
            step(
                &Op::Collection(operation),
                &mut inputs.clone(),
                &[],
                sigs,
                None,
                None,
                None,
            )
        };
        let bytes = class(crate::frontend::BYTES_CLASS);
        assert!(run(CollectionOp::BytesGetView(bytes.clone()), &sigs).is_ok());
        assert!(run(CollectionOp::BytesSetView(bytes.clone()), &sigs).is_ok());
        for (ty, sigs) in [
            (Ty::I64, &sigs),
            (class("app.Bytes"), &sigs),
            (bytes, &Sigs::default()),
        ] {
            for operation in [
                CollectionOp::BytesGetView(ty.clone()),
                CollectionOp::BytesSetView(ty.clone()),
            ] {
                let error = run(operation, sigs)
                    .err()
                    .expect("a byte view on another receiver must be rejected");
                assert_eq!(
                    error.to_string(),
                    "byte view access requires a Bytes receiver"
                );
            }
        }
    }

    #[test]
    fn protocol_calls_validate_index_arguments_identity_and_exclusivity() {
        let protocol = Arc::new(crate::boxed::ProtocolType {
            name: "P".into(),
            arguments: vec![],
            methods: vec![crate::boxed::ProtocolMethod {
                name: "set".into(),
                mutable: true,
                signature: CallableSig {
                    inputs: vec![Ty::I64],
                    output: None,
                },
            }],
        });
        let owner = Ty::BoxedProtocol(protocol.clone());
        for (index, stack, expected) in [
            (1, vec![], "index"),
            (
                0,
                vec![Ty::Ref(Arc::new(owner.clone()), true), Ty::Bool],
                "argument",
            ),
            (
                0,
                vec![Ty::Ref(Arc::new(owner.clone()), false), Ty::I64],
                "ownership",
            ),
            (
                0,
                vec![Ty::Ref(Arc::new(Ty::I64), true), Ty::I64],
                "ownership",
            ),
        ] {
            let error = step(
                &Op::ProtocolBoxCall(protocol.clone(), index),
                &mut stack.clone(),
                &[],
                &HashMap::default(),
                None,
                None,
                None,
            )
            .err()
            .expect("invalid protocol call must be rejected");
            assert!(error.to_string().contains(expected), "{error}");
        }
        let mut stack = vec![Ty::Ref(Arc::new(owner), true), Ty::I64];
        step(
            &Op::ProtocolBoxCall(protocol, 0),
            &mut stack,
            &[],
            &HashMap::default(),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(stack.is_empty());
    }

    /// Bodies are checked out of order, so the error must be chosen by position.
    #[test]
    fn first_error_in_program_order_for_every_worker_count() {
        let source: String = (0..64)
            .map(|i| format!("def unit{i}() -> i64:\n    {i}\n"))
            .chain(["def main() -> ():\n    pass\n".to_string()])
            .collect();
        let mut heap = crate::value::Heap::default();
        let mut program = crate::frontend::compile(&source, &mut heap).unwrap();
        fn corrupt(ops: &mut [Op], name: &str, body: Vec<Op>) {
            for op in ops {
                match op {
                    Op::DefineFn(n, f) if n == name => f.body = body.clone().into(),
                    _ => {}
                }
            }
        }
        corrupt(&mut program.ops, "unit40", Vec::new());
        let later = check_with(&program.ops, 1).unwrap_err().to_string();
        corrupt(&mut program.ops, "unit9", vec![Op::PushBool(true)]);
        let first = check_with(&program.ops, 1).unwrap_err().to_string();
        assert_ne!(first, later);
        for workers in [1, 2, 8] {
            let error = check_with(&program.ops, workers).unwrap_err();
            // The worker boundary keeps the diagnostic value.
            assert!(error.is::<crate::diagnostic::Diagnostic>(), "{error}");
            assert_eq!(error.to_string(), first);
        }
    }

    fn sequence(ops: Vec<Op>, locals: &[Ty]) -> Result<()> {
        let mut stack = Vec::new();
        check_sequence(&ops, &mut stack, locals, &Sigs::default(), None, None, None).map(|_| ())
    }

    fn proved(op: Op, lo: i128, hi: i128) -> Op {
        Op::Proved(Box::new(op), Interval { lo, hi })
    }

    fn int(value: i64) -> Op {
        Op::PushInt(Value::I64(value))
    }

    /// A `Proved` interval is recomputed from operands the operations
    /// determine, through literals, other proofs, casts, `//`, `%` and `&`.
    #[test]
    fn a_proved_interval_must_follow_from_known_operands() {
        let site = || {
            Op::Site(crate::ownership::Site {
                line: 1,
                column: 1,
                scope_end: false,
            })
        };
        for ops in [
            vec![int(3), int(4), proved(Op::Add, 7, 7)],
            // A wider claim is still true.
            vec![int(3), int(4), proved(Op::Add, 0, 9)],
            vec![
                int(3),
                int(4),
                site(),
                proved(Op::Mul, 12, 12),
                int(1),
                proved(Op::Sub, 11, 11),
            ],
            vec![
                int(5),
                int(2),
                proved(Op::Integer(IntegerOp::ShiftLeft), 20, 20),
            ],
            vec![
                Op::PushInt(Value::U8(200)),
                proved(Op::CheckedCast(Ty::I64), 200, 200),
                int(1000),
                Op::Modulo(Divisor::Positive),
                int(7),
                Op::Integer(IntegerOp::And),
                int(i64::MAX - 7),
                proved(Op::Add, i128::from(i64::MAX) - 7, i128::from(i64::MAX)),
            ],
        ] {
            sequence(ops, &[]).unwrap();
        }
        for (ops, message) in [
            (
                vec![int(3), int(4), proved(Op::Add, 0, 5)],
                "`Proved` Add 0..5 does not follow from its operands 3..3 and 4..4",
            ),
            (
                vec![int(i64::MAX), int(1), proved(Op::Add, 0, 5)],
                "does not follow from its operands",
            ),
            (
                vec![
                    int(3),
                    int(4),
                    proved(Op::Mul, 12, 12),
                    int(1),
                    proved(Op::Sub, 12, 12),
                ],
                "`Proved` Sub 12..12 does not follow from its operands 12..12 and 1..1",
            ),
            (
                vec![
                    int(5),
                    int(64),
                    proved(Op::Integer(IntegerOp::ShiftLeft), 0, 5),
                ],
                "does not follow from its operands",
            ),
            (
                vec![int(300), proved(Op::CheckedCast(Ty::U8), 0, 255)],
                "`Proved` CheckedCast(U8) 0..255 does not follow from its operand 300..300",
            ),
            (
                vec![
                    int(250),
                    int(1000),
                    Op::Modulo(Divisor::Positive),
                    proved(Op::CheckedCast(Ty::U8), 0, 100),
                ],
                "does not follow from its operand 0..250",
            ),
        ] {
            let error = sequence(ops, &[]).unwrap_err().to_string();
            assert!(error.contains(message), "{error}");
        }
    }

    /// A local's interval is the frontend's alone, so a proof over one is
    /// checked only for fitting its type.
    #[test]
    fn a_proof_over_a_local_is_checked_for_its_shape_alone() {
        let locals = [Ty::I64];
        sequence(
            vec![Op::LoadLocal(0), int(4), proved(Op::Add, 0, 9)],
            &locals,
        )
        .unwrap();
        // A call or any other operation forgets what was known below it.
        sequence(
            vec![int(3), int(4), Op::Dup, Op::Drop, proved(Op::Add, 0, 5)],
            &locals,
        )
        .unwrap();
        let error = sequence(
            vec![Op::LoadLocal(0), int(4), proved(Op::Add, 0, 1i128 << 70)],
            &locals,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("does not fit"), "{error}");
    }

    #[test]
    fn a_divisor_proof_must_follow_from_a_known_divisor() {
        let locals = [Ty::I64];
        for (divisor, proof, valid) in [
            (5, Divisor::Positive, true),
            (-1, Divisor::NonZero, true),
            (-2, Divisor::BelowMinusOne, true),
            (0, Divisor::Fits, true),
            (0, Divisor::NonZero, false),
            (0, Divisor::Positive, false),
            (-1, Divisor::BelowMinusOne, false),
            (-1, Divisor::Positive, false),
        ] {
            for op in [Op::FloorDiv(proof), Op::Modulo(proof)] {
                let result = sequence(vec![Op::LoadLocal(0), int(divisor), op], &locals);
                assert_eq!(result.is_ok(), valid, "{divisor} {proof:?}");
            }
        }
        // `MIN // -1` is excluded only by a known dividend.
        let fits = |dividend: i64| {
            sequence(
                vec![int(dividend), int(-1), Op::FloorDiv(Divisor::NonZeroFits)],
                &[],
            )
        };
        fits(i64::MIN + 1).unwrap();
        let error = fits(i64::MIN).unwrap_err().to_string();
        assert!(error.contains("the divisor proof NonZeroFits"), "{error}");
    }
}
