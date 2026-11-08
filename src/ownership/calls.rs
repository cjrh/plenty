//! What a called function can wait for: the results of futures and pool
//! shutdowns it reaches, directly or through further calls (issue #349), and
//! the jobs it can submit (issue #352), and whether a worker can wait at all
//! (issue #793).
use super::{Hazard, Site};
use crate::diagnostic::Position;
use crate::executor::{ExecutorOp, Job as Submitted};
use crate::hash::{HashMap, HashSet};
use crate::op::{CallableSig, CompiledFn, Op, Ty};
use std::sync::{Arc, Mutex, OnceLock};

/// The functions of a program whose bodies are being checked. The call table
/// is built on first use: only a call that hands a traced future, or a pool
/// after a submission, to another function, or a call whose result can hold a
/// future, needs it.
pub struct Program<'p> {
    ops: &'p [Op],
    table: OnceLock<HashMap<&'p str, Body<'p>>>,
    /// The jobs each callee reaches, so every function is searched once.
    jobs: Mutex<HashMap<&'p str, Arc<[Job<'p>]>>>,
    /// Effects and their witness are cached together, per specialized name.
    effects: Mutex<HashMap<&'p str, Arc<Effects>>>,
    /// Every function body by name, for `waits`. Separate from `table`,
    /// whose construction asks what each submitted job can block on.
    definitions: OnceLock<HashMap<&'p str, &'p CompiledFn>>,
    /// `waits` by worker and closure names and input types. The names fix
    /// the inputs today, since generic workers are named per instance; the
    /// types are part of the key so a reused name cannot reuse a stale answer.
    waits: Mutex<HashMap<(String, Vec<Ty>), bool>>,
}

/// What a submitted job can block on, with the job's result type.
type Job<'p> = (&'p Ty, Hazard);

/// One function's own waits and its direct callees.
#[derive(Default)]
struct Body<'p> {
    /// Result types `T` of the `Future[T]` values whose `result()` it calls.
    results: Vec<Ty>,
    /// It calls `shutdown()` or owns a pool, which its drop shuts down.
    shutdown: Option<Cause>,
    /// It calls a value whose function is unknown.
    indirect: bool,
    /// An unresolved call can reach shutdown authority or caller job inputs.
    unknown_shutdown: Option<Cause>,
    callees: Vec<(&'p str, Option<Position>)>,
    /// What each job it submits can block on, with the job's result type.
    jobs: Vec<Job<'p>>,
}

/// The waits a call can reach, matched by type like the local checks.
#[derive(Default)]
pub(super) struct Effects {
    results: Vec<Ty>,
    shutdown: Option<Arc<Shutdown>>,
    /// The callee, or a function it reaches, is not known.
    unknown: bool,
}

/// One diagnostic witness, bounded independently of call-graph size. Source
/// positions belong to the callee's file, not the rejected caller's file.
#[derive(Debug)]
pub(super) struct Shutdown {
    cause: Cause,
    function: Option<String>,
    chain: Vec<Edge>,
    omitted: usize,
}

#[derive(Clone, Debug)]
struct Cause {
    kind: CauseKind,
    at: Option<Position>,
}

#[derive(Clone, Debug)]
enum CauseKind {
    Explicit,
    Owned {
        binding: Option<String>,
        parameter: bool,
    },
    Unknown(&'static str),
}

#[derive(Debug)]
struct Edge {
    caller: String,
    callee: String,
    at: Option<Position>,
}

fn display_name(name: &str) -> String {
    super::callee_name(Some(name))
        .map(|name| super::short(&name).into_owned())
        .unwrap_or_else(|| "generated function or closure".into())
}

impl Cause {
    fn unknown(description: &'static str, at: Option<Position>) -> Self {
        Self {
            kind: CauseKind::Unknown(description),
            at,
        }
    }

    fn assumed(&self) -> bool {
        matches!(self.kind, CauseKind::Unknown(_))
    }

    fn description(&self) -> String {
        match &self.kind {
            CauseKind::Explicit => "explicit pool shutdown operation".into(),
            CauseKind::Owned { binding, parameter } => {
                let kind = if *parameter { "parameter" } else { "local" };
                let name = binding
                    .as_ref()
                    .map(|name| format!(" `{name}`"))
                    .unwrap_or_default();
                format!("owned-pool cleanup: {kind}{name} owns a pool whose cleanup waits")
            }
            CauseKind::Unknown(description) => (*description).into(),
        }
    }
}

impl Shutdown {
    const CHAIN_LIMIT: usize = 8;

