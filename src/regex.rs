//! Allocation-free scalar and text primitives behind `std.regex`. The parser,
//! compiler, Pike VM and every policy live in the Plenty module; these only
//! read Unicode properties and walk borrowed UTF-8 without copying it.
use crate::{op::Ty, sum};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RegexOp {
    Property,
    TextAt,
    TextByte,
    TextSlice,
    TextFind,
    Case,
}

impl RegexOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__regex_property" => Self::Property,
            "__regex_text_at" => Self::TextAt,
            "__regex_text_byte" => Self::TextByte,
            "__regex_text_slice" => Self::TextSlice,
            "__regex_text_find" => Self::TextFind,
            "__regex_case" => Self::Case,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        match self {
            Self::Property => (vec![Ty::U32, Ty::I64], Ty::Bool),
            // `code * 8 + width` of the scalar at a byte offset, -1 at the
            // end, or -2 when the offset is not a scalar boundary.
            Self::TextAt => (vec![Ty::Str, Ty::I64], Ty::I64),
            // The byte at an offset, or -1 outside the text.
            Self::TextByte => (vec![Ty::Str, Ty::I64], Ty::I64),
            Self::TextSlice => (
                vec![Ty::Str, Ty::I64, Ty::I64],
                sum::result(Ty::Str, sum::alloc_error()),
            ),
            // The first byte offset of a needle at or after a boundary, or -1.
            Self::TextFind => (vec![Ty::Str, Ty::Str, Ty::I64], Ty::I64),
            // The single-scalar lowercase (false) or uppercase (true) mapping.
            Self::Case => (vec![Ty::U32, Ty::Bool], Ty::U32),
        }
    }

    pub fn opcode(self) -> i64 {
        360 + self as i64
    }
}
