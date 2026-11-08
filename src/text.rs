//! Allocation-free primitives over borrowed `str` bytes for standard-library
//! protocol code. Policy and the public API stay in the Plenty modules.
use crate::collection::CollectionOp;
use crate::op::Ty;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextOp {
    AsciiEqualFolded,
    /// `__text_find_in(text, set, member)`: the scalar position of the first
    /// scalar that is (`member` 1) or is not (0) an ASCII byte of `set`, or
    /// -1. `set` is a `&[u8]` view. The text is read through a shared
    /// reference, so the call retains nothing (issue #383).
    FindIn,
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
                vec![shared(Ty::Str), crate::bytes::byte_view(), Ty::U8],
                Ty::I64,
            ),
        }
    }

    pub fn opcode(self) -> i64 {
        match self {
            Self::AsciiEqualFolded => 366,
            Self::FindIn => unreachable!("text set scans use a narrow runtime helper"),
        }
    }
}

/// Operations on `&strview` views (issue #595), which the runtime runs in
/// `aggregates/text_views.rs`. A view is the address of its first byte and
/// its byte count; positions count Unicode scalars, as on `str`.
#[derive(Clone, Debug, PartialEq)]
pub enum TextViewOp {
    /// An existing text builtin with borrowed UTF-8 range operands. Its result
    /// ownership, metadata and error contract remain those of the builtin.
    Apply(Arc<CollectionOp>),
    /// `&s[a:b]`: a reference to the string's slot, start, stop, and the
    /// written-bound flags. An inline string's bytes are in the slot itself.
    OfStr,
    /// `&v[a:b]`, with the operands of `OfStr`.
    Subview,
    /// The scalar count, which scans the view.
    Len,
    /// The same address/count as a shared byte view; lowered without a call.
    Bytes,
    /// First scalar position in (or outside) an ASCII byte set, or -1.
    FindIn,
    /// ASCII-only equality with case folding, for protocol readers.
    AsciiEqualFolded,
    /// An owned copy of the complete range.
    Copy,
    /// Two texts or views (`true` for a `str` operand) and their kind bits
    /// -> -1, 0, or 1 in scalar order.
    Compare(bool, bool),
    /// View, in-bounds byte offset -> the one-scalar string starting there
    /// (`Scalar`) or ending there (`ScalarBefore`), for iteration.
    Scalar,
    ScalarBefore,
    /// `v[i]`: the scalar at a position; out of range traps.
    Index,
    /// View, needle (`true` for a `str`), and the search mode operand of
    /// `TEXT_VIEW_SEARCH`; `find` returns `Option[i64]`, the rest `bool`.
    Search {
        needle_text: bool,
        find: bool,
    },
}

impl TextViewOp {
    pub fn apply(operation: CollectionOp) -> CollectionOp {
        assert!(
            matches!(
                operation,
                CollectionOp::TextTryConcat
                    | CollectionOp::TextTryJoin(_)
                    | CollectionOp::TextTrySplit
                    | CollectionOp::TextTrySplitWhitespace
                    | CollectionOp::TextTrySplitLines
                    | CollectionOp::TextGet
                    | CollectionOp::TextTrySlice
                    | CollectionOp::TextSlice
                    | CollectionOp::TextTryReplace
                    | CollectionOp::TextStartsWith
                    | CollectionOp::TextEndsWith
                    | CollectionOp::TextFind
                    | CollectionOp::TextRFind
                    | CollectionOp::TextCount
                    | CollectionOp::TextTryStrip
                    | CollectionOp::TextTryLStrip
                    | CollectionOp::TextTryRStrip
                    | CollectionOp::TextTryRepeat
                    | CollectionOp::TextTryRemovePrefix
                    | CollectionOp::TextTryRemoveSuffix
                    | CollectionOp::TextIsAscii
                    | CollectionOp::TextIsSpace
                    | CollectionOp::TextIsDigit
                    | CollectionOp::TextIsAlpha
                    | CollectionOp::TextIsAlnum
                    | CollectionOp::TextTryLower
                    | CollectionOp::TextTryUpper
                    | CollectionOp::TextOrd
                    | CollectionOp::ParseNumber(_)
            ),
            "only implemented text range builtins may be wrapped"
        );
        CollectionOp::TextView(Self::Apply(Arc::new(operation)))
    }

    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        let view = Ty::View(Arc::new(Ty::Text), false);
        let operand = |text: bool| if text { Ty::Str } else { view.clone() };
        match self {
            Self::Apply(operation) => {
                let (inputs, output) = operation.signature();
                (
                    inputs
                        .into_iter()
                        .map(|ty| if ty == Ty::Str { view.clone() } else { ty })
                        .collect(),
                    output,
                )
            }
            Self::OfStr => (
                vec![Ty::Ref(Arc::new(Ty::Str), false), Ty::I64, Ty::I64, Ty::I64],
                view,
            ),
            Self::Subview => (vec![view.clone(), Ty::I64, Ty::I64, Ty::I64], view),
            Self::Len => (vec![view], Ty::I64),
            Self::Bytes => (vec![view], crate::bytes::byte_view()),
            Self::FindIn => (vec![view, crate::bytes::byte_view(), Ty::U8], Ty::I64),
            Self::AsciiEqualFolded => (vec![view.clone(), view], Ty::Bool),
            Self::Copy => (
                vec![view],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            Self::Compare(a, b) => (vec![operand(*a), operand(*b), Ty::I64], Ty::I64),
            Self::Scalar | Self::ScalarBefore | Self::Index => (vec![view, Ty::I64], Ty::Str),
            Self::Search { needle_text, find } => (
                vec![view.clone(), operand(*needle_text), Ty::I64],
                if *find {
                    crate::sum::option(Ty::I64)
                } else {
                    Ty::Bool
                },
            ),
        }
    }

