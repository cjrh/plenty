use crate::memory::{self, AllocError, Header};
use std::mem::MaybeUninit;
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

pub(crate) fn pack(data: &[u8]) -> *mut Text {
    debug_assert!(data.len() <= INLINE_MAX);
    let len = data.len();
    // Two fixed-width reads that overlap in the middle cover each length
    // without a per-byte loop or a variable-length memcpy call (issue #931).
    let payload = match len {
        0 => 0,
        1 => u64::from(data[0]),
        2 | 3 => {
            let (first, last) = ([data[0], data[1]], [data[len - 2], data[len - 1]]);
            u64::from(u16::from_le_bytes(first))
                | u64::from(u16::from_le_bytes(last)) << (8 * (len - 2))
        }
        _ => {
            let first = [data[0], data[1], data[2], data[3]];
            let last = [data[len - 4], data[len - 3], data[len - 2], data[len - 1]];
            u64::from(u32::from_le_bytes(first))
                | u64::from(u32::from_le_bytes(last)) << (8 * (len - 4))
        }
    };
    (payload << 8 | (len as u64) << 1 | 1) as usize as *mut Text
}

/// Whether every data byte of an inline word is ASCII; the tag byte always is.
fn inline_ascii(word: *mut Text) -> bool {
    word as usize as u64 & 0x8080_8080_8080_8080 == 0
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
        // The header, the copied bytes and the terminator fill the whole object.
        let out = memory::try_allocate_uninit::<Text, u8>(size as usize)
            .map_err(CStrError::Allocation)?;
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

/// Build one private buffer containing a pointer table and terminated bytes.
/// Its Text header is used only for ownership/release, never text operations.
/// `get` must return the same live immutable text on both passes.
pub(crate) unsafe fn try_c_strings(
    count: usize,
    get: impl Fn(usize) -> *const Text,
) -> Result<*mut Text, CStrError> {
    let overflow = || CStrError::Allocation(AllocError::CapacityOverflow);
    let table_bytes = count
        .checked_add(1)
        .and_then(|n| n.checked_mul(size_of::<*const u8>()))
        .ok_or_else(overflow)?;
    let mut size = table_bytes;
    // Validate before allocation, so even invalid late elements need no rollback.
    for index in 0..count {
        let text = get(index);
        // SAFETY: caller keeps every text alive and unchanged for both passes.
        let source = unsafe { bytes(&text) };
        if source.contains(&0) {
            return Err(CStrError::EmbeddedNul);
        }
        size = size
            .checked_add(source.len())
            .and_then(|n| n.checked_add(1))
            .ok_or_else(overflow)?;
    }
    // SAFETY: layout arithmetic is checked by the allocator; data is 8-aligned,
    // sufficient for the target's pointers. No fallible work follows allocation.
    unsafe {
        let out = memory::try_allocate_uninit::<Text, u8>(size).map_err(CStrError::Allocation)?;
        out.write(Text {
            header: Header::new(destroy),
            byte_len: size as u64,
            scalar_len: 0,
            data: [],
        });
        let data = ptr::addr_of_mut!((*out).data).cast::<u8>();
        let table = data.cast::<*const u8>();
        let mut offset = table_bytes;
        for index in 0..count {
            let text = get(index);
            let source = bytes(&text);
            let target = data.add(offset);
            table.add(index).write(target);
            ptr::copy_nonoverlapping(source.as_ptr(), target, source.len());
            target.add(source.len()).write(0);
            offset += source.len() + 1;
        }
        table.add(count).write(ptr::null());
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
/// Whether two live texts hold the same bytes. Equal words are one inline
/// value or one object, which answers a dictionary hit on a short key
/// without reading either text (issue #931). Unequal words still compare
/// bytes: a literal and an inline text may hold the same short string.
#[inline]
pub(crate) unsafe fn equal(a: *const Text, b: *const Text) -> bool {
    // SAFETY: the caller keeps both texts alive.
    a == b || unsafe { bytes(&a) == bytes(&b) }
}

/// Whether two texts that are not inline hold the same bytes. Eight or more
/// bytes compare as whole words and one overlapping last word; no length
/// calls `memcmp` (issue #931).
#[inline(always)]
pub(crate) unsafe fn heap_bytes_equal(a: *const Text, b: *const Text) -> bool {
    // SAFETY: the caller keeps both heap or literal texts alive.
    unsafe { bytes_equal(heap_bytes(a), heap_bytes(b)) }
}

/// Slice equality as `heap_bytes_equal` compares it, for a key's bytes that
/// are not yet a text (issue #949).
#[inline(always)]
pub(crate) fn bytes_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    match (a.last_chunk::<8>(), b.last_chunk::<8>()) {
        (Some(a_last), Some(b_last)) => {
            let (a_words, b_words) = (a.as_chunks::<8>().0, b.as_chunks::<8>().0);
            a_words
                .iter()
                .zip(b_words)
                .all(|(a, b)| u64::from_ne_bytes(*a) == u64::from_ne_bytes(*b))
                && a_last == b_last
        }
        // A loop of at most seven bytes costs less than the call.
        _ => a.iter().zip(b).all(|(a, b)| a == b),
    }
}

/// The data of a text that is never inline, such as a compiler literal, for
/// as long as the text lives.
pub(crate) unsafe fn heap_bytes<'a>(text: *const Text) -> &'a [u8] {
    debug_assert!(!is_inline(text));
    // SAFETY: the caller passes a live heap or literal text.
    unsafe {
        slice::from_raw_parts(
            ptr::addr_of!((*text).data).cast::<u8>(),
            (*text).byte_len as usize,
        )
    }
}
pub(crate) unsafe fn utf8(text: &*const Text) -> &str {
    // SAFETY: both new() and compiler literals validate UTF-8.
    unsafe { str::from_utf8_unchecked(bytes(text)) }
}
/// Whether both texts are ASCII and equal ignoring ASCII case. Folding maps
/// only ASCII letters, so an ASCII `left` equal to `right` makes `right`
/// ASCII too.
///
/// # Safety
/// Both words must be live texts borrowed for the call.
pub(crate) unsafe fn ascii_equal_folded(left: *const Text, right: *const Text) -> bool {
    // SAFETY: the compiler passes borrowed text words that outlive the call;
    // inline views borrow the parameters, which live until return.
    let (one, other) = unsafe { (bytes(&left), bytes(&right)) };
    one.is_ascii() && one.eq_ignore_ascii_case(other)
}
#[cfg(test)]
pub(crate) fn new(bytes: &[u8]) -> *mut Text {
    let text = str::from_utf8(bytes).unwrap_or_else(|_| crate::fail("invalid UTF-8 input"));
    try_new(text).unwrap_or_else(|_| crate::fail("string allocation failed"))
}

pub(crate) fn try_new(text: &str) -> Result<*mut Text, AllocError> {
    let bytes = text.as_bytes();
    if bytes.len() <= INLINE_MAX {
        return Ok(pack(bytes));
    }
    // An ASCII check reads a word at a time and gives the count directly.
    let scalars = if bytes.is_ascii() {
        bytes.len()
    } else {
        text.chars().count()
    };
    try_copy(bytes, scalars)
}

/// The inline string of at most `INLINE_MAX` ASCII bytes. Inlined into a
/// caller, it answers short keys without `try_from_utf8`'s frame
/// (issue #931).
#[inline]
pub(crate) fn inline_ascii_text(bytes: &[u8]) -> Option<*mut Text> {
    if bytes.len() > INLINE_MAX {
        return None;
    }
    let word = pack(bytes);
    inline_ascii(word).then_some(word)
}

/// A new string holding `bytes`, or None when they are not strictly valid
/// UTF-8. ASCII, the common case, needs neither full validation nor a separate
/// scalar count.
pub(crate) fn try_from_utf8(bytes: &[u8]) -> Option<Result<*mut Text, AllocError>> {
    if bytes.len() <= INLINE_MAX {
        let word = pack(bytes);
        return (inline_ascii(word) || str::from_utf8(bytes).is_ok()).then_some(Ok(word));
    }
    if bytes.is_ascii() {
        return Some(try_copy(bytes, bytes.len()));
    }
    let text = str::from_utf8(bytes).ok()?;
    Some(try_copy(bytes, text.chars().count()))
}

/// Copy UTF-8 `bytes` holding `scalars` scalars into a new string.
fn try_copy(bytes: &[u8], scalars: usize) -> Result<*mut Text, AllocError> {
    if bytes.len() > i64::MAX as usize {
        return Err(AllocError::CapacityOverflow);
    }
    try_build(bytes.len() as u64, scalars as u64, |out| {
        out.write_copy_of_slice(bytes);
    })
}

/// Copy `data` into `out` at `offset` and return the offset after it.
fn put(out: &mut [MaybeUninit<u8>], offset: usize, data: &[u8]) -> usize {
    let end = offset + data.len();
    out[offset..end].write_copy_of_slice(data);
    end
}

pub(crate) fn from_codepoint(code: u32) -> Option<*mut Text> {
    char::from_u32(code).map(|character| {
        let mut bytes = [0; 4];
        pack(character.encode_utf8(&mut bytes).as_bytes())
    })
}

pub(crate) unsafe fn try_case(text: *const Text, upper: bool) -> Result<*mut Text, AllocError> {
    // SAFETY: the caller lends a live string word for this call.
    try_case_range(unsafe { utf8(&text) }, upper)
}

pub(crate) fn try_case_range(source: &str, upper: bool) -> Result<*mut Text, AllocError> {
    if upper {
        try_characters(source.chars().flat_map(char::to_uppercase))
    } else {
        try_characters(source.chars().flat_map(char::to_lowercase))
    }
}

fn try_characters(characters: impl Iterator<Item = char> + Clone) -> Result<*mut Text, AllocError> {
    let (mut bytes, mut scalars) = (0, 0);
    for character in characters.clone() {
        bytes = add_length(bytes, character.len_utf8() as u64)?;
        scalars = add_length(scalars, 1)?;
    }
    try_build(bytes, scalars, |out| {
        let mut offset = 0;
        for character in characters {
            offset = put(out, offset, character.encode_utf8(&mut [0; 4]).as_bytes());
        }
    })
}

/// Produce a string of known checked size. Short results are packed inline and
/// cannot fail; longer ones allocate once, without zeroing. `fill` must
/// initialize every one of the byte_len bytes: the result reads them all.
fn try_build(
    byte_len: u64,
    scalar_len: u64,
    fill: impl FnOnce(&mut [MaybeUninit<u8>]),
) -> Result<*mut Text, AllocError> {
    if byte_len as usize <= INLINE_MAX {
        let mut data = [MaybeUninit::new(0u8); INLINE_MAX];
        fill(&mut data[..byte_len as usize]);
        // SAFETY: the buffer starts zeroed and `fill` stores only bytes.
        let data = data.map(|byte| unsafe { byte.assume_init() });
        return Ok(pack(&data[..byte_len as usize]));
    }
    let out = memory::try_allocate_uninit::<Text, u8>(byte_len as usize)?;
    // SAFETY: the allocation has a header followed by exactly byte_len bytes.
    unsafe {
        out.write(Text {
            header: Header::new(destroy),
            byte_len,
            scalar_len,
            data: [],
        });
        fill(slice::from_raw_parts_mut(
            ptr::addr_of_mut!((*out).data).cast::<MaybeUninit<u8>>(),
            byte_len as usize,
        ));
    }
    Ok(out)
}
/// A new owner holding the same text, for a copy that must not share storage
/// with another actor heap.
pub(crate) unsafe fn try_duplicate(text: *const Text) -> Result<*mut Text, AllocError> {
    // SAFETY: the caller supplies a live heap Text.
    unsafe {
        let source = bytes(&text);
        try_build(byte_len(text), scalar_len(text), |out| {
            out.write_copy_of_slice(source);
        })
    }
}
/// Release one owned reference and free the text at once if it was the last:
/// a string owns nothing, so it needs no place in the destruction queue.
pub(crate) unsafe fn release_leaf(text: *mut Text) {
    unsafe {
        let header = text.cast::<Header>();
        if !memory::inert(header) && (*header).release_last() {
            destroy(header);
        }
    }
}
unsafe extern "C" fn destroy(header: *mut Header) {
    // SAFETY: callback installed only on dynamically allocated Text objects.
    unsafe {
        let text = header.cast::<Text>();
        memory::free::<Text, u8>(text, (*text).byte_len as usize);
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_concat(a: *const Text, b: *const Text) -> *mut Text {
    unsafe { try_concat(a, b) }.unwrap_or_else(|_| crate::fail("string allocation failed"))
}

pub(crate) unsafe fn try_concat(a: *const Text, b: *const Text) -> Result<*mut Text, AllocError> {
    unsafe { try_join(None, [a, b].into_iter()) }
}

pub(crate) fn try_concat_range(a: &str, b: &str) -> Result<*mut Text, AllocError> {
    let bytes = add_length(a.len() as u64, b.len() as u64)?;
    let scalars = add_length(a.chars().count() as u64, b.chars().count() as u64)?;
    try_build(bytes, scalars, |out| {
        let offset = put(out, 0, a.as_bytes());
        put(out, offset, b.as_bytes());
    })
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
    // SAFETY: the caller keeps all three owning words live through the call.
    unsafe {
        replace_range(
            utf8(&text),
            utf8(&old),
            utf8(&new),
            (scalar_len(text), scalar_len(old), scalar_len(new)),
        )
    }
}

pub(crate) fn try_replace_range(
    source: &str,
    old: &str,
    new: &str,
) -> Result<*mut Text, AllocError> {
    replace_range(
        source,
        old,
        new,
        (
            source.chars().count() as u64,
            old.chars().count() as u64,
            new.chars().count() as u64,
        ),
    )
}

fn replace_range(
    source: &str,
    old: &str,
    new: &str,
    scalars: (u64, u64, u64),
) -> Result<*mut Text, AllocError> {
    let matches = source.match_indices(old);
    let count = matches.clone().count() as u64;
    let total = replacement_length(
        source.len() as u64,
        old.len() as u64,
        new.len() as u64,
        count,
    )?;
    let scalars = replacement_length(scalars.0, scalars.1, scalars.2, count)?;
    let replacement = new.as_bytes();
    try_build(total, scalars, |out| {
        let mut offset = 0;
        let mut append = |data: &[u8]| offset = put(out, offset, data);
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

/// Borrow UTF-8 texts in a stable sequence. Cloning the iterator must enumerate
/// the same live pointers; both passes run without callbacks or source mutation.
/// Allocate only the final output, after checking its complete size.
pub(crate) unsafe fn try_join(
    separator: Option<*const Text>,
    pieces: impl Clone + Iterator<Item = *const Text>,
) -> Result<*mut Text, AllocError> {
    // SAFETY: every word remains live for both passes; inline separator bytes
    // borrow the stable local Option payload for this synchronous call.
    unsafe {
        let between = separator.as_ref().map_or("", |word| utf8(word));
        let scalars = separator.map_or(0, |word| scalar_len(word));
        join_range(between, scalars, pieces)
    }
}

pub(crate) unsafe fn try_join_range(
    separator: &str,
    pieces: impl Clone + Iterator<Item = *const Text>,
) -> Result<*mut Text, AllocError> {
    // SAFETY: the caller keeps every piece word live for both passes.
    unsafe { join_range(separator, separator.chars().count() as u64, pieces) }
}

unsafe fn join_range(
    separator: &str,
    separator_scalars: u64,
    pieces: impl Clone + Iterator<Item = *const Text>,
) -> Result<*mut Text, AllocError> {
    // SAFETY: each iterator pass yields live borrowed owning string words.
    unsafe {
        let (mut total, mut scalars) = (0, 0);
        let mut first = true;
        for piece in pieces.clone() {
            if !first {
                total = add_length(total, separator.len() as u64)?;
                scalars = add_length(scalars, separator_scalars)?;
            }
            total = add_length(total, byte_len(piece))?;
            scalars = add_length(scalars, scalar_len(piece))?;
            first = false;
        }
        try_build(total, scalars, |out| {
            let mut offset = 0;
            let mut first = true;
            for piece in pieces {
                if !first {
                    offset = put(out, offset, separator.as_bytes());
                }
                offset = put(out, offset, bytes(&piece));
                first = false;
            }
            debug_assert_eq!(offset, total as usize);
        })
    }
}

/// Insert the separator between consecutive Unicode scalars, as iterating the
/// text yields them. The size is known before the one output allocation.
pub(crate) unsafe fn try_join_scalars(
    separator: *const Text,
    text: *const Text,
) -> Result<*mut Text, AllocError> {
    // SAFETY: both owning words stay live throughout the synchronous build.
    unsafe {
        join_scalars_range(
            utf8(&separator),
            utf8(&text),
            scalar_len(separator),
            scalar_len(text),
        )
    }
}

pub(crate) fn try_join_scalars_range(separator: &str, text: &str) -> Result<*mut Text, AllocError> {
    join_scalars_range(
        separator,
        text,
        separator.chars().count() as u64,
        text.chars().count() as u64,
    )
}

fn join_scalars_range(
    separator: &str,
    source: &str,
    separator_scalars: u64,
    source_scalars: u64,
) -> Result<*mut Text, AllocError> {
    let gaps = source_scalars.saturating_sub(1);
    let repeated = |length: u64| length.checked_mul(gaps).ok_or(AllocError::CapacityOverflow);
    let total = add_length(source.len() as u64, repeated(separator.len() as u64)?)?;
    let scalars = add_length(source_scalars, repeated(separator_scalars)?)?;
    try_build(total, scalars, |out| {
        let mut offset = 0;
        for (index, character) in source.char_indices() {
            if index != 0 {
                offset = put(out, offset, separator.as_bytes());
            }
            let length = character.len_utf8();
            offset = put(out, offset, &source.as_bytes()[index..index + length]);
        }
        debug_assert_eq!(offset, total as usize);
    })
}

/// A generator cannot be measured without consuming it. Its pieces are
/// appended to a scratch buffer whose capacity at least doubles when it is
/// full, then copied into one exact-size text; the buffer is freed. Each owned
/// piece is released once copied. Failure stops the generator where it is, and
/// its owner drops the remaining frame.
pub(crate) unsafe fn try_join_generator(
    separator: *const Text,
    generator: *mut crate::generators::Generator,
) -> Result<*mut Text, AllocError> {
    // SAFETY: the caller lends the separator and owns the generator frame.
    unsafe { join_generator_range(utf8(&separator), scalar_len(separator), generator) }
}

pub(crate) unsafe fn try_join_generator_range(
    separator: &str,
    generator: *mut crate::generators::Generator,
) -> Result<*mut Text, AllocError> {
    // SAFETY: the caller lends the separator and owns the generator frame.
    unsafe { join_generator_range(separator, separator.chars().count() as u64, generator) }
}

unsafe fn join_generator_range(
    separator: &str,
    separator_scalars: u64,
    generator: *mut crate::generators::Generator,
) -> Result<*mut Text, AllocError> {
    // SAFETY: the compiler supplies a live owned generator of str values;
    // each yielded owner is released after its bytes enter the scratch buffer.
    unsafe {
        let mut buffer = Vec::<u8>::new();
        let (mut total, mut scalars) = (0, 0);
        let mut append = |data: &[u8], scalar_count: u64| -> Result<(), AllocError> {
            total = add_length(total, data.len() as u64)?;
            scalars = add_length(scalars, scalar_count)?;
            memory::try_reserve_amortized(&mut buffer, data.len())?;
            buffer.extend_from_slice(data);
            Ok(())
        };
        let mut first = true;
        loop {
            let mut item = 0;
            if crate::generators::plenty_generator_resume(generator, &mut item) == 0 {
                break;
            }
            let piece = item as *const Text;
            let appended = if first {
                append(bytes(&piece), scalar_len(piece))
            } else {
                append(separator.as_bytes(), separator_scalars)
                    .and_then(|()| append(bytes(&piece), scalar_len(piece)))
            };
            memory::plenty_release(piece as *mut Header);
            appended?;
            first = false;
        }
        try_build(total, scalars, |out| {
            out.write_copy_of_slice(&buffer);
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_str_eq(a: *const Text, b: *const Text) -> i8 {
    unsafe { (a == b || bytes(&a) == bytes(&b)) as i8 }
}
/// Dispatch for a large `match` on string literals (issue #565): the index of
/// `text` among `len` distinct literals, or `len` when none is equal.
///
/// The compiler-built table holds `len + 1` native-endian `u32` byte offsets,
/// then the literals' bytes back to back; literal `i` spans offsets `i` to
/// `i + 1`. Literals are ordered by byte length, then bytes, so most probes
/// stop after comparing lengths. The table holds no addresses, so it needs no
/// load-time relocation.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_str_match(
    text: *const Text,
    table: *const u32,
    len: u64,
) -> u64 {
    // SAFETY: the compiler keeps `text` alive through the call and passes an
    // immortal, 4-aligned table in the layout above.
    unsafe {
        let key = bytes(&text);
        let count = len as usize;
        let offsets = slice::from_raw_parts(table, count + 1);
        let data = table.add(count + 1).cast::<u8>();
        let (mut low, mut high) = (0, count);
        while low < high {
            let middle = low + (high - low) / 2;
            let start = offsets[middle] as usize;
            let literal =
                slice::from_raw_parts(data.add(start), offsets[middle + 1] as usize - start);
            match literal.len().cmp(&key.len()).then_with(|| literal.cmp(key)) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return middle as u64,
            }
        }
        len
    }
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_contains(haystack: *const Text, needle: *const Text) -> i8 {
    unsafe { utf8(&haystack).contains(utf8(&needle)) as i8 }
}

// Narrow ABI for scalar text queries (issue #364): a borrowed text word in a
// register, no ownership transfer, and no dispatcher argument block. Results
// match the dispatcher's string operations; a scalar result is always inline.

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_len(text: *const Text) -> u64 {
    // SAFETY: the compiler keeps the text alive through the call.
    unsafe { scalar_len(text) }
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_byte_len(text: *const Text) -> u64 {
    // SAFETY: as for `plenty_text_len`.
    unsafe { byte_len(text) }
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_is_ascii(text: *const Text) -> u64 {
    // SAFETY: as for `plenty_text_len`.
    unsafe { bytes(&text).is_ascii() as u64 }
}
/// `text[i]`: the scalar at index `i`, negatives from the end.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_at(
    text: *const Text,
    i: i64,
    site: *const crate::Site,
) -> *mut Text {
    // SAFETY: as for `plenty_text_len`.
    unsafe { at(text, i, site) }
}
/// `ord(text)`: the code point of a text holding exactly one scalar, or
/// `u64::MAX`, which the compiled caller turns into `Err(ParseError)`
/// (issue #379).
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_ord(text: *const Text) -> u64 {
    // SAFETY: as for `plenty_text_len`.
    let mut scalars = unsafe { utf8(&text) }.chars();
    match (scalars.next(), scalars.next()) {
        (Some(scalar), None) => u64::from(scalar),
        _ => u64::MAX,
    }
}
// Text searches over two borrowed texts (issue #387). The dispatcher's
// opcodes 48 to 52 call these too.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_starts_with(
    text: *const Text,
    prefix: *const Text,
) -> u64 {
    // SAFETY: as for `plenty_text_len`, for both texts.
    unsafe { bytes(&text).starts_with(bytes(&prefix)) as u64 }
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_ends_with(
    text: *const Text,
    suffix: *const Text,
) -> u64 {
    // SAFETY: as for `plenty_text_len`, for both texts.
    unsafe { bytes(&text).ends_with(bytes(&suffix)) as u64 }
}
/// The scalar index of the first match of `needle`, or `u64::MAX`, which the
/// compiled caller turns into `None`.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_find(text: *const Text, needle: *const Text) -> u64 {
    // SAFETY: as for `plenty_text_len`, for both texts.
    let (source, needle) = unsafe { (utf8(&text), utf8(&needle)) };
    source
        .find(needle)
        .map_or(u64::MAX, |index| scalar_count(source, index))
}
/// As for `plenty_text_find`, the last match.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_rfind(text: *const Text, needle: *const Text) -> u64 {
    // SAFETY: as for `plenty_text_len`, for both texts.
    let (source, needle) = unsafe { (utf8(&text), utf8(&needle)) };
    source
        .rfind(needle)
        .map_or(u64::MAX, |index| scalar_count(source, index))
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_count(text: *const Text, needle: *const Text) -> u64 {
    // SAFETY: as for `plenty_text_len`, for both texts.
    let (source, needle) = unsafe { (utf8(&text), utf8(&needle)) };
    source.matches(needle).count() as u64
}
/// The number of scalars before byte `index`, a boundary of `source`.
fn scalar_count(source: &str, index: usize) -> u64 {
    source[..index].chars().count() as u64
}
/// A `bytes` literal's byte at `i`, with the list index rules. The literal
/// is immortal data in the text layout.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_byte_str_item(
    literal: *const Text,
    i: i64,
    site: *const crate::Site,
) -> u64 {
    // SAFETY: the literal is static data that is never freed.
    let data = unsafe { bytes(&literal) };
    u64::from(data[crate::aggregates::index(i, data.len(), site)])
}
/// The scalar starting at a valid byte cursor of forward iteration.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_at_byte(text: *const Text, offset: u64) -> *mut Text {
    // SAFETY: as for `plenty_text_len`.
    unsafe { at_byte(text, offset as usize) }
}
/// The cursor after the scalar starting at a valid byte cursor.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_next_byte(text: *const Text, offset: u64) -> u64 {
    // SAFETY: as for `plenty_text_len`.
    let tail = unsafe { &utf8(&text)[offset as usize..] };
    offset + tail.chars().next().expect("valid string cursor").len_utf8() as u64
}
/// The cursor before the scalar ending at a positive byte cursor of
/// reverse iteration.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_prev_byte(text: *const Text, offset: u64) -> u64 {
    // SAFETY: as for `plenty_text_len`.
    let head = unsafe { &utf8(&text)[..offset as usize] };
    // Inspect at most four bytes rather than rescanning the prefix.
    let previous = head
        .chars()
        .next_back()
        .expect("valid reverse string cursor");
    offset - previous.len_utf8() as u64
}
/// Whether both texts are ASCII and equal ignoring ASCII case.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_ascii_equal_folded(
    left: *const Text,
    right: *const Text,
) -> u64 {
    // SAFETY: as for `plenty_text_len`, for both texts.
    unsafe { ascii_equal_folded(left, right) as u64 }
}
pub(crate) unsafe fn at(text: *const Text, index: i64, site: *const crate::Site) -> *mut Text {
    unsafe {
        scalar(text, index).unwrap_or_else(|| {
            crate::aggregates::out_of_bounds(index, scalar_len(text) as usize, site)
        })
    }
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
    // SAFETY: the caller keeps the string word live through the call.
    unsafe { repeat_range(bytes(&text), scalar_len(text), count) }
}

pub(crate) fn try_repeat_range(source: &str, count: i64) -> Result<*mut Text, AllocError> {
    repeat_range(source.as_bytes(), source.chars().count() as u64, count)
}

fn repeat_range(source: &[u8], scalar_len: u64, count: i64) -> Result<*mut Text, AllocError> {
    let total = repeated_length(source.len() as u64, count)?;
    let scalars = repeated_length(scalar_len, count)?;
    try_build(total, scalars, |out| {
        if out.is_empty() {
            return;
        }
        let mut filled = put(out, 0, source);
        while filled < out.len() {
            // Doubling the initialized prefix avoids one copy per repeat.
            let count = filled.min(out.len() - filled);
            out.copy_within(..count, filled);
            filled += count;
        }
    })
}

/// Trim Unicode White_Space at selected ends, then allocate only the final text.
pub(crate) unsafe fn try_strip(
    text: *const Text,
    left: bool,
    right: bool,
) -> Result<*mut Text, AllocError> {
    // SAFETY: the caller keeps the string word live through the call.
    try_strip_range(unsafe { utf8(&text) }, left, right)
}

pub(crate) fn try_strip_range(
    mut source: &str,
    left: bool,
    right: bool,
) -> Result<*mut Text, AllocError> {
    if left {
        source = source.trim_start();
    }
    if right {
        source = source.trim_end();
    }
    try_new(source)
}

/// Copy a forward Unicode-scalar slice directly from its UTF-8 byte interval.
pub(crate) unsafe fn try_slice(
    text: *const Text,
    start: i64,
    stop: i64,
) -> Result<*mut Text, AllocError> {
    unsafe {
        let bounds = crate::aggregates::slice_bounds(start, stop, scalar_len(text) as usize);
        try_scalar_range(text, bounds)
    }
}

/// Copy the scalars in `bounds`, which lie within the text.
unsafe fn try_scalar_range(
    text: *const Text,
    bounds: std::ops::Range<usize>,
) -> Result<*mut Text, AllocError> {
    // SAFETY: the caller keeps the string word live through the call.
    // SAFETY: the same live string exposes its cached byte/scalar counts.
    scalar_range(unsafe { utf8(&text) }, bounds, unsafe { is_ascii(text) })
}

fn scalar_range(
    source: &str,
    bounds: std::ops::Range<usize>,
    ascii: bool,
) -> Result<*mut Text, AllocError> {
    if bounds.is_empty() {
        return try_new("");
    }
    if ascii {
        return try_new(&source[bounds]);
    }
    let start = source.char_indices().nth(bounds.start).unwrap().0;
    let end = source[start..]
        .char_indices()
        .nth(bounds.len())
        .map_or(source.len(), |(offset, _)| start + offset);
    try_new(&source[start..end])
}

/// Copy `count` scalars `step` apart from scalar `first`, positions that
/// `slice_positions` computed for this text. A negative step reverses them.
/// The result has exact size and is the only allocation.
pub(crate) unsafe fn try_stepped_slice(
    text: *const Text,
    positions: (usize, i64, usize),
) -> Result<*mut Text, AllocError> {
    // SAFETY: the caller keeps the string word live through the call.
    // SAFETY: the same live string exposes its cached byte/scalar counts.
    stepped_slice_range(unsafe { utf8(&text) }, positions, unsafe { is_ascii(text) })
}

pub(crate) fn try_stepped_slice_range(
    source: &str,
    positions: (usize, i64, usize),
) -> Result<*mut Text, AllocError> {
    stepped_slice_range(source, positions, source.is_ascii())
}

fn stepped_slice_range(
    source: &str,
    (first, step, count): (usize, i64, usize),
    ascii: bool,
) -> Result<*mut Text, AllocError> {
    use crate::aggregates::slice_position;
    if step == 1 || count <= 1 {
        return scalar_range(source, first..first + count, ascii);
    }
    if ascii {
        let source = source.as_bytes();
        return try_build(count as u64, count as u64, |out| {
            for (k, byte) in out.iter_mut().enumerate() {
                byte.write(source[slice_position(first, step, k)]);
            }
        });
    }
    // Visit the selected scalars in text order, from the lowest position.
    let low = if step > 0 {
        first
    } else {
        slice_position(first, step, count - 1)
    };
    let selected = || {
        source
            .chars()
            .skip(low)
            .step_by(step.unsigned_abs() as usize)
            .take(count)
    };
    let total: usize = selected().map(char::len_utf8).sum();
    try_build(total as u64, count as u64, |out| {
        // A reversed slice writes each scalar so that it ends where the
        // previous one starts, filling the output from its end.
        let mut offset = if step > 0 { 0 } else { out.len() };
        for character in selected() {
            let length = character.len_utf8();
            if step < 0 {
                offset -= length;
            }
            put(out, offset, character.encode_utf8(&mut [0; 4]).as_bytes());
            if step > 0 {
                offset += length;
            }
        }
    })
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

    // Issue #931: `pack` reads a short key in at most two overlapping loads.
    #[test]
    fn pack_matches_the_inline_layout_at_every_length() {
        let data = [b'a', 0, 0xc3, 0xa9, b'z', 0x7f, 0xff];
        for len in 0..=INLINE_MAX {
            let mut expected = (len as u64) << 1 | 1;
            for (i, byte) in data[..len].iter().enumerate() {
                expected |= u64::from(*byte) << (8 * (i + 1));
            }
            let word = pack(&data[..len]);
            assert_eq!(word as usize as u64, expected, "length {len}");
            let text = word.cast_const();
            assert_eq!(unsafe { bytes(&text) }, &data[..len]);
        }
    }

    #[test]
    fn inline_ascii_text_leaves_other_bytes_to_validation() {
        assert_eq!(inline_ascii_text(b"GGT"), Some(pack(b"GGT")));
        assert_eq!(inline_ascii_text(b""), Some(pack(b"")));
        assert_eq!(inline_ascii_text(b"a\0b"), Some(pack(b"a\0b")));
        assert_eq!(inline_ascii_text("aé".as_bytes()), None);
        assert_eq!(inline_ascii_text(&[0xff]), None);
        assert_eq!(inline_ascii_text(b"12345678"), None);
        assert_eq!(
            try_from_utf8("aé".as_bytes()),
            Some(Ok(pack("aé".as_bytes())))
        );
        assert!(try_from_utf8(&[b'a', 0xff]).is_none());
    }

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
    fn utf8_copies_match_the_standard_validator_at_every_length() {
        // Issue #749: ASCII and inline fast paths must accept exactly what
        // str::from_utf8 accepts and keep the scalar count it implies.
        let source = "abcdefgé🙂hijklmnopqrstuvwxyz".as_bytes();
        let mut cases: Vec<Vec<u8>> = Vec::new();
        for start in 0..source.len() {
            for stop in start..=source.len().min(start + 20) {
                cases.push(source[start..stop].to_vec());
            }
        }
        for length in 0..=20 {
            for at in 0..length {
                for bad in [0x80, 0xc3, 0xff] {
                    let mut case = vec![b'x'; length];
                    case[at] = bad;
                    cases.push(case);
                }
            }
        }
        for case in cases {
            let made = try_from_utf8(&case).map(|made| made.unwrap());
            match (str::from_utf8(&case), made) {
                (Ok(expected), Some(made)) => unsafe {
                    assert_eq!(bytes(&(made as *const Text)), case);
                    assert_eq!(is_inline(made), case.len() <= INLINE_MAX);
                    assert_eq!(scalar_len(made), expected.chars().count() as u64);
                    release_leaf(made);
                },
                (Err(_), None) => {}
                (expected, made) => panic!("{case:?}: {expected:?} but {made:?}"),
            }
        }
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
