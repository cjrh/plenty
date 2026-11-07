//! What pool jobs can submit through a `Submitter`, and the rule that keeps a
//! job from waiting for a job it queued (issue #789; reference:
//! book/src/design/facilities/03-concurrency/executors.md, "Submit from
//! workers").
use super::{ExecutorOp, Job};
use crate::channel::{ChannelOp, SelectMode};
use crate::hash::{HashMap, HashSet};
use crate::op::{CompiledFn, Op, Ty};
use std::sync::Arc;

/// Whether a value of `ty` can hold a `Submitter`. A job has no other way to
/// reach one: pools cannot enter workers.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn holds_submitter(ty: &Ty) -> bool {
    let mut pending = vec![ty.clone()];
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if matches!(ty, Ty::Submitter) {
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
    visit: &mut impl FnMut(&'p Op, &str),
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
            visit(op, key);
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
        if !holds_submitter(&parent.input) {
            continue;
        }
        visit_reachable(definitions, parent, &mut |op, _| {
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

/// A job that can submit through a `Submitter` must not wait on a queue that
/// only a job it can submit might release: a receive on a queue such a job can
/// send to, or a send to a queue such a job can receive from. Queues are
/// matched by type, as the join and shutdown rules match them. When every
/// worker waits so, the jobs that would release them stay queued behind them.
/// Locks are not restricted: a lock's holder is running, so it releases the
/// lock unless it waits on a queue, which this rule or the job's own protocol
/// covers. Nonblocking operations and I/O are allowed.
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
        let inputs = || children.iter().map(|child| child.input.clone());
        let senders = crate::threading::endpoints(inputs(), true, true);
        let receivers = crate::threading::endpoints(inputs(), false, true);
        let mut error = None;
        visit_reachable(definitions, job, &mut |op, function| {
            if error.is_some() {
                return;
            }
            // The message types of the queues this operation waits to
            // receive from and to send to.
            let (received, sent): ([Option<&Ty>; 2], Option<&Ty>) = match op {
                Op::Channel(ChannelOp::Recv(message, false) | ChannelOp::RecvTimeout(message)) => {
                    ([Some(message), None], None)
                }
                Op::Channel(ChannelOp::Send(message, false) | ChannelOp::SendTimeout(message)) => {
                    ([None, None], Some(message))
                }
                Op::Channel(ChannelOp::Select(first, second, mode))
                    if *mode != SelectMode::Nowait =>
                {
                    ([Some(&**first), Some(&**second)], None)
                }
                _ => return,
            };
            if let Some(message) = received.into_iter().flatten().find(|m| senders.contains(m)) {
                error = Some((
                    format!("send on a `Sender[{message}]`"),
                    format!("receives from a `Receiver[{message}]`"),
                    function.to_owned(),
                ));
            } else if let Some(message) = sent.filter(|m| receivers.contains(m)) {
                error = Some((
                    format!("receive from a `Receiver[{message}]`"),
                    format!("sends on a `Sender[{message}]`"),
                    function.to_owned(),
                ));
            }
        });
        if let Some((reach, wait, function)) = error {
            return Err(format!(
                "executor worker {} can submit, through a Submitter, a job that can {reach}, and {} {wait} with a blocking or timed operation; if every worker waits so, the jobs that would release them stay queued behind them. Use the nonblocking form (`send_nowait`, `recv_nowait`, `select_recv_nowait`), or wait on a thread that is not a pool worker",
                describe(&job.worker),
                describe(&function),
            ));
        }
    }
    Ok(())
}
