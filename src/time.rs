//! Clock and sleep primitives; duration, deadline and timestamp policy lives in std.time.
use crate::op::Ty;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeOp {
    Monotonic,
    WallSeconds,
    WallNanoseconds,
    Sleep,
}

impl TimeOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__time_monotonic" => Self::Monotonic,
            "__time_wall_seconds" => Self::WallSeconds,
            "__time_wall_nanoseconds" => Self::WallNanoseconds,
            "__time_sleep" => Self::Sleep,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        match self {
            Self::Monotonic => (vec![], Ty::U64),
            Self::WallSeconds => (vec![], Ty::I64),
            Self::WallNanoseconds => (vec![], Ty::U32),
            Self::Sleep => (vec![Ty::U64], Ty::Unit),
        }
    }

    pub fn opcode(self) -> i64 {
        184 + self as i64
    }
}