    pub fn traps(&self) -> bool {
        match self {
            Self::Apply(operation) => operation.traps(),
            Self::Index => true,
            _ => false,
        }
    }

    pub fn opcode(&self) -> i64 {
        use crate::collection_opcodes::*;
        match self {
            Self::Apply(operation) => TEXT_RANGE_BASE + operation.opcode(),
            Self::OfStr => TEXT_VIEW_OF_STR,
            Self::Subview => TEXT_VIEW_SUBVIEW,
            Self::Len => TEXT_VIEW_LEN,
            Self::FindIn => TEXT_VIEW_FIND_IN,
            Self::AsciiEqualFolded => TEXT_VIEW_ASCII_EQUAL_FOLDED,
            Self::Copy => TEXT_VIEW_COPY,
            Self::Bytes => unreachable!("text byte views lower directly"),
            Self::Compare(..) => TEXT_VIEW_COMPARE,
            Self::Scalar => TEXT_VIEW_SCALAR,
            Self::ScalarBefore => TEXT_VIEW_SCALAR_BEFORE,
            Self::Index => TEXT_VIEW_INDEX,
            Self::Search { .. } => TEXT_VIEW_SEARCH,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_wrappers_preserve_results_effects_traps_and_native_arity() {
        let operations = [
            CollectionOp::TextTryConcat,
            CollectionOp::TextTryJoin(Ty::Str),
            CollectionOp::TextTryJoin(Ty::List(Arc::new(Ty::Str))),
            CollectionOp::TextTrySplit,
            CollectionOp::TextGet,
            CollectionOp::TextTryReplace,
            CollectionOp::TextTryLower,
            CollectionOp::TextSlice,
            CollectionOp::TextOrd,
            CollectionOp::ParseNumber(Ty::U8),
            CollectionOp::ParseNumber(Ty::F64),
        ];
        for original in operations {
            let wrapped = TextViewOp::apply(original.clone());
            let (inputs, output) = original.signature();
            let (range_inputs, range_output) = wrapped.signature();
            assert_eq!(range_output, output, "{original:?}");
            assert_eq!(inputs.len(), range_inputs.len());
            for (input, range_input) in inputs.into_iter().zip(range_inputs) {
                assert_eq!(
                    range_input,
                    if input == Ty::Str {
                        crate::frontend::text_view()
                    } else {
                        input
                    }
                );
            }
            assert_eq!(wrapped.traps(), original.traps());
            assert_eq!(wrapped.effect(), original.effect());
            assert_eq!(
                wrapped.opcode(),
                crate::collection_opcodes::TEXT_RANGE_BASE + original.opcode()
            );
            assert_eq!(
                crate::collection_opcodes::scratch_operand_count(wrapped.opcode()),
                crate::collection_opcodes::scratch_operand_count(original.opcode())
            );
        }
    }

    #[test]
    #[should_panic(expected = "only implemented text range builtins may be wrapped")]
    fn unsupported_operations_cannot_acquire_a_range_abi() {
        TextViewOp::apply(CollectionOp::OpenFile);
    }
}