    pub(super) fn assumed(&self) -> bool {
        self.cause.assumed()
    }

    pub(super) fn explain(&self, message: &mut String) {
        let note = |message: &mut String, at: Option<&Position>, text: &str| {
            message.push_str(&format!("\n  {}note: {text}", super::prefix(at)));
        };
        if self.omitted != 0 {
            note(
                message,
                None,
                &format!("{} outer call edges omitted", self.omitted),
            );
        }
        for edge in &self.chain {
            note(
                message,
                edge.at.as_ref(),
                &format!("`{}` calls `{}`", edge.caller, edge.callee),
            );
        }
        let function = self
            .function
            .as_ref()
            .map(|name| format!(" in `{name}`"))
            .unwrap_or_default();
        let certainty = if self.assumed() {
            "assumed"
        } else {
            "discovered"
        };
        note(
            message,
            self.cause.at.as_ref(),
            &format!(
                "pool shutdown is {certainty}{function}: {}",
                self.cause.description()
            ),
        );
        note(
            message,
            None,
            "shutdowns and submitted jobs are matched conservatively, not by pool identity or call order",
        );
        if self.assumed() {
            note(
                message,
                None,
                "the opaque call is not proved to shut down a pool; keep shutdown authority and caller job hazards out of opaque callbacks where possible",
            );
        }
    }
}

impl Effects {
    /// The call can wait for the result of a `Future[T]` it is handed.
    pub(super) fn waits(&self, result: &Ty) -> bool {
        self.unknown || self.results.contains(result)
    }

    /// The call can shut down a pool it is handed or creates for caller inputs.
    pub(super) fn shutdown(&self) -> Option<Arc<Shutdown>> {
        self.shutdown.clone()
    }

    pub(super) fn shuts(&self) -> bool {
        self.shutdown.is_some()
    }
}

impl<'p> Program<'p> {
    pub fn new(ops: &'p [Op]) -> Self {
        Self {
            ops,
            table: OnceLock::new(),
            jobs: Mutex::default(),
            effects: Mutex::default(),
            definitions: OnceLock::new(),
            waits: Mutex::default(),
        }
    }

    fn table(&self) -> &HashMap<&'p str, Body<'p>> {
        self.table.get_or_init(|| {
            let mut table = HashMap::default();
            crate::threading::walk(self.ops, &mut |op| {
                if let Op::DefineFn(name, function) = op {
                    table.insert(name.as_str(), body(function, self));
                }
            });
            table
        })
    }

    fn definitions(&self) -> &HashMap<&'p str, &'p CompiledFn> {
        self.definitions.get_or_init(|| {
            let mut definitions = HashMap::default();
            crate::threading::walk(self.ops, &mut |op| {
                if let Op::DefineFn(name, function) = op {
                    definitions.insert(name.as_str(), &**function);
                }
            });
            definitions
        })
    }

    /// Whether calling `function` can wait; see [`Self::waits`].
    pub(super) fn can_wait(&self, function: &str) -> bool {
        self.waits(&[function], &[])
    }

    /// Whether a scoped task's worker can wait for anything.
    pub(super) fn task_waits(&self, task: &crate::threading::Task) -> bool {
        let roots: Vec<&str> = std::iter::once(task.worker.as_str())
            .chain(task.closure.iter().map(|closure| closure.name.as_str()))
            .collect();
        self.waits(&roots, &task.inputs)
    }

    /// What a pool job can block on: the senders it can hold, unless it
    /// cannot wait for anything, and the mutexes it can reach; and the same
    /// for every job it can submit through a `Submitter` (issue #789).
    pub(super) fn job_hazards(&self, job: &Submitted) -> Vec<Hazard> {
        let mut hazards = self.own_hazards(job);
        let descendants = crate::executor::submitters::descendants(self.definitions(), job);
        for child in descendants {
            for hazard in self.own_hazards(child) {
                if !hazards.contains(&hazard) {
                    hazards.push(hazard);
                }
            }
        }
        hazards
    }

