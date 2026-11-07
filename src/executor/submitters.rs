//! What pool jobs can submit through a `Submitter`, and the rule that keeps a
//! job from waiting for a job it queued (issue #789; reference:
//! book/src/design/facilities/03-concurrency/executors.md, "Submit from
//! workers").
use super::{ExecutorOp, Job};
use crate::channel::{ChannelOp, SelectMode};
use crate::control::ControlOp;
use crate::hash::{HashMap, HashSet};
use crate::mutex::MutexOp;
use crate::op::{CompiledFn, Op, Ty};
use crate::threading::ThreadOp;
use std::sync::Arc;

/// Whether a value of one of `roots` can hold a value of type `wanted`.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn reaches(roots: impl IntoIterator<Item = Ty>, wanted: &Ty) -> bool {
    let mut pending: Vec<Ty> = roots.into_iter().collect();
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if ty == *wanted {
            return true;
        }
        if seen.insert(ty.clone()) {
            crate::threading::contained(&ty, &mut pending);
        }
    }
    false
}

/// The functions a job's code starts at.
fn roots(job: &Job) -> impl Iterator<Item = &str> {
    std::iter::once(job.worker.as_str()).chain(
        job.closure
            .iter()
            .chain(&job.shared)
            .map(|closure| closure.name.as_str()),
    )
}

/// Visit every operation of the functions `job`'s code can call, excluding
/// the jobs it submits.
fn visit_reachable<'p>(
    definitions: &HashMap<&str, &'p CompiledFn>,
    job: &Job,
    visit: &mut impl FnMut(&'p Op, &str, &'p CompiledFn),
) {
    let mut pending: Vec<String> = roots(job).map(str::to_owned).collect();
    let mut seen = HashSet::default();
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some((&key, function)) = definitions.get_key_value(name.as_str()) else {
            continue;
        };
        crate::threading::walk(&function.body, &mut |op| {
            // A submitted job runs as its own job, not in this one, and a
            // closure runs where it is called, not where it is made: a named
            // submission makes one to bind the job's arguments.
            if !matches!(op, Op::Executor(_) | Op::ClosureNew(_)) {
                crate::threading::function_references(op, &mut |name| {
                    pending.push(name.to_owned())
                });
            }
            // A join waits for everything the thread waits for, so the
            // thread's code counts as the job's own, wherever it started.
            if let Op::Thread(ThreadOp::Join(task) | ThreadOp::Finish(task, _)) = op {
                pending.push(task.worker.clone());
                pending.extend(task.closure.iter().map(|closure| closure.name.clone()));
            }
            visit(op, key, function);
        });
    }
}

/// The jobs that `job`, directly or through jobs they submit, can submit
/// through a `Submitter`, each once. Excludes `job` unless it can resubmit
/// itself.
pub fn descendants<'p>(
    definitions: &HashMap<&str, &'p CompiledFn>,
    job: &Job,
) -> Vec<&'p Arc<Job>> {
    let mut found: Vec<&'p Arc<Job>> = Vec::new();
    let mut pending = vec![job];
    let mut index = 0;
    while let Some(parent) = pending.pop() {
        // A job has no other way to reach a `Submitter`: pools cannot enter
        // workers.
        if !reaches([parent.input.clone()], &Ty::Submitter) {
            continue;
        }
        visit_reachable(definitions, parent, &mut |op, _, _| {
            if let Op::Executor(ExecutorOp::Submit(child, _)) = op
                && child.handle
                && !found.iter().any(|seen| Arc::ptr_eq(seen, child))
            {
                found.push(child);
            }
        });
        pending.extend(found[index..].iter().map(|child| &***child));
        index = found.len();
    }
    found
}

/// The coordination objects a set of values can reach, matched by type as the
/// join and shutdown rules match them.
struct Reach {
    senders: Vec<Ty>,
    receivers: Vec<Ty>,
    mutexes: Vec<Ty>,
    token: bool,
}

impl Reach {
    fn of(inputs: impl Iterator<Item = Ty> + Clone) -> Self {
        Self {
            senders: crate::threading::endpoints(inputs.clone(), true, true),
            receivers: crate::threading::endpoints(inputs.clone(), false, true),
            mutexes: crate::threading::mutexes(inputs.clone()),
            token: reaches(inputs, &Ty::CancellationToken),
        }
    }
}

