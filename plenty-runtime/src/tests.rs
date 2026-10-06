//! Exercise raw ABI boundaries without generated machine code, also under Miri.
use crate::aggregates::collection;
use crate::aggregates::{Type, Variant};
use crate::generators::{plenty_generator_new, Generator};
use crate::memory::{plenty_release, plenty_retain, Header};
use crate::strings::{self, plenty_concat, plenty_contains, plenty_str_eq};
use std::cell::RefCell;
use std::ptr;

thread_local! { static TRACE: RefCell<Vec<u128>> = const { RefCell::new(Vec::new()) }; }
const fn scalar(kind: u8) -> Type {
    Type {
        kind,
        affine: false,
        reflexive: kind != b'f' && kind != b'd',
        key: None,
        value: None,
        name: "",
        variants: &[],
    }
}
const fn list(key: &'static Type) -> Type {
    Type {
        kind: b'L',
        affine: true,
        reflexive: key.reflexive,
        key: Some(key),
        ..scalar(b'L')
    }
}
static INTEGER: Type = scalar(b'4');
static UNIT: Type = scalar(b'v');
static FLOAT32: Type = scalar(b'f');
static FLOAT64: Type = scalar(b'd');
static LIST_F32: Type = list(&FLOAT32);
static LIST_F64: Type = list(&FLOAT64);
static LIST_INT: Type = list(&INTEGER);
static LIST_LIST: Type = list(&LIST_INT);
static GUARD: Type = Type {
    affine: true,
    name: "Guard",
    variants: &[Variant {
        name: "id",
        fields: &[&INTEGER],
    }],
    ..scalar(b'C')
};
static LIST_GUARD: Type = list(&GUARD);
static DONE: Type = Type {
    name: "Done",
    variants: &[Variant {
        name: "Ok",
        fields: &[&UNIT],
    }],
    ..scalar(b'E')
};
static POINT: Type = Type {
    affine: true,
    name: "Point",
    variants: &[Variant {
        name: "x",
        fields: &[&INTEGER],
    }],
    ..scalar(b'C')
};
static PAIR: Type = Type {
    affine: true,
    name: "Pair",
    variants: &[
        Variant {
            name: "a",
            fields: &[&POINT],
        },
        Variant {
            name: "b",
            fields: &[&POINT],
        },
    ],
    ..scalar(b'C')
};
static DICT: Type = Type {
    affine: true,
    key: Some(&INTEGER),
    value: Some(&LIST_INT),
    ..scalar(b'D')
};
static GENERATOR: Type = scalar(b'G');
static OPTION_GUARD: Type = Type {
    affine: true,
    name: "Option[Guard]",
    variants: &[
        Variant {
            name: "Nothing",
            fields: &[],
        },
        Variant {
            name: "Some",
            fields: &[&GUARD],
        },
    ],
    ..scalar(b'B')
};
static RESULT_OPTION: Type = Type {
    affine: true,
    name: "Result[Option[Guard], i64]",
    variants: &[
        Variant {
            name: "Ok",
            fields: &[&OPTION_GUARD],
        },
        Variant {
            name: "Err",
            fields: &[&INTEGER],
        },
    ],
    ..scalar(b'B')
};
static LIST_RESULT: Type = list(&RESULT_OPTION);

