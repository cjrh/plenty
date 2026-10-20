//! What a called function can wait for: the results of futures and pool
//! shutdowns it reaches, directly or through further calls (issue #349), and
//! the jobs it can submit (issue #352).
use super::Hazard;
use crate::executor::ExecutorOp;
use crate::hash::{HashMap, HashSet};
use crate::op::{CompiledFn, Op, Ty};
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
}

/// What a submitted job can block on, with the job's result type.
type Job<'p> = (&'p Ty, Hazard);

/// One function's own waits and its direct callees.
#[derive(Default)]
struct Body<'p> {
    /// Result types `T` of the `Future[T]` values whose `result()` it calls.
    results: Vec<Ty>,
    /// It calls `shutdown()` or owns a pool, which its drop shuts down.
    shuts: bool,
    /// It calls a value whose function is unknown.
    indirect: bool,
    callees: Vec<&'p str>,
    /// What each job it submits can block on, with the job's result type.
    jobs: Vec<Job<'p>>,
}

/// The waits a call can reach, matched by type like the local checks.
#[derive(Default)]
pub(super) struct Effects {
    results: Vec<Ty>,
    shuts: bool,
    /// The callee, or a function it reaches, is not known.
    unknown: bool,
}

impl Effects {
    /// The call can wait for the result of a `Future[T]` it is handed.
    pub(super) fn waits(&self, result: &Ty) -> bool {
        self.unknown || self.results.contains(result)
    }

    /// The call can shut down a pool it is handed.
    pub(super) fn shuts(&self) -> bool {
        self.unknown || self.shuts
    }
}

impl<'p> Program<'p> {
    pub fn new(ops: &'p [Op]) -> Self {
        Self {
            ops,
            table: OnceLock::new(),
            jobs: Mutex::default(),
        }
    }

    fn table(&self) -> &HashMap<&'p str, Body<'p>> {
        self.table.get_or_init(|| {
            let mut table = HashMap::default();
            crate::threading::walk(self.ops, &mut |op| {
                if let Op::DefineFn(name, function) = op {
                    table.insert(name.as_str(), body(function));
                }
            });
            table
        })
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
                pending.extend(body.callees.iter().copied());
            }
        }
    }

    /// Everything a call to `callee` can reach, or every effect for an
    /// indirect call (`None`). A cycle's effects are the union of its
    /// members'.
    pub(super) fn effects(&self, callee: Option<&str>) -> Effects {
        let mut effects = Effects {
            unknown: callee.is_none(),
            ..Effects::default()
        };
        let Some(callee) = callee else {
            return effects;
        };
        self.reach(callee, |body| {
            let Some(body) = body.filter(|body| !body.indirect) else {
                effects.unknown = true;
                return false;
            };
            effects.shuts |= body.shuts;
            for result in &body.results {
                if !effects.results.contains(result) {
                    effects.results.push(result.clone());
                }
            }
            true
        });
        effects
    }

    /// What the jobs that a call to `callee` can submit, directly or through
    /// further calls, can block on, for jobs whose result type is one of
    /// `results`. Jobs submitted through a call to a value, or by a function
    /// compiled elsewhere, are not seen.
    pub(super) fn submissions(&self, callee: Option<&str>, results: &[Ty]) -> Vec<Hazard> {
        let mut hazards = Vec::new();
        let Some(callee) = callee else {
            return hazards;
        };
        let Some((&name, _)) = self.table().get_key_value(callee) else {
            return hazards;
        };
        let cached = self.jobs.lock().unwrap().get(name).cloned();
        let jobs = cached.unwrap_or_else(|| {
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
        });
        for (result, hazard) in jobs.iter() {
            if results.contains(result) && !hazards.contains(hazard) {
                hazards.push(hazard.clone());
            }
        }
        hazards
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

fn body(function: &CompiledFn) -> Body<'_> {
    let mut body = Body {
        shuts: function
            .sig
            .inputs
            .iter()
            .map(|(_, ty)| ty)
            .chain(function.locals.iter())
            .any(crate::threading::owns_executor),
        ..Body::default()
    };
    crate::threading::walk(&function.body, &mut |op| match op {
        Op::Executor(ExecutorOp::Result(result)) => {
            if !body.results.contains(result) {
                body.results.push(result.clone());
            }
        }
        Op::Executor(ExecutorOp::Shutdown) => body.shuts = true,
        Op::Executor(ExecutorOp::Submit(job, _)) => {
            for hazard in Hazard::of(&job.input) {
                let job = (&job.output, hazard);
                if !body.jobs.contains(&job) {
                    body.jobs.push(job);
                }
            }
        }
        Op::Call(name) | Op::TailCall(name) => body.callees.push(name),
        Op::ClosureCall(closure) if !closure.name.is_empty() => body.callees.push(&closure.name),
        Op::ClosureCall(_)
        | Op::CallIndirect(_)
        | Op::TailCallIndirect(_)
        | Op::BoxedCallableCall(_) => body.indirect = true,
        _ => {}
    });
    body
}