    fn own_hazards(&self, job: &Submitted) -> Vec<Hazard> {
        let mut sends = crate::threading::endpoints([job.input.clone()], true, true);
        if !sends.is_empty() {
            let roots: Vec<&str> = std::iter::once(job.worker.as_str())
                .chain(
                    job.closure
                        .iter()
                        .chain(&job.shared)
                        .map(|c| c.name.as_str()),
                )
                .collect();
            if !self.waits(&roots, std::slice::from_ref(&job.input)) {
                sends.clear();
            }
        }
        sends
            .into_iter()
            .map(Hazard::Sends)
            .chain(
                crate::threading::mutexes([job.input.clone()])
                    .into_iter()
                    .map(Hazard::Locks),
            )
            .collect()
    }

    /// Whether code started at `roots`, holding values of `inputs`, can wait
    /// for another thread (issue #793). Any wait counts, not only one on the
    /// held endpoint's queue: a worker waiting on a lock or another queue can
    /// wait for a thread that is itself blocked on the held queue. Only
    /// `send_nowait`, `recv_nowait`, nowait selection, and `submit_nowait`
    /// never wait; a timed operation's bound can be arbitrarily long. Waits
    /// for tokens, readiness, and child processes count, as do file, socket,
    /// and datagram transfers and foreign calls. A sleep is not counted: it
    /// ends without another thread. Nor are standard-stream reads and writes,
    /// whose other end is outside the program.
    ///
    /// Calls follow `function_references`, which also names the workers a
    /// function starts and the destructors it arms; the destructor of every
    /// class it can hold, receive or build is followed as well. Calls through
    /// values, erased types, and functions without a body count as waits.
    #[expect(
        clippy::mutable_key_type,
        reason = "nominal identities exclude definition cells"
    )]
    fn waits(&self, roots: &[&str], inputs: &[Ty]) -> bool {
        let key = (roots.join("\n"), inputs.to_vec());
        if let Some(&cached) = self.waits.lock().unwrap().get(&key) {
            return cached;
        }
        let definitions = self.definitions();
        let mut functions: Vec<String> = roots.iter().map(|&name| name.to_owned()).collect();
        let mut types = inputs.to_vec();
        let mut seen_functions = HashSet::default();
        let mut seen_types = HashSet::default();
        let waits = 'search: loop {
            while let Some(ty) = types.pop() {
                if !seen_types.insert(ty.clone()) {
                    continue;
                }
                match &ty {
                    Ty::Class(class) => functions.extend(class.get().destructor.iter().cloned()),
                    // Their destructors and calls are erased.
                    Ty::BoxedCallable(..) | Ty::BoxedProtocol(_) | Ty::Generator(_) => {
                        break 'search true;
                    }
                    _ => {}
                }
                crate::threading::contained(&ty, &mut types);
            }
            let Some(name) = functions.pop() else {
                break false;
            };
            if !seen_functions.insert(name.clone()) {
                continue;
            }
            // A function compiled elsewhere has no body to inspect.
            let Some(function) = definitions.get(name.as_str()) else {
                break true;
            };
            // Only the types and calls matter here; worker eligibility
            // reports the error separately.
            let mut calls = Vec::new();
            let _ = crate::threading::inspect(function, &mut types, &mut calls);
            functions.extend(calls);
            let mut waits = false;
            crate::threading::walk(&function.body, &mut |op| {
                crate::threading::function_references(op, &mut |name| {
                    functions.push(name.to_owned())
                });
                waits |= super::blocking::blocks(op);
            });
            if waits {
                break true;
            }
        };
        self.waits.lock().unwrap().insert(key, waits);
        waits
    }

    /// Visit the body of every function a call to `callee` can reach, or
    /// `None` once for a function without a body to inspect. Recursion
    /// terminates through the visited set. `visit` returns false to stop.
    fn reach(&self, callee: &str, mut visit: impl FnMut(Option<&Body<'p>>) -> bool) {
        let table = self.table();
        let mut seen = HashSet::default();
        let mut pending = vec![callee];
        while let Some(name) = pending.pop() {
            if !seen.insert(name) {
                continue;
            }
            // A function compiled elsewhere has no body to inspect.
            let body = table.get(name);
            if !visit(body) {
                return;
            }
            if let Some(body) = body {
                pending.extend(body.callees.iter().map(|(name, _)| *name));
            }
        }
    }

    /// Everything a call to `callee` can reach, or every effect for an
    /// indirect call (`None`). A cycle's effects are the union of its
    /// members'.
    pub(super) fn effects(&self, callee: Option<&str>) -> Arc<Effects> {
        let Some(callee) = callee else {
            return Arc::new(Effects {
                unknown: true,
                shutdown: Some(Arc::new(Shutdown {
                    cause: Cause::unknown("indirect call", None),
                    function: None,
                    chain: Vec::new(),
                    omitted: 0,
                })),
                ..Effects::default()
            });
        };
        let table = self.table();
        if let Some(cached) = self.effects.lock().unwrap().get(callee).cloned() {
            return cached;
        }
        // Breadth-first, in stable IR order. Each node keeps only its
        // predecessor, not a full path. Never stop at an unknown effect:
        // concrete shutdowns and future waits may still follow it.
        let mut nodes = vec![(callee, None, None)];
        let mut seen = HashSet::default();
        seen.insert(callee);
        let mut effects = Effects::default();
        let mut witness: Option<(usize, Cause)> = None;
        let mut index = 0;
        while index < nodes.len() {
            let name = nodes[index].0;
            let cause = if let Some(body) = table.get(name) {
                effects.unknown |= body.indirect;
                for result in &body.results {
                    if !effects.results.contains(result) {
                        effects.results.push(result.clone());
                    }
                }
                for (callee, at) in &body.callees {
                    if seen.insert(callee) {
                        nodes.push((callee, Some(index), at.clone()));
                    }
                }
                body.shutdown
                    .as_ref()
                    .or(body.unknown_shutdown.as_ref())
                    .cloned()
            } else {
                effects.unknown = true;
                Some(Cause::unknown(
                    "missing function body (source unavailable)",
                    None,
                ))
            };
            if let Some(cause) = cause
                && witness
                    .as_ref()
                    .is_none_or(|(_, old)| old.assumed() && !cause.assumed())
            {
                witness = Some((index, cause));
            }
            index += 1;
        }
        if let Some((index, cause)) = witness {
            let mut chain = Vec::new();
            let mut current = index;
            let mut omitted = 0;
            while let Some(parent) = nodes[current].1 {
                if chain.len() < Shutdown::CHAIN_LIMIT {
                    chain.push(Edge {
                        caller: display_name(nodes[parent].0),
                        callee: display_name(nodes[current].0),
                        at: nodes[current].2.clone(),
                    });
                } else {
                    omitted += 1;
                }
                current = parent;
            }
            chain.reverse();
            effects.shutdown = Some(Arc::new(Shutdown {
                function: Some(display_name(nodes[index].0)),
                cause,
                chain,
                omitted,
            }));
        }
        let effects = Arc::new(effects);
        if let Some((&name, _)) = table.get_key_value(callee) {
            self.effects.lock().unwrap().insert(name, effects.clone());
        }
        effects
    }

    /// What the jobs that a call to `callee` can submit, directly or through
    /// further calls, can block on, for jobs whose result type is one of
    /// `results`. Jobs submitted through a call to a value, or by a function
    /// compiled elsewhere, are not seen.
    pub(super) fn submissions(&self, callee: Option<&str>, results: &[Ty]) -> Vec<Hazard> {
        let mut hazards = Vec::new();
        for (result, hazard) in self.reached_jobs(callee).iter() {
            if results.contains(result) && !hazards.contains(hazard) {
                hazards.push(hazard.clone());
            }
        }
        hazards
    }

    /// What every job that a call to `callee` can submit, directly or
    /// through further calls, can block on, whatever its result type: a
    /// pool handed to the call can still hold such a job when the caller
    /// shuts it down (issue #855). Jobs are not matched to pools, so a job
    /// the callee submits to a pool of its own counts too. As for
    /// `submissions`, jobs behind a call to a value are not seen.
    pub(super) fn pool_submissions(&self, callee: Option<&str>) -> Vec<Hazard> {
        let mut hazards = Vec::new();
        for (_, hazard) in self.reached_jobs(callee).iter() {
            if !hazards.contains(hazard) {
                hazards.push(hazard.clone());
            }
        }
        hazards
    }

    /// What the jobs that a call to `callee` can submit can block on, when
    /// the call can also shut down a pool, so that the jobs can still be
    /// running at that shutdown inside the call although it is handed no
    /// pool (issue #860). Only senders and mutexes of types that the call's
    /// arguments, or a closure's `captures`, can reach count: a job of a
    /// pool the callee makes can hold a caller's endpoint only through them.
    /// The call is not checked to submit before it shuts down, nor to shut
    /// down the pool it submitted to.
    pub(super) fn own_pool_submissions(&self, callee: &str, captures: &[Ty]) -> Vec<Hazard> {
        let Some(function) = self.definitions().get(callee) else {
            return Vec::new();
        };
        let roots: Vec<Ty> = function
            .sig
            .inputs
            .iter()
            .map(|(_, ty)| ty.clone())
            .chain(captures.iter().cloned())
            .collect();
        let sends = crate::threading::endpoints(roots.iter().cloned(), true, true);
        let locks = crate::threading::mutexes(roots);
        if sends.is_empty() && locks.is_empty() {
            return Vec::new();
        }
        let hazards: Vec<Hazard> = self
            .pool_submissions(Some(callee))
            .into_iter()
            .filter(|hazard| match hazard {
                Hazard::Sends(message) => sends.contains(message),
                Hazard::Locks(element) => locks.contains(element),
                Hazard::Cancels(_) => false,
            })
            .collect();
        if hazards.is_empty() || !self.effects(Some(callee)).shuts() {
            return Vec::new();
        }
        hazards
    }

    fn reached_jobs(&self, callee: Option<&str>) -> Arc<[Job<'p>]> {
        let Some(callee) = callee else {
            return Arc::default();
        };
        let Some((&name, _)) = self.table().get_key_value(callee) else {
            return Arc::default();
        };
        let cached = self.jobs.lock().unwrap().get(name).cloned();
        cached.unwrap_or_else(|| {
            let mut jobs = Vec::new();
            self.reach(name, |body| {
                for job in body.map_or(&[][..], |body| &body.jobs) {
                    if !jobs.contains(job) {
                        jobs.push(job.clone());
                    }
                }
                true
            });
            let jobs: Arc<[_]> = jobs.into();
            self.jobs.lock().unwrap().insert(name, jobs.clone());
            jobs
        })
    }
}

