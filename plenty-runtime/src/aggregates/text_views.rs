//! `&substr` views, `&s[a:b]` (issue #595): a borrowed run of a string's
//! UTF-8 bytes. A view is two words, the first byte's address and the byte
//! count, like a `&[u8]`. The compiler holds a loan of the viewed string for
//! the view's whole life, so the bytes stay in place. Positions count Unicode
//! scalars, as `s[a:b]?` does; a view's ends always fall on scalar
//! boundaries. See book/src/design/language/04-expressions/slices.md.
use super::{fallible, index};
use crate::collection_opcodes::{
    SLICE_START, SLICE_STOP, TEXT_SEARCH_CONTAINS, TEXT_SEARCH_ENDS, TEXT_SEARCH_FIND,
    TEXT_SEARCH_STARTS, TEXT_VIEW_COMPARE, TEXT_VIEW_COPY, TEXT_VIEW_INDEX, TEXT_VIEW_LEN,
    TEXT_VIEW_OF_STR, TEXT_VIEW_SCALAR, TEXT_VIEW_SCALAR_BEFORE, TEXT_VIEW_SEARCH,
    TEXT_VIEW_SUBVIEW,
};
use crate::strings::{self, Text};
use std::cmp::Ordering;

/// The bytes of a view. An empty view's address may be dangling.
///
/// # Safety
/// `view` must be a live view: the compiler keeps its string borrowed.
pub(crate) unsafe fn bytes<'a>(view: u128) -> &'a [u8] {
    match (view >> 64) as usize {
        0 => &[],
        // SAFETY: as documented above.
        count => unsafe { std::slice::from_raw_parts(view as u64 as *const u8, count) },
    }
}

/// The view's text. Views start and end on scalar boundaries of valid UTF-8.
///
/// # Safety
/// As for [`bytes`].
pub(crate) unsafe fn text<'a>(view: u128) -> &'a str {
    // SAFETY: as documented above.
    unsafe { std::str::from_utf8_unchecked(bytes(view)) }
}

fn pack(bytes: &[u8]) -> u128 {
    bytes.as_ptr() as u64 as u128 | (bytes.len() as u128) << 64
}

/// The byte offset of scalar position `bound`, clamped into the text as a
/// step-1 slice bound is: a negative bound counts from the end. Finding it
/// scans only the scalars before (or after) the position.
fn boundary(text: &str, bound: i64) -> usize {
    if bound >= 0 {
        text.char_indices()
            .nth(bound as usize)
            .map_or(text.len(), |(at, _)| at)
    } else {
        text.char_indices()
            .rev()
            .nth(bound.unsigned_abs() as usize - 1)
            .map_or(0, |(at, _)| at)
    }
}

/// The scalars from `start` to `stop` of `text`, with the written-bound
/// `flags`; empty rather than out of range.
fn select(text: &str, start: i64, stop: i64, flags: i64) -> &str {
    let first = if flags & SLICE_START != 0 {
        boundary(text, start)
    } else {
        0
    };
    let end = if flags & SLICE_STOP != 0 {
        boundary(text, stop)
    } else {
        text.len()
    };
    &text[first..end.max(first)]
}

/// The bytes of a text or view operand; `text` says which. A text word is
/// copied into `word`, which holds an inline string's bytes.
unsafe fn operand(value: u128, text: bool, word: &*const Text) -> &[u8] {
    // SAFETY: the compiler keeps both kinds of operand live for the call.
    unsafe {
        if text {
            strings::bytes(word)
        } else {
            bytes(value)
        }
    }
}

/// One scalar starting at byte `at`, as an inline one-scalar string.
fn scalar(text: &[u8], at: usize) -> u128 {
    let width = match text[at] {
        0..0x80 => 1,
        0xc0..0xe0 => 2,
        0xe0..0xf0 => 3,
        _ => 4,
    };
    strings::pack(&text[at..at + width]) as u128
}

/// Runs text-view operation `op` on `operands`.
///
/// # Safety
/// The compiler passes each opcode's declared operands: views it keeps
/// borrowed, string words, references to string slots, and integers.
pub(crate) unsafe fn call(op: i64, [a, b, c, d, _]: [u128; 5], site: *const crate::Site) -> u128 {
    // SAFETY: as documented above.
    unsafe {
        match op {
            TEXT_VIEW_OF_STR => {
                // The string's slot stays borrowed, so an inline string's
                // bytes stay in it.
                let slot = &*(a as u64 as *const *const Text);
                pack(select(strings::utf8(slot), b as i64, c as i64, d as i64).as_bytes())
            }
            TEXT_VIEW_SUBVIEW => pack(select(text(a), b as i64, c as i64, d as i64).as_bytes()),
            TEXT_VIEW_LEN => text(a).chars().count() as u128,
            TEXT_VIEW_COMPARE => {
                let (left, right) = (a as u64 as *const Text, b as u64 as *const Text);
                let left = operand(a, c & 1 != 0, &left);
                let right = operand(b, c & 2 != 0, &right);
                // UTF-8 byte order is Unicode scalar order.
                match left.cmp(right) {
                    Ordering::Less => -1i64 as u64 as u128,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                }
            }
            TEXT_VIEW_SCALAR => scalar(bytes(a), b as usize),
            TEXT_VIEW_SCALAR_BEFORE => {
                let source = bytes(a);
                let mut at = b as usize - 1;
                while source[at] & 0xc0 == 0x80 {
                    at -= 1;
                }
                scalar(source, at)
            }
            TEXT_VIEW_INDEX => {
                let source = text(a);
                let position = index(b as i64, source.chars().count(), site);
                let (at, _) = source.char_indices().nth(position).expect("indexed scalar");
                scalar(source.as_bytes(), at)
            }
            TEXT_VIEW_COPY => fallible(strings::try_new(text(a)).map(|t| t as u128)),
            TEXT_VIEW_SEARCH => {
                let needle = b as u64 as *const Text;
                let needle = operand(b, c & 1 != 0, &needle);
                let source = bytes(a);
                match c as i64 >> 1 {
                    TEXT_SEARCH_STARTS => source.starts_with(needle) as u128,
                    TEXT_SEARCH_ENDS => source.ends_with(needle) as u128,
                    TEXT_SEARCH_FIND => {
                        let (source, needle) = (text(a), std::str::from_utf8_unchecked(needle));
                        match source.find(needle) {
                            None => super::wrap(0, 0),
                            Some(at) => super::wrap(source[..at].chars().count() as u128, 1),
                        }
                    }
                    TEXT_SEARCH_CONTAINS => {
                        text(a).contains(std::str::from_utf8_unchecked(needle)) as u128
                    }
                    _ => crate::fail("invalid text search"),
                }
            }
            _ => crate::fail("invalid text view operation"),
        }
    }
}
