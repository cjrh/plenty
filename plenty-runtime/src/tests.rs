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
static DICT_TEXT: Type = Type {
    affine: true,
    key: Some(&TEXT),
    value: Some(&TEXT),
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
static TEXT: Type = scalar(b's');
static LIST_TEXT: Type = list(&TEXT);
static ALLOC_ERROR: Type = Type {
    name: "AllocError",
    variants: &[
        Variant {
            name: "OutOfMemory",
            fields: &[],
        },
        Variant {
            name: "CapacityOverflow",
            fields: &[],
        },
    ],
    ..scalar(b'B')
};
static RESULT_TEXT_LIST: Type = Type {
    affine: true,
    name: "Result[list[str], AllocError]",
    variants: &[
        Variant {
            name: "Ok",
            fields: &[&LIST_TEXT],
        },
        Variant {
            name: "Err",
            fields: &[&ALLOC_ERROR],
        },
    ],
    ..scalar(b'B')
};
static OPTION_TEXT: Type = Type {
    name: "Option[str]",
    variants: &[
        Variant {
            name: "Nothing",
            fields: &[],
        },
        Variant {
            name: "Some",
            fields: &[&TEXT],
        },
    ],
    ..scalar(b'B')
};
static RESULT_OPTION_TEXT: Type = Type {
    name: "Result[Option[str], AllocError]",
    variants: &[
        Variant {
            name: "Ok",
            fields: &[&OPTION_TEXT],
        },
        Variant {
            name: "Err",
            fields: &[&ALLOC_ERROR],
        },
    ],
    ..scalar(b'B')
};

#[test]
fn optional_dictionary_lookup_retains_values_across_update_and_destruction() {
    unsafe {
        let dictionary = collection(0, 0, 0, 0, &DICT_TEXT);
        let key = strings::new("é\0".as_bytes());
        let equal_key = strings::new("é\0".as_bytes());
        let value = strings::new("Ada🙂".as_bytes());
        let replacement = strings::new(b"Bea");
        plenty_release(
            collection(1, dictionary, key as u128, value as u128, ptr::null()) as *mut Header,
        );
        let found = collection(38, dictionary, equal_key as u128, 0, ptr::null());
        assert_eq!(found >> 64, 1);
        assert_eq!(
            collection(38, dictionary, replacement as u128, 0, ptr::null()),
            0
        );
        plenty_release(
            collection(3, dictionary, key as u128, replacement as u128, ptr::null()) as *mut Header,
        );
        plenty_release(dictionary as *mut Header);
        for text in [key, equal_key, value, replacement] {
            plenty_release(text.cast());
        }
        assert_eq!(strings::utf8(found as *const strings::Text), "Ada🙂");
        crate::aggregates::retain(found, &OPTION_TEXT);
        crate::aggregates::release(found, &OPTION_TEXT);
        assert_eq!(strings::utf8(found as *const strings::Text), "Ada🙂");
        crate::aggregates::release(found, &OPTION_TEXT);
    }
}

#[test]
fn dictionary_removal_transfers_owned_payload_and_repairs_hash_storage() {
    static OPTION_INT_LIST: Type = Type {
        affine: true,
        name: "Option[list[i64]]",
        variants: &[
            Variant {
                name: "Nothing",
                fields: &[],
            },
            Variant {
                name: "Some",
                fields: &[&LIST_INT],
            },
        ],
        ..scalar(b'B')
    };
    unsafe {
        let dictionary = collection(0, 0, 0, 0, &DICT);
        for key in 0..32 {
            let child = collection(0, 0, 0, 0, &LIST_INT);
            plenty_release(collection(1, child, key, 0, ptr::null()) as *mut Header);
            plenty_release(collection(1, dictionary, key, child, ptr::null()) as *mut Header);
            plenty_release(child as *mut Header);
        }
        for key in (0..32).step_by(2) {
            let before = collection(4, dictionary, key, 0, ptr::null());
            let removed = collection(39, dictionary, key, 0, ptr::null());
            assert_eq!(removed >> 64, 1);
            assert_eq!(crate::aggregates::payload(removed), before);
            plenty_release(before as *mut Header);
            assert_eq!(collection(39, dictionary, key, 0, ptr::null()), 0);
            assert_eq!(
                collection(4, crate::aggregates::payload(removed), 0, 0, ptr::null()),
                key
            );
            crate::aggregates::release(removed, &OPTION_INT_LIST);
        }
        for key in (1..32).step_by(2) {
            let child = collection(4, dictionary, key, 0, ptr::null());
            assert_eq!(collection(4, child, 0, 0, ptr::null()), key);
            plenty_release(child as *mut Header);
        }
        let last = collection(39, dictionary, 31, 0, ptr::null());
        plenty_release(dictionary as *mut Header);
        assert_eq!(
            collection(4, crate::aggregates::payload(last), 0, 0, ptr::null()),
            31
        );
        crate::aggregates::release(last, &OPTION_INT_LIST);
    }
}

#[test]
fn list_removal_transfers_payload_and_preserves_remaining_order() {
    unsafe {
        let list = collection(0, 0, 0, 0, &LIST_LIST);
        let mut children = [0; 3];
        for (i, child) in children.iter_mut().enumerate() {
            *child = collection(0, 0, 0, 0, &LIST_INT);
            plenty_release(collection(1, *child, i as u128, 0, ptr::null()) as *mut Header);
            plenty_release(collection(1, list, *child, 0, ptr::null()) as *mut Header);
            plenty_release(*child as *mut Header);
        }
        for index in [i64::MIN, -4, 3, i64::MAX] {
            assert_eq!(collection(40, list, index as u128, 0, ptr::null()), 0);
        }
        let removed = collection(40, list, 1, 0, ptr::null());
        assert_eq!(removed >> 64, 1);
        assert_eq!(crate::aggregates::payload(removed), children[1]);
        assert_eq!(collection(5, list, 0, 0, ptr::null()), 2);
        for (index, expected) in [children[0], children[2]].into_iter().enumerate() {
            let child = collection(4, list, index as u128, 0, ptr::null());
            assert_eq!(child, expected);
            plenty_release(child as *mut Header);
        }
        plenty_release(list as *mut Header);
        let owned = crate::aggregates::payload(removed);
        assert_eq!(collection(4, owned, 0, 0, ptr::null()), 1);
        plenty_release(owned as *mut Header);
    }
}

#[test]
fn set_discard_releases_stored_strings_and_keeps_query_owners_valid() {
    static SET_TEXT: Type = Type {
        affine: true,
        key: Some(&TEXT),
        ..scalar(b'S')
    };
    unsafe {
        let set = collection(0, 0, 0, 0, &SET_TEXT);
        for text in ["é\0🙂", "other"] {
            let stored = strings::new(text.as_bytes());
            plenty_release(collection(1, set, stored as u128, 0, ptr::null()) as *mut Header);
            plenty_release(stored.cast());
        }
        let query = strings::new("é\0🙂".as_bytes());
        let other = strings::new(b"other");
        assert_eq!(collection(41, set, query as u128, 0, ptr::null()), 1);
        assert_eq!(collection(41, set, query as u128, 0, ptr::null()), 0);
        assert_eq!(strings::utf8(query), "é\0🙂");
        assert_eq!(collection(7, other as u128, set, 0, ptr::null()), 1);
        plenty_release(collection(1, set, query as u128, 0, ptr::null()) as *mut Header);
        assert_eq!(collection(7, query as u128, set, 0, ptr::null()), 1);
        assert_eq!(collection(41, set, other as u128, 0, ptr::null()), 1);
        assert_eq!(collection(41, set, query as u128, 0, ptr::null()), 1);
        assert_eq!(collection(5, set, 0, 0, ptr::null()), 0);
        plenty_release(set as *mut Header);
        assert_eq!(strings::utf8(query), "é\0🙂");
        plenty_release(query.cast());
        plenty_release(other.cast());
    }
}

#[test]
fn list_lookup_retains_string_payload_without_transferring_the_entry() {
    unsafe {
        let list = collection(0, 0, 0, 0, &LIST_TEXT);
        let text = strings::new("é\0🙂".as_bytes());
        plenty_release(collection(1, list, text as u128, 0, ptr::null()) as *mut Header);
        plenty_release(text.cast());
        for index in [i64::MIN, -2, 1, i64::MAX] {
            assert_eq!(collection(42, list, index as u128, 0, ptr::null()), 0);
        }
        let found = collection(42, list, (-1i64) as u128, 0, ptr::null());
        assert_eq!(found >> 64, 1);
        assert_eq!(crate::aggregates::payload(found), text as u128);
        assert_eq!(collection(5, list, 0, 0, ptr::null()), 1);
        let again = collection(42, list, 0, 0, ptr::null());
        assert_eq!(found, again);
        crate::aggregates::release(again, &OPTION_TEXT);
        plenty_release(list as *mut Header);
        assert_eq!(strings::utf8(text), "é\0🙂");
        crate::aggregates::release(found, &OPTION_TEXT);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn dictionary_lookup_succeeds_when_allocation_is_disabled() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    unsafe {
        let dictionary = collection(0, 0, 0, 0, &DICT_TEXT);
        let key = strings::new(b"key");
        let equal_key = strings::new(b"key");
        let value = strings::new(b"value");
        plenty_release(
            collection(1, dictionary, key as u128, value as u128, ptr::null()) as *mut Header,
        );
        let (found, missing) = {
            let _restore = Restore;
            crate::accounting::fail_after(Some(0));
            (
                collection(38, dictionary, equal_key as u128, 0, ptr::null()),
                collection(38, dictionary, value as u128, 0, ptr::null()),
            )
        };
        assert_eq!(found >> 64, 1);
        assert_eq!(missing, 0);
        crate::aggregates::release(found, &OPTION_TEXT);
        plenty_release(dictionary as *mut Header);
        for text in [key, equal_key, value] {
            plenty_release(text.cast());
        }
    }
}

#[test]
fn checked_character_results_use_inline_tags_and_outlive_the_source() {
    unsafe {
        let text = strings::new("é🙂\0".as_bytes());
        for index in [i64::MIN, -4, 3, i64::MAX] {
            assert_eq!(
                collection(37, text as u128, index as u128, 0, ptr::null()),
                0
            );
        }
        let result = collection(37, text as u128, (-2i64) as u128, 0, ptr::null());
        assert_eq!(result >> 64, 2);
        let character = result as *const strings::Text;
        plenty_release(text.cast());
        assert_eq!(strings::utf8(character), "🙂");
        assert_eq!((*character).byte_len, 4);
        assert_eq!((*character).scalar_len, 1);
        crate::aggregates::retain(result, &RESULT_OPTION_TEXT);
        crate::aggregates::release(result, &RESULT_OPTION_TEXT);
        assert_eq!(strings::utf8(character), "🙂");
        crate::aggregates::release(result, &RESULT_OPTION_TEXT);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn checked_character_allocation_failure_and_missing_index_are_distinct() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    unsafe {
        let text = strings::new("é🙂\0".as_bytes());
        for budget in 0..=1 {
            let (missing, present) = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                (
                    collection(37, text as u128, 3, 0, ptr::null()),
                    collection(37, text as u128, 2, 0, ptr::null()),
                )
            };
            assert_eq!(missing, 0);
            if budget == 0 {
                assert_eq!(present, 1u128 << 64);
            } else {
                assert_eq!(present >> 64, 2);
                assert_eq!(strings::bytes(present as *const strings::Text), b"\0");
            }
            crate::aggregates::release(present, &RESULT_OPTION_TEXT);
            assert_eq!(strings::utf8(text), "é🙂\0");
        }
        plenty_release(text.cast());
    }
}

#[test]
fn split_output_outlives_inputs_and_preserves_exact_utf8() {
    unsafe {
        let text = strings::new("é\0::🙂::::".as_bytes());
        let separator = strings::new(b"::");
        let result = collection(36, text as u128, separator as u128, 0, &RESULT_TEXT_LIST);
        plenty_release(text.cast());
        plenty_release(separator.cast());
        assert_eq!(result >> 64, 0);
        let list = crate::aggregates::payload(result);
        assert_eq!(collection(5, list, 0, 0, ptr::null()), 4);
        for (i, expected) in ["é\0", "🙂", "", ""].iter().enumerate() {
            let part = collection(4, list, i as u128, 0, ptr::null()) as *mut strings::Text;
            assert_eq!(strings::utf8(part), *expected);
            assert_eq!((*part).scalar_len, expected.chars().count() as u64);
            plenty_release(part.cast());
        }
        crate::aggregates::release(result, &RESULT_TEXT_LIST);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn partial_split_cleanup_handles_every_allocation_failure() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    unsafe {
        let text = strings::new("é\0::🙂::::".as_bytes());
        let separator = strings::new(b"::");
        // Buffer + owner + four output strings, including the empty pieces.
        for budget in 0..=6 {
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(36, text as u128, separator as u128, 0, &RESULT_TEXT_LIST)
            };
            if budget < 6 {
                assert_eq!(result, 1u128 << 64);
            } else {
                assert_eq!(result >> 64, 0);
            }
            crate::aggregates::release(result, &RESULT_TEXT_LIST);
            assert_eq!(strings::utf8(text), "é\0::🙂::::");
            assert_eq!(strings::utf8(separator), "::");
        }
        plenty_release(text.cast());
        plenty_release(separator.cast());
    }
}

#[test]
fn fallible_text_builders_copy_exact_bytes_and_unicode_lengths() {
    unsafe {
        let first = strings::new("é\0".as_bytes());
        let second = strings::new("🙂".as_bytes());
        let separator = strings::new("界".as_bytes());
        let joined = strings::try_join(
            Some(separator),
            [first.cast_const(), second.cast_const(), first.cast_const()].into_iter(),
        )
        .unwrap();
        assert_eq!(strings::bytes(joined), "é\0界🙂界é\0".as_bytes());
        assert_eq!((*joined).scalar_len, 7);
        let combined = strings::try_concat(first, second).unwrap();
        assert_eq!(strings::bytes(combined), "é\0🙂".as_bytes());
        assert_eq!((*combined).scalar_len, 3);
        let empty = strings::try_join(Some(separator), [].into_iter()).unwrap();
        assert!(strings::bytes(empty).is_empty());
        assert_eq!((*empty).scalar_len, 0);
        for text in [joined, combined, empty, first, second, separator] {
            plenty_release(text.cast());
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn text_builders_recover_from_allocation_failure_without_consuming_inputs() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    unsafe {
        let a = strings::new(b"a\0");
        let b = strings::new(b"b");
        for budget in 0..=1 {
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                strings::try_concat(a, b)
            };
            if budget == 0 {
                assert_eq!(result, Err(crate::memory::AllocError::OutOfMemory));
            } else {
                let result = result.unwrap();
                assert_eq!(strings::bytes(result), b"a\0b");
                plenty_release(result.cast());
            }
            assert_eq!(strings::bytes(a), b"a\0");
            assert_eq!(strings::bytes(b), b"b");
        }
        plenty_release(a.cast());
        plenty_release(b.cast());
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn partial_record_and_nested_collection_copies_release_only_owned_values() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for records in [false, true] {
        let ty = if records { &PAIR } else { &LIST_LIST };
        let allocations = if records { 3 } else { 6 };
        unsafe {
            let source = collection(if records { 30 } else { 0 }, 0, 0, 0, ty);
            for i in 0..2 {
                let child = if records {
                    let child = collection(30, 0, 0, 0, &POINT);
                    collection(21, child, 0, i + 1, ptr::null());
                    collection(21, source, i, child, ptr::null());
                    child
                } else {
                    let child = collection(0, 0, 0, 0, &LIST_INT);
                    plenty_release(collection(1, child, i + 1, 0, ptr::null()) as *mut Header);
                    plenty_release(collection(1, source, child, 0, ptr::null()) as *mut Header);
                    child
                };
                plenty_release(child as *mut Header);
            }
            for budget in 0..=allocations {
                let result = {
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(budget));
                    collection(33, source, 0, 0, ty)
                };
                if budget < allocations {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    assert_eq!(result >> 64, 0);
                    let copied = crate::aggregates::payload(result);
                    assert_ne!(copied, source);
                    assert_eq!(collection(8, source, copied, 0, ty), 1);
                    plenty_release(copied as *mut Header);
                }
            }
            plenty_release(source as *mut Header);
        }
    }
}

#[test]
fn fallible_collection_headers_and_buffers_use_matching_layouts() {
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
        for capacity in [0, 16] {
            unsafe {
                let result = collection(32, capacity, 0, 0, ty);
                assert_eq!(result >> 64, 0);
                let c = crate::aggregates::payload(result);
                assert_ne!(c, 0);
                for n in 0..16 {
                    assert_eq!(collection(29, c, n, n + 1, ptr::null()), 0);
                }
                assert_eq!(collection(5, c, 0, 0, ptr::null()), 16);
                plenty_release(c as *mut Header);
                assert_eq!(collection(32, u64::MAX as u128, 0, 0, ty), 3u128 << 64);
            }
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn partial_constructor_buffers_are_freed_on_each_allocation_failure() {
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
    for (ty, allocations) in [(&LIST_INT, 2), (&MAP, 3)] {
        for budget in 0..=allocations {
            unsafe {
                let result = {
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(budget));
                    collection(32, 8, 0, 0, ty)
                };
                if budget < allocations {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    assert_eq!(result >> 64, 0);
                    plenty_release(crate::aggregates::payload(result) as *mut Header);
                }
            }
        }
    }
}

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
