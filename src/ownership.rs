//! Definite ownership and last-use loans over an explicit access control-flow graph.
use crate::op::{Op, Ty};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::rc::Rc;

/// Source position of the operations that follow an `Op::Site` marker in the
/// same sequence. Nested blocks inherit it until they place their own marker.
/// The marker travels with the operations, so inserting or cloning operations
/// cannot detach an access from its source position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub line: u32,
    pub column: u32,
    /// The following operations are the cleanup of the `with` block that
    /// starts here, not source the programmer wrote at this position.
    pub scope_end: bool,
}

/// Per-function facts used only to render ownership diagnostics. They never
/// affect liveness or aliasing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Origins {
    pub source: Option<Rc<str>>,
    /// Source name of each local slot, parameters first. Slots are never
    /// reused, so a name stays valid after its scope ends. Compiler
    /// temporaries have none.
    pub names: Vec<Option<Rc<str>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Moved on every path reaching the use (`true`) or only on some.
    Moved(bool),
    /// Never initialized, or already released by scope cleanup.
    Unavailable,
    MovedInLoop,
    Conflict {
        action: &'static str,
        /// The borrowed place when it differs from the accessed one.
        borrowed: Option<String>,
        exclusive: bool,
    },
    LiveAcrossYield,
}

/// A rejected operation, the place it touches, and the related source sites.
#[derive(Debug)]
pub struct Error {
    pub kind: Kind,
    /// Binding or field path, absent for a compiler temporary.
    pub place: Option<String>,
    /// Absent only for operations that carry no source marker.
    pub primary: Option<Site>,
    pub notes: Vec<(Site, String)>,
    pub source: Option<Rc<str>>,
}

