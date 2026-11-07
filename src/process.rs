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
    SignalListen,
    DuplicateFile,
    FileDescriptor,
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
            "__signal_listen" => Self::SignalListen,
            "__process_duplicate_file" => Self::DuplicateFile,
            "__process_file_descriptor" => Self::FileDescriptor,
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
            // The request, its argument count, stream codes and flags, and
            // the redirected files, then the explicit inherited-file allowlist.
            Self::Spawn => (
                vec![
                    bytes(),
                    Ty::I64,
                    Ty::I64,
                    Ty::List(Arc::new(Ty::File)),
                    Ty::List(Arc::new(Ty::File)),
                ],
                Ty::I64,
            ),
            Self::Pipe => (vec![Ty::I64, Ty::U8], Ty::File),
            Self::Wait => (vec![Ty::I64, Ty::Bool], Ty::I64),
            Self::Signal => (vec![Ty::I64, Ty::I64, Ty::Bool], Ty::Unit),
            Self::Poll => (vec![Ty::I64, Ty::I64, Ty::I64], Ty::I64),
            // The readable end on which each caught signal arrives as a byte.
            Self::SignalListen => (vec![], Ty::File),
            Self::DuplicateFile => (vec![Ty::File], Ty::File),
            Self::FileDescriptor => (vec![Ty::File], Ty::I32),
        };
        (inputs, sum::result(output, sum::io_error()))
    }

    /// 193–199, 228–229, then 500–502: the ranges between belong to other modules.
    pub fn opcode(self) -> i64 {
        match self as i64 {
            index @ 0..=6 => 193 + index,
            index @ 7..=8 => 228 + index - 7,
            index => 500 + index - 9,
        }
    }
}