/// The result types `T` of the `Future[T]` values that a value of `ty` can
/// hold, directly or in aggregates. A future's own result is not searched.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
pub fn future_results(ty: &Ty) -> Vec<Ty> {
    let mut found = Vec::new();
    let mut pending = Vec::new();
    crate::threading::contained(ty, &mut pending);
    if pending.is_empty() {
        return found;
    }
    pending = vec![ty.clone()];
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if let Ty::Future(result) = &ty {
            if !found.contains(&**result) {
                found.push((**result).clone());
            }
        } else if seen.insert(ty.clone()) {
            crate::threading::contained(&ty, &mut pending);
        }
    }
    found
}

/// A plain `Callable` is a code address, not an environment, and module
/// constants cannot hold runtime resources. With only data in its signature,
/// an unknown target cannot shut down a caller's pool or feed caller-owned
/// endpoints/locks to a pool it creates. This is deliberately not a general
/// effect proof: erased values, destructors and unclassified types stay unknown.
/// Include results too, so returned authority is not silently certified.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn callable_may_shut_down(signature: &CallableSig) -> bool {
    let mut pending: Vec<_> = signature
        .inputs
        .iter()
        .chain(signature.output.iter())
        .cloned()
        .collect();
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if !seen.insert(ty.clone()) {
            continue;
        }
        match &ty {
            Ty::I8
            | Ty::I16
            | Ty::I32
            | Ty::I64
            | Ty::U8
            | Ty::U16
            | Ty::U32
            | Ty::U64
            | Ty::F32
            | Ty::F64
            | Ty::Bool
            | Ty::Unit
            | Ty::Str
            | Ty::ByteStr
            | Ty::Text
            // A Submitter cannot shut down its pool, even on drop.
            | Ty::Submitter => {}
            Ty::Class(class)
                if class.get().destructor.is_none() && crate::mutex::class(&ty).is_none() =>
            {
                crate::threading::contained(&ty, &mut pending);
            }
            Ty::Enum(_)
            | Ty::Ref(..)
            | Ty::View(..)
            | Ty::List(_)
            | Ty::Clist(_)
            | Ty::Set(_)
            | Ty::Dict(..)
            | Ty::Box(_)
            | Ty::Shared(_) => crate::threading::contained(&ty, &mut pending),
            _ => return true,
        }
    }
    false
}

