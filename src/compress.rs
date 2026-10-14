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

    pub fn signature(self) -> (Vec<Ty>, Ty) {
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
        }
    }

    /// Operands the primitive writes through: the codec state.
    pub fn exclusive(self, operand: usize) -> bool {
        match self {
            Self::New | Self::Ended | Self::Crc32 => false,
            Self::Step | Self::Flush | Self::Restart => operand == 0,
        }
    }

    pub fn opcode(self) -> i64 {
        320 + self as i64
    }
}