/// The type of local `slot` of `function`: parameters, then locals.
fn slot_type(function: &CompiledFn, slot: u8) -> Option<&Ty> {
    let slot = slot as usize;
    let inputs = &function.sig.inputs;
    match inputs.get(slot) {
        Some((_, ty)) => Some(ty),
        None => function.locals.get(slot - inputs.len()),
    }
}

/// A job that can submit through a `Submitter` must not wait without a time
/// limit for anything that only a job it can submit might release, since when
/// every worker waits so, the jobs that would release them stay queued behind
/// them. Matched by type, it rejects an untimed:
/// - receive or selection on a queue whose sender such a job can reach, and a
///   send on a queue whose receiver it can reach;
/// - token wait, when such a job can reach a token;
/// - lock on a `Mutex[T]` such a job can reach; locks have no timed form.
///
/// A timed form ends at its timeout, so it is allowed; socket readiness waits
/// always take a deadline. A join is checked through the joined thread's
/// code, which counts as the job's own. Futures cannot enter workers.
/// Nonblocking operations and other I/O are allowed.
pub fn check(
    jobs: &[&Arc<Job>],
    definitions: &HashMap<&str, &CompiledFn>,
    describe: impl Fn(&str) -> String,
) -> Result<(), String> {
    let mut checked: Vec<&Arc<Job>> = Vec::new();
    for &job in jobs {
        if checked.iter().any(|seen| Arc::ptr_eq(seen, job)) {
            continue;
        }
        checked.push(job);
        let children = descendants(definitions, job);
        if children.is_empty() {
            continue;
        }
        let reach = Reach::of(children.iter().map(|child| child.input.clone()));
        let mut error = None;
        visit_reachable(definitions, job, &mut |op, name, function| {
            if error.is_none() {
                error = conflict(&reach, op, function).map(|found| (found, name.to_owned()));
            }
        });
        if let Some(((reach, wait), function)) = error {
            let advice = if wait.starts_with("locks") {
                "Keep the state in an atomic, or lock it on a thread that is not a pool worker"
            } else {
                "Use a timed form (`recv_timeout_ms`, `send_timeout_ms`, `select_recv_timeout_ms`, `select_recv_before`, `wait_timeout_ms`), a nonblocking form, or wait on a thread that is not a pool worker"
            };
            return Err(format!(
                "executor worker {} can submit, through a Submitter, a job that can {reach}, and {} {wait}; if every worker waits so, the jobs that would release them stay queued behind them. {advice}",
                describe(&job.worker),
                describe(&function),
            ));
        }
    }
    Ok(())
}

/// What a submitted job can reach and what `op` waits for, if `op` waits on
/// something such a job may have to release.
fn conflict(reach: &Reach, op: &Op, function: &CompiledFn) -> Option<(String, String)> {
    let found = |child: String, wait: String| Some((child, wait));
    match op {
        Op::Channel(ChannelOp::Recv(message, false)) if reach.senders.contains(message) => found(
            format!("send on a `Sender[{message}]`"),
            format!("receives from a `Receiver[{message}]` without a time limit"),
        ),
        Op::Channel(ChannelOp::Select(first, second, SelectMode::Wait)) => {
            let message = [&**first, &**second]
                .into_iter()
                .find(|m| reach.senders.contains(m))?;
            found(
                format!("send on a `Sender[{message}]`"),
                format!("receives from a `Receiver[{message}]` without a time limit"),
            )
        }
        Op::Channel(ChannelOp::Send(message, false)) if reach.receivers.contains(message) => found(
            format!("receive from a `Receiver[{message}]`"),
            format!("sends on a `Sender[{message}]` without a time limit"),
        ),
        Op::Control(ControlOp::Wait) if reach.token => found(
            "cancel a `CancellationToken`".to_owned(),
            "waits on a `CancellationToken` without a time limit".to_owned(),
        ),
        Op::Mutex(MutexOp::Lock(slot)) => {
            let Some(Ty::Ref(mutex, _)) = slot_type(function, *slot) else {
                return None;
            };
            let element = crate::mutex::element(crate::mutex::class(mutex)?);
            reach.mutexes.contains(&element).then(|| {
                (
                    format!("lock a `Mutex[{element}]`"),
                    format!("locks a `Mutex[{element}]`"),
                )
            })
        }
        _ => None,
    }
}