impl Error {
    fn location(&self, site: Site) -> String {
        let source = self
            .source
            .as_ref()
            .map(|s| format!("{s}:"))
            .unwrap_or_default();
        format!("{source}{}:{}: ", site.line, site.column)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(site) = self.primary {
            f.write_str(&self.location(site))?;
        }
        let binding = match &self.place {
            Some(place) => format!("binding `{place}`"),
            None => "temporary value".into(),
        };
        let place = match &self.place {
            Some(place) => format!("`{place}`"),
            None => "a temporary value".into(),
        };
        match &self.kind {
            Kind::Moved(true) => write!(f, "use of moved {binding}")?,
            Kind::Moved(false) => write!(f, "use of possibly moved {binding}")?,
            Kind::Unavailable => write!(f, "use of moved or uninitialized {binding}")?,
            Kind::MovedInLoop => write!(
                f,
                "{place} is moved in a loop, so the next iteration would use a moved value; \
                 reinitialize it before continuing"
            )?,
            Kind::Conflict {
                action,
                borrowed,
                exclusive,
            } => write!(
                f,
                "conflicting borrow: cannot {action} {place} while {} is {}borrowed",
                match borrowed {
                    Some(borrowed) => format!("`{borrowed}`"),
                    None => "it".into(),
                },
                if *exclusive { "exclusively " } else { "" }
            )?,
            Kind::LiveAcrossYield => {
                write!(f, "a borrow of {place} cannot remain live across yield")?
            }
        }
        for (site, note) in &self.notes {
            write!(f, "\n  {}note: {note}", self.location(*site))?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

// Boxed so that the common success path does not carry a diagnostic-sized value.
type Result<T> = std::result::Result<T, Box<Error>>;

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Owned,
    /// Never initialized, or released by scope cleanup.
    Unset,
    /// `definite` is false when only some paths reaching here moved the value.
    Moved {
        at: Option<Site>,
        definite: bool,
    },
}
type State = Vec<Slot>;

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

struct Checker<'a> {
    locals: &'a [Ty],
    origins: &'a Origins,
}

pub fn check(ops: &[Op], locals: &[Ty], parameters: usize, origins: &Origins) -> Result<()> {
    let checker = Checker { locals, origins };
    let mut state = (0..locals.len())
        .map(|i| {
            if i < parameters {
                Slot::Owned
            } else {
                Slot::Unset
            }
        })
        .collect();
    checker.sequence(ops, &mut state, &mut Vec::new(), None)?;
    checker.loans(ops)
}

/// A value is owned after a join only if every continuing path owns it. One
/// moving path is kept as the move to report.
fn merge(states: &[State], into: &mut State) {
    let Some(first) = states.first() else {
        return;
    };
    *into = first.clone();
    for (i, slot) in into.iter_mut().enumerate() {
        let owned = states.iter().filter(|s| s[i] == Slot::Owned).count();
        if owned == states.len() {
            continue;
        }
        *slot = states
            .iter()
            .find_map(|s| match s[i] {
                Slot::Moved { at, .. } => Some(Slot::Moved {
                    at,
                    definite: owned == 0,
                }),
                _ => None,
            })
            .unwrap_or(Slot::Unset);
    }
}

#[derive(Default)]
struct Node {
    op: Option<Op>,
    successors: Vec<usize>,
    site: Option<Site>,
}

/// Flatten structured operations to an access CFG. Backedges and early exits are
/// explicit, so liveness reaches a fixed point rather than guessing at loop uses.
fn graph(
    ops: &[Op],
    inherited: Option<Site>,
    next: usize,
    targets: Option<(usize, usize)>,
    nodes: &mut Vec<Node>,
) -> usize {
    // Nodes are built back to front, but a marker applies to what follows it.
    let mut current = inherited;
    let sites: Vec<_> = ops
        .iter()
        .map(|op| {
            if let Op::Site(site) = op {
                current = Some(*site);
            }
            current
        })
        .collect();
    let mut next = next;
    for (op, site) in ops.iter().zip(sites).rev() {
        match op {
            Op::Site(_) => continue,
            Op::Try { cleanup, .. } => {
                let failure = graph(cleanup, site, 0, None, nodes);
                nodes.push(Node {
                    op: None,
                    successors: vec![next, failure],
                    site,
                });
            }
            Op::Match(arms) => {
                let successors = arms
                    .iter()
                    .map(|a| graph(&a.body, site, next, targets, nodes))
                    .collect();
                nodes.push(Node {
                    op: None,
                    successors,
                    site,
                });
            }
            Op::Loop { condition, body } => {
                let header = nodes.len();
                nodes.push(Node::default());
                let body = graph(body, site, header, Some((header, next)), nodes);
                let branch = nodes.len();
                nodes.push(Node {
                    op: None,
                    successors: vec![body, next],
                    site,
                });
                let test = graph(condition, site, branch, targets, nodes);
                nodes[header].successors.push(test);
                next = header;
                continue;
            }
            Op::Break => nodes.push(Node {
                op: None,
                successors: vec![targets.expect("checked loop").1],
                site,
            }),
            Op::Continue => nodes.push(Node {
                op: None,
                successors: vec![targets.expect("checked loop").0],
                site,
            }),
            Op::Return | Op::TailCall(_) | Op::TailCallIndirect(_) | Op::Unreachable => {
                nodes.push(Node::default())
            }
            _ => nodes.push(Node {
                op: Some(op.clone()),
                successors: vec![next],
                site,
            }),
        }
        next = nodes.len() - 1;
    }
    next
}

impl Checker<'_> {
    fn error(&self, kind: Kind, place: Option<String>, primary: Option<Site>) -> Box<Error> {
        Box::new(Error {
            kind,
            place,
            primary,
            notes: Vec::new(),
            source: self.origins.source.clone(),
        })
    }

    /// Render a binding and, for a precise loan, its field path. An imprecise
    /// origin covers the whole binding, so no field path is invented for it.
    fn place(&self, root: u8, fields: &[usize], precise: bool) -> Option<String> {
        let mut path = self
            .origins
            .names
            .get(root as usize)?
            .as_deref()?
            .to_owned();
        if !precise {
            return Some(path);
        }
        let mut ty = self.locals.get(root as usize).cloned();
        for &index in fields {
            while let Some(Ty::Ref(inner, _) | Ty::Box(inner)) = &ty {
                ty = Some((**inner).clone());
            }
            ty = match &ty {
                Some(Ty::Class(class)) => {
                    let Some((name, field)) = class.get().fields.get(index).cloned() else {
                        break;
                    };
                    path.push('.');
                    path.push_str(&name);
                    Some(field)
                }
                Some(Ty::Enum(t)) if t.tuple() => {
                    let Some(field) = t.get().variants[0].fields.get(index).cloned() else {
                        break;
                    };
                    path.push_str(&format!("[{index}]"));
                    Some(field)
                }
                // A matched payload has no source path of its own.
                _ => break,
            };
        }
        Some(path)
    }

    fn unavailable(&self, slot: u8, state: &State, site: Option<Site>) -> Box<Error> {
        let place = self.place(slot, &[], false);
        match state.get(slot as usize) {
            Some(Slot::Moved { at, definite }) => {
                let mut error = self.error(Kind::Moved(*definite), place.clone(), site);
                if let (Some(at), Some(place)) = (at, place) {
                    error.notes.push((
                        *at,
                        if *definite {
                            format!("`{place}` is moved here")
                        } else {
                            format!("`{place}` is moved here on some paths")
                        },
                    ));
                }
                error
            }
            _ => self.error(Kind::Unavailable, place, site),
        }
    }

