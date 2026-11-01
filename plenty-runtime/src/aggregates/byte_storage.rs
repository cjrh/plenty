//! Contiguous byte storage behind `std.bytes`: one byte per element, with every
//! buffer acquisition fallible. `std.bytes` validates ranges and reports them as
//! values; a violated precondition here is a library bug and stops the process.
//! No pointer into the buffer crosses the ABI, so growth can relocate freely.
use super::{Collection, OwnedValue, Type, byte_scan, wrap};
use crate::memory::{self, AllocError, Header};
use crate::strings;
use crate::text_io;
use std::io::{Read, Write};

/// `crypto::State` begins with this layout, so byte operations on a crypto
/// state see an empty, ordinary buffer.
#[repr(C)]
pub(super) struct Storage {
    pub(super) header: Header,
    pub(super) bytes: Vec<u8>,
}

unsafe extern "C" fn destroy(pointer: *mut Header) {
    // SAFETY: the last owner has gone; the buffer is released exactly once.
    unsafe {
        std::ptr::drop_in_place(pointer.cast::<Storage>());
        memory::free::<Storage, u8>(pointer.cast(), 0);
    }
}

/// The buffer is reserved before the owner, so failure leaves nothing live.
fn create(capacity: usize) -> Result<*mut Storage, AllocError> {
    let mut bytes = Vec::new();
    memory::try_reserve(&mut bytes, capacity)?;
    let owner = memory::try_allocate::<Storage, u8>(0)?;
    // SAFETY: fresh, exclusively owned storage of the right layout.
    unsafe {
        owner.write(Storage {
            header: Header::new(destroy),
            bytes,
        });
    }
    Ok(owner)
}

pub(super) fn from_slice(source: &[u8]) -> Result<u128, AllocError> {
    let owner = create(source.len())?;
    // SAFETY: `create` reserved the full length for its sole owner.
    unsafe { (*owner).bytes.extend_from_slice(source) };
    Ok(owner as u128)
}

/// Content equality, so `==` on a `Bytes` never compares owner addresses.
pub(super) unsafe fn equal(a: u128, b: u128) -> bool {
    // SAFETY: both operands are live storage owners.
    unsafe { (*(a as *const Storage)).bytes == (*(b as *const Storage)).bytes }
}

/// The byte-literal spelling of a buffer; see `render_slice`.
pub(super) unsafe fn render(value: u128, out: &mut crate::render_buffer::Buffer) {
    // SAFETY: the operand is a live storage owner.
    render_slice(unsafe { &(*(value as *const Storage)).bytes }, out);
}

/// The spelling `Bytes`, `bytes`, and `&[u8]` values share, Python's: a
/// valid byte literal that reads back as the same bytes. Printable ASCII is
/// literal; `\\`, `\"`, LF, CR, and TAB use their escapes; every other byte
/// is `\xhh`.
pub(super) fn render_slice(bytes: &[u8], out: &mut crate::render_buffer::Buffer) {
    out.extend_from_slice(b"b\"");
    for byte in bytes {
        match byte {
            b'\\' | b'"' => out.extend_from_slice(&[b'\\', *byte]),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x20..=0x7e => out.push(*byte),
            _ => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.extend_from_slice(&[
                    b'\\',
                    b'x',
                    HEX[(byte >> 4) as usize],
                    HEX[(byte & 15) as usize],
                ]);
            }
        }
    }
    out.push(b'"');
}

/// The bytes of an operand that is only read: a buffer's, or a `bytes`
/// literal's. A literal is compiler-emitted immortal text, the only value
/// without a destructor that the compiler passes as a byte source.
pub(super) unsafe fn source<'a>(value: u128) -> &'a [u8] {
    // SAFETY: the operand is a live buffer or literal, both of which begin
    // with a header.
    unsafe {
        if (*(value as *const Header)).destroy.is_none() {
            strings::heap_bytes(value as *const strings::Text)
        } else {
            (*(value as *const Storage)).bytes.as_slice()
        }
    }
}

/// The bytes of a `&[u8]` view operand: the address of its first byte in the
/// low word and its length in the high word, as slice views lower. The
/// caller's loan keeps the viewed owner in place through the call.
pub(crate) unsafe fn view_bytes<'a>(value: u128) -> &'a [u8] {
    let (address, len) = (value as u64 as usize as *const u8, (value >> 64) as usize);
    if len == 0 {
        return &[];
    }
    // SAFETY: a view selects `len` initialized bytes of a live owner.
    unsafe { std::slice::from_raw_parts(address, len) }
}

