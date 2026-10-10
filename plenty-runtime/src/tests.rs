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
        inline_range: kind == b'R',
        inline_bytes: 0,
        key: None,
        value: None,
        name: "",
        variants: &[],
        drop: None,
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
static UNSIGNED: Type = scalar(b'8');

#[test]
fn c_string_adapter_preserves_utf8_terminates_and_rejects_nuls() {
    // SAFETY: text operands own their allocation until released; returned buffers
    // are separately owned, and the C view never survives that owner.
    unsafe {
        for source in ["", "é🦀"] {
            let text = strings::new(source.as_bytes());
            let result = strings::try_c_string(text).ok().unwrap();
            let bytes = strings::text_bytes(result);
            assert_eq!(&bytes[..bytes.len() - 1], source.as_bytes());
            assert_eq!(bytes.last(), Some(&0));
            assert_eq!(strings::text_bytes(text), source.as_bytes());
            plenty_release(result.cast());
            plenty_release(text.cast());
        }
        let text = strings::new(b"a\0b");
        assert!(matches!(
            strings::try_c_string(text),
            Err(strings::CStrError::EmbeddedNul)
        ));
        plenty_release(text.cast());
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn c_string_conversion_failure_preserves_source_without_allocating_an_error() {
    // SAFETY: the source remains live, and allocation failure returns no buffer.
    unsafe {
        let text = strings::new(b"hello");
        crate::accounting::fail_after(Some(0));
        let result = strings::try_c_string(text);
        crate::accounting::fail_after(None);
        assert!(matches!(
            result,
            Err(strings::CStrError::Allocation(
                crate::memory::AllocError::OutOfMemory
            ))
        ));
        assert_eq!(strings::text_bytes(text), b"hello");
        plenty_release(text.cast());
    }
}
static UNSIGNED_RANGE: Type = Type {
    key: Some(&UNSIGNED),
    ..scalar(b'R')
};

#[test]
fn unsigned_ranges_keep_full_width_bounds_and_signed_steps() {
    // SAFETY: operands and metadata match; stack payloads outlive every read.
    unsafe {
        let stop = u64::MAX as u128;
        let range = crate::ranges::Range::new((stop - 3) as u64, stop as u64, 1, false);
        let range = &range as *const crate::ranges::Range as u128;
        assert_eq!(collection(5, range, 0, 0, &UNSIGNED_RANGE), 3);
        assert_eq!(collection(4, range, 2, 0, &UNSIGNED_RANGE), stop - 1);
        assert_eq!(collection(7, stop - 2, range, 0, &UNSIGNED_RANGE), 1);
        let range = crate::ranges::Range::new(stop as u64, (stop - 3) as u64, -1, false);
        let range = &range as *const crate::ranges::Range as u128;
        assert_eq!(collection(4, range, 2, 0, &UNSIGNED_RANGE), stop - 2);
    }
}

#[test]
fn inline_range_rows_relocate_and_removed_values_outlive_shifts() {
    use crate::entries::{Entries, Entry};
    use crate::ranges::Range;
    let mut entries = Entries::new(&UNSIGNED_RANGE, None);
    entries.try_reserve(2).unwrap();
    let mut source = Range::new(10, 14, 1, false);
    // SAFETY: each insertion copies the live source into reserved inline storage;
    // every read uses the current row or the still-live removal scratch buffer.
    unsafe {
        entries.push(Entry {
            key: &source as *const Range as u128,
            value: 0,
        });
        source = Range::new(20, 25, 1, false);
        entries.push(Entry {
            key: &source as *const Range as u128,
            value: 0,
        });
        entries.try_reserve(4096).unwrap();
        assert_eq!((*(entries.get(0).key as *const Range)).start, 10);
        assert_eq!((*(entries.get(1).key as *const Range)).start, 20);
        entries.reverse();
        let removed = entries.remove(0);
        let snapshot = *(removed.key as *const Range);
        assert_eq!(snapshot.start, 20);
        assert_eq!((*(entries.get(0).key as *const Range)).start, 10);
        entries.clear();
        assert_eq!(snapshot.len, 5);
    }
}

#[test]
fn inline_range_sum_slots_copy_and_relocate_only_the_active_payload() {
    use crate::aggregates::{payload, wrap};
    use crate::ranges::{self, Range};
    static OPTIONAL: Type = Type {
        inline_range: true,
        variants: &[
            Variant {
                name: "Nothing",
                fields: &[],
            },
            Variant {
                name: "Some",
                fields: &[&UNSIGNED_RANGE],
            },
        ],
        ..scalar(b'B')
    };
    let source = Range::new(100, 104, 1, false);
    let mut first = [0u128; 3];
    let mut second = [0u128; 3];
    // SAFETY: both typed slots reserve a header plus their inline range payload.
    unsafe {
        ranges::store(
            first.as_mut_ptr(),
            wrap(&source as *const Range as u128, 1),
            &OPTIONAL,
        );
        second.copy_from_slice(&first);
        ranges::relocate(second.as_mut_ptr(), &OPTIONAL);
        first.fill(0);
        assert_eq!((*(payload(second[0]) as *const Range)).start, 100);
        assert_eq!((second[0] >> 64) & 1, 1);
        ranges::store(second.as_mut_ptr(), wrap(0, 0), &OPTIONAL);
        assert_eq!(second[0], 0);
    }
}

#[test]
fn ordered_inline_rows_skip_dead_slots_during_relocation_and_drain() {
    use crate::entries::{Entries, Entry};
    use crate::ranges::Range;
    let mut rows = Entries::ordered(&INTEGER, Some(&UNSIGNED_RANGE));
    rows.try_reserve(4).unwrap();
    // SAFETY: sources are copied into reserved inline rows; extracted payloads
    // are read before mutation, and iterators keep the backing buffers alive.
    unsafe {
        for key in 0..4 {
            let range = Range::new(key as u64, 10, 1, false);
            rows.push(Entry {
                key,
                value: &range as *const Range as u128,
            });
        }
        let removed = rows.remove(1);
        assert_eq!((*(removed.value as *const Range)).start, 1);
        rows.remove(0);
        rows.try_reserve(1000).unwrap();
        let range = Range::new(20, 30, 1, false);
        rows.push(Entry {
            key: 20,
            value: &range as *const Range as u128,
        });
        assert_eq!(rows.iter().map(|e| e.key).collect::<Vec<_>>(), [2, 3, 20]);
        assert_eq!(
            rows.iter().rev().map(|e| e.key).collect::<Vec<_>>(),
            [20, 3, 2]
        );
        let capacity = rows.capacity();
        for entry in rows.drain() {
            assert_eq!((*(entry.value as *const Range)).start, entry.key as u64);
        }
        assert_eq!(rows.len(), 0);
        assert_eq!(rows.capacity(), capacity);
        rows.push(Entry {
            key: 20,
            value: &range as *const Range as u128,
        });
        assert_eq!(rows.first(), Some(0));
        assert_eq!(rows.next(0), None);
    }
}

#[test]
fn inline_range_dictionary_removal_and_record_copy_keep_valid_payloads() {
    use crate::aggregates::payload;
    use crate::ranges::{self, Range};
    static MAP: Type = Type {
        value: Some(&UNSIGNED_RANGE),
        ..DICT
    };
    static RECORD: Type = Type {
        affine: true,
        inline_bytes: 64,
        name: "Ranges",
        variants: &[
            Variant {
                name: "range",
                fields: &[&UNSIGNED_RANGE],
            },
            Variant {
                name: "number",
                fields: &[&INTEGER],
            },
        ],
        ..scalar(b'C')
    };
    // SAFETY: inputs remain live for each copy and all heap owners are released.
    unsafe {
        let source = Range::new(10, 13, 1, false);
        let address = &source as *const Range as u128;
        let map = collection(0, 0, 0, 0, &MAP);
        for key in 0..20 {
            assert_eq!(collection(29, map, key, address, ptr::null()), 0);
        }
        let removed = collection(39, map, 3, 0, ptr::null());
        assert_eq!((*(payload(removed) as *const Range)).start, 10);
        // Inline class storage: a range slot (3 words), then a number slot.
        let mut record = [0u128; 4];
        ranges::store(record.as_mut_ptr(), address, &UNSIGNED_RANGE);
        record[3] = 42;
        let mut copied = [0u128; 4];
        let duplicate = payload(collection(
            33,
            record.as_mut_ptr() as u128,
            copied.as_mut_ptr() as u128,
            0,
            &RECORD,
        ));
        record.fill(0);
        assert_eq!(duplicate as u64, copied.as_ptr() as u64);
        assert_eq!(copied[0] as u64, copied.as_ptr().add(1) as u64);
        assert_eq!((*(copied[0] as u64 as *const Range)).len, 3);
        assert_eq!(copied[3], 42);
        plenty_release(map as *mut Header);
    }
}
static FLOAT32: Type = scalar(b'f');
static FLOAT64: Type = scalar(b'd');
static LIST_F32: Type = list(&FLOAT32);
static LIST_F64: Type = list(&FLOAT64);
static LIST_INT: Type = list(&INTEGER);
static LIST_LIST: Type = list(&LIST_INT);
static GUARD_CLASS: Type = Type {
    affine: true,
    name: "Guard",
    inline_bytes: 32,
    variants: &[Variant {
        name: "id",
        fields: &[&INTEGER],
    }],
    drop: Some(hook),
    ..scalar(b'C')
};
/// A boxed guard: a heap owner whose content runs a hook when dropped.
static GUARD: Type = Type {
    affine: true,
    key: Some(&GUARD_CLASS),
    ..scalar(b'O')
};
static LIST_GUARD: Type = list(&GUARD);
static POINT: Type = Type {
    affine: true,
    inline_bytes: 16,
    name: "Point",
    variants: &[Variant {
        name: "x",
        fields: &[&INTEGER],
    }],
    ..scalar(b'C')
};
static PAIR: Type = Type {
    affine: true,
    inline_bytes: 64,
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

#[test]
fn reader_lines_release_partial_prefixes_and_own_their_results() {
    unsafe {
        let lines = crate::aggregates::try_reader_lines(
            &mut std::io::Cursor::new(b"one\r\ntwo"),
            &mut false,
            &LIST_TEXT,
        )
        .unwrap();
        assert_eq!(collection(5, lines, 0, 0, ptr::null()), 2);
        let last = collection(4, lines, 1, 0, ptr::null());
        plenty_release(lines as *mut Header);
        assert_eq!(strings::text(last as *const strings::Text), "two");
        plenty_release(last as *mut Header);
        assert!(crate::aggregates::try_reader_lines(
            &mut std::io::Cursor::new(b"one\n\xff\n"),
            &mut false,
            &LIST_TEXT
        )
        .is_err());
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn reader_line_prefix_cleanup_survives_allocation_failures() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    let mut success = false;
    for budget in 0..=12 {
        let result = {
            let _restore = Restore;
            crate::accounting::fail_after(Some(budget));
            unsafe {
                crate::aggregates::try_reader_lines(
                    &mut std::io::Cursor::new(b"one\ntwo\nthree"),
                    &mut false,
                    &LIST_TEXT,
                )
            }
        };
        if let Ok(value) = result {
            success = true;
            unsafe {
                plenty_release(value as *mut Header);
            }
        }
    }
    assert!(success);
}

#[test]
fn text_snapshots_own_members_and_clean_partial_validation_failures() {
    unsafe {
        let value =
            crate::aggregates::try_text_list([Ok("first"), Ok("é\0")].into_iter(), &LIST_TEXT)
                .unwrap();
        let text = collection(4, value, 1, 0, ptr::null());
        plenty_release(value as *mut Header);
        assert_eq!(strings::text(text as *const strings::Text), "é\0");
        plenty_release(text as *mut Header);
        assert!(crate::aggregates::try_text_list(
            [Ok("first"), Err(crate::text_io::Error::InvalidUtf8)].into_iter(),
            &LIST_TEXT
        )
        .is_err());
    }
}
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

#[test]
fn split_line_results_own_members_after_source_destruction() {
    unsafe {
        let source = strings::new("é\u{2028}\u{85}🦀\r\n".as_bytes());
        let result = collection(107, source as u128, 0, 0, &RESULT_TEXT_LIST);
        assert_eq!(result >> 64, 0);
        plenty_release(source.cast());
        assert_eq!(collection(5, result, 0, 0, ptr::null()), 3);
        let last = collection(4, result, 2, 0, ptr::null());
        plenty_release(result as *mut Header);
        assert_eq!(strings::text(last as *const strings::Text), "🦀");
        plenty_release(last as *mut Header);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn split_line_prefixes_are_cleaned_on_allocation_failure() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for budget in 0..=5 {
        unsafe {
            let source = strings::new(b"first line\nsecond line\nthird line");
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(107, source as u128, 1, 0, &RESULT_TEXT_LIST)
            };
            assert_eq!(result >> 64, u128::from(budget < 5));
            plenty_release(source.cast());
            crate::aggregates::release(result, &RESULT_TEXT_LIST);
        }
    }
}
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
        assert_eq!(strings::text(found as *const strings::Text), "Ada🙂");
        crate::aggregates::retain(found, &OPTION_TEXT);
        crate::aggregates::release(found, &OPTION_TEXT);
        assert_eq!(strings::text(found as *const strings::Text), "Ada🙂");
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
        assert_eq!(strings::text(query), "é\0🙂");
        assert_eq!(collection(7, other as u128, set, 0, ptr::null()), 1);
        plenty_release(collection(1, set, query as u128, 0, ptr::null()) as *mut Header);
        assert_eq!(collection(7, query as u128, set, 0, ptr::null()), 1);
        assert_eq!(collection(41, set, other as u128, 0, ptr::null()), 1);
        assert_eq!(collection(41, set, query as u128, 0, ptr::null()), 1);
        assert_eq!(collection(5, set, 0, 0, ptr::null()), 0);
        plenty_release(set as *mut Header);
        assert_eq!(strings::text(query), "é\0🙂");
        plenty_release(query.cast());
        plenty_release(other.cast());
    }
}

#[test]
fn set_relations_allow_identical_borrowed_operands() {
    static SET: Type = Type {
        affine: true,
        key: Some(&TEXT),
        ..scalar(b'S')
    };
    unsafe {
        let a = collection(0, 0, 0, 0, &SET);
        let b = collection(0, 0, 0, 0, &SET);
        assert_eq!(collection(64, a, a, 0, ptr::null()), 1);
        for set in [a, b] {
            let text = strings::new("é\0🙂".as_bytes());
            plenty_release(collection(1, set, text as u128, 0, ptr::null()) as *mut Header);
            plenty_release(text.cast());
        }
        assert_eq!(collection(62, a, b, 0, ptr::null()), 1);
        assert_eq!(collection(63, b, a, 0, ptr::null()), 1);
        assert_eq!(collection(64, a, a, 0, ptr::null()), 0);
        plenty_release(a as *mut Header);
        plenty_release(b as *mut Header);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn set_algebra_cleans_partial_storage_and_retains_result_members() {
    static SET: Type = Type {
        affine: true,
        key: Some(&TEXT),
        ..scalar(b'S')
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for (op, length) in [(65, 3), (66, 1), (67, 1), (68, 2)] {
        for budget in 0..=4 {
            unsafe {
                let a = collection(0, 0, 0, 0, &SET);
                let b = collection(0, 0, 0, 0, &SET);
                for (set, words) in [(a, ["é", "left"]), (b, ["é", "right"])] {
                    for word in words {
                        let text = strings::new(word.as_bytes());
                        plenty_release(
                            collection(1, set, text as u128, 0, ptr::null()) as *mut Header
                        );
                        plenty_release(text.cast());
                    }
                }
                let reset = Restore;
                crate::accounting::fail_after(Some(budget));
                let result = collection(op, a, b, 0, ptr::null());
                drop(reset);
                assert_eq!(collection(5, a, 0, 0, ptr::null()), 2);
                assert_eq!(collection(5, b, 0, 0, ptr::null()), 2);
                plenty_release(a as *mut Header);
                plenty_release(b as *mut Header);
                if budget < 4 {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    let result = crate::aggregates::payload(result);
                    assert_eq!(collection(5, result, 0, 0, ptr::null()), length);
                    // Reads after both inputs die validate retained string owners.
                    for i in 0..length {
                        let text = collection(6, result, i, 0, ptr::null());
                        assert!(!strings::text(text as *const strings::Text).is_empty());
                        plenty_release(text as *mut Header);
                    }
                    plenty_release(result as *mut Header);
                }
            }
        }
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
        assert_eq!(strings::text(text), "é\0🙂");
        crate::aggregates::release(found, &OPTION_TEXT);
    }
}

#[test]
fn set_filter_releases_removed_strings_and_keeps_source_owners() {
    static SET: Type = Type {
        affine: true,
        key: Some(&TEXT),
        ..scalar(b'S')
    };
    for (op, expected) in [(69, "é"), (70, "remove")] {
        unsafe {
            let target = collection(0, 0, 0, 0, &SET);
            let other = collection(0, 0, 0, 0, &SET);
            for (set, words) in [(target, ["é", "remove"]), (other, ["é", "extra"])] {
                for word in words {
                    let text = strings::new(word.as_bytes());
                    plenty_release(collection(1, set, text as u128, 0, ptr::null()) as *mut Header);
                    plenty_release(text.cast());
                }
            }
            plenty_release(collection(op, target, other, 0, ptr::null()) as *mut Header);
            plenty_release(other as *mut Header);
            assert_eq!(collection(5, target, 0, 0, ptr::null()), 1);
            let kept = collection(6, target, 0, 0, ptr::null());
            assert_eq!(strings::text(kept as *const strings::Text), expected);
            plenty_release(target as *mut Header);
            assert_eq!(strings::text(kept as *const strings::Text), expected);
            plenty_release(kept as *mut Header);
        }
    }
}

#[test]
fn list_queries_compare_float_values_and_return_inline_positions() {
    unsafe {
        let list = collection(0, 0, 0, 0, &LIST_F64);
        for value in [f64::NAN, -0.0, 1.5, 0.0] {
            plenty_release(
                collection(1, list, value.to_bits() as u128, 0, ptr::null()) as *mut Header
            );
        }
        assert_eq!(collection(73, list, 0, 0, ptr::null()), 2);
        assert_eq!(collection(74, list, 0, 0, ptr::null()), (1u128 << 64) | 1);
        assert_eq!(collection(75, list, 0, 0, ptr::null()), (1u128 << 64) | 3);
        for op in [73, 74, 75] {
            assert_eq!(
                collection(op, list, f64::NAN.to_bits() as u128, 0, ptr::null()),
                0
            );
        }
        plenty_release(list as *mut Header);
    }
}

#[test]
fn classification_reads_utf8_with_explicit_lengths() {
    unsafe {
        for (text, ascii, space) in [
            ("", 1, 0),
            (" \t", 1, 1),
            ("\0", 1, 0),
            ("\u{a0}\u{3000}", 0, 1),
            ("\u{200b}", 0, 0),
        ] {
            let text = strings::new(text.as_bytes());
            assert_eq!(collection(76, text as u128, 0, 0, ptr::null()), ascii);
            assert_eq!(collection(77, text as u128, 0, 0, ptr::null()), space);
            plenty_release(text.cast());
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn removed_affixes_have_independent_storage_and_recoverable_failure() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for (op, expected) in [(71, "🙂🙂é"), (72, "é🙂🙂")] {
        for budget in 0..=1 {
            unsafe {
                let source = strings::new("é🙂🙂é".as_bytes());
                let affix = strings::new("é".as_bytes());
                let reset = Restore;
                crate::accounting::fail_after(Some(budget));
                let result = collection(op, source as u128, affix as u128, 0, ptr::null());
                drop(reset);
                plenty_release(source.cast());
                plenty_release(affix.cast());
                if budget == 0 {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    let result = crate::aggregates::payload(result) as *mut strings::Text;
                    assert_eq!(strings::text(result), expected);
                    assert_eq!(strings::scalar_len(result), 3);
                    plenty_release(result.cast());
                }
            }
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn set_update_preserves_string_owners_across_duplicate_and_failure_paths() {
    static SET_TEXT: Type = Type {
        kind: b'S',
        ..list(&TEXT)
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for budget in 0..=3 {
        unsafe {
            let target = collection(0, 0, 0, 0, &SET_TEXT);
            let stored = strings::new(b"key0");
            plenty_release(collection(1, target, stored as u128, 0, ptr::null()) as *mut Header);
            plenty_release(stored.cast());
            let source = collection(0, 0, 0, 0, &SET_TEXT);
            let query = strings::new(b"key0");
            plenty_release(collection(1, source, query as u128, 0, ptr::null()) as *mut Header);
            for n in 1..10 {
                let text = strings::new(format!("key{n}").as_bytes());
                plenty_release(collection(1, source, text as u128, 0, ptr::null()) as *mut Header);
                plenty_release(text.cast());
            }
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(61, target, source, 0, ptr::null())
            };
            assert_eq!(result, if budget < 3 { 1u128 << 64 } else { 0 });
            assert_eq!(
                collection(5, target, 0, 0, ptr::null()),
                if budget < 3 { 1 } else { 10 }
            );
            assert_eq!(
                collection(5, source, 0, 0, ptr::null()),
                if budget < 3 { 10 } else { 0 }
            );
            assert_eq!(collection(7, query as u128, target, 0, ptr::null()), 1);
            plenty_release(source as *mut Header);
            plenty_release(target as *mut Header);
            assert_eq!(strings::text(query), "key0");
            plenty_release(query.cast());
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn dictionary_update_reserves_all_storage_before_transferring_owned_values() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for budget in 0..=3 {
        unsafe {
            let target = collection(0, 0, 0, 0, &DICT);
            let old = collection(0, 0, 0, 0, &LIST_INT);
            plenty_release(collection(1, old, 99, 0, ptr::null()) as *mut Header);
            plenty_release(collection(1, target, 0, old, ptr::null()) as *mut Header);
            let source = collection(0, 0, 0, 0, &DICT);
            for n in 0..10 {
                let child = collection(0, 0, 0, 0, &LIST_INT);
                plenty_release(collection(1, child, n, 0, ptr::null()) as *mut Header);
                plenty_release(collection(1, source, n, child, ptr::null()) as *mut Header);
            }
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(60, target, source, 0, ptr::null())
            };
            assert_eq!(result, if budget < 3 { 1u128 << 64 } else { 0 });
            assert_eq!(
                collection(5, target, 0, 0, ptr::null()),
                if budget < 3 { 1 } else { 10 }
            );
            assert_eq!(
                collection(5, source, 0, 0, ptr::null()),
                if budget < 3 { 10 } else { 0 }
            );
            plenty_release(source as *mut Header);
            for n in 0..if budget < 3 { 1 } else { 10 } {
                let child = collection(4, target, n, 0, ptr::null());
                assert_eq!(
                    collection(4, child, 0, 0, ptr::null()),
                    if budget < 3 { 99 } else { n }
                );
                plenty_release(child as *mut Header);
            }
            plenty_release(target as *mut Header);
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn list_extension_reserves_before_transferring_owned_entries() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for budget in 0..=1 {
        unsafe {
            let destination = collection(0, 0, 0, 0, &LIST_LIST);
            let source = collection(0, 0, 0, 0, &LIST_LIST);
            let child = collection(0, 0, 0, 0, &LIST_INT);
            plenty_release(collection(1, child, 42, 0, ptr::null()) as *mut Header);
            plenty_release(collection(1, source, child, 0, ptr::null()) as *mut Header);
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(59, destination, source, 0, ptr::null())
            };
            assert_eq!(result, if budget == 0 { 1u128 << 64 } else { 0 });
            assert_eq!(
                collection(5, destination, 0, 0, ptr::null()),
                budget as u128
            );
            assert_eq!(
                collection(5, source, 0, 0, ptr::null()),
                (1 - budget) as u128
            );
            if budget == 1 {
                plenty_release(source as *mut Header);
                let stored = collection(4, destination, 0, 0, ptr::null());
                assert_eq!(stored, child);
                assert_eq!(collection(4, stored, 0, 0, ptr::null()), 42);
                plenty_release(stored as *mut Header);
            } else {
                plenty_release(source as *mut Header);
            }
            plenty_release(destination as *mut Header);
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn dictionary_clear_releases_entries_but_reuses_buffers() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    unsafe {
        let source = collection(0, 0, 0, 0, &DICT_TEXT);
        let text = strings::new("é\0🙂".as_bytes());
        plenty_release(
            collection(1, source, text as u128, text as u128, ptr::null()) as *mut Header,
        );
        let saved = collection(4, source, text as u128, 0, ptr::null());
        plenty_release(text.cast());
        let inserted = {
            let _restore = Restore;
            crate::accounting::fail_after(Some(0));
            plenty_release(collection(58, source, 0, 0, ptr::null()) as *mut Header);
            collection(29, source, saved, saved, ptr::null())
        };
        assert_eq!(inserted, 0);
        assert_eq!(collection(5, source, 0, 0, ptr::null()), 1);
        assert_eq!(collection(7, saved, source, 0, ptr::null()), 1);
        plenty_release(source as *mut Header);
        assert_eq!(strings::text(saved as *const _), "é\0🙂");
        plenty_release(saved as *mut Header);
    }
}

#[test]
fn list_reverse_reorders_owned_slots_without_duplication() {
    unsafe {
        let source = collection(0, 0, 0, 0, &LIST_LIST);
        let child = collection(0, 0, 0, 0, &LIST_INT);
        plenty_release(collection(1, child, 42, 0, ptr::null()) as *mut Header);
        plenty_release(collection(1, source, child, 0, ptr::null()) as *mut Header);
        let other = collection(0, 0, 0, 0, &LIST_INT);
        plenty_release(collection(1, source, other, 0, ptr::null()) as *mut Header);
        plenty_release(collection(57, source, 0, 0, ptr::null()) as *mut Header);
        let found = collection(4, source, 1, 0, ptr::null());
        assert_eq!(found, child);
        plenty_release(source as *mut Header);
        assert_eq!(collection(4, found, 0, 0, ptr::null()), 42);
        plenty_release(found as *mut Header);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn repeated_strings_fill_checked_storage_without_temporary_allocations() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for count in [-1i64, 0, 1, 2, 3, 7, 31] {
        for budget in 0..=1 {
            unsafe {
                let source = strings::new("é\0🙂".as_bytes());
                let result = {
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(budget));
                    collection(56, source as u128, count as u128, 0, ptr::null())
                };
                plenty_release(source.cast());
                if budget == 0 && "é\0🙂".len() * count.max(0) as usize > strings::INLINE_MAX {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    assert_eq!(result >> 64, 0);
                    let output = crate::aggregates::payload(result) as *mut crate::strings::Text;
                    assert_eq!(strings::text(output), "é\0🙂".repeat(count.max(0) as usize));
                    assert_eq!(strings::scalar_len(output), count.max(0) as u64 * 3);
                    plenty_release(output.cast());
                }
            }
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn trimmed_strings_have_independent_storage_and_recover_from_allocation_failure() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for (op, expected) in [(53, "é\0🙂"), (54, "é\0🙂 "), (55, "　é\0🙂")] {
        for budget in 0..=1 {
            unsafe {
                let source = strings::new("　é\0🙂 ".as_bytes());
                let result = {
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(budget));
                    collection(op, source as u128, 0, 0, ptr::null())
                };
                assert_eq!(strings::text(source), "　é\0🙂 ");
                plenty_release(source.cast());
                if budget == 0 && expected.len() > strings::INLINE_MAX {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    assert_eq!(result >> 64, 0);
                    let output = crate::aggregates::payload(result) as *mut crate::strings::Text;
                    assert_eq!(strings::text(output), expected);
                    assert_eq!(strings::scalar_len(output), expected.chars().count() as u64);
                    plenty_release(output.cast());
                }
            }
        }
    }
}

#[test]
fn text_search_returns_scalar_positions_without_borrowed_result_storage() {
    unsafe {
        let text = strings::new("é🙂\0é🙂".as_bytes());
        let needle = strings::new("🙂".as_bytes());
        let first = collection(50, text as u128, needle as u128, 0, ptr::null());
        let last = collection(51, text as u128, needle as u128, 0, ptr::null());
        plenty_release(text.cast());
        plenty_release(needle.cast());
        assert_eq!(first, (1u128 << 64) | 1);
        assert_eq!(last, (1u128 << 64) | 4);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn string_replacement_is_fallible_and_outputs_survive_all_inputs() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for (text, old, new, expected) in [
        ("Aé\0🙂é", "é", "中🙂", "A中🙂\0🙂中🙂"),
        ("é\0", "", "🙂", "🙂é🙂\0🙂"),
        ("", "", "", ""),
        ("abc", "x", "y", "abc"),
        ("abc", "abc", "", ""),
    ] {
        for budget in 0..=1 {
            unsafe {
                let source = strings::new(text.as_bytes());
                let old_text = strings::new(old.as_bytes());
                let new_text = strings::new(new.as_bytes());
                let result = {
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(budget));
                    collection(
                        47,
                        source as u128,
                        old_text as u128,
                        new_text as u128,
                        ptr::null(),
                    )
                };
                assert_eq!(strings::text(source), text);
                assert_eq!(strings::text(old_text), old);
                assert_eq!(strings::text(new_text), new);
                plenty_release(source.cast());
                plenty_release(old_text.cast());
                plenty_release(new_text.cast());
                if budget == 0 && expected.len() > strings::INLINE_MAX {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    assert_eq!(result >> 64, 0);
                    let output = crate::aggregates::payload(result) as *mut crate::strings::Text;
                    assert_eq!(strings::text(output), expected);
                    assert_eq!(strings::scalar_len(output), expected.chars().count() as u64);
                    plenty_release(output.cast());
                }
            }
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn string_slice_failure_and_utf8_result_lifetime() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for (start, stop, expected) in [(1, 4, "é\0🙂"), (4, 1, ""), (i64::MIN, i64::MAX, "Aé\0🙂Z")]
    {
        for budget in 0..=1 {
            unsafe {
                let source = strings::new("Aé\0🙂Z".as_bytes());
                let result = {
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(budget));
                    collection(46, source as u128, start as u128, stop as u128, ptr::null())
                };
                assert_eq!(strings::text(source), "Aé\0🙂Z");
                plenty_release(source.cast());
                if budget == 0 && expected.len() > strings::INLINE_MAX {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    assert_eq!(result >> 64, 0);
                    let text = crate::aggregates::payload(result) as *mut crate::strings::Text;
                    assert_eq!(strings::text(text), expected);
                    assert_eq!(strings::scalar_len(text), expected.chars().count() as u64);
                    plenty_release(text.cast());
                }
            }
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn list_slices_transfer_only_selected_payloads_after_reserving_storage() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for budget in 0..=2 {
        unsafe {
            let source = collection(0, 0, 0, 0, &LIST_LIST);
            let mut children = [0; 3];
            for (i, child) in children.iter_mut().enumerate() {
                *child = collection(0, 0, 0, 0, &LIST_INT);
                plenty_release(collection(1, *child, i as u128, 0, ptr::null()) as *mut Header);
                plenty_release(collection(1, source, *child, 0, ptr::null()) as *mut Header);
            }
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(45, source, 1, 2, &LIST_LIST)
            };
            if budget < 2 {
                assert_eq!(result, 1u128 << 64);
                for (i, child) in children.iter().enumerate() {
                    let stored = collection(4, source, i as u128, 0, ptr::null());
                    assert_eq!(stored, *child);
                    plenty_release(stored as *mut Header);
                }
                plenty_release(source as *mut Header);
            } else {
                assert_eq!(result >> 64, 0);
                plenty_release(source as *mut Header);
                let output = crate::aggregates::payload(result);
                let child = collection(4, output, 0, 0, ptr::null());
                assert_eq!(child, children[1]);
                assert_eq!(collection(4, child, 0, 0, ptr::null()), 1);
                plenty_release(child as *mut Header);
                plenty_release(output as *mut Header);
            }
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn dictionary_snapshots_retain_strings_only_after_successful_reservation() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for op in [43, 44] {
        for budget in 0..=2 {
            unsafe {
                let source = collection(0, 0, 0, 0, &DICT_TEXT);
                let key = strings::new(b"key");
                let value = strings::new("é\0🙂".as_bytes());
                plenty_release(
                    collection(1, source, key as u128, value as u128, ptr::null()) as *mut Header,
                );
                let result = {
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(budget));
                    collection(op, source, 0, 0, &RESULT_TEXT_LIST)
                };
                let stored = collection(4, source, key as u128, 0, ptr::null());
                assert_eq!(stored, value as u128);
                plenty_release(stored as *mut Header);
                plenty_release(key.cast());
                plenty_release(value.cast());
                plenty_release(source as *mut Header);
                if budget < 2 {
                    assert_eq!(result, 1u128 << 64);
                } else {
                    assert_eq!(result >> 64, 0);
                    let list = crate::aggregates::payload(result);
                    let item = collection(4, list, 0, 0, ptr::null());
                    assert_eq!(item, if op == 43 { key as u128 } else { value as u128 });
                    assert_eq!(
                        strings::text(item as *const _),
                        if op == 43 { "key" } else { "é\0🙂" }
                    );
                    plenty_release(item as *mut Header);
                    crate::aggregates::release(result, &RESULT_TEXT_LIST);
                }
            }
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn dictionary_snapshot_transfers_owned_payload_only_on_success() {
    static RESULT_LISTS: Type = Type {
        affine: true,
        variants: &[
            Variant {
                name: "Ok",
                fields: &[&LIST_LIST],
            },
            Variant {
                name: "Err",
                fields: &[&ALLOC_ERROR],
            },
        ],
        ..scalar(b'B')
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for budget in 0..=2 {
        unsafe {
            let source = collection(0, 0, 0, 0, &DICT);
            let child = collection(0, 0, 0, 0, &LIST_INT);
            plenty_release(collection(1, child, 42, 0, ptr::null()) as *mut Header);
            plenty_release(collection(1, source, 1, child, ptr::null()) as *mut Header);
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(44, source, 0, 0, &RESULT_LISTS)
            };
            if budget < 2 {
                assert_eq!(result, 1u128 << 64);
                let stored = collection(4, source, 1, 0, ptr::null());
                assert_eq!(stored, child);
                plenty_release(stored as *mut Header);
            }
            plenty_release(source as *mut Header);
            if budget == 2 {
                assert_eq!(result >> 64, 0);
                let list = crate::aggregates::payload(result);
                let stored = collection(4, list, 0, 0, ptr::null());
                assert_eq!(stored, child);
                assert_eq!(collection(4, stored, 0, 0, ptr::null()), 42);
                plenty_release(stored as *mut Header);
                crate::aggregates::release(result, &RESULT_LISTS);
            }
        }
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
fn character_lookup_returns_inline_options_that_outlive_the_source() {
    unsafe {
        let text = strings::new("é🙂\0".as_bytes());
        for index in [i64::MIN, -4, 3, i64::MAX] {
            assert_eq!(
                collection(37, text as u128, index as u128, 0, ptr::null()),
                0
            );
        }
        let result = collection(37, text as u128, (-2i64) as u128, 0, ptr::null());
        assert_eq!(result >> 64, 1);
        let character = result as *const strings::Text;
        plenty_release(text.cast());
        assert_eq!(strings::text(character), "🙂");
        assert_eq!(strings::byte_len(character), 4);
        assert_eq!(strings::scalar_len(character), 1);
        crate::aggregates::retain(result, &OPTION_TEXT);
        crate::aggregates::release(result, &OPTION_TEXT);
        assert_eq!(strings::text(character), "🙂");
        crate::aggregates::release(result, &OPTION_TEXT);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn character_lookup_never_allocates() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    unsafe {
        let text = strings::new("é🙂\0".as_bytes());
        let (missing, present) = {
            let _restore = Restore;
            crate::accounting::fail_after(Some(0));
            (
                collection(37, text as u128, 3, 0, ptr::null()),
                collection(37, text as u128, 2, 0, ptr::null()),
            )
        };
        assert_eq!(missing, 0);
        assert_eq!(present >> 64, 1);
        assert_eq!(strings::text_bytes(present as *const strings::Text), b"\0");
        crate::aggregates::release(present, &OPTION_TEXT);
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
            assert_eq!(strings::text(part), *expected);
            assert_eq!(strings::scalar_len(part), expected.chars().count() as u64);
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
        // Buffer + owner; every piece is short enough to be stored inline.
        for budget in 0..=2 {
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(36, text as u128, separator as u128, 0, &RESULT_TEXT_LIST)
            };
            if budget < 2 {
                assert_eq!(result, 1u128 << 64);
            } else {
                assert_eq!(result >> 64, 0);
            }
            crate::aggregates::release(result, &RESULT_TEXT_LIST);
            assert_eq!(strings::text(text), "é\0::🙂::::");
            assert_eq!(strings::text(separator), "::");
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
        assert_eq!(strings::text_bytes(joined), "é\0界🙂界é\0".as_bytes());
        assert_eq!(strings::scalar_len(joined), 7);
        let combined = strings::try_concat(first, second).unwrap();
        assert_eq!(strings::text_bytes(combined), "é\0🙂".as_bytes());
        assert_eq!(strings::scalar_len(combined), 3);
        let empty = strings::try_join(Some(separator), [].into_iter()).unwrap();
        assert!(strings::text_bytes(empty).is_empty());
        assert_eq!(strings::scalar_len(empty), 0);
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
        let a = strings::new(b"abcd\0");
        let b = strings::new(b"efgh");
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
                assert_eq!(strings::text_bytes(result), b"abcd\0efgh");
                plenty_release(result.cast());
            }
            assert_eq!(strings::text_bytes(a), b"abcd\0");
            assert_eq!(strings::text_bytes(b), b"efgh");
        }
        plenty_release(a.cast());
        plenty_release(b.cast());
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn partial_nested_collection_copies_release_only_owned_values() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    let allocations = 6;
    unsafe {
        let source = collection(0, 0, 0, 0, &LIST_LIST);
        for i in 0..2 {
            let child = collection(0, 0, 0, 0, &LIST_INT);
            plenty_release(collection(1, child, i + 1, 0, ptr::null()) as *mut Header);
            plenty_release(collection(1, source, child, 0, ptr::null()) as *mut Header);
        }
        for budget in 0..=allocations {
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                collection(33, source, 0, 0, &LIST_LIST)
            };
            if budget < allocations {
                assert_eq!(result, 1u128 << 64);
            } else {
                assert_eq!(result >> 64, 0);
                let copied = crate::aggregates::payload(result);
                assert_ne!(copied, source);
                assert_eq!(collection(8, source, copied, 0, &LIST_LIST), 1);
                plenty_release(copied as *mut Header);
            }
        }
        plenty_release(source as *mut Header);
        // Inline records copy into caller storage without allocating.
        let mut pair = [0u128; 4];
        let mut copied = [0u128; 4];
        let result = {
            let _restore = Restore;
            crate::accounting::fail_after(Some(0));
            collection(
                33,
                pair.as_mut_ptr() as u128,
                copied.as_mut_ptr() as u128,
                0,
                &PAIR,
            )
        };
        assert_eq!(result >> 64, 0);
        pair.fill(0);
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
    for (ty, allocations) in [(&LIST_INT, 2), (&MAP, 4)] {
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
        for budget in 0..=3 {
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
                assert_eq!(result, if budget == 3 { 0 } else { 1u128 << 64 });
                assert_eq!(
                    collection(5, c, 0, 0, ptr::null()),
                    len + u128::from(budget == 3)
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
    }
}

/// Storage of an initialized class with `__del__`: a state word, then fields.
const INITIALIZED: u128 = 1;
unsafe fn guard(id: u128) -> u128 {
    unsafe {
        let mut storage = [INITIALIZED, id];
        let result = collection(119, storage.as_mut_ptr() as u128, 0, 0, &GUARD);
        assert_eq!(result >> 64, 0);
        result as u64 as u128
    }
}
unsafe extern "C" fn hook(owner: *mut u128) {
    unsafe {
        let id = *((*owner) as u64 as *const u128).add(1);
        TRACE.with(|trace| trace.borrow_mut().push(id));
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
        assert_eq!(strings::text(joined), "é\0🦀");
        assert_eq!(strings::byte_len(joined), 7);
        assert_eq!(strings::scalar_len(joined), 3);
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
            refs: std::sync::atomic::AtomicU64::new(u64::MAX),
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
        assert_eq!(strings::text_bytes(pointer), b"a\0b");
        let copy = plenty_concat(pointer, pointer);
        assert_eq!(strings::text_bytes(copy), b"a\0ba\0b");
        plenty_release(copy.cast());
    }
}

#[test]
fn destruction_queue_preserves_children_and_nested_drops() {
    unsafe {
        let list = collection(0, 0, 0, 0, &LIST_GUARD);
        for id in [9, 2, 3] {
            // Affine elements move into the list.
            let retained = collection(1, list, guard(id), 0, ptr::null());
            plenty_release(retained as *mut Header);
        }
        plenty_release(list as *mut Header);
        assert_eq!(trace(), [9, 77, 99, 2, 3]);
    }
}

#[test]
fn dictionary_heap_keys_finish_before_inline_values() {
    static DICTIONARY: Type = Type {
        key: Some(&GUARD),
        value: Some(&GUARD_CLASS),
        ..DICT
    };
    unsafe {
        let dictionary = collection(0, 0, 0, 0, &DICTIONARY);
        for id in [11, 21] {
            let mut value = [INITIALIZED, id + 1];
            let retained = collection(
                1,
                dictionary,
                guard(id),
                value.as_mut_ptr() as u128,
                ptr::null(),
            );
            plenty_release(retained as *mut Header);
        }
        plenty_release(dictionary as *mut Header);
        assert_eq!(trace(), [11, 12, 21, 22]);
    }
}

#[test]
fn owned_iteration_removes_the_source_owner() {
    unsafe {
        let list = collection(0, 0, 0, 0, &LIST_GUARD);
        let retained = collection(1, list, guard(5), 0, ptr::null());
        plenty_release(retained as *mut Header);
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
        // Pair stores two inline Points; both fields share one descriptor.
        let mut point = [3u128];
        let mut pair = [0u128; 4];
        crate::ranges::store(pair.as_mut_ptr(), point.as_mut_ptr() as u128, &POINT);
        point[0] = 4;
        crate::ranges::store(pair.as_mut_ptr().add(2), point.as_mut_ptr() as u128, &POINT);
        assert_eq!(pair[1], 3);
        assert_eq!(pair[3], 4);
        let mut copied = [0u128; 4];
        let pair = pair.as_mut_ptr();
        let source = pair as u128;
        let copy = crate::aggregates::payload(collection(
            33,
            source,
            copied.as_mut_ptr() as u128,
            0,
            &PAIR,
        ));
        assert_eq!(collection(8, source, copy, 0, &PAIR), 1);
        *pair.add(1) = 10;
        assert_eq!(collection(8, source, copy, 0, &PAIR), 0);
        assert_eq!(copied[1], 3);
    }
}

#[test]
fn dictionaries_preserve_order_and_copy_owned_contents() {
    unsafe {
        let dict = collection(0, 0, 0, 0, &DICT);
        let list = collection(0, 0, 0, 0, &LIST_INT);
        plenty_release(collection(1, list, 42, 0, ptr::null()) as *mut Header);
        plenty_release(collection(1, dict, 1, list, ptr::null()) as *mut Header);
        // The affine list moved into the dictionary; only the dictionary owns it.
        let independent = crate::aggregates::payload(collection(33, dict, 0, 0, &DICT));
        assert_eq!(collection(8, dict, independent, 0, ptr::null()), 1);
        let values = collection(11, dict, 0, 0, &LIST_LIST);
        let copied_values = crate::aggregates::payload(collection(33, values, 0, 0, &LIST_LIST));
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
fn extracted_closure_rows_relocate_large_payloads_without_allocating() {
    use crate::entries::{Entries, Entry};
    use crate::ranges::{self, Range};
    static ENV: Type = Type {
        inline_bytes: 80,
        affine: true,
        variants: &[Variant {
            name: "captures",
            fields: &[&UNSIGNED_RANGE, &GUARD],
        }],
        ..scalar(b'H')
    };
    for dictionary in [false, true] {
        let mut entries = if dictionary {
            Entries::new(&INTEGER, Some(&ENV))
        } else {
            Entries::new(&ENV, None)
        };
        entries.try_reserve(4).unwrap();
        for i in 0..3 {
            let mut environment = [0u128; 5];
            let range = Range::new(i, i + 2, 1, false);
            // SAFETY: this buffer has the exact descriptor layout. push moves
            // the environment; the source is not subsequently used or dropped.
            unsafe {
                environment.as_mut_ptr().cast::<*const Type>().write(&ENV);
                ranges::store(
                    environment.as_mut_ptr().add(1),
                    &range as *const Range as u128,
                    &UNSIGNED_RANGE,
                );
                environment[4] = guard(i as u128);
                let pointer = environment.as_mut_ptr() as u128;
                entries.push(if dictionary {
                    Entry {
                        key: i as u128,
                        value: pointer,
                    }
                } else {
                    Entry {
                        key: pointer,
                        value: 0,
                    }
                });
            }
        }
        entries.try_reserve(128).unwrap();
        entries.reverse();
        TRACE.with(|trace| trace.borrow_mut().reserve(3));
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(Some(0));
        for expected in [2, 1, 0] {
            unsafe {
                let entry = entries.remove(0);
                let pointer = if dictionary { entry.value } else { entry.key };
                let range = *(pointer as *const u128).add(1) as *const Range;
                assert_eq!((*range).start, expected);
                crate::aggregates::release(pointer, &ENV);
            }
        }
        assert!(entries.is_empty());
        entries.clear();
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(None);
        assert_eq!(trace(), [2, 1, 0]);
    }
}

#[test]
fn inline_generator_moves_rebase_nested_frames_ranges_and_sum_payloads() {
    use crate::aggregates::{release, wrap};
    use crate::generators::{plenty_generator_init, plenty_generator_resume};
    use crate::ranges::{self, Range};
    static INNER: Type = Type {
        inline_bytes: 128,
        ..scalar(b'G')
    };
    static OUTER: Type = Type {
        inline_bytes: 208,
        ..scalar(b'G')
    };
    static OPTIONAL: Type = Type {
        inline_bytes: 208,
        variants: &[
            Variant {
                name: "Nothing",
                fields: &[],
            },
            Variant {
                name: "Some",
                fields: &[&OUTER],
            },
        ],
        ..scalar(b'B')
    };
    static INNER_SLOTS: [&Type; 2] = [&UNSIGNED_RANGE, &GUARD];
    static OUTER_SLOTS: [&Type; 1] = [&INNER];
    unsafe extern "C" fn resume_range(frame: *mut Generator, out: *mut u128) -> u8 {
        unsafe {
            let slots = frame.cast::<u8>().add(64).cast::<u128>();
            let range = *slots as *const Range;
            let state = frame.cast::<u8>().add(24).cast::<u64>();
            if *state < (*range).len {
                out.write((*range).at(*state as usize, false));
                *state += 1;
                1
            } else {
                crate::generators::plenty_generator_finish(frame);
                0
            }
        }
    }
    let mut inner = [0u128; 8];
    let mut outer = [0u128; 13];
    let mut wrapped = [0u128; 14];
    let mut moved = [0u128; 14];
    let inner_ptr = inner.as_mut_ptr();
    let outer_ptr = outer.as_mut_ptr();
    let wrapped_ptr = wrapped.as_mut_ptr();
    let range = Range::new(7, 10, 1, false);
    // SAFETY: buffers reserve their exact aligned layouts, metadata is static,
    // and each move relinquishes the old owner's bytes before resuming/dropping.
    unsafe {
        let captures = [&range as *const Range as u128, guard(73)];
        plenty_generator_init(
            inner_ptr.cast(),
            resume_range,
            2,
            INNER_SLOTS.as_ptr().cast(),
            captures.as_ptr(),
            2,
        );
        let mut item = 0;
        assert_eq!(plenty_generator_resume(inner_ptr.cast(), &mut item), 1);
        assert_eq!(item, 7);
        let captures = [inner_ptr as u128];
        plenty_generator_init(
            outer_ptr.cast(),
            never_resume,
            1,
            OUTER_SLOTS.as_ptr().cast(),
            captures.as_ptr(),
            1,
        );
        inner.fill(0);
        ranges::store(wrapped_ptr, wrap(outer_ptr as u128, 1), &OPTIONAL);
        outer.fill(0);
        moved.copy_from_slice(&wrapped);
        ranges::relocate(moved.as_mut_ptr(), &OPTIONAL);
        wrapped.fill(0);
        let parent = moved[0] as u64 as *mut u8;
        let child = *parent.add(64).cast::<u128>() as *mut Generator;
        assert_eq!(plenty_generator_resume(child, &mut item), 1);
        assert_eq!(item, 8);
        assert!(trace().is_empty());
        release(moved[0], &OPTIONAL);
        assert_eq!(trace(), [73]);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn inline_generator_construction_and_exhaustion_need_no_allocation() {
    use crate::generators::{plenty_generator_init, plenty_generator_resume};
    static INLINE: Type = Type {
        inline_bytes: 64,
        ..scalar(b'G')
    };
    unsafe extern "C" fn finish(frame: *mut Generator, _: *mut u128) -> u8 {
        unsafe { crate::generators::plenty_generator_finish(frame) };
        0
    }
    let mut frame = [0u128; 4];
    unsafe {
        crate::accounting::fail_after(Some(0));
        plenty_generator_init(
            frame.as_mut_ptr().cast(),
            finish,
            0,
            ptr::null(),
            ptr::null(),
            0,
        );
        let mut item = 0;
        assert_eq!(
            plenty_generator_resume(frame.as_mut_ptr().cast(), &mut item),
            0
        );
        assert_eq!(
            plenty_generator_resume(frame.as_mut_ptr().cast(), &mut item),
            0
        );
        crate::aggregates::release(frame.as_mut_ptr() as u128, &INLINE);
        crate::accounting::fail_after(None);
    }
}

#[test]
fn oversized_generator_frame_fails_before_reading_metadata() {
    let result = unsafe { crate::generators::try_new(never_resume, u64::MAX, ptr::null()) };
    assert_eq!(
        result.unwrap_err(),
        crate::memory::AllocError::CapacityOverflow
    );
}

#[cfg(feature = "allocation-checks")]
#[test]
fn native_checked_generator_abi_consumes_captures_on_both_paths() {
    static MANAGED: [&Type; 1] = [&LIST_INT];
    for budget in 0..=1 {
        unsafe {
            let child = collection(0, 0, 0, 0, &LIST_INT);
            let captures = [child];
            let mut out = 0;
            crate::accounting::fail_after(Some(budget));
            crate::generators::plenty_generator_try_new(
                never_resume,
                1,
                MANAGED.as_ptr().cast(),
                captures.as_ptr(),
                1,
                &mut out,
            );
            crate::accounting::fail_after(None);
            if budget == 0 {
                assert_eq!(out, crate::aggregates::wrap(0, 1));
            } else {
                assert_eq!(out >> 64, 0);
                plenty_release(out as *mut Header);
            }
        }
    }
}

#[test]
fn partial_class_cleanup_skips_hook_until_explicitly_armed() {
    unsafe {
        let mut storage = [0, 41u128];
        crate::aggregates::release(storage.as_mut_ptr() as u128, &GUARD_CLASS);
        assert!(trace().is_empty());
        storage = [INITIALIZED, 42];
        let value = storage.as_mut_ptr() as u128;
        // An observer's release leaves the instance alive.
        crate::aggregates::retain(value, &GUARD_CLASS);
        crate::aggregates::release(value, &GUARD_CLASS);
        assert!(trace().is_empty());
        crate::aggregates::release(value, &GUARD_CLASS);
        assert_eq!(trace(), [42]);
    }
}

#[test]
fn element_addresses_survive_shared_projection_and_allow_exclusive_writes() {
    unsafe {
        let owner = collection(0, 0, 0, 0, &LIST_INT);
        plenty_release(collection(1, owner, 4, 0, ptr::null()) as *mut Header);
        plenty_release(collection(1, owner, 8, 0, ptr::null()) as *mut Header);
        let slot = owner;
        let first = collection(112, &slot as *const u128 as u128, 0, 0, ptr::null()) as *mut u128;
        let second = collection(112, &slot as *const u128 as u128, 1, 0, ptr::null()) as *mut u128;
        assert_eq!((*first, *second), (4, 8));
        *first = 12;
        assert_eq!(collection(4, owner, 0, 0, ptr::null()), 12);
        plenty_release(owner as *mut Header);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn generator_frame_failure_does_not_consume_captures() {
    static MANAGED: [&Type; 1] = [&GENERATOR];
    unsafe {
        let child = guard(77);
        crate::accounting::fail_after(Some(0));
        let result = crate::generators::try_new(never_resume, 1, MANAGED.as_ptr().cast());
        crate::accounting::fail_after(None);
        assert_eq!(result.unwrap_err(), crate::memory::AllocError::OutOfMemory);
        assert!(trace().is_empty());
        let frame = crate::generators::try_new(never_resume, 1, MANAGED.as_ptr().cast()).unwrap();
        *frame.cast::<u8>().add(64).cast::<u128>() = child;
        plenty_release(frame.cast());
        assert_eq!(trace(), [77]);
    }
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

// A class reaches itself through Option[Box[RecursiveNode]].
static RECURSIVE_NODE: Type = Type {
    affine: true,
    name: "RecursiveNode",
    inline_bytes: 32,
    variants: &[Variant {
        name: "next",
        fields: &[&OPTION_RECURSIVE_BOX],
    }],
    drop: Some(recursive_node_drop),
    ..scalar(b'C')
};
static RECURSIVE_BOX: Type = Type {
    affine: true,
    key: Some(&RECURSIVE_NODE),
    ..scalar(b'O')
};
static OPTION_RECURSIVE_BOX: Type = Type {
    affine: true,
    name: "Option[Box[RecursiveNode]]",
    variants: &[
        Variant {
            name: "Nothing",
            fields: &[],
        },
        Variant {
            name: "Some",
            fields: &[&RECURSIVE_BOX],
        },
    ],
    ..scalar(b'B')
};
thread_local! {
    static RECURSIVE_DROPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
unsafe extern "C" fn recursive_node_drop(_: *mut u128) {
    RECURSIVE_DROPS.with(|count| count.set(count.get() + 1));
}

#[test]
fn recursive_boxes_drop_on_a_small_worker_stack_without_allocating() {
    const DEPTH: usize = 100_000;
    let mut child = 0;
    // SAFETY: each node's storage holds one valid Option[Box] payload.
    // Ownership of the previous box transfers into the next node.
    unsafe {
        for _ in 0..DEPTH {
            let mut storage = [
                INITIALIZED,
                crate::aggregates::wrap(child, u64::from(child != 0)),
            ];
            let result = collection(119, storage.as_mut_ptr() as u128, 0, 0, &RECURSIVE_BOX);
            assert_eq!(result >> 64, 0);
            child = result as u64 as u128;
        }
    }
    // The parent no longer accesses this graph. Its only payload is an owned
    // successor and its only callback updates a worker-local counter.
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            RECURSIVE_DROPS.with(|count| count.set(0));
            #[cfg(feature = "allocation-checks")]
            crate::accounting::fail_after(Some(0));
            unsafe { plenty_release(child as *mut Header) };
            #[cfg(feature = "allocation-checks")]
            crate::accounting::fail_after(None);
            assert_eq!(RECURSIVE_DROPS.with(std::cell::Cell::get), DEPTH);
        })
        .unwrap()
        .join()
        .unwrap();
}

thread_local! {
    static MIXED_TAIL_DROPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
unsafe extern "C" fn mixed_tail_drop(owner: *mut u128) {
    unsafe {
        let fields = (*owner as *const u128).add(1);
        MIXED_TAIL_DROPS.with(|count| {
            assert_eq!(*fields as usize, count.get());
            count.set(count.get() + 1);
        });
    }
}

#[test]
fn recursive_heap_children_finish_before_inline_siblings_on_a_small_stack() {
    static TAIL: Type = Type {
        drop: Some(mixed_tail_drop),
        ..GUARD_CLASS
    };
    static NODE: Type = Type {
        affine: true,
        inline_bytes: 64,
        variants: &[
            Variant {
                name: "next",
                fields: &[&OPTION],
            },
            Variant {
                name: "tail",
                fields: &[&TAIL],
            },
        ],
        ..scalar(b'C')
    };
    static BOX: Type = Type {
        affine: true,
        key: Some(&NODE),
        ..scalar(b'O')
    };
    static OPTION: Type = Type {
        affine: true,
        variants: &[
            Variant {
                name: "Nothing",
                fields: &[],
            },
            Variant {
                name: "Some",
                fields: &[&BOX],
            },
        ],
        ..scalar(b'B')
    };
    const DEPTH: usize = 100_000;
    let mut child = 0;
    unsafe {
        for id in 0..DEPTH {
            let mut tail = [INITIALIZED, id as u128];
            let mut fields = [
                crate::aggregates::wrap(child, u64::from(child != 0)),
                0,
                0,
                0,
            ];
            crate::ranges::store(fields.as_mut_ptr().add(1), tail.as_mut_ptr() as u128, &TAIL);
            let result = collection(119, fields.as_mut_ptr() as u128, 0, 0, &BOX);
            assert_eq!(result >> 64, 0);
            child = result as u64 as u128;
        }
    }
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            MIXED_TAIL_DROPS.with(|count| count.set(0));
            #[cfg(feature = "allocation-checks")]
            crate::accounting::fail_after(Some(0));
            unsafe { plenty_release(child as *mut Header) };
            #[cfg(feature = "allocation-checks")]
            crate::accounting::fail_after(None);
            assert_eq!(MIXED_TAIL_DROPS.with(std::cell::Cell::get), DEPTH);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(feature = "allocation-checks")]
#[test]
fn failed_box_allocation_drops_the_value_it_would_own() {
    RECURSIVE_DROPS.with(|count| count.set(0));
    unsafe {
        let mut leaf = [INITIALIZED, 0];
        let child = collection(119, leaf.as_mut_ptr() as u128, 0, 0, &RECURSIVE_BOX) as u64 as u128;
        let mut storage = [INITIALIZED, crate::aggregates::wrap(child, 1)];
        crate::accounting::fail_after(Some(0));
        let parent = collection(119, storage.as_mut_ptr() as u128, 0, 0, &RECURSIVE_BOX);
        crate::accounting::fail_after(None);
        assert_eq!(parent >> 64 & 1, 1); // Result.Err(AllocError.OutOfMemory)
        assert_eq!(RECURSIVE_DROPS.with(std::cell::Cell::get), 2);
    }
}
