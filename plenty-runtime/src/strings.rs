use crate::memory::{self, Header};
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
    if bytes.len() > i64::MAX as usize {
        crate::fail("string capacity overflow");
    }
    let pointer = memory::allocate::<Text, u8>(bytes.len());
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
    pointer
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
    unsafe {
        let len = (*a)
            .byte_len
            .checked_add((*b).byte_len)
            .filter(|n| *n <= i64::MAX as u64)
            .unwrap_or_else(|| crate::fail("string capacity overflow"));
        let out = memory::allocate::<Text, u8>(len as usize);
        out.write(Text {
            header: Header::new(destroy),
            byte_len: len,
            scalar_len: (*a).scalar_len + (*b).scalar_len,
            data: [],
        });
        ptr::copy_nonoverlapping(
            bytes(a).as_ptr(),
            ptr::addr_of_mut!((*out).data).cast::<u8>(),
            (*a).byte_len as usize,
        );
        ptr::copy_nonoverlapping(
            bytes(b).as_ptr(),
            ptr::addr_of_mut!((*out).data)
                .cast::<u8>()
                .add((*a).byte_len as usize),
            (*b).byte_len as usize,
        );
        out
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
    unsafe {
        let index = crate::aggregates::index(index, (*text).scalar_len as usize);
        let character = utf8(text)
            .chars()
            .nth(index)
            .expect("validated string index");
        new(character.encode_utf8(&mut [0; 4]).as_bytes())
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
