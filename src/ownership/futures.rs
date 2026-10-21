//! Local submission provenance for blocking future retrieval.
use super::{Hazard, Node, Op, Program, Site, graph};
use crate::executor::ExecutorOp;
use crate::hash::HashSet;
use crate::op::{Forward, TokenMark, Ty};
use crate::sum::EnumOp;

type Jobs = Vec<(Hazard, Option<Site>)>;

/// Local assignments are joined conservatively, including loop-carried moves.
/// The result of a call bracketed by `Op::Forwards` can hold every future its
/// arguments read or submit, and any job the callee reaches submits with a
/// matching result type (issue #352). Other calls and multi-field aggregate
/// storage are outside this analysis.
pub(super) struct Futures {
    jobs: Vec<Jobs>,
}

fn transparent(op: &Op) -> bool {
    matches!(
        op,
        Op::Site(_)
            | Op::CallerSite(_)
            | Op::Returning(..)
            | Op::Building(_)
            | Op::UseLoan(_)
            | Op::Access(..)
            | Op::Loan(_)
            // Scope cleanup after an extraction, such as `drop_job()`'s
            // moved source, leaves the extracted value on the stack.
            | Op::DropLocal(_)
            | Op::Forwards(_)
    ) || matches!(
        op,
        Op::Enum(EnumOp::Unwrap(_) | EnumOp::Field(..) | EnumOp::Take(..) | EnumOp::Widen(..))
    ) || matches!(op, Op::Enum(EnumOp::New(ty, tag)) if ty.get().variants[*tag].fields.len() == 1)
}

/// The ops after the conversion that `drop_job()` applies to a submission
/// Result at the start of `ops` (see `Generator::drop_job`), when there is
/// one. The converted Result fails exactly when the submission failed.
pub(super) fn after_drop_job(ops: &[Op]) -> Option<&[Op]> {
    let [
        Op::StoreLocal(source),
        Op::LoadLocal(load),
        Op::Enum(EnumOp::Tag(_)),
        Op::PushInt(_),
        Op::Eq,
        Op::Match(arms),
        rest @ ..,
    ] = ops
    else {
        return None;
    };
    let [ok, _] = &arms[..] else {
        return None;
    };
    if source != load || ok.body.first() != Some(&Op::MoveLocal(*source)) {
        return None;
    }
    let end = rest
        .iter()
        .position(|op| !matches!(op, Op::DropLocal(_)))
        .unwrap_or(rest.len());
    Some(&rest[end..])
}

/// The function a call op names, if any.
fn callee(op: &Op) -> Option<&str> {
    match op {
        Op::Call(name) => Some(name),
        Op::ClosureCall(closure) if !closure.name.is_empty() => Some(&closure.name),
        _ => None,
    }
}

fn calls(op: &Op) -> bool {
    matches!(
        op,
        Op::Call(_) | Op::ClosureCall(_) | Op::CallIndirect(_) | Op::BoxedCallableCall(_)
    )
}

/// The `Forward::Call` marker before the call at node `call`, through the
/// other markers the frontend places between them.
fn marker<'a>(
    nodes: &[Node<'a>],
    predecessors: &[Vec<usize>],
    call: usize,
) -> Option<(usize, u32, &'a [Ty])> {
    let mut pending = predecessors[call].clone();
    while let Some(index) = pending.pop() {
        match nodes[index].op {
            Some(Op::Forwards(Forward::Call(id, results))) => return Some((index, *id, results)),
            Some(Op::Passes(_) | Op::ForwardsReferences) => pending.extend(&predecessors[index]),
            _ => {}
        }
    }
    None
}

