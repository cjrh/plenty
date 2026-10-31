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

/// Operations on `&substr` views (issue #595), which the runtime runs in
/// `aggregates/text_views.rs`. A view is the address of its first byte and
/// its byte count; positions count Unicode scalars, as on `str`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextViewOp {
    /// `&s[a:b]`: a reference to the string's slot, start, stop, and the
    /// written-bound flags. An inline string's bytes are in the slot itself.
    OfStr,
    /// `&v[a:b]`, with the operands of `OfStr`.
    Subview,
    /// The scalar count, which scans the view.
    Len,
    /// Two texts or views (`true` for a `str` operand) and their kind bits
    /// -> -1, 0, or 1 in scalar order.
    Compare(bool, bool),
    /// View, in-bounds byte offset -> the one-scalar string starting there
    /// (`Scalar`) or ending there (`ScalarBefore`), for iteration.
    Scalar,
    ScalarBefore,
    /// `v[i]`: the scalar at a position; out of range traps.
    Index,
    /// `v[:]?`: an owned copy.
    Copy,
    /// View, needle (`true` for a `str`), and the search mode operand of
    /// `TEXT_VIEW_SEARCH`; `find` returns `Option[i64]`, the rest `bool`.
    Search {
        needle_text: bool,
        find: bool,
    },
}

impl TextViewOp {
    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let view = Ty::View(Arc::new(Ty::Text), false);
        let operand = |text: bool| if text { Ty::Str } else { view.clone() };
        match self {
            Self::OfStr => (
                vec![Ty::Ref(Arc::new(Ty::Str), false), Ty::I64, Ty::I64, Ty::I64],
                view,
            ),
            Self::Subview => (vec![view.clone(), Ty::I64, Ty::I64, Ty::I64], view),
            Self::Len => (vec![view], Ty::I64),
            Self::Compare(a, b) => (vec![operand(a), operand(b), Ty::I64], Ty::I64),
            Self::Scalar | Self::ScalarBefore | Self::Index => (vec![view, Ty::I64], Ty::Str),
            Self::Copy => (
                vec![view],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            Self::Search { needle_text, find } => (
                vec![view.clone(), operand(needle_text), Ty::I64],
                if find {
                    crate::sum::option(Ty::I64)
                } else {
                    Ty::Bool
                },
            ),
        }
    }

    pub fn traps(self) -> bool {
        self == Self::Index
    }

    pub fn opcode(self) -> i64 {
        use crate::collection_opcodes::*;
        match self {
            Self::OfStr => TEXT_VIEW_OF_STR,
            Self::Subview => TEXT_VIEW_SUBVIEW,
            Self::Len => TEXT_VIEW_LEN,
            Self::Compare(..) => TEXT_VIEW_COMPARE,
            Self::Scalar => TEXT_VIEW_SCALAR,
            Self::ScalarBefore => TEXT_VIEW_SCALAR_BEFORE,
            Self::Index => TEXT_VIEW_INDEX,
            Self::Copy => TEXT_VIEW_COPY,
            Self::Search { .. } => TEXT_VIEW_SEARCH,
        }
    }
}
