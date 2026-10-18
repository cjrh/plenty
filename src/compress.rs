//! Native primitives behind `std.compress`: zlib-rs DEFLATE, zlib and gzip
//! codecs in the runtime. Buffering, limits and the error policy live in
//! `std.compress` and `std.bytes`.
use crate::{op::Ty, sum};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompressOp {
    New,
    Step,
    Flush,
    Ended,
    Restart,
    Crc32,
    // The operations above reading their `source` operand from a `bytes`
    // literal, which the runtime tells from a buffer by its header.
    StepLiteral,
}

impl CompressOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__compress_new" => Self::New,
            "__compress_step" => Self::Step,
            "__compress_flush" => Self::Flush,
            "__compress_ended" => Self::Ended,
            "__compress_restart" => Self::Restart,
            "__compress_crc32" => Self::Crc32,
            _ => return None,
        })
    }

    /// The operand the primitive only reads bytes from, which a `Bytes`
    /// buffer or a `bytes` literal can supply.
    pub fn source(self) -> Option<usize> {
        match self.buffer_form() {
            Self::Step => Some(1),
            _ => None,
        }
    }

    /// The variant reading `source` from a `bytes` literal.
    pub fn literal(self) -> Option<Self> {
        Some(match self {
            Self::Step => Self::StepLiteral,
            _ => return None,
        })
    }

    fn buffer_form(self) -> Self {
        match self {
            Self::StepLiteral => Self::Step,
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
        let storage = || Ty::ByteStorage;
        match self {
            // Kind, level, output capacity.
            Self::New => (
                vec![Ty::U8, Ty::I64, Ty::I64],
                sum::result(storage(), sum::alloc_error()),
            ),
            // Bytes consumed, or a negative failure code.
            Self::Step => (vec![storage(), storage(), Ty::I64, Ty::I64], Ty::I64),
            // 1 once complete, 0 while output is full, or a negative code.
            Self::Flush => (vec![storage(), Ty::I64], Ty::I64),
            Self::Ended | Self::Restart => (vec![storage()], Ty::Bool),
            Self::Crc32 => (vec![Ty::I64, storage(), Ty::I64, Ty::I64], Ty::I64),
            Self::StepLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        }
    }

    /// Operands the primitive writes through: the codec state.
    pub fn exclusive(self, operand: usize) -> bool {
        let form = self.buffer_form();
        if form != self {
            return form.exclusive(operand);
        }
        match self {
            Self::New | Self::Ended | Self::Crc32 => false,
            Self::Step | Self::Flush | Self::Restart => operand == 0,
            Self::StepLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        }
    }

    pub fn opcode(self) -> i64 {
        let form = self.buffer_form();
        if form != self {
            return form.opcode();
        }
        320 + self as i64
    }
}
