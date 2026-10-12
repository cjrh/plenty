//! Definite ownership and last-use loans over an explicit access control-flow graph.
use crate::op::{Op, Ty};
use crate::threading::{endpoints, Task, ThreadOp};
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

/// Per-function facts used only to render ownership diagnostics and trap
/// reports. They never affect liveness or aliasing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Origins {
    pub source: Option<Rc<str>>,
    /// `source` relative to the source root, for runtime trap reports.
    /// Generated code must not depend on where the tree is checked out.
    pub trap_file: Option<Rc<str>>,
    /// Source name of each local slot, parameters first. Slots are never
    /// reused, so a name stays valid after its scope ends. Compiler
    /// temporaries have none.
    pub names: Vec<Option<Rc<str>>>,
    /// Present when the function is a generic instance.
    pub required: Option<Rc<Instantiation>>,
}

/// The use that first required a generic instance. A body is checked once per
/// instance, so an error in it belongs to that use: the body may be imported
/// code the reader never wrote. The body position becomes a note.
#[derive(Clone, Debug, PartialEq)]
pub struct Instantiation {
    /// `source:line:column` of the use in code that is not itself an instance.
    pub at: Rc<str>,
    /// The instance that use asked for.
    pub requested: Rc<str>,
    /// The instance whose body is checked. It differs from `requested` when a
    /// generic body requires further instances; the links between are omitted
    /// to keep the diagnostic at two locations.
    pub instance: Rc<str>,
}

impl Instantiation {
    /// Render a body diagnostic at the requiring use. `body` is the body
    /// position including its `: ` separator, or empty when there is none.
    pub fn report(&self, body: &str, message: &str) -> String {
        format!(
            "{}: cannot instantiate `{}`: its definition is rejected\n  \
             {body}note: in instantiation `{}`: {message}",
            self.at,
            short(&self.requested),
            short(&self.instance)
        )
    }
}

/// Expanding recursion nests type arguments up to the implementation limits.
/// The start of such a name identifies it; the rest would bury the message.
fn short(name: &str) -> std::borrow::Cow<'_, str> {
    const LIMIT: usize = 96;
    match name.char_indices().nth(LIMIT) {
        Some((end, _)) => format!("{}...", &name[..end]).into(),
        None => name.into(),
    }
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
        through: Option<String>,
        /// The borrowed place when it differs from the accessed one.
        borrowed: Option<String>,
        exclusive: bool,
    },
    LiveAcrossYield,
    /// A join that a worker waiting to send could keep waiting forever.
    BlockedJoin {
        /// Message type of the worker's sender and the surviving receiver.
        message: String,
        /// The join belongs to a `with` block exit, not an explicit `join()`.
        automatic: bool,
    },
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
    pub required: Option<Rc<Instantiation>>,
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
        let location = self
            .primary
            .map(|site| self.location(site))
            .unwrap_or_default();
        let binding = match &self.place {
            Some(place) => format!("binding `{place}`"),
            None => "temporary value".into(),
        };
        let place = match &self.place {
            Some(place) => format!("`{place}`"),
            None => "a temporary value".into(),
        };
        let message = match &self.kind {
            Kind::Moved(true) => format!("use of moved {binding}"),
            Kind::Moved(false) => format!("use of possibly moved {binding}"),
            Kind::Unavailable => format!("use of moved or uninitialized {binding}"),
            Kind::MovedInLoop => format!(
                "{place} is moved in a loop, so the next iteration would use a moved value; \
                 reinitialize it before continuing"
            ),
            Kind::Conflict {
                action,
                through,
                borrowed,
                exclusive,
            } => format!(
                "conflicting borrow: cannot {action} {place}{} while {} is {}borrowed",
                through
                    .as_ref()
                    .map(|name| format!(" (through `{name}`)"))
                    .unwrap_or_default(),
                match borrowed {
                    Some(borrowed) => format!("`{borrowed}`"),
                    None => "it".into(),
                },
                if *exclusive { "exclusively " } else { "" }
            ),
            Kind::LiveAcrossYield => {
                format!("a borrow of {place} cannot remain live across yield")
            }
            Kind::BlockedJoin { message, automatic } => format!(
                "{} a task whose worker holds `Sender[{message}]` while {} still holds \
                 `Receiver[{message}]`: a full queue could block the worker's send, and \
                 this join, forever",
                if *automatic {
                    "leaving this `with` block joins"
                } else {
                    "this join waits for"
                },
                match &self.place {
                    Some(place) => format!("`{place}`"),
                    None => "an unnamed value".into(),
                }
            ),
        };
        match &self.required {
            Some(required) => f.write_str(&required.report(&location, &message))?,
            None => write!(f, "{location}{message}")?,
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
    /// Diagnostic-only source reference used for this reborrow.
    pub through: Option<u8>,
    /// Diagnostic-only named reference holding this loan, if any.
    pub binding: Option<u8>,
    /// Borrowing an environment keeps all references stored inside it live.
    pub dependencies: Vec<usize>,
}
#[derive(Default)]
struct Loop {
    entry: State,
    breaks: Vec<State>,
}

