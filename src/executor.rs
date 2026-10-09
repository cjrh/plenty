//! Checked executor operations and statically certified job adapters.
use crate::op::Ty;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub adapter: String,
    pub worker: String,
    pub input: Ty,
    pub output: Ty,
    pub closure: Option<Rc<crate::closure::ClosureType>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExecutorOp {
    New,
    Submit(Rc<Job>, bool),
    Map(Rc<Job>, Ty),
    Result(Ty),
    Done(Ty),
    Cancel(Ty),
    Shutdown,
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
                    Ty::Future(Rc::new(job.output.clone())),
                    crate::sum::submit_error(job.input.clone()),
                ),
            ),
            Self::Map(job, input) => (
                vec![Ty::Executor, input.clone()],
                crate::sum::result(
                    Ty::List(Rc::new(job.output.clone())),
                    crate::sum::pool_map_error(),
                ),
            ),
            Self::Result(t) => (
                vec![Ty::Future(Rc::new(t.clone()))],
                crate::sum::result(t.clone(), crate::sum::future_error()),
            ),
            Self::Done(t) | Self::Cancel(t) => (vec![Ty::Future(Rc::new(t.clone()))], Ty::Bool),
            Self::Shutdown => (vec![Ty::Executor, Ty::Bool], Ty::Unit),
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
            Self::Map(..) => 7,
        }
    }
}
