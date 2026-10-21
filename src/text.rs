//! Allocation-free primitives over borrowed `str` bytes for standard-library
//! protocol code. Policy and the public API stay in the Plenty modules.
use crate::op::Ty;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextOp {
    AsciiEqualFolded,
    /// `__text_find_in(text, set, member)`: the scalar position of the first
    /// scalar that is (`member` 1) or is not (0) an ASCII byte of `set`, or
    /// -1. `set` is a `Bytes` buffer's storage; `FindInLiteral` reads a
    /// `bytes` literal. Operands are read through shared references, so the
    /// call retains nothing (issue #383).
    FindIn,
    FindInLiteral,
}

impl TextOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__text_ascii_equal_folded" => Self::AsciiEqualFolded,
            "__text_find_in" => Self::FindIn,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let shared = |ty| Ty::Ref(Arc::new(ty), false);
        match self {
            // Whether both texts are ASCII and equal ignoring ASCII case.
            Self::AsciiEqualFolded => (vec![Ty::Str, Ty::Str], Ty::Bool),
            Self::FindIn => (
                vec![shared(Ty::Str), shared(Ty::ByteStorage), Ty::U8],
                Ty::I64,
            ),
            Self::FindInLiteral => (vec![shared(Ty::Str), shared(Ty::ByteStr), Ty::U8], Ty::I64),
        }
    }

    pub fn opcode(self) -> i64 {
        match self {
            Self::AsciiEqualFolded => 366,
            Self::FindIn | Self::FindInLiteral => {
                unreachable!("text set scans use a narrow runtime helper")
            }
        }
    }
}
