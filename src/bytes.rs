//! Narrow native primitives over contiguous byte storage. Range validation,
//! error policy, and the public API live in `std.bytes`; the runtime repeats
//! each bound check and stops the process if a caller skipped one.
use crate::{op::Ty, sum};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BytesOp {
    New,
    Len,
    Capacity,
    Get,
    Set,
    Reserve,
    Resize,
    Push,
    Append,
    Overwrite,
    FromList,
    ToList,
    FromText,
    ToText,
    WriteFloat,
}

impl BytesOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__bytes_new" => Self::New,
            "__bytes_len" => Self::Len,
            "__bytes_capacity" => Self::Capacity,
            "__bytes_get" => Self::Get,
            "__bytes_set" => Self::Set,
            "__bytes_reserve" => Self::Reserve,
            "__bytes_resize" => Self::Resize,
            "__bytes_push" => Self::Push,
            "__bytes_append" => Self::Append,
            "__bytes_overwrite" => Self::Overwrite,
            "__bytes_from_list" => Self::FromList,
            "__bytes_to_list" => Self::ToList,
            "__bytes_from_text" => Self::FromText,
            "__bytes_to_text" => Self::ToText,
            "__bytes_write_float" => Self::WriteFloat,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let storage = || Ty::ByteStorage;
        let list = || Ty::List(Arc::new(Ty::U8));
        let (inputs, output) = match self {
            Self::Len | Self::Capacity => return (vec![storage()], Ty::I64),
            Self::Get => return (vec![storage(), Ty::I64], Ty::U8),
            Self::Set => return (vec![storage(), Ty::I64, Ty::U8], Ty::Unit),
            Self::WriteFloat => return (vec![storage(), Ty::F64], Ty::I64),
            Self::Overwrite => {
                return (
                    vec![storage(), Ty::I64, storage(), Ty::I64, Ty::I64],
                    Ty::Unit,
                )
            }
            Self::New => (vec![Ty::I64], storage()),
            Self::Reserve => (vec![storage(), Ty::I64], Ty::Unit),
            Self::Resize => (vec![storage(), Ty::I64, Ty::U8], Ty::Unit),
            Self::Push => (vec![storage(), Ty::U8], Ty::Unit),
            Self::Append => (vec![storage(), storage(), Ty::I64, Ty::I64], Ty::Unit),
            Self::FromList => (vec![list()], storage()),
            Self::ToList => (vec![storage()], list()),
            Self::FromText => (vec![Ty::Str], storage()),
            Self::ToText => (vec![storage()], sum::option(Ty::Str)),
        };
        (inputs, sum::result(output, sum::alloc_error()))
    }

    /// The first operand is written, so it needs an exclusive loan.
    pub fn mutable(self) -> bool {
        matches!(
            self,
            Self::Set
                | Self::Reserve
                | Self::Resize
                | Self::Push
                | Self::Append
                | Self::Overwrite
                | Self::WriteFloat
        )
    }

    pub fn opcode(self) -> i64 {
        match self {
            Self::WriteFloat => 280,
            _ => 170 + self as i64,
        }
    }
}