fn body<'p>(function: &'p CompiledFn, program: &Program<'p>) -> Body<'p> {
    let owned = function
        .sig
        .inputs
        .iter()
        .map(|(_, ty)| ty)
        .chain(function.locals.iter())
        .position(crate::threading::owns_executor);
    let mut body = Body::default();
    if let Some(slot) = owned {
        body.shutdown = Some(Cause {
            kind: CauseKind::Owned {
                binding: function
                    .origins
                    .names
                    .get(slot)
                    .and_then(|name| name.as_deref())
                    .map(|name| super::short(name).into_owned()),
                parameter: slot < function.sig.inputs.len(),
            },
            at: function.location.clone(),
        });
    }
    crate::threading::walk_sited(&function.body, &mut |op, site| {
        let at = || site.map(|site| position(function, site));
        match op {
            Op::Executor(ExecutorOp::Result(result)) => {
                if !body.results.contains(result) {
                    body.results.push(result.clone());
                }
            }
            Op::Executor(ExecutorOp::Shutdown) => {
                // An explicit operation explains more than type-level cleanup.
                if body
                    .shutdown
                    .as_ref()
                    .is_none_or(|cause| matches!(cause.kind, CauseKind::Owned { .. }))
                {
                    body.shutdown = Some(Cause {
                        kind: CauseKind::Explicit,
                        at: at(),
                    });
                }
            }
            Op::StoreLocal(slot)
                if owned == Some(*slot as usize) && *slot as usize >= function.sig.inputs.len() =>
            {
                if let Some(cause) = &mut body.shutdown
                    && matches!(cause.kind, CauseKind::Owned { .. })
                    && cause.at == function.location
                {
                    cause.at = at().or_else(|| function.location.clone());
                }
            }
            Op::Executor(ExecutorOp::Submit(job, _)) => {
                for hazard in program.job_hazards(job) {
                    let job = (&job.output, hazard);
                    if !body.jobs.contains(&job) {
                        body.jobs.push(job);
                    }
                }
            }
            Op::Call(name) | Op::TailCall(name) => body.callees.push((name, at())),
            Op::ClosureCall(closure) if !closure.name.is_empty() => {
                body.callees.push((&closure.name, at()))
            }
            Op::CallIndirect(signature) | Op::TailCallIndirect(signature) => {
                body.indirect = true;
                if callable_may_shut_down(signature) && body.unknown_shutdown.is_none() {
                    body.unknown_shutdown = Some(Cause::unknown(
                        "indirect Callable call; its input/result types may expose pool authority or job hazards",
                        at(),
                    ));
                }
            }
            Op::ClosureCall(_) | Op::BoxedCallableCall(..) | Op::ProtocolBoxCall(..) => {
                body.indirect = true;
                let construct = match op {
                    Op::ClosureCall(_) => "unresolved closure call",
                    Op::BoxedCallableCall(..) => "erased BoxedCallable call",
                    _ => "erased boxed-protocol call",
                };
                body.unknown_shutdown
                    .get_or_insert_with(|| Cause::unknown(construct, at()));
            }
            _ => {}
        }
    });
    body
}

