//! Definite ownership and last-use loans over an explicit access control-flow graph.
use crate::executor::ExecutorOp;
use crate::mutex::MutexOp;
use crate::op::{Op, Ty};
use crate::threading::{endpoints, owns_executor, Task, ThreadOp};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

mod futures;

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
    pub source: Option<Arc<str>>,
    /// `source` relative to the source root, for runtime trap reports.
    /// Generated code must not depend on where the tree is checked out.
    pub trap_file: Option<Arc<str>>,
    /// Provenance supplied by the embedded-module loader, never a path prefix.
    pub standard_library: bool,
    /// Source name of each local slot, parameters first. Slots are never
    /// reused, so a name stays valid after its scope ends. Compiler
    /// temporaries have none.
    pub names: Vec<Option<Arc<str>>>,
    /// Present when the function is a generic instance.
    pub required: Option<Arc<Instantiation>>,
}

/// The use that first required a generic instance. A body is checked once per
/// instance, so an error in it belongs to that use: the body may be imported
/// code the reader never wrote. The body position becomes a note.
#[derive(Clone, Debug, PartialEq)]
pub struct Instantiation {
    /// `source:line:column` of the use in code that is not itself an instance.
    pub at: Arc<str>,
    /// The instance that use asked for.
    pub requested: Arc<str>,
    /// The instance whose body is checked. It differs from `requested` when a
    /// generic body requires further instances; the links between are omitted
    /// to keep the diagnostic at two locations.
    pub instance: Arc<str>,
}

