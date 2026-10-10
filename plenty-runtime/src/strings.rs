use crate::memory::{self, AllocError, Header};
use std::io::BufRead;
use std::{ptr, slice, str};

#[repr(C)]
pub(crate) struct Text {
    header: Header,
    byte_len: u64,
    scalar_len: u64,
    data: [u8; 0],
}
const _: () = assert!(std::mem::offset_of!(Text, data) == 32);
const _: () = assert!(std::mem::offset_of!(Text, byte_len) == 16);

/// Strings of at most this many UTF-8 bytes live in the value word itself:
/// the low byte holds `len << 1 | 1` and the next seven bytes hold the data.
/// Heap and literal Text pointers are 8-aligned, so their low bit is clear.
/// Inline values are never null, own nothing, and need no retain or release.
pub(crate) const INLINE_MAX: usize = 7;
const _: () = assert!(cfg!(target_endian = "little") && size_of::<usize>() == 8);

pub(crate) fn is_inline(text: *const Text) -> bool {
    text as usize & 1 != 0
}

fn pack(data: &[u8]) -> *mut Text {
    debug_assert!(data.len() <= INLINE_MAX);
    let mut word = [0u8; 8];
    word[0] = (data.len() as u8) << 1 | 1;
    word[1..=data.len()].copy_from_slice(data);
    usize::from_le_bytes(word) as *mut Text
}

pub(crate) unsafe fn byte_len(text: *const Text) -> u64 {
    if is_inline(text) {
        (text as usize as u64 & 0xff) >> 1
    } else {
        // SAFETY: a clear low bit means a live heap or literal Text.
        unsafe { (*text).byte_len }
    }
}

pub(crate) unsafe fn scalar_len(text: *const Text) -> u64 {
    if is_inline(text) {
        unsafe { utf8(&text).chars().count() as u64 }
    } else {
        unsafe { (*text).scalar_len }
    }
}

pub(crate) enum CStrError {
    EmbeddedNul,
    Allocation(AllocError),
}

/// Build a call-scoped C string using the normal text owner, including its
/// terminator in the allocation length so ordinary release frees it correctly.
pub(crate) unsafe fn try_c_string(text: *const Text) -> Result<*mut Text, CStrError> {
    // SAFETY: the compiler borrows a live immutable Text for this entire call.
    unsafe {
        let source = bytes(&text);
        if source.contains(&0) {
            return Err(CStrError::EmbeddedNul);
        }
        let size = add_length(byte_len(text), 1).map_err(CStrError::Allocation)?;
        let out = memory::try_allocate::<Text, u8>(size as usize).map_err(CStrError::Allocation)?;
        out.write(Text {
            header: Header::new(destroy),
            byte_len: size,
            scalar_len: scalar_len(text) + 1,
            data: [],
        });
        let data = ptr::addr_of_mut!((*out).data).cast::<u8>();
        ptr::copy_nonoverlapping(source.as_ptr(), data, source.len());
        data.add(source.len()).write(0);
        Ok(out)
    }
}

/// Inline data lives in the word, so the view borrows the word's storage.
pub(crate) unsafe fn bytes(text: &*const Text) -> &[u8] {
    // SAFETY: the caller keeps a heap text alive for the returned view's lifetime.
    unsafe {
        if is_inline(*text) {
            let word = (text as *const *const Text).cast::<u8>();
            return slice::from_raw_parts(word.add(1), byte_len(*text) as usize);
        }
        slice::from_raw_parts(
            ptr::addr_of!((**text).data).cast::<u8>(),
            (**text).byte_len as usize,
        )
    }
}
pub(crate) unsafe fn utf8(text: &*const Text) -> &str {
    // SAFETY: both new() and compiler literals validate UTF-8.
    unsafe { str::from_utf8_unchecked(bytes(text)) }
}
pub(crate) fn new(bytes: &[u8]) -> *mut Text {
    let text = str::from_utf8(bytes).unwrap_or_else(|_| crate::fail("invalid UTF-8 input"));
    try_new(text).unwrap_or_else(|_| crate::fail("string allocation failed"))
}

pub(crate) fn try_new(text: &str) -> Result<*mut Text, AllocError> {
    let bytes = text.as_bytes();
    if bytes.len() > i64::MAX as usize {
        return Err(AllocError::CapacityOverflow);
    }
    try_build(bytes.len() as u64, text.chars().count() as u64, |out| {
        out.copy_from_slice(bytes)
    })
}