struct Checker<'a> {
    body: &'a [Op],
    locals: &'a [Ty],
    parameters: usize,
    origins: &'a Origins,
}

pub fn check(ops: &[Op], locals: &[Ty], parameters: usize, origins: &Origins) -> Result<()> {
    let checker = Checker {
        body: ops,
        locals,
        parameters,
        origins,
    };
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

/// The statement whose inline cleanup `rest` continues, when it leaves the
/// block early. Cleanup at the end of a block is followed by other source or
/// by nothing.
fn early_exit(rest: &[Op], written: Option<Site>) -> Option<&'static str> {
    let moved_on = rest
        .iter()
        .any(|op| matches!(op, Op::Site(site) if !site.scope_end && Some(*site) != written));
    match rest.last() {
        _ if moved_on => None,
        Some(Op::Return) => Some("`return`"),
        Some(Op::Break) => Some("`break`"),
        Some(Op::Continue) => Some("`continue`"),
        _ => None,
    }
}

/// Name the early exit that reaches a rejected automatic join. The join is
/// reported at its `with` statement, which need not be near the exit.
fn leaving(mut error: Box<Error>, at: Option<Site>, exit: Option<&str>) -> Box<Error> {
    let automatic = matches!(
        error.kind,
        Kind::BlockedJoin {
            automatic: true,
            ..
        }
    );
    if let (true, Some(at), Some(exit)) = (automatic, at, exit) {
        error
            .notes
            .push((at, format!("{exit} leaves the block here")));
    }
    error
}