impl Instantiation {
    /// Render a body diagnostic at the requiring use. `body` is the body
    /// position including its `: ` separator, or empty when there is none.
    pub fn report(&self, body: &str, message: &str) -> String {
        // An f-string's generated function is not code the reader wrote; its
        // body positions are the fields themselves.
        if self.requested.starts_with("__plenty_fstring_") && self.requested == self.instance {
            if body.is_empty() {
                return format!("{}: {message}", self.at);
            }
            return format!("{body}{message}");
        }
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
    /// A future result waiting on a job that can send to a live receiver.
    BlockedFuture {
        message: String,
    },
    /// A join that a worker waiting on a channel could keep waiting forever.
    BlockedJoin {
        /// Message type shared by the worker and the surviving endpoint.
        message: String,
        worker_sends: bool,
        /// The join belongs to a `with` block exit, not an explicit `join()`.
        automatic: bool,
    },
    /// A join that a worker waiting for a mutex this function holds could
    /// keep waiting forever.
    BlockedLock {
        /// Guarded type of the held mutex and of one the worker can reach.
        element: String,
        /// The join belongs to a `with` block exit, not an explicit `join()`.
        automatic: bool,
    },
    /// A pool shutdown that a job waiting to send could keep waiting forever.
    BlockedShutdown {
        /// Message type of the job's sender and the surviving receiver.
        message: String,
        /// The shutdown belongs to the end of the pool's scope, not to
        /// `shutdown()`, `drop()`, or a reassignment.
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
    pub source: Option<Arc<str>>,
    pub required: Option<Arc<Instantiation>>,
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
            Kind::BlockedFuture { message } => format!(
                "this result() waits for a job that holds `Sender[{message}]` while \
                 {place} still holds `Receiver[{message}]`: a full queue could block \
                 the job's send, and this result wait, forever"
            ),
            Kind::BlockedJoin {
                message,
                worker_sends,
                automatic,
            } => format!(
                "{} a task whose worker holds `{}[{message}]` while {} still holds \
                 `{}[{message}]`: {} could block the worker's {}, and \
                 this join, forever",
                if *automatic {
                    "leaving this `with` block joins"
                } else {
                    "this join waits for"
                },
                if *worker_sends { "Sender" } else { "Receiver" },
                match &self.place {
                    Some(place) => format!("`{place}`"),
                    None => "an unnamed value".into(),
                },
                if *worker_sends { "Receiver" } else { "Sender" },
                if *worker_sends {
                    "a full queue"
                } else {
                    "an empty queue"
                },
                if *worker_sends { "send" } else { "receive" },
            ),
            Kind::BlockedLock { element, automatic } => format!(
                "{} a task whose worker can lock a `Mutex[{element}]` while this function \
                 holds a lock on a `Mutex[{element}]`: the worker could wait for that lock, \
                 and this join, forever",
                if *automatic {
                    "leaving this `with` block joins"
                } else {
                    "this join waits for"
                },
            ),
            Kind::BlockedShutdown { message, automatic } => format!(
                "{} after a job that holds `Sender[{message}]` was submitted, while {} still \
                 holds `Receiver[{message}]`: a full queue could block the job's send, and the \
                 shutdown, forever",
                if *automatic {
                    "leaving its scope shuts down this pool"
                } else {
                    "this shuts down a pool"
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
/// Live loan ids, sorted ascending: the order in which conflicts are tried.
/// Liveness revisits every node until nothing changes, so a set is rebuilt in
/// a reused buffer and copied only when it differs; a `BTreeSet` per visit
/// made allocation the checker's main cost.
#[derive(Clone, Default, PartialEq)]
struct Live(Vec<usize>);

impl Live {
    fn insert(&mut self, id: usize) -> bool {
        match self.0.binary_search(&id) {
            Ok(_) => false,
            Err(at) => {
                self.0.insert(at, id);
                true
            }
        }
    }

    fn remove(&mut self, id: &usize) {
        if let Ok(at) = self.0.binary_search(id) {
            self.0.remove(at);
        }
    }

    fn contains(&self, id: &usize) -> bool {
        self.0.binary_search(id).is_ok()
    }

    fn union(&mut self, other: &Live, scratch: &mut Vec<usize>) {
        if other.0.is_empty() {
            return;
        }
        if self.0.is_empty() {
            self.0.extend_from_slice(&other.0);
            return;
        }
        scratch.clear();
        let (mut a, mut b) = (self.0.iter().peekable(), other.0.iter().peekable());
        while let (Some(&&x), Some(&&y)) = (a.peek(), b.peek()) {
            scratch.push(x.min(y));
            if x <= y {
                a.next();
            }
            if y <= x {
                b.next();
            }
        }
        scratch.extend(a.chain(b));
        std::mem::swap(&mut self.0, scratch);
    }
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
    /// Message types that a job submitted by operations already checked can
    /// send, each with its first submission. Jobs are not matched to pools,
    /// and one is never assumed to have finished.
    submitted: RefCell<Vec<(Ty, Option<Site>)>>,
    futures: futures::Futures,
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub fn check(ops: &[Op], locals: &[Ty], parameters: usize, origins: &Origins) -> Result<()> {
    let checker = Checker {
        body: ops,
        locals,
        parameters,
        origins,
        submitted: RefCell::default(),
        futures: crate::profile_block!(
            "ownership::prepare_futures",
            futures::Futures::new(ops, locals.len())
        ),
    };
    let mut state = crate::profile_block!(
        "ownership::initialize_state",
        (0..locals.len())
            .map(|i| {
                if i < parameters {
                    Slot::Owned
                } else {
                    Slot::Unset
                }
            })
            .collect()
    );
    crate::profile_block!("ownership::check_moves", {
        if checker.sequence(ops, &mut state, &mut Vec::new(), None)? {
            checker.release(&mut state, None)?;
        }
    });
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

/// Name the early exit that reaches a rejected automatic join or shutdown.
/// Either is reported where its task or pool starts, which need not be near
/// the exit.
fn leaving(mut error: Box<Error>, at: Option<Site>, exit: Option<&str>) -> Box<Error> {
    let (Some(at), Some(exit)) = (at, exit) else {
        return error;
    };
    let note = match error.kind {
        Kind::BlockedJoin {
            automatic: true, ..
        }
        | Kind::BlockedLock {
            automatic: true, ..
        } => format!("{exit} leaves the block here"),
        Kind::BlockedShutdown {
            automatic: true, ..
        } => format!("{exit} leaves the pool's scope here"),
        _ => return error,
    };
    error.notes.push((at, note));
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
struct Node<'a> {
    /// Borrowed from the body: cloning every operation dominated the graph's cost.
    op: Option<&'a Op>,
    successors: Vec<usize>,
    site: Option<Site>,
}

/// Flatten structured operations to an access CFG. Backedges and early exits are
/// explicit, so liveness reaches a fixed point rather than guessing at loop uses.
fn graph<'a>(
    ops: &'a [Op],
    inherited: Option<Site>,
    next: usize,
    targets: Option<(usize, usize)>,
    nodes: &mut Vec<Node<'a>>,
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
            Op::Site(_) | Op::CallerSite(_) => continue,
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
                op: Some(op),
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

    /// The error for a wait on a thread that can send one of `sends` while a
    /// live binding can hold the matching receiver, with a note per holder.
    /// Nothing but the last receiver's drop releases a thread waiting for queue
    /// space. Endpoints are matched by type, so this neither identifies queues
    /// nor proves a wait completes. `advice` says how to release an owned holder.
    fn blocked(
        &self,
        sends: &[Ty],
        state: &State,
        site: Option<Site>,
        kind: impl Fn(String) -> Kind,
        advice: impl Fn(Option<&str>) -> String,
    ) -> Option<Box<Error>> {
        self.blocked_endpoints(sends, true, state, site, kind, advice)
    }

    /// Match the worker's endpoints against live owners of the opposite side.
    fn blocked_endpoints(
        &self,
        messages: &[Ty],
        worker_sends: bool,
        state: &State,
        site: Option<Site>,
        kind: impl Fn(String) -> Kind,
        advice: impl Fn(Option<&str>) -> String,
    ) -> Option<Box<Error>> {
        if messages.is_empty() {
            return None;
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
            let Some(message) = endpoints([self.locals[slot].clone()], !worker_sends, parameter)
                .into_iter()
                .find(|message| messages.contains(message))
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
            let error = error
                .get_or_insert_with(|| self.error(kind(message.to_string()), place.clone(), site));
            let endpoint = if worker_sends { "Receiver" } else { "Sender" };
            let holds = format!(
                "{name} {}holds `{endpoint}[{message}]`{}",
                if first { "" } else { "also " },
                if partly { " on some paths" } else { "" }
            );
            let advice = if matches!(self.locals[slot], Ty::Ref(..)) {
                format!(
                    "its owner outlives this call, so take the {} by value",
                    endpoint.to_lowercase()
                )
            } else {
                advice(place.as_deref())
            };
            if let Some(at) = at.or(site) {
                error.notes.push((at, format!("{holds}; {advice}")));
            }
        }
        error
    }

    /// Reject a join while this function retains the opposite channel endpoint.
    fn join(&self, task: &Task, state: &State, site: Option<Site>, automatic: bool) -> Result<()> {
        let blocked = |worker_sends| {
            let messages = endpoints(task.inputs.iter().cloned(), worker_sends, true);
            let kind = |message| Kind::BlockedJoin {
                message,
                worker_sends,
                automatic,
            };
            let guard = if worker_sends { "inbox" } else { "outbox" };
            let advice = |place: Option<&str>| match place {
                _ if !automatic => "drop it, or end its `with` block, before joining".to_owned(),
                Some(place) => format!(
                    "drop it before leaving the block, or guard it directly after the task: \
                 `with spawn(...)?, {place} as {guard}:`"
                ),
                None => "end its scope before leaving the block".into(),
            };
            self.blocked_endpoints(&messages, worker_sends, state, site, kind, advice)
        };
        let Some(mut error) = blocked(true)
            .or_else(|| blocked(false))
            .or_else(|| self.held_lock(task, state, site, automatic))
        else {
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

    /// A lock guard this function still holds on a `Mutex[T]` while the task's
    /// worker can reach a `Mutex[T]`. Mutexes are matched by guarded type, so
    /// this neither identifies mutexes nor proves the worker locks one. A guard
    /// slot belongs to one lock block and is never reused, so its state is
    /// live exactly from its store to the `Unlock(slot)` that resets it.
    fn held_lock(
        &self,
        task: &Task,
        state: &State,
        site: Option<Site>,
        automatic: bool,
    ) -> Option<Box<Error>> {
        let reachable = crate::threading::mutexes(task.inputs.iter().cloned());
        if reachable.is_empty() {
            return None;
        }
        let mut guards = Vec::new();
        crate::threading::walk(self.body, &mut |op| {
            if let Op::Mutex(MutexOp::Lock(slot)) = op {
                guards.push(*slot as usize);
            }
        });
        guards.into_iter().find_map(|slot| {
            if !matches!(
                state[slot],
                Slot::Owned
                    | Slot::Moved {
                        definite: false,
                        ..
                    }
            ) {
                return None;
            }
            let Ty::Ref(mutex, _) = &self.locals[slot] else {
                return None;
            };
            let element = crate::mutex::element(crate::mutex::class(mutex)?);
            if !reachable.contains(&element) {
                return None;
            }
            let mut error = self.error(
                Kind::BlockedLock {
                    element: element.to_string(),
                    automatic,
                },
                None,
                site,
            );
            let advice = if automatic {
                "end the lock's `with` block before this one, or start the task outside it"
            } else {
                "end the lock's `with` block before joining"
            };
            if let Some(at) = self.bound(slot) {
                error
                    .notes
                    .push((at, format!("the lock is acquired here; {advice}")));
            }
            Some(error)
        })
    }

    /// Record the message types a job accepted by a pool can send.
    fn submit(&self, sends: impl IntoIterator<Item = (Ty, Option<Site>)>) {
        let mut submitted = self.submitted.borrow_mut();
        for (message, at) in sends {
            if !submitted.iter().any(|(other, _)| *other == message) {
                submitted.push((message, at));
            }
        }
    }

    fn future_result(&self, before: &[Op], state: &State, site: Option<Site>) -> Result<()> {
        let jobs = self.futures.before(before);
        let sends: Vec<_> = jobs.iter().map(|(message, _)| message.clone()).collect();
        let Some(mut error) = self.blocked(
            &sends,
            state,
            site,
            |message| Kind::BlockedFuture { message },
            |_| "drop it, or end its `with` block, before waiting for the result".into(),
        ) else {
            return Ok(());
        };
        let Kind::BlockedFuture { message } = &error.kind else {
            unreachable!()
        };
        if let Some(at) = jobs
            .iter()
            .find(|(sent, _)| sent.to_string() == *message)
            .and_then(|(_, at)| *at)
        {
            error.notes.push((at, "the job is submitted here".into()));
        }
        Err(error)
    }

    /// Dropping the value in `slot` shuts a pool down. No pool is looked for
    /// until a job that could block the shutdown has been submitted.
    fn closes(&self, slot: usize, state: &State) -> bool {
        !self.submitted.borrow().is_empty()
            && matches!(
                state[slot],
                Slot::Owned
                    | Slot::Moved {
                        definite: false,
                        ..
                    }
            )
            && owns_executor(&self.locals[slot])
    }

    /// Shutdown drains accepted work and joins the workers. Reject one reached
    /// while this function still keeps a receiver alive for a message type that
    /// a job it submitted earlier can send. `scope` is the pool's slot when the
    /// shutdown belongs to the end of the pool's scope.
    fn shutdown(&self, scope: Option<usize>, state: &State, site: Option<Site>) -> Result<()> {
        let submitted = self.submitted.borrow();
        let sends: Vec<_> = submitted
            .iter()
            .map(|(message, _)| message.clone())
            .collect();
        let automatic = scope.is_some();
        let kind = |message| Kind::BlockedShutdown { message, automatic };
        let advice = |place: Option<&str>| match place {
            _ if !automatic => {
                "drop it, or end its `with` block, before shutting the pool down".to_owned()
            }
            Some(place) => format!(
                "drop it before the pool shuts down, or guard it directly after the pool: \
                 `with ThreadPoolExecutor(...)? as pool, {place} as inbox:`"
            ),
            None => "end its scope before the pool shuts down".into(),
        };
        let Some(mut error) = self.blocked(&sends, state, site, kind, advice) else {
            return Ok(());
        };
        // An automatic shutdown is reported where its pool starts.
        if let Some(at) = scope.and_then(|slot| self.bound(slot)) {
            error.primary = Some(at);
        }
        let Kind::BlockedShutdown { message, .. } = &error.kind else {
            unreachable!()
        };
        let job = submitted
            .iter()
            .find(|(sent, _)| sent.to_string() == *message)
            .and_then(|(_, at)| *at);
        if let Some(at) = job {
            error.notes.push((at, "the job is submitted here".into()));
        }
        Err(error)
    }

    /// A function exit releases every local, the last declared first.
    fn release(&self, state: &mut State, site: Option<Site>) -> Result<()> {
        if self.submitted.borrow().is_empty() {
            return Ok(());
        }
        for slot in (0..state.len()).rev() {
            if self.closes(slot, state) {
                self.shutdown(Some(slot), state, site)?;
            }
            state[slot] = Slot::Unset;
        }
        Ok(())
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
        // Sends of a job that `submit(...)?` moves into a pool only on success.
        let mut accepted = Vec::new();
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
                Op::Executor(ExecutorOp::Submit(job, _)) => {
                    let sends = endpoints([job.input.clone()], true, true)
                        .into_iter()
                        .map(|message| (message, site));
                    let result = ops[index + 1..]
                        .iter()
                        .find(|op| !matches!(op, Op::UseLoan(_) | Op::Site(_) | Op::CallerSite(_)));
                    if matches!(result, Some(Op::Try { .. })) {
                        accepted.extend(sends);
                    } else {
                        self.submit(sends);
                    }
                }
                Op::Executor(ExecutorOp::Shutdown) => self.shutdown(None, state, site)?,
                Op::Mutex(MutexOp::Unlock(slot)) => state[*slot as usize] = Slot::Unset,
                Op::Executor(ExecutorOp::Result(_)) => {
                    self.future_result(&ops[..index], state, site)?;
                }
                Op::Try { cleanup, .. } => {
                    let mut failed = state.clone();
                    self.sequence(cleanup, &mut failed, &mut Vec::new(), site)
                        .and_then(|_| self.release(&mut failed, site))
                        .map_err(|error| leaving(error, site, Some("`?`")))?;
                    self.submit(accepted.drain(..));
                }
                Op::MoveLocal(i) | Op::Next(i) => {
                    if state.get(*i as usize) != Some(&Slot::Owned) {
                        return Err(self.unavailable(*i, state, site));
                    }
                    if matches!(op, Op::MoveLocal(_)) {
                        // `drop(pool)` shuts the pool down where it is written.
                        if matches!(ops.get(index + 1), Some(Op::Drop))
                            && self.closes(*i as usize, state)
                        {
                            self.shutdown(None, state, site)?;
                        }
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
                Op::StoreLocal(i) => {
                    // Assignment drops the pool the binding held.
                    if self.closes(*i as usize, state) {
                        self.shutdown(None, state, site)?;
                    }
                    state[*i as usize] = Slot::Owned
                }
                Op::DropLocal(i) => {
                    if self.closes(*i as usize, state) {
                        self.shutdown(Some(*i as usize), state, site)
                            .map_err(|error| {
                                leaving(error, written, early_exit(&ops[index + 1..], written))
                            })?;
                    }
                    state[*i as usize] = Slot::Unset
                }
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
                    let submitted = self.submitted.borrow().len();
                    loops.push(Loop {
                        entry,
                        breaks: vec![zero.clone()],
                    });
                    if self.sequence(body, state, loops, site)? {
                        self.backedge(state, &loops.last().unwrap().entry, site)?;
                    }
                    let Loop {
                        entry,
                        breaks: exits,
                    } = loops.pop().unwrap();
                    // A job submitted in one iteration can still be in its pool
                    // in the next, so shutdowns before the submit see it too.
                    if self.submitted.borrow().len() > submitted {
                        loops.push(Loop {
                            entry,
                            breaks: Vec::new(),
                        });
                        let again = self.sequence(body, &mut zero.clone(), loops, site);
                        loops.pop();
                        again?;
                    }
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
                Op::Return | Op::TailCall(_) | Op::TailCallIndirect(_) => {
                    self.release(state, site).map_err(|mut error| {
                        // A tail call returns without the keyword.
                        if let Some(at) = written {
                            error.notes.push((at, "the function returns here".into()));
                        }
                        error
                    })?;
                    return Ok(false);
                }
                Op::Unreachable => return Ok(false),
                _ => {}
            }
        }
        Ok(true)
    }

    #[cfg_attr(
        feature = "profile",
        hotpath::measure(label = "ownership::check_loans")
    )]
    fn loans(&self, ops: &[Op]) -> Result<()> {
        fn use_loan(id: usize, loans: &HashMap<usize, &Loan>, live: &mut Live) {
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
        let (nodes, reachable, loans, created) = crate::profile_block!("ownership::loan_graph", {
            let mut nodes = vec![Node::default()];
            let entry = graph(ops, None, 0, None, &mut nodes);
            let mut seen = vec![false; nodes.len()];
            let mut todo = vec![entry];
            while let Some(n) = todo.pop() {
                if !std::mem::replace(&mut seen[n], true) {
                    todo.extend(&nodes[n].successors);
                }
            }
            // Ascending, as liveness visits successors before their predecessors.
            let reachable: Vec<usize> = (0..nodes.len()).filter(|&n| seen[n]).collect();
            let mut loans = HashMap::new();
            let mut created = HashMap::new();
            for node in &nodes {
                if let Some(Op::Loan(l)) = node.op {
                    loans.insert(l.id, l);
                    created.insert(l.id, node.site);
                }
            }
            (nodes, reachable, loans, created)
        });
        let mut scratch = Vec::new();
        let live = crate::profile_block!("ownership::loan_liveness", {
            let mut live = vec![Live::default(); nodes.len()];
            let mut before = Live::default();
            loop {
                let mut changed = false;
                for &i in &reachable {
                    before.0.clear();
                    for &s in &nodes[i].successors {
                        before.union(&live[s], &mut scratch);
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
                        live[i].0.clone_from(&before.0);
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            live
        });
        crate::profile_block!("ownership::validate_loans", {
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
                            let mut used = Live::default();
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
            let mut after = Live::default();
            for &i in &reachable {
                after.0.clear();
                for &s in &nodes[i].successors {
                    after.union(&live[s], &mut scratch);
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
                    if let Some(&id) = after.0.first() {
                        let loan = &loans[&id];
                        let place = self.place(loan.root, &loan.fields, loan.precise);
                        found = Some((self.error(Kind::LiveAcrossYield, place, site), id));
                    }
                }
                if let Some((root, write, via, defining, action)) = access {
                    for id in &after.0 {
                        let loan = &loans[id];
                        if Some(*id) == defining || via.is_some_and(|v| ancestor(*id, v)) {
                            continue;
                        }
                        let accessed = defining.or(via).map(|id| &loans[&id]);
                        let fields = accessed.map(|l| l.fields.as_slice()).unwrap_or(&[]);
                        let overlaps =
                            loan.fields.starts_with(fields) || fields.starts_with(&loan.fields);
                        if loan.root == root && overlaps && (write || loan.mutable) {
                            let place =
                                self.place(root, fields, accessed.is_none_or(|l| l.precise));
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
        })
    }
}
