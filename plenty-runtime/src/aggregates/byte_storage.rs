//! Contiguous byte storage behind `std.bytes`: one byte per element, with every
//! buffer acquisition fallible. `std.bytes` validates ranges and reports them as
//! values; a violated precondition here is a library bug and stops the process.
//! No pointer into the buffer crosses the ABI, so growth can relocate freely.
use super::{wrap, Collection, OwnedValue, Type};
use crate::memory::{self, AllocError, Header};
use crate::strings;
use crate::text_io;
use std::io::{Read, Write};

#[repr(C)]
struct Storage {
    header: Header,
    bytes: Vec<u8>,
}

unsafe extern "C" fn destroy(pointer: *mut Header) {
    // SAFETY: the last owner has gone; the buffer is released exactly once.
    unsafe {
        std::ptr::drop_in_place(pointer.cast::<Storage>());
        memory::free::<Storage, u8>(pointer.cast(), 0);
    }
}

/// Amortized growth: `Vec` at least doubles, so repeated appends stay linear.
fn grow(bytes: &mut Vec<u8>, additional: usize) -> Result<(), AllocError> {
    bytes
        .len()
        .checked_add(additional)
        .filter(|total| *total <= isize::MAX as usize)
        .ok_or(AllocError::CapacityOverflow)?;
    bytes
        .try_reserve(additional)
        .map_err(|_| AllocError::OutOfMemory)
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

fn from_slice(source: &[u8]) -> Result<u128, AllocError> {
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

/// Diagnostic spelling: printable ASCII, with every other byte as `\xhh`.
pub(super) unsafe fn render(value: u128, out: &mut crate::render_buffer::Buffer) {
    out.extend_from_slice(b"b\"");
    // SAFETY: the operand is a live storage owner.
    for byte in unsafe { &(*(value as *const Storage)).bytes } {
        match byte {
            b'\\' | b'"' => out.extend_from_slice(&[b'\\', *byte]),
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

fn size(value: u128) -> usize {
    usize::try_from(value as i64).unwrap_or_else(|_| crate::fail("negative byte offset"))
}

fn range(start: u128, stop: u128, len: usize) -> std::ops::Range<usize> {
    let (start, stop) = (size(start), size(stop));
    if start > stop || stop > len {
        crate::fail("byte range out of bounds");
    }
    start..stop
}

fn result(value: Result<u128, AllocError>) -> u128 {
    match value {
        Ok(value) => wrap(value, 0),
        Err(error) => wrap(wrap(0, error as u64), 1),
    }
}

/// Operands are live values of the statically checked signature. A mutable
/// operation's first operand is exclusively borrowed and never aliases another.
pub(super) unsafe fn call(op: i64, operands: [u128; 5], descriptor: *const Type) -> u128 {
    let [a, b, c, d, e] = operands;
    // SAFETY: see above; descriptors are process-lifetime compiler metadata.
    unsafe {
        let storage = |value: u128| &mut (*(value as *mut Storage)).bytes;
        match op {
            170 => result(create(size(a)).map(|owner| owner as u128)),
            171 => storage(a).len() as u128,
            172 => storage(a).capacity() as u128,
            173 => match storage(a).get(size(b)) {
                Some(byte) => *byte as u128,
                None => crate::fail("byte index out of bounds"),
            },
            174 => match storage(a).get_mut(size(b)) {
                Some(byte) => {
                    *byte = c as u8;
                    0
                }
                None => crate::fail("byte index out of bounds"),
            },
            175 => result(memory::try_reserve(storage(a), size(b)).map(|()| 0)),
            176 => {
                let (bytes, len) = (storage(a), size(b));
                let additional = len.saturating_sub(bytes.len());
                result(grow(bytes, additional).map(|()| {
                    bytes.resize(len, c as u8);
                    0
                }))
            }
            177 => {
                let bytes = storage(a);
                result(grow(bytes, 1).map(|()| {
                    bytes.push(b as u8);
                    0
                }))
            }
            178 => {
                let source = storage(b);
                let source = &source[range(c, d, source.len())];
                let bytes = storage(a);
                result(grow(bytes, source.len()).map(|()| {
                    bytes.extend_from_slice(source);
                    0
                }))
            }
            179 => {
                let source = storage(c);
                let source = &source[range(d, e, source.len())];
                let bytes = storage(a);
                let offset = size(b);
                match offset
                    .checked_add(source.len())
                    .and_then(|end| bytes.get_mut(offset..end))
                {
                    Some(target) => target.copy_from_slice(source),
                    None => crate::fail("byte range out of bounds"),
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
            183 => result(match std::str::from_utf8(storage(a)) {
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
                let bytes = storage(a);
                let source = &bytes[range(b, c, bytes.len())];
                text_io::result(crate::files::write_bytes(
                    d as *mut crate::files::File,
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
                let bytes = storage(a);
                let source = &bytes[range(b, c, bytes.len())];
                // The process streams keep their existing buffering, so bytes
                // stay ordered with `print` and the text writers.
                let written = if d == 2 {
                    std::io::stderr().lock().write(source)
                } else {
                    std::io::stdout().lock().write(source)
                };
                crate::io::io_result(written.map(|count| count as u128))
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
        unsafe { call(op, operands, std::ptr::null()) }
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
            assert_eq!(run(178, [target.0, source.0, 1, 4, 0]), 0);
            assert_eq!(bytes(&target), [255, 0x80, b'\n']);
            assert_eq!(run(178, [target.0, source.0, 5, 5, 0]), 0);
            assert_eq!(bytes(&target).len(), 3);
            assert_eq!(run(176, [target.0, 6, 7, 0, 0]), 0);
            assert_eq!(bytes(&target), [255, 0x80, b'\n', 7, 7, 7]);
            run(179, [target.0, 4, source.0, 0, 2]);
            assert_eq!(bytes(&target), [255, 0x80, b'\n', 7, 0, 255]);
            assert_eq!(run(176, [target.0, 1, 0, 0, 0]), 0);
            assert_eq!(bytes(&target), [255]);
            assert!(!equal(target.0, source.0));
            assert!(equal(target.0, target.0));
            let mut out = crate::render_buffer::Buffer::default();
            render(source.0, &mut out);
            assert_eq!(out.finish().unwrap(), br#"b"\x00\xff\x80\x0a\x00""#);
        }
    }

    #[test]
    fn text_conversion_validates_and_keeps_embedded_nul() {
        unsafe {
            let valid = Owner(from_slice("a\0é".as_bytes()).unwrap());
            let value = run(183, [valid.0, 0, 0, 0, 0]);
            // Ok(Some(text)): the option tag sits above the result tag.
            assert_eq!(value >> 64, 2);
            let text = value as u64 as *const strings::Text;
            assert_eq!(strings::bytes(&text), "a\0é".as_bytes());
            memory::plenty_release(text as *mut Header);
            let invalid = Owner(from_slice(&[0xff, 0xfe]).unwrap());
            assert_eq!(run(183, [invalid.0, 0, 0, 0, 0]), 0);
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