/// The marker in force once `ops` have run, which a body following them
/// inherits. Markers inside nested bodies do not reach their parent sequence.
pub(crate) fn site_after(ops: &[Op], inherited: Option<Site>) -> Option<Site> {
    ops.iter()
        .rev()
        .find_map(|op| match op {
            Op::Site(site) => Some(*site),
            _ => None,
        })
        .or(inherited)
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
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                // (marker entering the condition, marker entering the body)
                let mut entering = site;
                let markers: Vec<_> = elifs
                    .iter()
                    .map(|arm| {
                        let before = entering;
                        entering = site_after(&arm.condition, before);
                        (before, entering)
                    })
                    .collect();
                let mut fall = graph(otherwise, entering, next, targets, nodes);
                for (arm, (before, after)) in elifs.iter().zip(markers).rev() {
                    let body = graph(&arm.body, after, next, targets, nodes);
                    nodes.push(Node {
                        op: None,
                        successors: vec![body, fall],
                        site: after,
                    });
                    fall = graph(&arm.condition, before, nodes.len() - 1, targets, nodes);
                }
                let then = graph(then, site, next, targets, nodes);
                nodes.push(Node {
                    op: None,
                    successors: vec![then, fall],
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
            required: self.origins.required.clone(),
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

    /// Where a slot first receives its value. Computed only to render a note.
    fn bound(&self, slot: usize) -> Option<Site> {
        let mut nodes = vec![Node::default()];
        graph(self.body, None, 0, None, &mut nodes);
        nodes
            .iter()
            .filter(|node| match &node.op {
                Some(Op::StoreLocal(i) | Op::Thread(ThreadOp::Start(_, i))) => *i as usize == slot,
                _ => false,
            })
            .filter_map(|node| node.site)
            .min_by_key(|site| (site.line, site.column))
    }

    /// A worker waiting for queue space is released only when the last
    /// receiver drops. Reject a join reached while this function still keeps
    /// one alive for a message type the worker can send. Endpoints are matched
    /// by type, so this neither identifies queues nor proves a join completes.
    fn join(&self, task: &Task, state: &State, site: Option<Site>, automatic: bool) -> Result<()> {
        let sends = endpoints(task.inputs.iter().cloned(), true, true);
        if sends.is_empty() {
            return Ok(());
        }
        let mut error: Option<Box<Error>> = None;
        for (slot, owned) in state.iter().enumerate() {
            let partly = match owned {
                Slot::Owned => false,
                Slot::Moved {
                    definite: false, ..
                } => true,
                _ => continue,
            };
            // A borrowed parameter's owner is alive for the whole call.
            let parameter = slot < self.parameters;
            let Some(message) = endpoints([self.locals[slot].clone()], false, parameter)
                .into_iter()
                .find(|message| sends.contains(message))
            else {
                continue;
            };
            let place = self.place(slot as u8, &[], false);
            let name = match &place {
                Some(place) => format!("`{place}`"),
                None => "this value".into(),
            };
            let at = self.bound(slot);
            let first = error.is_none();
            let error = error.get_or_insert_with(|| {
                self.error(
                    Kind::BlockedJoin {
                        message: message.to_string(),
                        automatic,
                    },
                    place.clone(),
                    site,
                )
            });
            let holds = format!(
                "{name} {}holds `Receiver[{message}]`{}",
                if first { "" } else { "also " },
                if partly { " on some paths" } else { "" }
            );
            let advice = if matches!(self.locals[slot], Ty::Ref(..)) {
                "its owner outlives this call, so take the receiver by value".into()
            } else if !automatic {
                "drop it, or end its `with` block, before joining".to_owned()
            } else {
                match &place {
                    Some(place) => format!(
                        "drop it before leaving the block, or guard it directly after the \
                         task: `with spawn(...)?, {place} as inbox:`"
                    ),
                    None => "end its scope before leaving the block".into(),
                }
            };
            if let Some(at) = at.or(site) {
                error.notes.push((at, format!("{holds}; {advice}")));
            }
        }
        let Some(mut error) = error else {
            return Ok(());
        };
        // An automatic join is reported at the `with` statement that starts the task.
        if !automatic {
            let started = self
                .locals
                .iter()
                .position(|ty| matches!(ty, Ty::Task(other) if **other == *task))
                .and_then(|slot| self.bound(slot));
            if let Some(at) = started {
                error.notes.push((at, "the task starts here".into()));
            }
        }
        Err(error)
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
        // The last position the programmer wrote, as opposed to a scope end.
        let mut written = site.filter(|site| !site.scope_end);
        for (index, op) in ops.iter().enumerate() {
            match op {
                Op::Site(at) => {
                    site = Some(*at);
                    if !at.scope_end {
                        written = site;
                    }
                }
                Op::Thread(ThreadOp::Start(_, slot)) => state[*slot as usize] = Slot::Owned,
                Op::Thread(ThreadOp::Join(task)) => self.join(task, state, site, false)?,
                Op::Thread(ThreadOp::Finish(task, slot)) => {
                    // A task joined explicitly on every path is not waited for again.
                    if !matches!(state[*slot as usize], Slot::Moved { definite: true, .. }) {
                        self.join(task, state, site, true).map_err(|error| {
                            leaving(error, written, early_exit(&ops[index + 1..], written))
                        })?;
                    }
                    state[*slot as usize] = Slot::Unset
                }
                Op::Try { cleanup, .. } => {
                    self.sequence(cleanup, &mut state.clone(), &mut Vec::new(), site)
                        .map_err(|error| leaving(error, site, Some("`?`")))?;
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
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    let mut continuing = Vec::new();
                    let mut next = state.clone();
                    if self.sequence(then, &mut next, loops, site)? {
                        continuing.push(next);
                    }
                    // `state` follows the path on which every condition so far
                    // was false, the only one that reaches the next condition.
                    let mut entering = site;
                    for arm in elifs.iter() {
                        if !self.sequence(&arm.condition, state, loops, entering)? {
                            break;
                        }
                        entering = site_after(&arm.condition, entering);
                        let mut next = state.clone();
                        if self.sequence(&arm.body, &mut next, loops, entering)? {
                            continuing.push(next);
                        }
                    }
                    if self.sequence(otherwise, state, loops, entering)? {
                        continuing.push(state.clone());
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
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    has_loans(then)
                        || has_loans(otherwise)
                        || elifs
                            .iter()
                            .any(|arm| has_loans(&arm.condition) || has_loans(&arm.body))
                }
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
                            through: accessed
                                .and_then(|l| {
                                    if defining.is_some() {
                                        l.through
                                    } else {
                                        l.binding.or(l.through)
                                    }
                                })
                                .filter(|slot| *slot != root)
                                .and_then(|slot| self.place(slot, &[], false)),
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