#[test]
fn fallible_reservation_and_growth_preserve_hash_lookups() {
    static SET: Type = Type {
        kind: b'S',
        ..list(&INTEGER)
    };
    static MAP: Type = Type {
        kind: b'D',
        value: Some(&INTEGER),
        ..list(&INTEGER)
    };
    for ty in [&LIST_INT, &SET, &MAP] {
        unsafe {
            let c = collection(0, 0, 0, 0, ty);
            assert_eq!(collection(28, c, 0, 0, ptr::null()), 0);
            assert_eq!(collection(28, c, 32, 0, ptr::null()), 0);
            for n in 0..32 {
                assert_eq!(collection(29, c, n, n + 1, ptr::null()), 0);
            }
            // Both negative lengths and impossible layouts return the inline
            // Err(CapacityOverflow) tag path, without touching existing entries.
            for count in [u64::MAX as u128, i64::MAX as u128] {
                assert_eq!(collection(28, c, count, 0, ptr::null()), 3u128 << 64);
            }
            for n in 0..32 {
                assert_eq!(collection(7, n, c, 0, ptr::null()), 1);
                if ty.kind == b'D' {
                    assert_eq!(collection(4, c, n, 0, ptr::null()), n + 1);
                }
            }
            plenty_release(c as *mut Header);
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn actual_allocator_failures_preserve_empty_and_populated_hash_tables() {
    static MAP: Type = Type {
        kind: b'D',
        value: Some(&INTEGER),
        ..list(&INTEGER)
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for len in [0, 8] {
        for budget in 0..=2 {
            unsafe {
                let c = collection(0, 0, 0, 0, &MAP);
                for n in 0..len {
                    assert_eq!(collection(29, c, n, n, ptr::null()), 0);
                }
                let result = {
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(budget));
                    collection(29, c, 42, 42, ptr::null())
                };
                assert_eq!(result, if budget == 2 { 0 } else { 1u128 << 64 });
                assert_eq!(
                    collection(5, c, 0, 0, ptr::null()),
                    len + u128::from(budget == 2)
                );
                for n in 0..len {
                    assert_eq!(collection(4, c, n, 0, ptr::null()), n);
                }
                assert_eq!(collection(29, c, 42, 99, ptr::null()), 0);
                assert_eq!(collection(4, c, 42, 0, ptr::null()), 99);
                plenty_release(c as *mut Header);
            }
        }
    }
}

#[test]
fn inline_payloads_retain_and_drop_through_runtime_slots() {
    use crate::aggregates::{release, wrap};
    unsafe {
        let first = wrap(wrap(guard(42), 1), 0);
        let list = collection(0, 0, 0, 0, &LIST_RESULT);
        plenty_release(collection(1, list, first, 0, ptr::null()) as *mut Header);
        release(first, &RESULT_OPTION);
        let moved = collection(15, list, 0, 0, ptr::null());
        plenty_release(list as *mut Header);
        assert!(trace().is_empty());
        release(moved, &RESULT_OPTION);
        assert_eq!(trace(), [42]);
        // These inactive branches must never interpret scalar data as pointers.
        release(wrap(u64::MAX as u128, 1), &RESULT_OPTION);
        release(wrap(wrap(0, 0), 0), &RESULT_OPTION);
    }
}

#[test]
fn float_slots_preserve_bits_and_ieee_equality_and_unit_payloads() {
    unsafe {
        for (descriptor, negative_zero, nan) in [
            (
                &LIST_F32,
                u128::from((-0.0f32).to_bits()),
                u128::from(f32::NAN.to_bits()),
            ),
            (
                &LIST_F64,
                u128::from((-0.0f64).to_bits()),
                u128::from(f64::NAN.to_bits()),
            ),
        ] {
            let a = collection(0, 0, 0, 0, descriptor);
            let b = collection(0, 0, 0, 0, descriptor);
            plenty_release(collection(1, a, negative_zero, 0, ptr::null()) as *mut Header);
            plenty_release(collection(1, b, 0, 0, ptr::null()) as *mut Header);
            assert_eq!(collection(4, a, 0, 0, ptr::null()), negative_zero);
            assert_eq!(collection(8, a, b, 0, ptr::null()), 1);
            plenty_release(collection(3, a, 0, nan, ptr::null()) as *mut Header);
            assert_eq!(collection(8, a, a, 0, ptr::null()), 0);
            assert_eq!(collection(7, nan, a, 0, ptr::null()), 0);
            plenty_release(a as *mut Header);
            plenty_release(b as *mut Header);
        }
        let done = collection(20, 0, 0, 0, &DONE);
        collection(21, done, 0, 0, ptr::null());
        assert_eq!(collection(23, done, 0, 0, ptr::null()), 0);
        plenty_release(done as *mut Header);
    }
}

unsafe fn guard(id: u128) -> u128 {
    unsafe {
        let value = collection(30, hook as *const () as u128, 0, 0, &GUARD);
        *((value as *mut u8).add(32).cast::<u128>()) = id;
        value
    }
}
unsafe extern "C" fn hook(owner: *mut u128) {
    unsafe {
        let object = *owner as *mut u8;
        let id = *object.add(32).cast::<u128>();
        TRACE.with(|trace| trace.borrow_mut().push(id));
        // Reading the dying receiver may retain/release its immortalized header.
        plenty_retain(object.cast());
        plenty_release(object.cast());
        if id == 9 {
            plenty_release(guard(77) as *mut Header);
            TRACE.with(|trace| trace.borrow_mut().push(99));
        }
    }
}
fn trace() -> Vec<u128> {
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
        let list = collection(0, 0, 0, 0, &LIST_GUARD);
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
        let list = collection(0, 0, 0, 0, &LIST_GUARD);
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
        // Pair contains two Points; both fields reference the same immutable metadata.
        let pair = collection(30, 0, 0, 0, &PAIR);
        let a = collection(30, 0, 0, 0, &POINT);
        let b = collection(30, 0, 0, 0, &POINT);
        *((a as *mut u8).add(32).cast::<u128>()) = 3;
        *((b as *mut u8).add(32).cast::<u128>()) = 4;
        *((pair as *mut u8).add(32).cast::<u128>()) = a;
        *((pair as *mut u8).add(48).cast::<u128>()) = b;
        let copy = collection(14, pair, 0, 0, ptr::null());
        assert_eq!(collection(8, pair, copy, 0, ptr::null()), 1);
        *((a as *mut u8).add(32).cast::<u128>()) = 10;
        assert_eq!(collection(8, pair, copy, 0, ptr::null()), 0);
        plenty_release(pair as *mut Header);
        plenty_release(copy as *mut Header);
    }
}

#[test]
fn dictionaries_preserve_order_and_copy_owned_contents() {
    unsafe {
        let dict = collection(0, 0, 0, 0, &DICT);
        let list = collection(0, 0, 0, 0, &LIST_INT);
        plenty_release(collection(1, list, 42, 0, ptr::null()) as *mut Header);
        plenty_release(collection(1, dict, 1, list, ptr::null()) as *mut Header);
        plenty_release(list as *mut Header);
        let independent = collection(14, dict, 0, 0, ptr::null());
        assert_eq!(collection(8, dict, independent, 0, ptr::null()), 1);
        let values = collection(11, dict, 0, 0, &LIST_LIST);
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

unsafe extern "C" fn never_resume(_: *mut Generator, _: *mut u128) -> u8 {
    panic!("dropping must not resume a generator")
}
#[test]
fn deeply_nested_generator_frames_drop_iteratively() {
    static MANAGED: [&Type; 1] = [&GENERATOR];
    unsafe {
        let mut child = guard(1);
        for _ in 0..1000 {
            let frame = plenty_generator_new(never_resume, 1, MANAGED.as_ptr().cast());
            *frame.cast::<u8>().add(64).cast::<u128>() = child;
            child = frame as u128;
        }
        plenty_release(child as *mut Header);
        assert_eq!(trace(), [1]);
    }
}