/// Produce a string of known checked size. Short results are packed inline and
/// cannot fail; longer ones allocate once. `fill` writes exactly byte_len bytes.
fn try_build(
    byte_len: u64,
    scalar_len: u64,
    fill: impl FnOnce(&mut [u8]),
) -> Result<*mut Text, AllocError> {
    if byte_len as usize <= INLINE_MAX {
        let mut data = [0u8; INLINE_MAX];
        fill(&mut data[..byte_len as usize]);
        return Ok(pack(&data[..byte_len as usize]));
    }
    let out = memory::try_allocate::<Text, u8>(byte_len as usize)?;
    // SAFETY: the allocation has a header followed by exactly byte_len bytes.
    unsafe {
        out.write(Text {
            header: Header::new(destroy),
            byte_len,
            scalar_len,
            data: [],
        });
        fill(slice::from_raw_parts_mut(
            ptr::addr_of_mut!((*out).data).cast::<u8>(),
            byte_len as usize,
        ));
    }
    Ok(out)
}
unsafe extern "C" fn destroy(header: *mut Header) {
    // SAFETY: callback installed only on dynamically allocated Text objects.
    unsafe {
        let text = header.cast::<Text>();
        memory::free::<Text, u8>(text, (*text).byte_len as usize);
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_concat(a: *const Text, b: *const Text) -> *mut Text {
    unsafe { try_concat(a, b) }.unwrap_or_else(|_| crate::fail("string allocation failed"))
}

pub(crate) unsafe fn try_concat(a: *const Text, b: *const Text) -> Result<*mut Text, AllocError> {
    unsafe { try_join(None, [a, b].into_iter()) }
}

fn add_length(total: u64, length: u64) -> Result<u64, AllocError> {
    total
        .checked_add(length)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(AllocError::CapacityOverflow)
}

fn replacement_length(total: u64, old: u64, new: u64, count: u64) -> Result<u64, AllocError> {
    let removed = old.checked_mul(count).ok_or(AllocError::CapacityOverflow)?;
    let added = new.checked_mul(count).ok_or(AllocError::CapacityOverflow)?;
    let remaining = total
        .checked_sub(removed)
        .ok_or(AllocError::CapacityOverflow)?;
    add_length(remaining, added)
}

/// Literal, non-overlapping replacement with no intermediate buffer. Empty
/// patterns match Unicode-scalar boundaries, including the beginning and end.
pub(crate) unsafe fn try_replace(
    text: *const Text,
    old: *const Text,
    new: *const Text,
) -> Result<*mut Text, AllocError> {
    unsafe {
        let source = utf8(&text);
        let matches = source.match_indices(utf8(&old));
        let count = matches.clone().count() as u64;
        let total = replacement_length(byte_len(text), byte_len(old), byte_len(new), count)?;
        let scalars =
            replacement_length(scalar_len(text), scalar_len(old), scalar_len(new), count)?;
        let replacement = bytes(&new);
        try_build(total, scalars, |out| {
            let mut offset = 0;
            let mut append = |data: &[u8]| {
                out[offset..offset + data.len()].copy_from_slice(data);
                offset += data.len();
            };
            let mut previous = 0;
            for (index, matched) in matches {
                append(&source.as_bytes()[previous..index]);
                append(replacement);
                previous = index + matched.len();
            }
            append(&source.as_bytes()[previous..]);
            debug_assert_eq!(offset, total as usize);
        })
    }
}

/// Borrow UTF-8 texts in a stable sequence. Cloning the iterator must enumerate
/// the same live pointers; both passes run without callbacks or source mutation.
/// Allocate only the final output, after checking its complete size.
pub(crate) unsafe fn try_join(
    separator: Option<*const Text>,
    pieces: impl Clone + Iterator<Item = *const Text>,
) -> Result<*mut Text, AllocError> {
    unsafe {
        let (mut total, mut scalars) = (0, 0);
        let mut first = true;
        for piece in pieces.clone() {
            if !first {
                if let Some(separator) = separator {
                    total = add_length(total, byte_len(separator))?;
                    scalars = add_length(scalars, scalar_len(separator))?;
                }
            }
            total = add_length(total, byte_len(piece))?;
            scalars = add_length(scalars, scalar_len(piece))?;
            first = false;
        }
        try_build(total, scalars, |out| {
            let mut offset = 0;
            let mut append = |text: *const Text| {
                let data = bytes(&text);
                out[offset..offset + data.len()].copy_from_slice(data);
                offset += data.len();
            };
            let mut first = true;
            for piece in pieces {
                if !first {
                    if let Some(separator) = separator {
                        append(separator);
                    }
                }
                append(piece);
                first = false;
            }
            debug_assert_eq!(offset, total as usize);
        })
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_str_eq(a: *const Text, b: *const Text) -> i8 {
    unsafe { (a == b || bytes(&a) == bytes(&b)) as i8 }
}
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_contains(haystack: *const Text, needle: *const Text) -> i8 {
    unsafe { utf8(&haystack).contains(utf8(&needle)) as i8 }
}
#[no_mangle]
pub(crate) extern "C" fn plenty_readline() -> *mut Text {
    let mut line = Vec::new();
    match std::io::stdin().lock().read_until(b'\n', &mut line) {
        Ok(0) => return ptr::null_mut(),
        Ok(_) => {}
        Err(_) => return ptr::null_mut(),
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    }
    new(&line)
}

pub(crate) unsafe fn at(text: *const Text, index: i64) -> *mut Text {
    unsafe { scalar(text, index) }.unwrap_or_else(|| crate::fail("index out of bounds"))
}

/// Borrow a live UTF-8 string. A scalar is at most four UTF-8 bytes, so the
/// result is always inline: independent of the source and never allocated.
pub(crate) unsafe fn scalar(text: *const Text, index: i64) -> Option<*mut Text> {
    unsafe {
        let index = crate::aggregates::checked_index(index, scalar_len(text) as usize)?;
        let source = utf8(&text);
        if is_ascii(text) {
            return Some(pack(&source.as_bytes()[index..=index]));
        }
        let character = source.chars().nth(index).expect("validated string index");
        Some(pack(character.encode_utf8(&mut [0; 4]).as_bytes()))
    }
}

/// Byte and scalar counts agree only for ASCII, where scalar indices are byte
/// offsets. This keeps indexed loops linear without a per-string index.
unsafe fn is_ascii(text: *const Text) -> bool {
    unsafe { byte_len(text) == scalar_len(text) }
}

fn repeated_length(length: u64, count: i64) -> Result<u64, AllocError> {
    length
        .checked_mul(count.max(0) as u64)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(AllocError::CapacityOverflow)
}

pub(crate) unsafe fn try_repeat(text: *const Text, count: i64) -> Result<*mut Text, AllocError> {
    unsafe {
        let total = repeated_length(byte_len(text), count)?;
        let scalars = repeated_length(scalar_len(text), count)?;
        let source = bytes(&text);
        try_build(total, scalars, |out| {
            if out.is_empty() {
                return;
            }
            out[..source.len()].copy_from_slice(source);
            let mut filled = source.len();
            while filled < out.len() {
                // Doubling the initialized prefix avoids one copy per repeat.
                let count = filled.min(out.len() - filled);
                out.copy_within(..count, filled);
                filled += count;
            }
        })
    }
}

/// Trim Unicode White_Space at selected ends, then allocate only the final text.
pub(crate) unsafe fn try_strip(
    text: *const Text,
    left: bool,
    right: bool,
) -> Result<*mut Text, AllocError> {
    unsafe {
        let mut source = utf8(&text);
        if left {
            source = source.trim_start();
        }
        if right {
            source = source.trim_end();
        }
        try_new(source)
    }
}

/// Copy a forward Unicode-scalar slice directly from its UTF-8 byte interval.
pub(crate) unsafe fn try_slice(
    text: *const Text,
    start: i64,
    stop: i64,
) -> Result<*mut Text, AllocError> {
    unsafe {
        let bounds = crate::aggregates::slice_bounds(start, stop, scalar_len(text) as usize);
        if bounds.is_empty() {
            return try_new("");
        }
        let source = utf8(&text);
        if is_ascii(text) {
            return try_new(&source[bounds]);
        }
        let start = source.char_indices().nth(bounds.start).unwrap().0;
        let end = source[start..]
            .char_indices()
            .nth(bounds.len())
            .map_or(source.len(), |(offset, _)| start + offset);
        try_new(&source[start..end])
    }
}

pub(crate) unsafe fn at_byte(text: *const Text, offset: usize) -> *mut Text {
    unsafe {
        let s = utf8(&text);
        let tail = s
            .get(offset..)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| crate::fail("invalid string cursor"));
        let ch = tail.chars().next().unwrap();
        pack(&tail.as_bytes()[..ch.len_utf8()])
    }
}

/// Owned copies for runtime tests, which need no borrowed value word.
#[cfg(test)]
pub(crate) unsafe fn text(value: *const Text) -> String {
    unsafe { utf8(&value).to_owned() }
}
#[cfg(test)]
pub(crate) unsafe fn text_bytes(value: *const Text) -> Vec<u8> {
    unsafe { bytes(&value).to_vec() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_lengths_and_header_layout_are_checked_before_allocation() {
        assert_eq!(add_length(i64::MAX as u64 - 1, 1), Ok(i64::MAX as u64));
        assert_eq!(
            add_length(i64::MAX as u64, 1),
            Err(AllocError::CapacityOverflow)
        );
        assert_eq!(add_length(u64::MAX, 1), Err(AllocError::CapacityOverflow));
        assert_eq!(
            memory::try_allocate::<Text, u8>(i64::MAX as usize),
            Err(AllocError::CapacityOverflow)
        );
    }

    #[test]
    fn replacement_lengths_check_growth_and_allow_shrinking_at_the_limit() {
        assert_eq!(
            replacement_length(i64::MAX as u64, 2, 1, 1),
            Ok(i64::MAX as u64 - 1)
        );
        assert_eq!(replacement_length(0, 0, 1, 1), Ok(1));
        assert_eq!(
            replacement_length(i64::MAX as u64, 0, 1, 1),
            Err(AllocError::CapacityOverflow)
        );
        assert_eq!(
            replacement_length(0, 0, u64::MAX, 2),
            Err(AllocError::CapacityOverflow)
        );
        assert_eq!(
            replacement_length(1, 2, 0, 1),
            Err(AllocError::CapacityOverflow)
        );
        assert_eq!(
            replacement_length(0, u64::MAX, 0, 2),
            Err(AllocError::CapacityOverflow)
        );
    }

    #[test]
    fn repeated_lengths_check_overflow_and_empty_or_negative_counts() {
        assert_eq!(repeated_length(3, 5), Ok(15));
        assert_eq!(repeated_length(0, i64::MAX), Ok(0));
        assert_eq!(repeated_length(u64::MAX, i64::MIN), Ok(0));
        assert_eq!(
            repeated_length(2, i64::MAX),
            Err(AllocError::CapacityOverflow)
        );
        assert_eq!(
            repeated_length(u64::MAX, 3),
            Err(AllocError::CapacityOverflow)
        );
    }

    #[test]
    fn short_strings_are_inline_and_long_strings_allocate() {
        unsafe {
            let ascii_text = try_new("hello, world").unwrap();
            let mixed = try_new("héllo wörld").unwrap();
            assert!(!is_inline(ascii_text) && !is_inline(mixed));
            for (index, expected) in [(1, "e"), (-1, "d"), (0, "h")] {
                let character = scalar(ascii_text, index).unwrap();
                assert!(is_inline(character));
                assert_eq!(text(character), expected);
            }
            for (index, expected) in [(1, "é"), (7, "ö"), (-1, "d")] {
                let character = scalar(mixed, index).unwrap();
                assert!(is_inline(character));
                assert_eq!(text(character), expected);
                assert_eq!(scalar_len(character), 1);
            }
            assert_eq!(scalar(mixed, 11), None);
            let emoji = try_new("🙂").unwrap();
            assert!(is_inline(emoji));
            assert_eq!((byte_len(emoji), scalar_len(emoji)), (4, 1));
            let seven = try_new("é\0🙂").unwrap();
            assert!(is_inline(seven));
            assert_eq!((byte_len(seven), scalar_len(seven)), (7, 3));
            let eight = try_concat(seven, scalar(ascii_text, 0).unwrap()).unwrap();
            assert!(!is_inline(eight));
            assert_eq!(text(eight), "é\0🙂h");
            assert_eq!(text(try_slice(ascii_text, 1, 4).unwrap()), "ell");
            assert_eq!(text(try_slice(mixed, 1, 4).unwrap()), "éll");
            assert!(is_inline(try_new("").unwrap()));
            assert_eq!(
                plenty_str_eq(
                    try_new("hel").unwrap(),
                    try_slice(ascii_text, 0, 3).unwrap()
                ),
                1
            );
            let c = match try_c_string(emoji) {
                Ok(c) => c,
                Err(_) => panic!("C string"),
            };
            assert!(!is_inline(c));
            assert_eq!(bytes(&(c as *const Text)), "🙂\0".as_bytes());
        }
    }
}