/// The operand a `&[u8]` view of `bytes` lowers to, for runtime tests.
#[cfg(test)]
pub(crate) fn view_operand(bytes: &[u8]) -> u128 {
    bytes.as_ptr() as u128 | (bytes.len() as u128) << 64
}

/// Whether the `view` operand shares a byte with the buffer `storage`, so a
/// native writing `storage` while reading `view` would alias them.
pub(crate) unsafe fn view_overlaps(view: u128, storage: u128) -> bool {
    // SAFETY: the caller passes a live view and a live storage prefix.
    let (input, output) = unsafe {
        (
            view_bytes(view),
            (*(storage as *const Storage)).bytes.as_slice(),
        )
    };
    !input.is_empty()
        && !output.is_empty()
        && input.as_ptr() < output.as_ptr_range().end
        && output.as_ptr() < input.as_ptr_range().end
}

pub(super) fn size(value: u128) -> usize {
    usize::try_from(value as i64).unwrap_or_else(|_| crate::fail("negative byte offset"))
}

pub(super) fn range(start: u128, stop: u128, len: usize) -> std::ops::Range<usize> {
    let (start, stop) = (size(start), size(stop));
    if start > stop || stop > len {
        crate::fail_kind(
            crate::actor_heap::TrapKind::Index,
            "byte range out of bounds",
        );
    }
    start..stop
}

/// A found index, or -1: `std.bytes` turns the sentinel into an `Option`.
fn found(index: Option<usize>) -> u128 {
    index.map_or(-1i64, |index| index as i64) as u128
}

pub(super) fn result(value: Result<u128, AllocError>) -> u128 {
    match value {
        Ok(value) => wrap(value, 0),
        Err(error) => wrap(wrap(0, error as u64), 1),
    }
}

/// Whether `bytes` holds `other` at `offset`. Too few remaining bytes is a
/// mismatch; an offset past the end is a skipped check in `std.bytes`.
fn prefixed(bytes: &[u8], offset: usize, other: &[u8]) -> bool {
    match bytes.get(offset..) {
        Some(tail) => tail.starts_with(other),
        None => crate::fail_kind(
            crate::actor_heap::TrapKind::Index,
            "byte offset out of bounds",
        ),
    }
}