fn position(function: &CompiledFn, site: Site) -> Position {
    Position {
        file: function.origins.source.clone(),
        line: site.line,
        column: site.column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{BoxedMode, FnSig};

    fn signature(inputs: Vec<Ty>, output: Option<Ty>) -> Arc<CallableSig> {
        Arc::new(CallableSig { inputs, output })
    }

    #[test]
    fn callable_shutdown_capabilities_include_inputs_and_results() {
        for ty in [
            Ty::Executor,
            Ty::Ref(Arc::new(Ty::Executor), false),
            Ty::List(Arc::new(Ty::Channel(Arc::new(Ty::I64), true))),
            Ty::BoxedCallable(signature(vec![], None), BoxedMode::Shared),
            Ty::ForeignPtr("Opaque".into(), crate::foreign::Threads::None),
        ] {
            assert!(callable_may_shut_down(&signature(vec![ty.clone()], None)));
            assert!(callable_may_shut_down(&signature(vec![], Some(ty))));
        }
        for ty in [
            Ty::I64,
            Ty::Submitter,
            Ty::Ref(Arc::new(Ty::List(Arc::new(Ty::Str))), false),
        ] {
            assert!(!callable_may_shut_down(&signature(vec![ty.clone()], None)));
            assert!(!callable_may_shut_down(&signature(vec![], Some(ty))));
        }
    }

    fn function(body: Vec<Op>) -> CompiledFn {
        CompiledFn {
            location: None,
            origins: Arc::default(),
            generator: None,
            sig: Arc::new(FnSig {
                inputs: vec![],
                outputs: vec![],
            }),
            doc: "".into(),
            body: body.into(),
            locals: Arc::default(),
        }
    }

    #[test]
    fn unknown_future_waits_do_not_imply_unknown_shutdown() {
        let ops = [Op::DefineFn(
            "apply".into(),
            function(vec![Op::CallIndirect(signature(vec![], None))]).into(),
        )];
        let program = Program::new(&ops);
        let effects = program.effects(Some("apply"));
        assert!(effects.waits(&Ty::I64));
        assert!(!effects.shuts());
        assert!(program.effects(None).shuts());
        assert!(program.effects(Some("missing")).shuts());
        assert!(explanation(&program.effects(Some("missing"))).contains(
            "pool shutdown is assumed in `missing`: missing function body (source unavailable)"
        ));
    }

    #[test]
    fn effect_traversal_keeps_concrete_effects_after_unknown_calls() {
        let ops = [
            Op::DefineFn(
                "root".into(),
                function(vec![
                    Op::Call("concrete".into()),
                    Op::Call("unknown".into()),
                ])
                .into(),
            ),
            Op::DefineFn(
                "unknown".into(),
                function(vec![
                    Op::CallIndirect(signature(vec![], None)),
                    Op::Call("root".into()),
                    Op::Call("missing".into()),
                ])
                .into(),
            ),
            Op::DefineFn(
                "concrete".into(),
                function(vec![
                    Op::Executor(ExecutorOp::Shutdown),
                    Op::Executor(ExecutorOp::Result(Ty::I64)),
                ])
                .into(),
            ),
        ];
        let program = Program::new(&ops);
        let effects = program.effects(Some("root"));
        assert!(effects.unknown);
        assert!(!effects.shutdown.as_ref().unwrap().cause.assumed());
        assert!(effects.shuts());
        assert_eq!(effects.results, [Ty::I64]);
    }
    fn sited_function(file: &str, line: u32, ops: Vec<Op>) -> CompiledFn {
        let mut f = function(ops);
        f.location = Some(Position {
            file: Some(file.into()),
            line,
            column: 1,
        });
        f.origins = Arc::new(super::super::Origins {
            source: Some(file.into()),
            ..Default::default()
        });
        f
    }

    fn site(line: u32) -> Op {
        Op::Site(Site {
            line,
            column: 5,
            scope_end: false,
        })
    }

    fn explanation(effects: &Effects) -> String {
        let mut text = String::new();
        effects.shutdown.as_ref().unwrap().explain(&mut text);
        text
    }

    #[test]
    fn shutdown_witness_is_sited_recursive_deterministic_and_cached() {
        let ops = [
            Op::DefineFn(
                "helper[i64]".into(),
                sited_function(
                    "caller.p",
                    1,
                    vec![
                        site(2),
                        Op::Call("opaque".into()),
                        site(3),
                        Op::Call("stop".into()),
                        site(4),
                        Op::Call("other".into()),
                    ],
                )
                .into(),
            ),
            Op::DefineFn(
                "opaque".into(),
                function(vec![
                    Op::Call("helper[i64]".into()),
                    Op::Call("missing".into()),
                ])
                .into(),
            ),
            Op::DefineFn(
                "stop".into(),
                sited_function(
                    "shutdown.p",
                    10,
                    vec![site(11), Op::Executor(ExecutorOp::Shutdown)],
                )
                .into(),
            ),
            Op::DefineFn(
                "other".into(),
                function(vec![Op::Executor(ExecutorOp::Shutdown)]).into(),
            ),
            Op::DefineFn(
                "helper[str]".into(),
                function(vec![Op::Call("missing".into())]).into(),
            ),
        ];
        let program = Program::new(&ops);
        let effects = program.effects(Some("helper[i64]"));
        assert!(effects.unknown);
        assert!(effects.waits(&Ty::Bool));
        assert!(!effects.shutdown.as_ref().unwrap().assumed());
        assert!(Arc::ptr_eq(&effects, &program.effects(Some("helper[i64]"))));
        let text = explanation(&effects);
        assert!(
            text.contains("caller.p:3:5: note: `helper[i64]` calls `stop`"),
            "{text}"
        );
        assert!(
            text.contains("shutdown.p:11:5: note: pool shutdown is discovered in `stop`: explicit"),
            "{text}"
        );
        assert!(
            !text.contains("opaque") && !text.contains("other") && !text.contains("missing"),
            "{text}"
        );
        assert!(
            explanation(&program.effects(Some("helper[str]")))
                .contains("assumed in `missing`: missing function body (source unavailable)")
        );
        for _ in 0..10 {
            assert_eq!(
                text,
                explanation(&Program::new(&ops).effects(Some("helper[i64]")))
            );
        }
        // New IR provenance must not reuse a cache keyed only by symbol name.
        let relocated = [Op::DefineFn(
            "stop".into(),
            sited_function(
                "relocated.p",
                20,
                vec![site(21), Op::Executor(ExecutorOp::Shutdown)],
            )
            .into(),
        )];
        assert!(
            explanation(&Program::new(&relocated).effects(Some("stop")))
                .contains("relocated.p:21:5")
        );
    }

    #[test]
    fn shutdown_witness_bounds_long_chains_without_losing_the_cause() {
        let mut ops = Vec::new();
        for n in 0..40 {
            ops.push(Op::DefineFn(
                format!("f{n}"),
                function(vec![Op::Call(format!("f{}", n + 1))]).into(),
            ));
        }
        let program = Program::new(&ops);
        let effects = program.effects(Some("f0"));
        let witness = effects.shutdown.as_ref().unwrap();
        assert_eq!(witness.chain.len(), Shutdown::CHAIN_LIMIT);
        assert_eq!(witness.omitted, 32);
        let text = explanation(&effects);
        assert!(text.contains("32 outer call edges omitted"), "{text}");
        assert!(text.contains("`f39` calls `f40`"), "{text}");
        assert!(
            text.contains("assumed in `f40`: missing function body (source unavailable)"),
            "{text}"
        );
    }

    #[test]
    fn shutdown_witness_records_owned_parameter_and_local_bindings() {
        for parameter in [true, false] {
            let mut f = sited_function("owned.p", 1, vec![site(2), Op::StoreLocal(0)]);
            f.origins = Arc::new(super::super::Origins {
                source: Some("owned.p".into()),
                names: vec![Some("pool".into())],
                ..Default::default()
            });
            if parameter {
                f.sig = Arc::new(FnSig {
                    inputs: vec![("pool".into(), Ty::Executor)],
                    outputs: vec![],
                });
            } else {
                f.locals = vec![Ty::Executor].into();
            }
            let ops = [Op::DefineFn("own".into(), f.into())];
            let program = Program::new(&ops);
            let text = explanation(&program.effects(Some("own")));
            let kind = if parameter { "parameter" } else { "local" };
            assert!(
                text.contains(&format!("owned-pool cleanup: {kind} `pool`")),
                "{text}"
            );
            let line = if parameter { 1 } else { 2 };
            let column = if parameter { 1 } else { 5 };
            assert!(
                text.contains(&format!("owned.p:{line}:{column}:")),
                "{text}"
            );
            assert!(!program.effects(Some("own")).unknown);
        }
    }

    #[test]
    fn capable_and_erased_calls_have_assumed_witnesses_but_data_only_calls_do_not() {
        for (call, expected) in [
            (
                Op::ClosureCall(Arc::new(
                    crate::closure::ClosureType::new(
                        String::new(),
                        None,
                        (*signature(vec![], None)).clone(),
                        vec![],
                        vec![],
                        false,
                    )
                    .unwrap(),
                )),
                "unresolved closure call",
            ),
            (
                Op::CallIndirect(signature(
                    vec![Ty::Ref(Arc::new(Ty::Executor), false)],
                    None,
                )),
                "indirect Callable call",
            ),
            (
                Op::BoxedCallableCall(signature(vec![], None), BoxedMode::Shared),
                "erased BoxedCallable call",
            ),
        ] {
            let ops = [Op::DefineFn(
                "apply".into(),
                sited_function("apply.p", 1, vec![site(2), call]).into(),
            )];
            let program = Program::new(&ops);
            let effects = program.effects(Some("apply"));
            assert!(effects.waits(&Ty::I64));
            let text = explanation(&effects);
            assert!(
                text.contains(&format!(
                    "apply.p:2:5: note: pool shutdown is assumed in `apply`: {expected}"
                )),
                "{text}"
            );
        }
        let effects = Program::new(&[]).effects(None);
        assert!(explanation(&effects).contains("assumed: indirect call"));
    }
}
