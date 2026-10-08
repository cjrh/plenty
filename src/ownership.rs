//! Definite ownership and last-use loans over an explicit access control-flow graph.
use crate::op::{Op, Ty};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type State = Vec<bool>;
#[derive(Clone, Debug, PartialEq)]
pub struct Loan {
    pub id: usize,
    pub root: u8,
    /// Statically known field projections; an empty path covers the whole root.
    pub fields: Vec<usize>,
    /// Calls expose an origin, not a field mapping. Descendants must retain its footprint.
    pub precise: bool,
    pub mutable: bool,
    pub parent: Option<usize>,
    /// Borrowing an environment keeps all references stored inside it live.
    pub dependencies: Vec<usize>,
}
#[derive(Default)]
struct Loop {
    entry: State,
    breaks: Vec<State>,
}

pub fn check(ops: &[Op], locals: &[Ty], parameters: usize) -> Result<()> {
    let mut state = (0..locals.len()).map(|i| i < parameters).collect();
    sequence(ops, locals, &mut state, &mut Vec::new())?;
    check_loans(ops)?;
    Ok(())
}
fn merge(states: &[State], into: &mut State) {
    if let Some(first) = states.first() {
        *into = first.clone();
        for state in &states[1..] {
            for (a, b) in into.iter_mut().zip(state) {
                *a &= b;
            }
        }
    }
}
fn backedge(state: &[bool], entry: &[bool], locals: &[Ty]) -> Result<()> {
    if locals
        .iter()
        .enumerate()
        .any(|(i, _)| entry[i] && !state[i])
    {
        return Err(
            "owned value moved across a loop backedge; reinitialize it before continuing".into(),
        );
    }
    Ok(())
}
fn sequence(ops: &[Op], locals: &[Ty], state: &mut State, loops: &mut Vec<Loop>) -> Result<bool> {
    for op in ops {
        match op {
            Op::Try { cleanup, .. } => {
                sequence(cleanup, locals, &mut state.clone(), &mut Vec::new())?;
            }
            Op::MoveLocal(i, site) | Op::Next(i, site) => {
                if !state.get(*i as usize).copied().unwrap_or(false) {
                    return Err(format!("{site}: use of moved or possibly moved binding").into());
                }
                if matches!(op, Op::MoveLocal(..)) {
                    state[*i as usize] = false;
                }
            }
            Op::LoadLocal(i) | Op::BorrowLocal(i, _) | Op::Access(i, _, _) => {
                if !state[*i as usize] {
                    return Err("use of moved or possibly moved binding".into());
                }
            }
            Op::StoreLocal(i) => state[*i as usize] = true,
            Op::DropLocal(i) => state[*i as usize] = false,
            Op::Match(arms) => {
                let initial = state.clone();
                let mut continuing = Vec::new();
                for arm in arms.iter() {
                    let mut next = initial.clone();
                    if sequence(&arm.body, locals, &mut next, loops)? {
                        continuing.push(next);
                    }
                }
                if continuing.is_empty() {
                    return Ok(false);
                }
                merge(&continuing, state);
            }
            Op::Loop { condition, body } => {
                let entry = state.clone();
                sequence(condition, locals, state, &mut Vec::new())?;
                let zero = state.clone();
                loops.push(Loop {
                    entry,
                    breaks: vec![zero.clone()],
                });
                if sequence(body, locals, state, loops)? {
                    backedge(state, &loops.last().unwrap().entry, locals)?;
                }
                let exits = loops.pop().unwrap().breaks;
                merge(&exits, state);
            }
            Op::Continue => {
                let target = loops.last().ok_or("continue outside loop")?;
                backedge(state, &target.entry, locals)?;
                return Ok(false);
            }
            Op::Break => {
                loops
                    .last_mut()
                    .ok_or("break outside loop")?
                    .breaks
                    .push(state.clone());
                return Ok(false);
            }
            Op::Return | Op::TailCall(_) | Op::TailCallIndirect(_) | Op::Unreachable => {
                return Ok(false)
            }
            _ => {}
        }
    }
    Ok(true)
}

#[derive(Default)]
struct Node {
    op: Option<Op>,
    successors: Vec<usize>,
}

