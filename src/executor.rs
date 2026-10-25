//! Checked executor operations and statically certified job adapters.
use crate::op::Ty;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub adapter: String,
    pub worker: String,
    pub input: Ty,
    pub output: Ty,
    pub closure: Option<Arc<crate::closure::ClosureType>>,
    /// One caller-owned immutable environment shared until the map drains.
    pub shared: Option<Arc<crate::closure::ClosureType>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExecutorOp {
    New,
    Submit(Arc<Job>, bool),
    Map(Arc<Job>, Ty, bool),
    Reduce(Arc<Job>, Ty),
    Result(Ty),
    Done(Ty),
    Cancel(Ty),
    WaitTimeout(Ty),
    Shutdown,
    /// The pool's worker count: a parallel pipeline's default chunk count.
    Workers,
}

impl ExecutorOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        match self {
            Self::New => (
                vec![Ty::U64, Ty::U64],
                crate::sum::result(Ty::Executor, crate::sum::pool_error()),
            ),
            Self::Submit(job, _) => (
                vec![Ty::Executor, job.input.clone()],
                crate::sum::result(
                    Ty::Future(Arc::new(job.output.clone())),
                    crate::sum::submit_error(job.input.clone()),
                ),
            ),
            Self::Map(job, input, fallible) => (
                std::iter::once(Ty::Executor)
                    .chain(job.shared.iter().map(|t| Ty::Closure(t.clone())))
                    .chain(std::iter::once(input.clone()))
                    .collect(),
                if *fallible {
                    let Ty::Enum(result) = &job.output else {
                        unreachable!()
                    };
                    crate::sum::result(
                        Ty::List(Arc::new(result.get().variants[0].fields[0].clone())),
                        crate::sum::parallel_error(result.get().variants[1].fields[0].clone()),
                    )
                } else {
                    crate::sum::result(
                        Ty::List(Arc::new(job.output.clone())),
                        crate::sum::pool_map_error(),
                    )
                },
            ),
            Self::Reduce(job, input) => (
                vec![Ty::Executor, input.clone()],
                crate::sum::result(
                    crate::sum::option(job.output.clone()),
                    crate::sum::pool_map_error(),
                ),
            ),
            Self::Result(t) => (
                vec![Ty::Future(Arc::new(t.clone()))],
                crate::sum::result(t.clone(), crate::sum::future_error()),
            ),
            Self::Done(t) | Self::Cancel(t) => (vec![Ty::Future(Arc::new(t.clone()))], Ty::Bool),
            Self::WaitTimeout(t) => (vec![Ty::Future(Arc::new(t.clone())), Ty::U64], Ty::Bool),
            Self::Shutdown => (vec![Ty::Executor, Ty::Bool], Ty::Unit),
            Self::Workers => (vec![Ty::Executor], Ty::I64),
        }
    }
    pub fn opcode(&self) -> i64 {
        match self {
            Self::New => 0,
            Self::Submit(_, false) => 1,
            Self::Submit(_, true) => 2,
            Self::Result(_) => 3,
            Self::Done(_) => 4,
            Self::Cancel(_) => 5,
            Self::Shutdown => 6,
            Self::Map(_, _, false) => 7,
            Self::Map(_, _, true) => 8,
            Self::Reduce(..) => 9,
            Self::WaitTimeout(_) => 10,
            Self::Workers => 11,
        }
    }

    pub fn job(&self) -> Option<&Arc<Job>> {
        match self {
            Self::Submit(job, _) | Self::Map(job, ..) | Self::Reduce(job, _) => Some(job),
            _ => None,
        }
    }
}
