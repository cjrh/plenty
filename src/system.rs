//! CPU availability query; the public policy and error mapping live in std.system.
use crate::op::Ty;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SystemOp {
    AvailableParallelism,
}

impl SystemOp {
    pub fn named(name: &str) -> Option<Self> {
        (name == "__system_available_parallelism").then_some(Self::AvailableParallelism)
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        (vec![], Ty::I64)
    }

    pub fn opcode(self) -> i64 {
        640
    }
}