impl Futures {
    pub(super) fn new(body: &[Op], locals: usize, program: &Program) -> Self {
        fn submits(ops: &[Op]) -> bool {
            ops.iter().any(|op| match op {
                Op::Executor(ExecutorOp::Submit(..)) | Op::Forwards(Forward::Call(..)) => true,
                Op::Try { cleanup, .. } => submits(cleanup),
                Op::Match(arms) => arms.iter().any(|arm| submits(&arm.body)),
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    submits(then)
                        || elifs
                            .iter()
                            .any(|arm| submits(&arm.condition) || submits(&arm.body))
                        || submits(otherwise)
                }
                Op::Loop { condition, body } => submits(condition) || submits(body),
                _ => false,
            })
        }
        if !submits(body) {
            return Self { jobs: Vec::new() };
        }
        let mut jobs = vec![Jobs::new(); locals];
        let mut nodes = vec![Node::default()];
        graph(body, None, 0, None, &mut nodes);
        if !nodes.iter().any(|node| match &node.op {
            Some(Op::Executor(ExecutorOp::Submit(job, _))) => {
                Hazard::of(&job.input).next().is_some()
            }
            Some(Op::Forwards(Forward::Call(..)) | Op::Tokens(TokenMark::Holds(_))) => true,
            _ => false,
        }) {
            return Self { jobs };
        }
        let mut predecessors = vec![Vec::new(); nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            for &next in node.successors.as_slice() {
                predecessors[next].push(index);
            }
        }
        let mut aliases = vec![Vec::new(); locals];
        let mut visited = vec![0; nodes.len()];
        // Nodes visited while searching a call's arguments, by call.
        let mut arguments = HashSet::default();
        for (index, node) in nodes.iter().enumerate() {
            let Some(Op::StoreLocal(slot)) = node.op else {
                continue;
            };
            let slot = *slot as usize;
            // Each node to visit, with the call whose arguments it is searched
            // in, or `None` while following the stored value itself.
            let mut pending: Vec<_> = predecessors[index].iter().map(|&p| (p, None)).collect();
            let generation = index + 1;
            arguments.clear();
            let mut add = |origin: (Hazard, Option<Site>)| {
                if !jobs[slot].contains(&origin) {
                    jobs[slot].push(origin);
                }
            };
            while let Some((index, call)) = pending.pop() {
                let fresh = match call {
                    None => std::mem::replace(&mut visited[index], generation) != generation,
                    Some(id) => arguments.insert((index, id)),
                };
                if !fresh {
                    continue;
                }
                let node = &nodes[index];
                let op = node.op;
                if let Some(op) = op.filter(|op| calls(op)) {
                    // A forwarding call in an argument also adds its callee's jobs.
                    if let Some((marker, id, results)) = marker(&nodes, &predecessors, index) {
                        for hazard in program.submissions(callee(op), results) {
                            add((hazard, node.site));
                        }
                        if call.is_none() {
                            pending.extend(predecessors[marker].iter().map(|&p| (p, Some(id))));
                            continue;
                        }
                    }
                }
                match (op, call) {
                    (Some(Op::Forwards(Forward::Arguments(start))), Some(id)) if *start == id => {
                        continue;
                    }
                    (Some(Op::Executor(ExecutorOp::Submit(job, _))), _) => {
                        for hazard in Hazard::of(&job.input) {
                            add((hazard, node.site));
                        }
                        // The frontend places the job's token marker directly before it.
                        for &marker in &predecessors[index] {
                            if let Some(Op::Tokens(TokenMark::Holds(slots))) = nodes[marker].op {
                                for &slot in slots.iter() {
                                    add((Hazard::Cancels(slot), node.site));
                                }
                            }
                        }
                        if call.is_none() {
                            continue;
                        }
                    }
                    (Some(Op::LoadLocal(source) | Op::MoveLocal(source)), _)
                    | (Some(Op::BorrowLocal(source, _)), Some(_)) => {
                        if !aliases[slot].contains(source) {
                            aliases[slot].push(*source);
                        }
                        if call.is_none() {
                            continue;
                        }
                    }
                    (None, _) | (_, Some(_)) => {}
                    (Some(op), None) if transparent(op) => {}
                    _ => continue,
                }
                pending.extend(predecessors[index].iter().map(|&p| (p, call)));
            }
        }
        loop {
            let mut changed = false;
            for (slot, sources) in aliases.iter().enumerate() {
                for &source in sources {
                    for origin in jobs[source as usize].clone() {
                        if !jobs[slot].contains(&origin) {
                            jobs[slot].push(origin);
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                return Self { jobs };
            }
        }
    }

    /// The frontend materializes a future receiver in a local before consuming it.
    pub(super) fn before(&self, ops: &[Op]) -> &[(Hazard, Option<Site>)] {
        match ops.iter().rev().find(|op| !transparent(op)) {
            Some(Op::MoveLocal(slot)) => self.jobs.get(*slot as usize).map_or(&[], Vec::as_slice),
            _ => &[],
        }
    }

    /// Every job a future stored in `slot` can come from.
    pub(super) fn of(&self, slot: u8) -> &[(Hazard, Option<Site>)] {
        self.jobs.get(slot as usize).map_or(&[], Vec::as_slice)
    }
}
