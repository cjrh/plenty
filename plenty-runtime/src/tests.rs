//! Exercise raw ABI boundaries without generated machine code, also under Miri.
use crate::aggregates::plenty_collection as collection;
use crate::generators::{plenty_generator_new, Generator};
use crate::memory::{plenty_release, plenty_retain, Header};
use crate::strings::{self, plenty_concat, plenty_contains, plenty_str_eq};
use std::cell::RefCell;
use std::ffi::CStr;
use std::ptr;

thread_local! { static TRACE: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) }; }
const GUARD: &CStr = c"C5:Guard1:2:id4";

unsafe fn guard(id: u64) -> u64 {
    unsafe {
        let value = collection(30, hook as *const () as u64, 0, 0, GUARD.as_ptr());
        *((value as *mut u8).add(32).cast::<u64>()) = id;
        value
    }
}
unsafe extern "C" fn hook(owner: *mut *mut u8) {
    unsafe {
        let id = *(*owner).add(32).cast::<u64>();
        TRACE.with(|trace| trace.borrow_mut().push(id));
        // Reading the dying receiver may retain/release its immortalized header.
        plenty_retain((*owner).cast());
        plenty_release((*owner).cast());
        if id == 9 {
            plenty_release(guard(77) as *mut Header);
            TRACE.with(|trace| trace.borrow_mut().push(99));
        }
    }
}
fn trace() -> Vec<u64> {
    TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}

#[test]
fn strings_keep_lengths_utf8_and_nul_bytes() {
    unsafe {
        let a = strings::new("é\0".as_bytes());
        let b = strings::new("🦀".as_bytes());
        let joined = plenty_concat(a, b);
        assert_eq!(strings::utf8(joined), "é\0🦀");
        assert_eq!((*joined).byte_len, 7);
        assert_eq!((*joined).scalar_len, 3);
        assert_eq!(plenty_contains(joined, b), 1);
        let last = strings::at(joined, -1);
        assert_eq!(plenty_str_eq(last, b), 1);
        for p in [a, b, joined, last] {
            plenty_release(p.cast());
        }
    }
}

#[test]
fn compiler_literal_prefix_is_read_only_and_immortal() {
    #[repr(C)]
    struct Literal {
        header: Header,
        byte_len: u64,
        scalar_len: u64,
        bytes: [u8; 3],
    }
    static LITERAL: Literal = Literal {
        header: Header {
            refs: u64::MAX,
            destroy: None,
        },
        byte_len: 3,
        scalar_len: 3,
        bytes: *b"a\0b",
    };
    unsafe {
        let pointer = std::ptr::addr_of!(LITERAL).cast::<strings::Text>();
        plenty_retain(pointer.cast_mut().cast());
        plenty_release(pointer.cast_mut().cast());
        assert_eq!(strings::bytes(pointer), b"a\0b");
        let copy = plenty_concat(pointer, pointer);
        assert_eq!(strings::bytes(copy), b"a\0ba\0b");
        plenty_release(copy.cast());
    }
}

#[test]
fn destruction_queue_preserves_children_and_nested_drops() {
    unsafe {
        let list = collection(0, 0, 0, 0, c"LC5:Guard1:2:id4".as_ptr());
        for id in [9, 2, 3] {
            let value = guard(id);
            let retained = collection(1, list, value, 0, ptr::null());
            plenty_release(retained as *mut Header);
            plenty_release(value as *mut Header);
        }
        plenty_release(list as *mut Header);
        assert_eq!(trace(), [9, 77, 99, 2, 3]);
    }
}

#[test]
fn owned_iteration_removes_the_source_owner() {
    unsafe {
        let list = collection(0, 0, 0, 0, c"LC5:Guard1:2:id4".as_ptr());
        let value = guard(5);
        let retained = collection(1, list, value, 0, ptr::null());
        plenty_release(retained as *mut Header);
        plenty_release(value as *mut Header);
        let taken = collection(15, list, 0, 0, ptr::null());
        plenty_release(taken as *mut Header);
        assert_eq!(trace(), [5]);
        plenty_release(list as *mut Header);
        assert!(trace().is_empty());
    }
}

#[test]
fn shared_descriptor_graphs_and_recursive_copies() {
    unsafe {
        // Pair contains two Points; the second uses the descriptor's backreference.
        let pair = collection(30, 0, 0, 0, c"C4:Pair2:1:aC5:Point1:1:x41:b@1:".as_ptr());
        let a = collection(30, 0, 0, 0, c"C5:Point1:1:x4".as_ptr());
        let b = collection(30, 0, 0, 0, c"C5:Point1:1:x4".as_ptr());
        *((a as *mut u8).add(32).cast::<u64>()) = 3;
        *((b as *mut u8).add(32).cast::<u64>()) = 4;
        *((pair as *mut u8).add(32).cast::<u64>()) = a;
        *((pair as *mut u8).add(40).cast::<u64>()) = b;
        let copy = collection(14, pair, 0, 0, ptr::null());
        assert_eq!(collection(8, pair, copy, 0, ptr::null()), 1);
        *((a as *mut u8).add(32).cast::<u64>()) = 10;
        assert_eq!(collection(8, pair, copy, 0, ptr::null()), 0);
        plenty_release(pair as *mut Header);
        plenty_release(copy as *mut Header);
    }
}

#[test]
fn dictionaries_preserve_order_and_copy_owned_contents() {
    unsafe {
        let dict = collection(0, 0, 0, 0, c"D4L4".as_ptr());
        let list = collection(0, 0, 0, 0, c"L4".as_ptr());
        plenty_release(collection(1, list, 42, 0, ptr::null()) as *mut Header);
        plenty_release(collection(1, dict, 1, list, ptr::null()) as *mut Header);
        plenty_release(list as *mut Header);
        let independent = collection(14, dict, 0, 0, ptr::null());
        assert_eq!(collection(8, dict, independent, 0, ptr::null()), 1);
        let values = collection(11, dict, 0, 0, ptr::null());
        let copied_values = collection(14, values, 0, 0, ptr::null());
        let item = collection(15, values, 0, 0, ptr::null());
        plenty_release(collection(2, item, 7, 0, ptr::null()) as *mut Header);
        let copied_item = collection(15, copied_values, 0, 0, ptr::null());
        assert_eq!(collection(5, copied_item, 0, 0, ptr::null()), 1);
        for object in [dict, independent, values, copied_values, item, copied_item] {
            plenty_release(object as *mut Header);
        }
    }
}

unsafe extern "C" fn never_resume(_: *mut Generator, _: *mut u64) -> u8 {
    panic!("dropping must not resume a generator")
}
#[test]
fn deeply_nested_generator_frames_drop_iteratively() {
    static MANAGED: [u8; 1] = [1];
    unsafe {
        let mut child = guard(1);
        for _ in 0..1000 {
            let frame = plenty_generator_new(never_resume, 1, MANAGED.as_ptr());
            *frame.cast::<u8>().add(56).cast::<u64>() = child;
            child = frame as u64;
        }
        plenty_release(child as *mut Header);
        assert_eq!(trace(), [1]);
    }
}