    fn backedge(&self, state: &State, entry: &State, site: Option<Site>) -> Result<()> {
        for (i, slot) in state.iter().enumerate() {
            if entry[i] == Slot::Owned && *slot != Slot::Owned {
                let moved = match slot {
                    Slot::Moved { at, .. } => *at,
                    _ => None,
                };
                return Err(self.error(
                    Kind::MovedInLoop,
                    self.place(i as u8, &[], false),
                    moved.or(site),
                ));
            }
        }
        Ok(())
    }

    fn sequence(
        &self,
        ops: &[Op],
        state: &mut State,
        loops: &mut Vec<Loop>,
        mut site: Option<Site>,
    ) -> Result<bool> {
        for op in ops {
            match op {
                Op::Site(at) => site = Some(*at),
                Op::Thread(crate::threading::ThreadOp::Start(_, slot)) => {
                    state[*slot as usize] = Slot::Owned
                }
                Op::Thread(crate::threading::ThreadOp::Finish(_, slot)) => {
                    state[*slot as usize] = Slot::Unset
                }
                Op::Try { cleanup, .. } => {
                    self.sequence(cleanup, &mut state.clone(), &mut Vec::new(), site)?;
                }
                Op::MoveLocal(i) | Op::Next(i) => {
                    if state.get(*i as usize) != Some(&Slot::Owned) {
                        return Err(self.unavailable(*i, state, site));
                    }
                    if matches!(op, Op::MoveLocal(_)) {
                        state[*i as usize] = Slot::Moved {
                            at: site,
                            definite: true,
                        };
                    }
                }
                Op::LoadLocal(i) | Op::BorrowLocal(i, _) | Op::Access(i, _, _) => {
                    if state.get(*i as usize) != Some(&Slot::Owned) {
                        return Err(self.unavailable(*i, state, site));
                    }
                }
                Op::StoreLocal(i) => state[*i as usize] = Slot::Owned,
                Op::DropLocal(i) => state[*i as usize] = Slot::Unset,
                Op::Match(arms) => {
                    let initial = state.clone();
                    let mut continuing = Vec::new();
                    for arm in arms.iter() {
                        let mut next = initial.clone();
                        if self.sequence(&arm.body, &mut next, loops, site)? {
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
                    self.sequence(condition, state, &mut Vec::new(), site)?;
                    let zero = state.clone();
                    loops.push(Loop {
                        entry,
                        breaks: vec![zero.clone()],
                    });
                    if self.sequence(body, state, loops, site)? {
                        self.backedge(state, &loops.last().unwrap().entry, site)?;
                    }
                    let exits = loops.pop().unwrap().breaks;
                    merge(&exits, state);
                }
                Op::Continue => {
                    let target = loops.last().expect("continue inside a loop");
                    self.backedge(state, &target.entry, site)?;
                    return Ok(false);
                }
                Op::Break => {
                    loops
                        .last_mut()
                        .expect("break inside a loop")
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

    fn loans(&self, ops: &[Op]) -> Result<()> {
        fn use_loan(id: usize, loans: &HashMap<usize, Loan>, live: &mut BTreeSet<usize>) {
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
        let entry = graph(ops, None, 0, None, &mut nodes);
        let mut reachable = BTreeSet::new();
        let mut todo = vec![entry];
        while let Some(n) = todo.pop() {
            if reachable.insert(n) {
                todo.extend(&nodes[n].successors);
            }
        }
        let mut loans = HashMap::new();
        let mut created = HashMap::new();
        for node in &nodes {
            if let Some(Op::Loan(l)) = &node.op {
                loans.insert(l.id, l.clone());
                created.insert(l.id, node.site);
            }
        }
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
        // The nearest operation that keeps `loan` live after node `from`, and
        // whether reaching it re-enters a loop that contains `from`.
        let later_use = |from: usize, loan: usize| -> Option<(Site, bool)> {
            let mut seen = BTreeSet::new();
            let mut todo: std::collections::VecDeque<_> =
                nodes[from].successors.iter().map(|&s| (s, false)).collect();
            while let Some((n, repeated)) = todo.pop_front() {
                if !live[n].contains(&loan) || !seen.insert(n) {
                    continue;
                }
                let keeps = match &nodes[n].op {
                    Some(Op::UseLoan(id)) | Some(Op::Access(_, _, Some(id))) => {
                        let mut used = BTreeSet::new();
                        use_loan(*id, &loans, &mut used);
                        used.contains(&loan)
                    }
                    Some(Op::Loan(l)) => l.parent == Some(loan),
                    _ => false,
                };
                if keeps {
                    return nodes[n].site.map(|site| (site, repeated));
                }
                // Nodes are numbered back to front, so only a loop header has
                // a higher-numbered successor: its test, the last node built
                // for the loop. `from` is inside that loop when it lies between.
                todo.extend(
                    nodes[n]
                        .successors
                        .iter()
                        .map(|&s| (s, repeated || (n < from && from <= s))),
                );
            }
            None
        };
        // Report the conflict that comes first in the source.
        let mut first: Option<Box<Error>> = None;
        for &i in &reachable {
            let mut after = BTreeSet::new();
            for &s in &nodes[i].successors {
                after.extend(&live[s]);
            }
            let site = nodes[i].site;
            let access = match &nodes[i].op {
                Some(Op::Loan(l)) => Some((
                    l.root,
                    l.mutable,
                    l.parent,
                    Some(l.id),
                    if l.mutable {
                        "modify or exclusively borrow"
                    } else {
                        "read or borrow"
                    },
                )),
                Some(Op::Access(root, write, via)) => Some((
                    *root,
                    *write,
                    *via,
                    None,
                    if *write { "modify" } else { "read" },
                )),
                Some(Op::MoveLocal(root)) => Some((*root, true, None, None, "move")),
                Some(Op::StoreLocal(root)) => Some((*root, true, None, None, "assign to")),
                Some(Op::DropLocal(root)) => Some((*root, true, None, None, "drop")),
                Some(Op::Next(root)) => Some((*root, true, None, None, "advance")),
                _ => None,
            };
            let mut found = None;
            if let Some(Op::Yield(_)) = &nodes[i].op {
                if let Some(&id) = after.first() {
                    let loan = &loans[&id];
                    let place = self.place(loan.root, &loan.fields, loan.precise);
                    found = Some((self.error(Kind::LiveAcrossYield, place, site), id));
                }
            }
            if let Some((root, write, via, defining, action)) = access {
                for id in &after {
                    let loan = &loans[id];
                    if Some(*id) == defining || via.is_some_and(|v| ancestor(*id, v)) {
                        continue;
                    }
                    let accessed = defining.or(via).map(|id| &loans[&id]);
                    let fields = accessed.map(|l| l.fields.as_slice()).unwrap_or(&[]);
                    let overlaps =
                        loan.fields.starts_with(fields) || fields.starts_with(&loan.fields);
                    if loan.root == root && overlaps && (write || loan.mutable) {
                        let place = self.place(root, fields, accessed.is_none_or(|l| l.precise));
                        let borrowed = self.place(loan.root, &loan.fields, loan.precise);
                        let kind = Kind::Conflict {
                            action,
                            borrowed: borrowed.clone().filter(|b| Some(b) != place.as_ref()),
                            exclusive: loan.mutable,
                        };
                        found = Some((self.error(kind, place, site), *id));
                        break;
                    }
                }
            }
            let Some((mut error, id)) = found else {
                continue;
            };
            let position = |e: &Error| e.primary.map(|s| (s.line, s.column));
            if first
                .as_ref()
                .is_some_and(|f| position(f).is_some() && position(f) <= position(&error))
            {
                continue;
            }
            let loan = &loans[&id];
            if let Some(at) = created[&id] {
                error.notes.push((
                    at,
                    format!(
                        "the {} borrow{} starts here",
                        if loan.mutable { "exclusive" } else { "shared" },
                        self.place(loan.root, &loan.fields, loan.precise)
                            .map(|place| format!(" of `{place}`"))
                            .unwrap_or_default()
                    ),
                ));
            }
            // A later use inside the conflicting expression itself, such as
            // the call both arguments feed, adds nothing to the primary line.
            if let Some((at, repeated)) =
                later_use(i, id).filter(|(at, _)| Some(*at) != error.primary)
            {
                error.notes.push((
                    at,
                    if at.scope_end {
                        "the borrow is held until the end of this `with` block".into()
                    } else if repeated {
                        "the borrow is used again here on the next loop iteration".into()
                    } else {
                        "the borrow is used again here".into()
                    },
                ));
            }
            first = Some(error);
        }
        first.map_or(Ok(()), Err)
    }
}