/// Operands are live values of the statically checked signature. A mutable
/// operation's first operand is exclusively borrowed and never aliases another.
/// `site` locates indexed access, the only byte operation a correct program
/// can trap in; null prints the message alone.
// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(super) unsafe fn call(
    op: i64,
    operands: [u128; 5],
    descriptor: *const Type,
    site: *const crate::Site,
) -> u128 {
    let [a, b, c, d, e] = operands;
    // SAFETY: see above; descriptors are process-lifetime compiler metadata.
    unsafe {
        let storage = |value: u128| &mut (*(value as *mut Storage)).bytes;
        // Scans only read, and their operands may be the same buffer.
        let view = |value: u128| (*(value as *const Storage)).bytes.as_slice();
        match op {
            170 => result(create(size(a)).map(|owner| owner as u128)),
            280 => crate::formatting::write_float(storage(a), f64::from_bits(b as u64)) as u128,
            281 => crate::formatting::write_float_spec(
                storage(a),
                f64::from_bits(b as u64),
                c as i64,
                d as i64,
                e as i64,
            ) as u128,
            171 => storage(a).len() as u128,
            172 => storage(a).capacity() as u128,
            173 => match storage(a).get(size(b)) {
                Some(byte) => *byte as u128,
                None => crate::fail_kind(
                    crate::actor_heap::TrapKind::Index,
                    "byte index out of bounds",
                ),
            },
            174 => match storage(a).get_mut(size(b)) {
                Some(byte) => {
                    *byte = c as u8;
                    0
                }
                None => crate::fail_kind(
                    crate::actor_heap::TrapKind::Index,
                    "byte index out of bounds",
                ),
            },
            175 => result(memory::try_reserve(storage(a), size(b)).map(|()| 0)),
            176 => {
                let (bytes, len) = (storage(a), size(b));
                let additional = len.saturating_sub(bytes.len());
                result(memory::try_reserve_amortized(bytes, additional).map(|()| {
                    bytes.resize(len, c as u8);
                    0
                }))
            }
            178 => {
                let source = view_bytes(b);
                let bytes = storage(a);
                result(
                    memory::try_reserve_amortized(bytes, source.len()).map(|()| {
                        bytes.extend_from_slice(source);
                        0
                    }),
                )
            }
            179 => {
                let source = view_bytes(c);
                let bytes = storage(a);
                let offset = size(b);
                match offset
                    .checked_add(source.len())
                    .and_then(|end| bytes.get_mut(offset..end))
                {
                    Some(target) => target.copy_from_slice(source),
                    None => crate::fail_kind(
                        crate::actor_heap::TrapKind::Index,
                        "byte range out of bounds",
                    ),
                }
                0
            }
            180 => {
                let list = &*(a as *const Collection);
                result(create(list.entries.len()).map(|owner| {
                    (*owner)
                        .bytes
                        .extend(list.entries.iter().map(|entry| entry.key as u8));
                    owner as u128
                }))
            }
            181 => {
                let ty = (*descriptor).variants[0].fields[0];
                let bytes = storage(a);
                result(super::try_collection_new(ty, bytes.len()).and_then(|list| {
                    // The guard releases a partially filled list.
                    let owner = OwnedValue {
                        value: list as u128,
                        ty,
                    };
                    for byte in bytes.iter() {
                        (*list).try_insert(*byte as u128, 0)?;
                    }
                    Ok(owner.into_value())
                }))
            }
            182 => result(from_slice(strings::bytes(&(a as *const strings::Text)))),
            183 => result(match std::str::from_utf8(view_bytes(a)) {
                Ok(text) => strings::try_new(text).map(|text| wrap(text as u128, 1)),
                Err(_) => Ok(wrap(0, 0)),
            }),
            188 => {
                let bytes = storage(a);
                let within = range(b, c, bytes.len());
                let target = &mut bytes[within];
                text_io::result(crate::files::read_bytes(
                    d as *mut crate::files::File,
                    target,
                ))
            }
            189 => {
                let source = view_bytes(a);
                text_io::result(crate::files::write_bytes(
                    b as *mut crate::files::File,
                    source,
                ))
            }
            190 => {
                let bytes = storage(a);
                let within = range(b, c, bytes.len());
                let target = &mut bytes[within];
                crate::io::io_result(text_io::Stdin.read(target).map(|count| count as u128))
            }
            191 => {
                let source = view_bytes(a);
                // The process streams keep their existing buffering, so bytes
                // stay ordered with `print` and the text writers.
                let written = if b == 2 {
                    crate::io::stderr().write(source)
                } else {
                    std::io::stdout().lock().write(source)
                };
                crate::io::io_result(written.map(|count| count as u128))
            }
            // Scans read a `&[u8]` view; indices are relative to it.
            230 => found(byte_scan::find(view_bytes(a), b as u8)),
            231 => found(byte_scan::find_in(view_bytes(a), view_bytes(b), c != 0)),
            232 => found(byte_scan::find_bytes(view_bytes(a), view_bytes(b))),
            233 => byte_scan::count(view_bytes(a), b as u8) as u128,
            234 => prefixed(view_bytes(a), size(b), view_bytes(c)) as u128,
            235 => prefixed(
                view_bytes(a),
                size(b),
                strings::bytes(&(c as *const strings::Text)),
            ) as u128,
            236 => {
                let bytes = view_bytes(a);
                let run = if b == 0 {
                    byte_scan::ascii_run(bytes)
                } else {
                    byte_scan::utf8_run(bytes)
                };
                run as u128
            }
            237 => {
                let bytes = storage(a);
                let within = range(b, c, bytes.len());
                bytes[within].fill(d as u8);
                0
            }
            // `copy_within`: memmove inside one buffer, so the ranges may overlap.
            // `source[start:stop:step]` with clamped bounds, into a new
            // buffer of exact capacity.
            283 => {
                let source = source(a);
                let (first, step, count) =
                    super::slice_positions(source.len(), b as i64, c as i64, d as i64, e as i64);
                result(create(count).map(|owner| {
                    let bytes = &mut (*owner).bytes;
                    if step == 1 {
                        bytes.extend_from_slice(&source[first..first + count]);
                    } else {
                        bytes.extend(
                            (0..count).map(|k| source[super::slice_position(first, step, k)]),
                        );
                    }
                    owner as u128
                }))
            }
            282 => {
                let bytes = storage(a);
                let within = range(b, c, bytes.len());
                let to = size(d);
                if to > bytes.len() - within.len() {
                    crate::fail_kind(
                        crate::actor_heap::TrapKind::Index,
                        "byte range out of bounds",
                    );
                }
                bytes.copy_within(within, to);
                0
            }
            // `b[i]` and `b[i] = v`: list index rules, negatives from the end.
            238 => {
                let bytes = view(a);
                bytes[super::index(b as i64, bytes.len(), site)] as u128
            }
            239 => {
                let bytes = storage(a);
                let index = super::index(b as i64, bytes.len(), site);
                bytes[index] = c as u8;
                0
            }
            248 => {
                let bytes = storage(a);
                let within = range(b, c, bytes.len());
                crate::io::io_result(crate::entropy::read(&mut bytes[within]))
            }
            _ => unreachable!("byte storage opcode"),
        }
    }
}

