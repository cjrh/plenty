//! Single-call byte transport between `ByteStorage` and files or the standard
//! streams, plus byte-offset seeking and the OS entropy read. Each primitive
//! performs at most one native read or write; retry, EOF and partial-progress
//! policy lives in `std.io` and `std.secrets`.
use crate::{op::Ty, sum};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StreamOp {
    FileRead,
    FileWrite,
    StdinRead,
    StdWrite,
    FileSeek,
    EntropyRead,
    Wait,
    // The operations above reading their `source` operand from a `bytes`
    // literal, which the runtime tells from a buffer by its header.
    FileWriteLiteral,
    StdWriteLiteral,
}

impl StreamOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__io_file_read" => Self::FileRead,
            "__io_file_write" => Self::FileWrite,
            "__io_stdin_read" => Self::StdinRead,
            "__io_std_write" => Self::StdWrite,
            "__io_file_seek" => Self::FileSeek,
            "__io_entropy_read" => Self::EntropyRead,
            "__io_wait" => Self::Wait,
            _ => return None,
        })
    }

    /// The operand the primitive only reads bytes from, which a `Bytes`
    /// buffer or a `bytes` literal can supply.
    pub fn source(self) -> Option<usize> {
        match self.buffer_form() {
            Self::FileWrite => Some(0),
            Self::StdWrite => Some(0),
            _ => None,
        }
    }

    /// The variant reading `source` from a `bytes` literal.
    pub fn literal(self) -> Option<Self> {
        Some(match self {
            Self::FileWrite => Self::FileWriteLiteral,
            Self::StdWrite => Self::StdWriteLiteral,
            _ => return None,
        })
    }

    fn buffer_form(self) -> Self {
        match self {
            Self::FileWriteLiteral => Self::FileWrite,
            Self::StdWriteLiteral => Self::StdWrite,
            other => other,
        }
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let form = self.buffer_form();
        if form != self {
            let (mut inputs, output) = form.signature();
            inputs[form.source().unwrap()] = Ty::ByteStr;
            return (inputs, output);
        }
        let range = || vec![Ty::ByteStorage, Ty::I64, Ty::I64];
        let inputs = match self {
            Self::FileRead | Self::FileWrite => [range(), vec![Ty::File]].concat(),
            Self::StdinRead | Self::EntropyRead => range(),
            Self::StdWrite => [range(), vec![Ty::U8]].concat(),
            Self::FileSeek => vec![Ty::File, Ty::U8, Ty::I64],
            // Interest mask and a timeout in nanoseconds; readiness only
            // observes the handle.
            Self::Wait => vec![Ty::File, Ty::U8, Ty::U64],
            Self::FileWriteLiteral | Self::StdWriteLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        };
        (inputs, sum::result(Ty::I64, sum::io_error()))
    }

    /// Operands the primitive writes through, which need exclusive loans. A
    /// file is always exclusive: transfers move its cursor.
    pub fn exclusive(self, operand: usize) -> bool {
        let form = self.buffer_form();
        if form != self {
            return form.exclusive(operand);
        }
        match self {
            Self::FileRead => operand == 0 || operand == 3,
            Self::FileWrite => operand == 3,
            Self::StdinRead | Self::FileSeek | Self::EntropyRead => operand == 0,
            Self::StdWrite | Self::Wait => false,
            Self::FileWriteLiteral | Self::StdWriteLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        }
    }

    pub fn opcode(self) -> i64 {
        let form = self.buffer_form();
        if form != self {
            return form.opcode();
        }
        match self {
            // Outside the contiguous stream block, which later opcodes follow.
            Self::EntropyRead => 248,
            Self::Wait => 390,
            _ => 188 + self as i64,
        }
    }
}
