//! Shared cancellation state. Cancellation is explicit and cooperative.
use crate::op::Ty;

#[derive(Clone, Debug, PartialEq)]
pub enum ControlOp {
    New,
    Cancel,
    IsCancelled,
    Wait,
    WaitTimeout,
}

impl ControlOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        match self {
            Self::New => (
                vec![],
                crate::sum::result(Ty::CancellationToken, crate::sum::alloc_error()),
            ),
            Self::Cancel | Self::Wait => (vec![Ty::CancellationToken], Ty::Unit),
            Self::IsCancelled => (vec![Ty::CancellationToken], Ty::Bool),
            Self::WaitTimeout => (vec![Ty::CancellationToken, Ty::U64], Ty::Bool),
        }
    }
    pub fn opcode(&self) -> i64 {
        match self {
            Self::New => 0,
            Self::Cancel => 1,
            Self::IsCancelled => 2,
            Self::Wait => 3,
            Self::WaitTimeout => 4,
        }
    }
}
