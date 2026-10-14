//! Single-call byte transport between `ByteStorage` and files or the standard
//! streams, plus byte-offset seeking. Each primitive performs at most one
//! native read or write; retry, EOF and partial-progress policy lives in
//! `std.io`.
use crate::{op::Ty, sum};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StreamOp {
    FileRead,
    FileWrite,
    StdinRead,
    StdWrite,
    FileSeek,
}

impl StreamOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__io_file_read" => Self::FileRead,
            "__io_file_write" => Self::FileWrite,
            "__io_stdin_read" => Self::StdinRead,
            "__io_std_write" => Self::StdWrite,
            "__io_file_seek" => Self::FileSeek,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let range = || vec![Ty::ByteStorage, Ty::I64, Ty::I64];
        let inputs = match self {
            Self::FileRead | Self::FileWrite => [range(), vec![Ty::File]].concat(),
            Self::StdinRead => range(),
            Self::StdWrite => [range(), vec![Ty::U8]].concat(),
            Self::FileSeek => vec![Ty::File, Ty::U8, Ty::I64],
        };
        (inputs, sum::result(Ty::I64, sum::io_error()))
    }

    /// Operands the primitive writes through, which need exclusive loans. A
    /// file is always exclusive: transfers move its cursor.
    pub fn exclusive(self, operand: usize) -> bool {
        match self {
            Self::FileRead => operand == 0 || operand == 3,
            Self::FileWrite => operand == 3,
            Self::StdinRead | Self::FileSeek => operand == 0,
            Self::StdWrite => false,
        }
    }

    pub fn opcode(self) -> i64 {
        188 + self as i64
    }
}
