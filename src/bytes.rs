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
    // Bulk scans: read-only, infallible, one runtime call per range.
    Find,
    FindIn,
    FindBytes,
    Count,
    Matches,
    MatchesText,
    Run,
    // 237-239: bulk fill, then indexed access with list index rules.
    Fill,
    Item,
    SetItem,
    // `Item` on a temporary buffer value rather than a named place.
    ValueItem,
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
            "__bytes_find" => Self::Find,
            "__bytes_find_in" => Self::FindIn,
            "__bytes_find_bytes" => Self::FindBytes,
            "__bytes_count" => Self::Count,
            "__bytes_matches" => Self::Matches,
            "__bytes_matches_text" => Self::MatchesText,
            "__bytes_run" => Self::Run,
            "__bytes_fill" => Self::Fill,
            "__bytes_item" => Self::Item,
            "__bytes_set_item" => Self::SetItem,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let storage = || Ty::ByteStorage;
        let storage_ref = |mutable| Ty::Ref(Arc::new(Ty::ByteStorage), mutable);
        let list = || Ty::List(Arc::new(Ty::U8));
        let (inputs, output) = match self {
            Self::Len | Self::Capacity => return (vec![storage()], Ty::I64),
            Self::Get => return (vec![storage(), Ty::I64], Ty::U8),
            Self::Set => return (vec![storage(), Ty::I64, Ty::U8], Ty::Unit),
            // Index syntax reads the owner through a borrowed field reference.
            Self::Item => return (vec![storage_ref(false), Ty::I64], Ty::U8),
            Self::ValueItem => return (vec![storage(), Ty::I64], Ty::U8),
            Self::SetItem => return (vec![storage_ref(true), Ty::I64, Ty::U8], Ty::Unit),
            Self::Fill => return (vec![storage(), Ty::I64, Ty::I64, Ty::U8], Ty::Unit),
            Self::WriteFloat => return (vec![storage(), Ty::F64], Ty::I64),
            Self::Overwrite => {
                return (
                    vec![storage(), Ty::I64, storage(), Ty::I64, Ty::I64],
                    Ty::Unit,
                )
            }
            // A found index, or -1 for none.
            Self::Find | Self::Count => {
                return (vec![storage(), Ty::U8, Ty::I64, Ty::I64], Ty::I64)
            }
            // The last operand selects members (1) or non-members (0).
            Self::FindIn => {
                return (
                    vec![storage(), storage(), Ty::I64, Ty::I64, Ty::U8],
                    Ty::I64,
                )
            }
            Self::FindBytes => return (vec![storage(), storage(), Ty::I64, Ty::I64], Ty::I64),
            Self::Matches => {
                return (
                    vec![storage(), Ty::I64, storage(), Ty::I64, Ty::I64],
                    Ty::Bool,
                )
            }
            Self::MatchesText => return (vec![storage(), Ty::I64, Ty::Str], Ty::Bool),
            // The last operand selects ASCII (0) or valid UTF-8 (1).
            Self::Run => return (vec![storage(), Ty::I64, Ty::I64, Ty::U8], Ty::I64),
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
                | Self::SetItem
                | Self::Fill
                | Self::Reserve
                | Self::Resize
                | Self::Push
                | Self::Append
                | Self::Overwrite
                | Self::WriteFloat
        )
    }

    /// Indexed access traps at the source position on a missing index.
    pub fn traps(self) -> bool {
        matches!(self, Self::Item | Self::SetItem | Self::ValueItem)
    }

    pub fn opcode(self) -> i64 {
        // The scans, fill, and indexed access sit in their own range (230-239);
        // 184 onward belongs to others.
        match self {
            Self::WriteFloat => 280,
            Self::Fill => 237,
            Self::Item | Self::ValueItem => 238,
            Self::SetItem => 239,
            _ => match (self as i64).checked_sub(Self::Find as i64) {
                Some(scan) if scan >= 0 => 230 + scan,
                _ => 170 + self as i64,
            },
        }
    }
}
