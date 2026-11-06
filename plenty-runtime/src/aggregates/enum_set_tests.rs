use super::*;

static ENUM: Type = Type {
    kind: b'B',
    affine: false,
    reflexive: true,
    inline_range: false,
    inline_bytes: 0,
    key: None,
    value: None,
    name: "E",
    variants: &[const {
        Variant {
            name: "V",
            fields: &[],
        }
    }; 64],
    drop: None,
    wrapper: None,
    plan: None,
    redacted: false,
    transparent: false,
    debug: None,
    display: None,
    quantity: None,
    finite: None,
};
static SET: Type = Type {
    kind: b'S',
    affine: true,
    key: Some(&ENUM),
    ..ENUM
};
static LARGE: Type = Type {
    variants: &[const {
        Variant {
            name: "V",
            fields: &[],
        }
    }; 257],
    ..ENUM
};
static PAYLOAD: Type = Type {
    variants: &[Variant {
        name: "V",
        fields: &[&ENUM],
    }],
    ..ENUM
};

#[test]
fn enum_bitset_admission_is_bounded_and_payload_free() {
    assert!(crate::entries::enum_bits::eligible(&ENUM));
    assert!(!crate::entries::enum_bits::eligible(&LARGE));
    assert!(!crate::entries::enum_bits::eligible(&PAYLOAD));
}

#[test]
fn enum_bitset_membership_iteration_and_borrows_agree_at_every_bit() {
    // SAFETY: all keys are payload-free enum values; the guard owns the set.
    unsafe {
        let pointer = try_collection_new(&SET, 64).unwrap();
        let _owner = OwnedValue {
            value: pointer as u128,
            ty: &SET,
        };
        let set = &mut *pointer;
        let mut expected = 0u64;
        let mut state = 17u64;
        for step in 0..1024 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let tag = (state >> 32) as usize % 64;
            let key = ENUM.pack(0, tag);
            if step % 3 == 0 {
                assert_eq!(set.remove(key).is_some(), expected & (1 << tag) != 0);
                expected &= !(1 << tag);
            } else {
                set.try_insert(key, 0).unwrap();
                expected |= 1 << tag;
            }
            assert_eq!(
                set.entries.indices().collect::<Vec<_>>(),
                (0..64)
                    .filter(|i| expected & (1 << i) != 0)
                    .collect::<Vec<_>>()
            );
            assert_eq!(set.len(), expected.count_ones() as usize);
            for tag in 0..64 {
                assert_eq!(
                    set.find(ENUM.pack(0, tag)).is_some(),
                    expected & (1 << tag) != 0
                );
            }
            assert_eq!(set.entries.iter().len(), set.len());
            assert_eq!(
                set.entries.indices().rev().collect::<Vec<_>>(),
                set.entries
                    .indices()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
            );
        }
        set.try_insert(ENUM.pack(0, 63), 0).unwrap();
        let borrowed = set.entries.key_ref(63);
        assert_eq!(*borrowed, ENUM.pack(0, 63));
        assert_eq!(set.entries.next(63), None);
        assert!(set.table.is_empty());
        set.entries.clear();
        assert_eq!(set.len(), 0);
        // Canonical values are immutable, independent of owner mutation/lifetime.
        assert_eq!(*borrowed, ENUM.pack(0, 63));
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn enum_bitset_uses_one_owner_allocation_and_no_growth_allocations() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    let pointer = {
        let _restore = Restore;
        crate::accounting::fail_after(Some(1));
        try_collection_new(&SET, 64).unwrap()
    };
    // SAFETY: the owner guard releases the live set, including on panic.
    unsafe {
        let _owner = OwnedValue {
            value: pointer as u128,
            ty: &SET,
        };
        let set = &mut *pointer;
        let _restore = Restore;
        crate::accounting::fail_after(Some(0));
        for tag in 0..64 {
            set.try_insert(ENUM.pack(0, tag), 0).unwrap();
        }
        set.try_reserve(1000).unwrap();
        assert_eq!(set.entries.len(), 64);
        for tag in 0..64 {
            assert_eq!(set.remove(ENUM.pack(0, tag)), Some(0));
        }
        assert_eq!(set.len(), 0);
        assert_eq!(
            set.try_reserve(usize::MAX),
            Err(AllocError::CapacityOverflow)
        );
    }
}

