//! Allocation-free primitives over borrowed `str` bytes for standard-library
//! protocol code. Policy and the public API stay in the Plenty modules.
use crate::op::Ty;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextOp {
    AsciiEqualFolded,
}

impl TextOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__text_ascii_equal_folded" => Self::AsciiEqualFolded,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        match self {
            // Whether both texts are ASCII and equal ignoring ASCII case.
            Self::AsciiEqualFolded => (vec![Ty::Str, Ty::Str], Ty::Bool),
        }
    }

    pub fn opcode(self) -> i64 {
        366 + self as i64
    }
}
