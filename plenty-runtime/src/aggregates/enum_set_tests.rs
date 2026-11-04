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
    }; 65],
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
            assert_eq!(set.entries.bits(), Some(expected));
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
        assert_eq!(set.entries.bits(), Some(u64::MAX));
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
        assert_eq!(set.entries.bits(), Some(63));
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