static BOOL: Type = Type {
    kind: b'b',
    variants: &[],
    ..ENUM
};
static FINITE_ENUM: Type = Type {
    variants: &[
        Variant {
            name: "Empty",
            fields: &[],
        },
        Variant {
            name: "Flag",
            fields: &[&BOOL],
        },
    ],
    finite: Some(&Finite {
        values: &[0, 1 << 64, (1 << 64) | 1],
        offsets: &[0, 1],
    }),
    ..ENUM
};
static NESTED: Type = Type {
    variants: &[
        Variant {
            name: "First",
            fields: &[&FINITE_ENUM],
        },
        Variant {
            name: "Second",
            fields: &[&FINITE_ENUM],
        },
    ],
    finite: Some(&Finite {
        values: &[0, 2 << 64, (2 << 64) | 1, 1 << 64, 3 << 64, (3 << 64) | 1],
        offsets: &[0, 3],
    }),
    ..ENUM
};

#[test]
fn finite_enum_bitsets_encode_nested_payloads_and_keep_canonical_borrows() {
    static NESTED_SET: Type = Type {
        key: Some(&NESTED),
        ..SET
    };
    assert!(crate::entries::enum_bits::eligible(&NESTED));
    // SAFETY: every canonical value has the descriptor's native representation.
    unsafe {
        let pointer = try_collection_new(&NESTED_SET, 6).unwrap();
        let _owner = OwnedValue {
            value: pointer as u128,
            ty: &NESTED_SET,
        };
        let set = &mut *pointer;
        for (index, value) in NESTED.finite.unwrap().values.iter().copied().enumerate() {
            assert_eq!(crate::entries::enum_bits::index(&NESTED, value), index);
            set.try_insert(value, 0).unwrap();
            assert_eq!(set.find(value), Some(index));
            assert_eq!(*set.entries.key_ref(index), value);
        }
        assert_eq!(set.entries.len(), 6);
        assert_eq!(set.entries.capacity(), 6);
        assert!(set.table.is_empty());
        let borrowed = set.entries.key_ref(5);
        let saved = *borrowed;
        assert_eq!(set.remove(saved), Some(0));
        assert_eq!(set.find(saved), None);
        set.entries.clear();
        assert_eq!(*borrowed, saved);
    }
}

static WIDE: Type = Type {
    variants: &[const {
        Variant {
            name: "V",
            fields: &[],
        }
    }; 256],
    ..ENUM
};
static WIDE_SET: Type = Type {
    key: Some(&WIDE),
    ..SET
};

#[test]
fn wide_enum_bitsets_match_an_independent_membership_model() {
    // SAFETY: the type describes all tags used here; the guard owns the set.
    unsafe {
        let pointer = try_collection_new(&WIDE_SET, 0).unwrap();
        let _owner = OwnedValue {
            value: pointer as u128,
            ty: &WIDE_SET,
        };
        let set = &mut *pointer;
        let mut model = [false; 256];
        let mut state = 17u64;
        for step in 0..2048 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let tag = (state >> 32) as usize % 256;
            let key = WIDE.pack(0, tag);
            if step % 3 == 0 {
                assert_eq!(set.remove(key).is_some(), model[tag]);
                model[tag] = false;
            } else {
                set.try_insert(key, 0).unwrap();
                model[tag] = true;
            }
            let expected: Vec<_> = (0..256).filter(|i| model[*i]).collect();
            assert_eq!(set.len(), expected.len());
            assert_eq!(set.entries.indices().collect::<Vec<_>>(), expected);
            let mut cursor = set.entries.first();
            for i in &expected {
                assert_eq!(cursor, Some(*i));
                assert_eq!(*set.entries.key_ref(*i), WIDE.pack(0, *i));
                cursor = set.entries.next(*i);
            }
            assert_eq!(cursor, None);
            // Alternating ends must neither duplicate nor skip any word-boundary bit.
            let mut iter = set.entries.indices();
            let mut deque = std::collections::VecDeque::from(expected);
            while !deque.is_empty() {
                assert_eq!(iter.next(), deque.pop_front());
                assert_eq!(iter.next_back(), deque.pop_back());
                assert_eq!(iter.len(), deque.len());
            }
            assert_eq!(iter.next(), None);
            assert_eq!(iter.next_back(), None);
            for (i, present) in model.iter().enumerate() {
                assert_eq!(set.find(WIDE.pack(0, i)).is_some(), *present);
            }
        }
        let expected: Vec<_> = set.entries.iter().map(|e| e.key).collect();
        assert_eq!(
            set.entries.drain().map(|e| e.key).collect::<Vec<_>>(),
            expected
        );
        assert!(set.entries.is_empty());
        assert!(set.table.is_empty());
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn wide_enum_bitsets_need_only_the_owner_allocation() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    let _restore = Restore;
    crate::accounting::fail_after(Some(1));
    let pointer = try_collection_new(&WIDE_SET, 256).unwrap();
    // SAFETY: the guard owns the set; all 256 tags are valid.
    unsafe {
        let _owner = OwnedValue {
            value: pointer as u128,
            ty: &WIDE_SET,
        };
        let set = &mut *pointer;
        for i in 0..256 {
            set.try_insert(WIDE.pack(0, i), 0).unwrap();
        }
        set.try_reserve(1000).unwrap();
        assert_eq!(set.len(), 256);
        for i in 0..256 {
            assert_eq!(set.remove(WIDE.pack(0, i)), Some(0));
        }
        assert_eq!(set.len(), 0);
    }
}

