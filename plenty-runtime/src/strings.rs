use crate::memory::{self, AllocError, Header};
use std::io::BufRead;
use std::{ptr, slice, str};

#[repr(C)]
pub(crate) struct Text {
    header: Header,
    pub byte_len: u64,
    pub scalar_len: u64,
    data: [u8; 0],
}
const _: () = assert!(std::mem::offset_of!(Text, data) == 32);

pub(crate) unsafe fn bytes<'a>(text: *const Text) -> &'a [u8] {
    // SAFETY: the caller keeps text alive for the returned view's lifetime.
    unsafe {
        slice::from_raw_parts(
            ptr::addr_of!((*text).data).cast::<u8>(),
            (*text).byte_len as usize,
        )
    }
}
pub(crate) unsafe fn utf8<'a>(text: *const Text) -> &'a str {
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
    let pointer = memory::try_allocate::<Text, u8>(bytes.len())?;
    // SAFETY: the allocation has a header followed by exactly bytes.len() bytes.
    unsafe {
        pointer.write(Text {
            header: Header::new(destroy),
            byte_len: bytes.len() as u64,
            scalar_len: text.chars().count() as u64,
            data: [],
        });
        ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            ptr::addr_of_mut!((*pointer).data).cast::<u8>(),
            bytes.len(),
        );
    }
    Ok(pointer)
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
        let source = utf8(text);
        let matches = source.match_indices(utf8(old));
        let count = matches.clone().count() as u64;
        let byte_len =
            replacement_length((*text).byte_len, (*old).byte_len, (*new).byte_len, count)?;
        let scalar_len = replacement_length(
            (*text).scalar_len,
            (*old).scalar_len,
            (*new).scalar_len,
            count,
        )?;
        let out = memory::try_allocate::<Text, u8>(byte_len as usize)?;
        out.write(Text {
            header: Header::new(destroy),
            byte_len,
            scalar_len,
            data: [],
        });
        let mut offset = 0;
        let mut append = |data: &[u8]| {
            ptr::copy_nonoverlapping(
                data.as_ptr(),
                ptr::addr_of_mut!((*out).data).cast::<u8>().add(offset),
                data.len(),
            );
            offset += data.len();
        };
        let mut previous = 0;
        for (index, matched) in matches {
            append(&source.as_bytes()[previous..index]);
            append(bytes(new));
            previous = index + matched.len();
        }
        append(&source.as_bytes()[previous..]);
        debug_assert_eq!(offset, byte_len as usize);
        Ok(out)
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
        let (mut byte_len, mut scalar_len) = (0, 0);
        let mut first = true;
        for piece in pieces.clone() {
            if !first {
                if let Some(separator) = separator {
                    byte_len = add_length(byte_len, (*separator).byte_len)?;
                    scalar_len = add_length(scalar_len, (*separator).scalar_len)?;
                }
            }
            byte_len = add_length(byte_len, (*piece).byte_len)?;
            scalar_len = add_length(scalar_len, (*piece).scalar_len)?;
            first = false;
        }
        let out = memory::try_allocate::<Text, u8>(byte_len as usize)?;
        out.write(Text {
            header: Header::new(destroy),
            byte_len,
            scalar_len,
            data: [],
        });
        let mut offset = 0;
        let mut append = |text| {
            let data = bytes(text);
            ptr::copy_nonoverlapping(
                data.as_ptr(),
                ptr::addr_of_mut!((*out).data).cast::<u8>().add(offset),
                data.len(),
            );
            offset += data.len();
        };
        first = true;
        for piece in pieces {
            if !first {
                if let Some(separator) = separator {
                    append(separator);
                }
            }
            append(piece);
            first = false;
        }
        debug_assert_eq!(offset, byte_len as usize);
        Ok(out)
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_str_eq(a: *const Text, b: *const Text) -> i8 {
    unsafe { (a == b || bytes(a) == bytes(b)) as i8 }
}
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_contains(haystack: *const Text, needle: *const Text) -> i8 {
    unsafe { utf8(haystack).contains(utf8(needle)) as i8 }
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
    unsafe { try_get(text, index) }
        .unwrap_or_else(|_| crate::fail("string allocation failed"))
        .unwrap_or_else(|| crate::fail("index out of bounds"))
}

/// Borrow a live UTF-8 string. Missing indices need no allocation; a present
/// scalar is copied into its own string so it outlives the source owner.
pub(crate) unsafe fn try_get(
    text: *const Text,
    index: i64,
) -> Result<Option<*mut Text>, AllocError> {
    unsafe {
        let Some(index) = crate::aggregates::checked_index(index, (*text).scalar_len as usize)
        else {
            return Ok(None);
        };
        let character = utf8(text)
            .chars()
            .nth(index)
            .expect("validated string index");
        try_new(character.encode_utf8(&mut [0; 4])).map(Some)
    }
}
/// Trim Unicode White_Space at selected ends, then allocate only the final text.
pub(crate) unsafe fn try_strip(
    text: *const Text,
    left: bool,
    right: bool,
) -> Result<*mut Text, AllocError> {
    unsafe {
        let mut source = utf8(text);
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
        let bounds = crate::aggregates::slice_bounds(start, stop, (*text).scalar_len as usize);
        if bounds.is_empty() {
            return try_new("");
        }
        let source = utf8(text);
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
        let s = utf8(text);
        let tail = s
            .get(offset..)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| crate::fail("invalid string cursor"));
        let ch = tail.chars().next().unwrap();
        new(&tail.as_bytes()[..ch.len_utf8()])
    }
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
}