// Narrow ABI for the per-byte queries (issue #364): a live borrowed storage
// owner in a register, no ownership transfer, and no dispatcher argument
// block. Indexing uses the dispatcher's list rules and trap message.

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_bytes_len(owner: *const u8) -> u64 {
    // SAFETY: the compiler keeps the storage owner alive through the call.
    unsafe { (*owner.cast::<Storage>()).bytes.len() as u64 }
}

/// The address of the first byte of a live buffer or `bytes` literal, for a
/// slice view. Its loan keeps the storage from growing while the view exists;
/// a shared view of a literal only reads it.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_byte_view_rows(owner: *const u8) -> *const u8 {
    // SAFETY: the compiler keeps the owner alive through the call.
    unsafe { source(owner as usize as u128).as_ptr() }
}

/// The byte count of a live buffer or `bytes` literal, for a slice view.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_byte_view_len(owner: *const u8) -> u64 {
    // SAFETY: the compiler keeps the owner alive through the call.
    unsafe { source(owner as usize as u128).len() as u64 }
}

/// `std.text.find_in`/`find_not_in`: the scalar position of the first scalar
/// of `text` that is (`member` 1) or is not (0) an ASCII byte of the `set`
/// view (its address and length), or -1 (issue #383).
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_text_find_in(
    text: *const strings::Text,
    set: *const u8,
    set_len: u64,
    member: u64,
) -> i64 {
    // SAFETY: the compiler passes both operands through shared loans that
    // outlive the call; an inline text view borrows the parameter.
    let found = unsafe {
        byte_scan::text_find_in(
            strings::bytes(&text),
            view_bytes(set as usize as u128 | u128::from(set_len) << 64),
            member != 0,
        )
    };
    found.map_or(-1, |index| index as i64)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_bytes_item(
    owner: *const u8,
    i: i64,
    site: *const crate::Site,
) -> u64 {
    // SAFETY: as for `plenty_bytes_len`.
    unsafe {
        let bytes = &(*owner.cast::<Storage>()).bytes;
        u64::from(bytes[super::index(i, bytes.len(), site)])
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_bytes_set_item(
    owner: *mut u8,
    i: i64,
    value: u64,
    site: *const crate::Site,
) {
    // SAFETY: the caller holds an exclusive loan of a live storage owner.
    unsafe {
        let bytes = &mut (*owner.cast::<Storage>()).bytes;
        let index = super::index(i, bytes.len(), site);
        bytes[index] = value as u8;
    }
}

/// `Bytes.push` (issue #389): the high word of the dispatcher's
/// `Result[(), AllocError]`, whose low word is zero in both cases.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_bytes_push(owner: *mut u8, value: u64) -> u64 {
    // SAFETY: the caller holds an exclusive loan of a live storage owner.
    let bytes = unsafe { &mut (*owner.cast::<Storage>()).bytes };
    let pushed = memory::try_reserve_amortized(bytes, 1).map(|()| {
        bytes.push(value as u8);
        0
    });
    (result(pushed) >> 64) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Owner(u128);
    impl Drop for Owner {
        fn drop(&mut self) {
            // SAFETY: each test owner holds the sole count of live storage.
            unsafe { memory::plenty_release(self.0 as *mut Header) }
        }
    }

    unsafe fn run(op: i64, operands: [u128; 5]) -> u128 {
        unsafe { call(op, operands, std::ptr::null(), std::ptr::null()) }
    }

    fn bytes(owner: &Owner) -> &[u8] {
        // SAFETY: the owner outlives the returned view within each test.
        unsafe { &(*(owner.0 as *const Storage)).bytes }
    }

    #[test]
    fn one_byte_per_element_and_exact_explicit_capacity() {
        unsafe {
            let owner = Owner(run(170, [64, 0, 0, 0, 0]));
            assert_eq!(run(171, [owner.0, 0, 0, 0, 0]), 0);
            assert_eq!(run(172, [owner.0, 0, 0, 0, 0]), 64);
            assert_eq!(size_of::<Storage>(), size_of::<Header>() + 24);
        }
    }

    #[test]
    fn append_overwrite_and_resize_preserve_every_byte_value() {
        unsafe {
            let source = Owner(from_slice(&[0, 255, 0x80, b'\n', 0]).unwrap());
            let target = Owner(run(170, [0; 5]));
            let all = bytes(&source);
            assert_eq!(run(178, [target.0, view_operand(&all[1..4]), 0, 0, 0]), 0);
            assert_eq!(bytes(&target), [255, 0x80, b'\n']);
            assert_eq!(run(178, [target.0, view_operand(&[]), 0, 0, 0]), 0);
            assert_eq!(bytes(&target).len(), 3);
            assert_eq!(run(176, [target.0, 6, 7, 0, 0]), 0);
            assert_eq!(bytes(&target), [255, 0x80, b'\n', 7, 7, 7]);
            run(179, [target.0, 4, view_operand(&all[..2]), 0, 0]);
            assert_eq!(bytes(&target), [255, 0x80, b'\n', 7, 0, 255]);
            assert_eq!(run(176, [target.0, 1, 0, 0, 0]), 0);
            assert_eq!(bytes(&target), [255]);
            assert!(!equal(target.0, source.0));
            assert!(equal(target.0, target.0));
            let mut out = crate::render_buffer::Buffer::default();
            render(source.0, &mut out);
            assert_eq!(out.finish().unwrap(), br#"b"\x00\xff\x80\n\x00""#);
        }
    }

    #[test]
    fn scans_report_view_relative_indices_or_the_missing_sentinel() {
        const NONE: u128 = -1i64 as u128;
        unsafe {
            let text = b"key=\"a,b\";\r\nnext line";
            let data = view_operand(text);
            let tail = view_operand(&text[4..]);
            assert_eq!(run(230, [data, b'=' as u128, 0, 0, 0]), 3);
            assert_eq!(run(230, [tail, b'=' as u128, 0, 0, 0]), NONE);
            let set = view_operand(b",;");
            assert_eq!(run(231, [data, set, 1, 0, 0]), 6);
            assert_eq!(run(231, [view_operand(&text[7..]), set, 1, 0, 0]), 2);
            assert_eq!(run(231, [view_operand(&text[10..]), set, 1, 0, 0]), NONE);
            assert_eq!(run(231, [view_operand(&text[6..]), set, 0, 0, 0]), 1);
            assert_eq!(run(231, [data, data, 0, 0, 0]), NONE);
            let crlf = view_operand(b"\r\n");
            assert_eq!(run(232, [data, crlf, 0, 0, 0]), 10);
            assert_eq!(run(232, [view_operand(&text[..11]), crlf, 0, 0, 0]), NONE);
            assert_eq!(run(233, [data, b'e' as u128, 0, 0, 0]), 3);
            assert_eq!(run(234, [data, 10, crlf, 0, 0]), 1);
            assert_eq!(run(234, [data, 9, crlf, 0, 0]), 0);
            let len = text.len() as u128;
            assert_eq!(run(234, [data, len - 1, crlf, 0, 0]), 0);
            assert_eq!(run(234, [data, len, view_operand(&[]), 0, 0]), 1);
            let next = strings::try_new("next").unwrap();
            assert_eq!(run(235, [data, 12, next as u128, 0, 0]), 1);
            assert_eq!(run(235, [data, 13, next as u128, 0, 0]), 0);
            memory::plenty_release(next as *mut Header);
            let mixed: &[u8] = &[b'a', b'b', 0xc3, 0xa9, 0xff, b'c'];
            assert_eq!(run(236, [view_operand(mixed), 0, 0, 0, 0]), 2);
            assert_eq!(run(236, [view_operand(mixed), 1, 0, 0, 0]), 4);
            assert_eq!(run(236, [view_operand(&mixed[5..]), 1, 0, 0, 0]), 1);
            assert_eq!(run(236, [view_operand(&mixed[..3]), 1, 0, 0, 0]), 2);
        }
    }

    #[test]
    fn fill_and_indexed_access_follow_list_index_rules() {
        unsafe {
            let owner = Owner(from_slice(&[1, 2, 3, 4, 5]).unwrap());
            assert_eq!(run(237, [owner.0, 1, 4, 9, 0]), 0);
            assert_eq!(bytes(&owner), [1, 9, 9, 9, 5]);
            assert_eq!(run(237, [owner.0, 5, 5, 0, 0]), 0);
            assert_eq!(run(238, [owner.0, 0, 0, 0, 0]), 1);
            assert_eq!(run(238, [owner.0, -1i64 as u128, 0, 0, 0]), 5);
            assert_eq!(run(238, [owner.0, -5i64 as u128, 0, 0, 0]), 1);
            assert_eq!(run(239, [owner.0, -2i64 as u128, 255, 0, 0]), 0);
            assert_eq!(bytes(&owner), [1, 9, 9, 255, 5]);
        }
    }

    #[test]
    fn copy_within_moves_overlapping_ranges_in_both_directions() {
        unsafe {
            let owner = Owner(from_slice(&[0, 1, 2, 3, 4, 5]).unwrap());
            // Toward the front, as buffer compaction does.
            assert_eq!(run(282, [owner.0, 2, 6, 0, 0]), 0);
            assert_eq!(bytes(&owner), [2, 3, 4, 5, 4, 5]);
            // Toward the back.
            assert_eq!(run(282, [owner.0, 0, 4, 2, 0]), 0);
            assert_eq!(bytes(&owner), [2, 3, 2, 3, 4, 5]);
            // An empty range may sit at the end.
            assert_eq!(run(282, [owner.0, 3, 3, 6, 0]), 0);
            assert_eq!(bytes(&owner), [2, 3, 2, 3, 4, 5]);
        }
    }

    #[test]
    fn text_conversion_validates_and_keeps_embedded_nul() {
        unsafe {
            let valid = "a\0é".as_bytes();
            let value = run(183, [view_operand(valid), 0, 0, 0, 0]);
            // Ok(Some(text)): the option tag sits above the result tag.
            assert_eq!(value >> 64, 2);
            let text = value as u64 as *const strings::Text;
            assert_eq!(strings::bytes(&text), valid);
            memory::plenty_release(text as *mut Header);
            // A view converts only its own bytes.
            let value = run(183, [view_operand(&valid[1..2]), 0, 0, 0, 0]);
            assert_eq!(value >> 64, 2);
            let text = value as u64 as *const strings::Text;
            assert_eq!(strings::bytes(&text), b"\0");
            memory::plenty_release(text as *mut Header);
            // Cutting the two-byte é in half is invalid.
            assert_eq!(run(183, [view_operand(&valid[..3]), 0, 0, 0, 0]), 0);
            assert_eq!(run(183, [view_operand(&[0xff, 0xfe]), 0, 0, 0, 0]), 0);
        }
    }

    #[test]
    fn impossible_sizes_fail_without_changing_the_buffer() {
        unsafe {
            let value = run(170, [i64::MAX as u128, 0, 0, 0, 0]);
            assert_eq!(value >> 64, (AllocError::OutOfMemory as u128) * 2 + 1);
            let owner = Owner(from_slice(b"ab").unwrap());
            let value = run(176, [owner.0, i64::MAX as u128, 0, 0, 0]);
            assert_eq!(value >> 64, (AllocError::OutOfMemory as u128) * 2 + 1);
            let value = run(175, [owner.0, i64::MAX as u128, 0, 0, 0]);
            assert_eq!(value >> 64, (AllocError::CapacityOverflow as u128) * 2 + 1);
            assert_eq!(run(172, [owner.0, 0, 0, 0, 0]), 2);
            assert_eq!(bytes(&owner), b"ab");
        }
    }
}
