//! Environment reads and child-process primitives. Argument-vector encoding,
//! pipe draining, status decoding and reaping policy live in `std.env` and
//! `std.process`; each primitive here is one OS request.
use crate::{op::Ty, sum};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProcessOp {
    EnvGet,
    EnvVars,
    CurrentDir,
    ChangeDir,
    Spawn,
    Pipe,
    Wait,
    Signal,
    Poll,
}

impl ProcessOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__env_get" => Self::EnvGet,
            "__env_vars" => Self::EnvVars,
            "__env_current_dir" => Self::CurrentDir,
            "__env_change_dir" => Self::ChangeDir,
            "__process_spawn" => Self::Spawn,
            "__process_pipe" => Self::Pipe,
            "__process_wait" => Self::Wait,
            "__process_signal" => Self::Signal,
            "__process_poll" => Self::Poll,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let bytes = || Ty::List(Arc::new(Ty::U8));
        let (inputs, output) = match self {
            Self::EnvGet => (vec![bytes()], sum::option(bytes())),
            Self::EnvVars => (vec![], Ty::List(Arc::new(bytes()))),
            Self::CurrentDir => (vec![], bytes()),
            Self::ChangeDir => (vec![bytes()], Ty::Unit),
            Self::Spawn => (vec![bytes(), Ty::I64, Ty::I64], Ty::I64),
            Self::Pipe => (vec![Ty::I64, Ty::U8], Ty::File),
            Self::Wait => (vec![Ty::I64, Ty::Bool], Ty::I64),
            Self::Signal => (vec![Ty::I64, Ty::I64], Ty::Unit),
            Self::Poll => (vec![Ty::I64, Ty::I64, Ty::I64], Ty::I64),
        };
        (inputs, sum::result(output, sum::io_error()))
    }

    /// 193–199, then 228 onward: the range between belongs to other modules.
    pub fn opcode(self) -> i64 {
        match self as i64 {
            index @ 0..=6 => 193 + index,
            index => 228 + index - 7,
        }
    }
}
