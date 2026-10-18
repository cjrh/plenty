//! Local submission provenance for blocking future retrieval.
use super::{graph, Node, Op, Site, Ty};
use crate::executor::ExecutorOp;
use crate::sum::EnumOp;

type Jobs = Vec<(Ty, Option<Site>)>;

/// Local assignments are joined conservatively, including loop-carried moves.
/// Calls and multi-field aggregate storage are outside this analysis.
pub(super) struct Futures {
    jobs: Vec<Jobs>,
}

fn transparent(op: &Op) -> bool {
    matches!(
        op,
        Op::Site(_) | Op::CallerSite(_) | Op::UseLoan(_) | Op::Access(..) | Op::Loan(_)
    ) || matches!(
        op,
        Op::Enum(EnumOp::Unwrap(_) | EnumOp::Field(..) | EnumOp::Take(..) | EnumOp::Widen(..))
    ) || matches!(op, Op::Enum(EnumOp::New(ty, tag)) if ty.get().variants[*tag].fields.len() == 1)
}

impl Futures {
    pub(super) fn new(body: &[Op], locals: usize) -> Self {
        fn submits(ops: &[Op]) -> bool {
            ops.iter().any(|op| match op {
                Op::Executor(ExecutorOp::Submit(..)) => true,
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
        if !nodes.iter().any(|node| {
            matches!(&node.op, Some(Op::Executor(ExecutorOp::Submit(job, _)))
                if !crate::threading::endpoints([job.input.clone()], true, true).is_empty())
        }) {
            return Self { jobs };
        }
        let mut predecessors = vec![Vec::new(); nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            for &next in &node.successors {
                predecessors[next].push(index);
            }
        }
        let mut aliases = vec![Vec::new(); locals];
        let mut visited = vec![0; nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            let Some(Op::StoreLocal(slot)) = node.op else {
                continue;
            };
            let slot = slot as usize;
            let mut pending = predecessors[index].clone();
            let generation = index + 1;
            while let Some(index) = pending.pop() {
                if std::mem::replace(&mut visited[index], generation) == generation {
                    continue;
                }
                let node = &nodes[index];
                match &node.op {
                    Some(Op::Executor(ExecutorOp::Submit(job, _))) => {
                        for message in crate::threading::endpoints([job.input.clone()], true, true)
                        {
                            let origin = (message, node.site);
                            if !jobs[slot].contains(&origin) {
                                jobs[slot].push(origin);
                            }
                        }
                    }
                    Some(Op::LoadLocal(source) | Op::MoveLocal(source)) => {
                        if !aliases[slot].contains(source) {
                            aliases[slot].push(*source);
                        }
                    }
                    None => pending.extend(&predecessors[index]),
                    Some(op) if transparent(op) => pending.extend(&predecessors[index]),
                    _ => {}
                }
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
    pub(super) fn before(&self, ops: &[Op]) -> &[(Ty, Option<Site>)] {
        match ops.iter().rev().find(|op| !transparent(op)) {
            Some(Op::MoveLocal(slot)) => self.jobs.get(*slot as usize).map_or(&[], Vec::as_slice),
            _ => &[],
        }
    }
}