#[test]
fn wide_enum_set_storage_measurements() {
    static HASH_SET: Type = Type {
        key: Some(&LARGE),
        ..SET
    };
    for members in [0, 4, 64, 128, 256] {
        for ty in [&WIDE_SET, &HASH_SET] {
            // SAFETY: the guard owns the collection; all inserted tags exist.
            unsafe {
                let pointer = try_collection_new(ty, 0).unwrap();
                let _owner = OwnedValue {
                    value: pointer as u128,
                    ty,
                };
                let set = &mut *pointer;
                for i in 0..members {
                    set.try_insert(ty.key().pack(0, i), 0).unwrap();
                }
                let bytes = std::mem::size_of::<Collection>()
                    + set.entries.storage_bytes()
                    + set.table.capacity_bytes();
                println!(
                    "{} members={members}: {bytes} requested heap bytes (owner {})",
                    if set.entries.bits().is_some() {
                        "bitset"
                    } else {
                        "hash"
                    },
                    std::mem::size_of::<Collection>()
                );
                if set.entries.bits().is_some() {
                    assert_eq!(bytes, std::mem::size_of::<Collection>());
                }
            }
        }
    }
}

#[test]
fn enum_set_width_is_selected_by_domain_and_survives_copying_and_emptying() {
    use crate::entries::enum_bits::Bits;
    macro_rules! check {
        ($size:literal, $variant:ident) => {{
            static KEY: Type = Type {
                variants: &[const {
                    Variant {
                        name: "V",
                        fields: &[],
                    }
                }; $size],
                ..ENUM
            };
            static TY: Type = Type {
                key: Some(&KEY),
                ..SET
            };
            // SAFETY: the guard owns the set; all inserted tags belong to KEY.
            unsafe {
                let pointer = try_collection_new(&TY, 0).unwrap();
                let _owner = OwnedValue {
                    value: pointer as u128,
                    ty: &TY,
                };
                let set = &mut *pointer;
                assert!(matches!(set.entries.bits(), Some(Bits::$variant(_))));
                let last = KEY.pack(0, $size - 1);
                set.try_insert(last, 0).unwrap();
                #[cfg(feature = "allocation-checks")]
                {
                    struct Restore;
                    impl Drop for Restore {
                        fn drop(&mut self) {
                            crate::accounting::fail_after(None);
                        }
                    }
                    let _restore = Restore;
                    crate::accounting::fail_after(Some(0));
                    assert!(try_copy_collection(pointer as u128, &TY).is_err());
                    crate::accounting::fail_after(Some(1));
                    let copied = try_copy_collection(pointer as u128, &TY).unwrap();
                    let _copy_owner = OwnedValue {
                        value: copied,
                        ty: &TY,
                    };
                    let copy = &*(copied as *const Collection);
                    assert!(matches!(copy.entries.bits(), Some(Bits::$variant(_))));
                    assert_eq!(copy.find(last), Some($size - 1));
                    set.remove(last);
                    assert_eq!(copy.find(last), Some($size - 1));
                }
                set.entries.clear();
                assert!(matches!(set.entries.bits(), Some(Bits::$variant(_))));
                set.try_insert(last, 0).unwrap();
                assert_eq!(
                    set.entries
                        .drain()
                        .map(|entry| entry.key)
                        .collect::<Vec<_>>(),
                    [last]
                );
                assert!(matches!(set.entries.bits(), Some(Bits::$variant(_))));
                set.try_insert(last, 0).unwrap();
                assert_eq!(set.find(last), Some($size - 1));
                assert_eq!(set.remove(last), Some(0));
                assert!(matches!(set.entries.bits(), Some(Bits::$variant(_))));
            }
        }};
    }
    check!(1, Small);
    check!(64, Small);
    check!(65, Medium);
    check!(128, Medium);
    check!(129, Large);
    check!(256, Large);
}
