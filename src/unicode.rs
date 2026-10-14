//! Allocation-free, versioned Unicode scalar table lookups.
use crate::op::Ty;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnicodeOp {
    CombiningClass,
    DecomposeAt,
    Compose,
    IsMark,
}

impl UnicodeOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__unicode_ccc" => Self::CombiningClass,
            "__unicode_decompose_at" => Self::DecomposeAt,
            "__unicode_compose" => Self::Compose,
            "__unicode_is_mark" => Self::IsMark,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        match self {
            Self::CombiningClass => (vec![Ty::U32], Ty::U32),
            Self::DecomposeAt => (vec![Ty::U32, Ty::Bool, Ty::I64], Ty::U32),
            Self::Compose => (vec![Ty::U32, Ty::U32], Ty::U32),
            Self::IsMark => (vec![Ty::U32], Ty::Bool),
        }
    }

    pub fn opcode(self) -> i64 {
        224 + self as i64
    }
}