/// Flatten structured operations to an access CFG. Backedges and early exits are
/// explicit, so liveness reaches a fixed point rather than guessing at loop uses.
fn graph(ops: &[Op], next: usize, targets: Option<(usize, usize)>, nodes: &mut Vec<Node>) -> usize {
    let mut next = next;
    for op in ops.iter().rev() {
        match op {
            Op::Try { cleanup, .. } => {
                let failure = graph(cleanup, 0, None, nodes);
                nodes.push(Node {
                    op: None,
                    successors: vec![next, failure],
                });
            }
            Op::Match(arms) => {
                let successors = arms
                    .iter()
                    .map(|a| graph(&a.body, next, targets, nodes))
                    .collect();
                nodes.push(Node {
                    op: None,
                    successors,
                });
            }
            Op::Loop { condition, body } => {
                let header = nodes.len();
                nodes.push(Node::default());
                let body = graph(body, header, Some((header, next)), nodes);
                let branch = nodes.len();
                nodes.push(Node {
                    op: None,
                    successors: vec![body, next],
                });
                let test = graph(condition, branch, targets, nodes);
                nodes[header].successors.push(test);
                next = header;
                continue;
            }
            Op::Break => nodes.push(Node {
                op: None,
                successors: vec![targets.expect("checked loop").1],
            }),
            Op::Continue => nodes.push(Node {
                op: None,
                successors: vec![targets.expect("checked loop").0],
            }),
            Op::Return | Op::TailCall(_) | Op::TailCallIndirect(_) | Op::Unreachable => {
                nodes.push(Node::default())
            }
            _ => nodes.push(Node {
                op: Some(op.clone()),
                successors: vec![next],
            }),
        }
        next = nodes.len() - 1;
    }
    next
}

fn check_loans(ops: &[Op]) -> Result<()> {
    fn use_loan(
        id: usize,
        loans: &std::collections::HashMap<usize, Loan>,
        live: &mut BTreeSet<usize>,
    ) {
        if live.insert(id) {
            for dependency in &loans[&id].dependencies {
                use_loan(*dependency, loans, live);
            }
        }
    }
    fn has_loans(ops: &[Op]) -> bool {
        ops.iter().any(|op| match op {
            Op::Try { cleanup, .. } => has_loans(cleanup),
            Op::Loan(_) => true,
            Op::Match(arms) => arms.iter().any(|arm| has_loans(&arm.body)),
            Op::Loop { condition, body } => has_loans(condition) || has_loans(body),
            _ => false,
        })
    }
    if !has_loans(ops) {
        return Ok(());
    }
    let mut nodes = vec![Node::default()];
    let entry = graph(ops, 0, None, &mut nodes);
    let mut reachable = BTreeSet::new();
    let mut todo = vec![entry];
    while let Some(n) = todo.pop() {
        if reachable.insert(n) {
            todo.extend(&nodes[n].successors);
        }
    }
    let loans: std::collections::HashMap<usize, Loan> = nodes
        .iter()
        .filter_map(|n| match &n.op {
            Some(Op::Loan(l)) => Some((l.id, l.clone())),
            _ => None,
        })
        .collect();
    let mut live = vec![BTreeSet::new(); nodes.len()];
    loop {
        let mut changed = false;
        for &i in &reachable {
            let mut before = BTreeSet::new();
            for &s in &nodes[i].successors {
                before.extend(&live[s]);
            }
            match &nodes[i].op {
                Some(Op::UseLoan(id)) | Some(Op::Access(_, _, Some(id))) => {
                    before.remove(id);
                    use_loan(*id, &loans, &mut before);
                }
                Some(Op::Loan(l)) => {
                    before.remove(&l.id);
                    if let Some(p) = l.parent {
                        before.insert(p);
                    }
                }
                _ => {}
            }
            if before != live[i] {
                live[i] = before;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let ancestor = |candidate: usize, mut child: usize| loop {
        if candidate == child {
            return true;
        }
        match loans.get(&child).and_then(|l| l.parent) {
            Some(p) => child = p,
            None => return false,
        }
    };
    for &i in &reachable {
        let mut after = BTreeSet::new();
        for &s in &nodes[i].successors {
            after.extend(&live[s]);
        }
        let access = match &nodes[i].op {
            Some(Op::Loan(l)) => Some((l.root, l.mutable, l.parent, Some(l.id))),
            Some(Op::Access(root, write, via)) => Some((*root, *write, *via, None)),
            Some(Op::MoveLocal(root, _))
            | Some(Op::StoreLocal(root))
            | Some(Op::DropLocal(root))
            | Some(Op::Next(root, _)) => Some((*root, true, None, None)),
            Some(Op::Yield(_)) if !after.is_empty() => {
                return Err("a borrow cannot remain live across yield".into())
            }
            _ => None,
        };
        if let Some((root, write, via, defining)) = access {
            for id in &after {
                let loan = &loans[id];
                if Some(*id) == defining || via.is_some_and(|v| ancestor(*id, v)) {
                    continue;
                }
                let fields = defining
                    .or(via)
                    .map(|id| loans[&id].fields.as_slice())
                    .unwrap_or(&[]);
                let overlaps = loan.fields.starts_with(fields) || fields.starts_with(&loan.fields);
                if loan.root == root && overlaps && (write || loan.mutable) {
                    return Err(format!(
                        "conflicting borrow: cannot {} binding while {} borrow is live",
                        if write {
                            "modify, move, or exclusively borrow"
                        } else {
                            "read or borrow"
                        },
                        if loan.mutable {
                            "an exclusive"
                        } else {
                            "a shared"
                        }
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}
