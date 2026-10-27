use super::*;

static INTEGER: Type = Type {
    kind: b'4',
    affine: false,
    reflexive: true,
    inline_range: false,
    inline_bytes: 0,
    key: None,
    value: None,
    name: "",
    variants: &[],
    drop: None,
    wrapper: None,
    plan: None,
};
static WIDE: Type = Type {
    inline_bytes: 64,
    ..INTEGER
};
static RANGE: Type = Type {
    kind: b'R',
    ..INTEGER
};
#[cfg(feature = "allocation-checks")]
static EMPTY: Type = Type {
    kind: b'C',
    ..INTEGER
};

#[cfg(feature = "allocation-checks")]
#[test]
fn copy_failure_clears_mixed_width_fields_without_consuming_source() {
    static BOX: Type = Type {
        kind: b'O',
        affine: true,
        key: Some(&INTEGER),
        ..INTEGER
    };
    static TUPLE: Type = Type {
        kind: b'B',
        affine: true,
        variants: &[Variant {
            name: "",
            fields: &[&BOX, &WIDE, &BOX, &EMPTY, &BOX],
        }],
        ..INTEGER
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    // SAFETY: fields own their boxes, and the WIDE scalar reserves its payload
    // without embedded pointers. EMPTY has no payload, but still has a value slot.
    unsafe {
        let mut source = [0u128; 9];
        for index in [0, 6, 8] {
            source[index] = try_box_new(&BOX, 42).unwrap() as u128;
        }
        source[1] = 17;
        for budget in 0..3 {
            let mut copy = source;
            let mut value = copy.as_mut_ptr() as u128;
            let result = {
                let _restore = Restore;
                crate::accounting::fail_after(Some(budget));
                try_copy_into(&mut value, &TUPLE)
            };
            assert_eq!(result, Err(AllocError::OutOfMemory));
            assert_eq!(value, 0);
            for index in [0, 1, 6, 7, 8] {
                assert_eq!(copy[index], 0);
            }
            for index in [0, 6, 8] {
                let boxed = source[index] as *const Boxed;
                assert_eq!(std::ptr::addr_of!((*boxed).slot).cast::<u128>().read(), 42);
            }
        }
        release(source.as_mut_ptr() as u128, &TUPLE);
    }
}

#[test]
fn record_slots_cover_variable_widths_and_class_state() {
    static FIELDS: &[&Type] = &[&INTEGER, &WIDE, &RANGE, &INTEGER];
    static CLASS: &[Variant] = &[
        Variant {
            name: "a",
            fields: &[&INTEGER],
        },
        Variant {
            name: "b",
            fields: &[&WIDE],
        },
        Variant {
            name: "c",
            fields: &[&RANGE],
        },
        Variant {
            name: "d",
            fields: &[&INTEGER],
        },
    ];
    for fields in [Fields::Variant(FIELDS), Fields::Class(CLASS)] {
        for state in [false, true] {
            let mut storage = [0u128; 11];
            let record = Record {
                fields,
                base: storage.as_mut_ptr(),
                state,
            };
            assert_eq!(record.words(), 10 + usize::from(state));
            for ((slot, ty), (offset, expected)) in
                record.slots().zip([0, 1, 6, 9].into_iter().zip(FIELDS))
            {
                assert!(!slot.flat);
                assert_eq!(
                    slot.at,
                    storage
                        .as_mut_ptr()
                        .wrapping_add(offset + usize::from(state))
                );
                assert!(std::ptr::eq(ty, *expected));
            }
            assert_eq!(record.slots().count(), 4);
            assert_eq!(
                record.slots().nth(2).unwrap().0.at,
                storage.as_mut_ptr().wrapping_add(6 + usize::from(state))
            );
        }
    }
    for fields in [Fields::Variant(&[]), Fields::Class(&[])] {
        let record = Record {
            fields,
            base: std::ptr::null_mut(),
            state: false,
        };
        assert_eq!(record.words(), 0);
        assert_eq!(record.slots().count(), 0);
    }
}

#[test]
fn variant_records_store_class_records_flat() {
    // A class with a two-word record, such as `Point(x: i64, y: i64)`.
    static POINT: Type = Type {
        kind: b'C',
        inline_bytes: 32,
        ..INTEGER
    };
    static FIELDS: &[&Type] = &[&POINT, &INTEGER, &POINT];
    let mut storage = [0u128; 5];
    let record = Record {
        fields: Fields::Variant(FIELDS),
        base: storage.as_mut_ptr(),
        state: false,
    };
    assert_eq!(record.words(), 5);
    let slots: Vec<_> = record
        .slots()
        .map(|(field, _)| (field.at, field.flat))
        .collect();
    let base = storage.as_mut_ptr();
    assert_eq!(
        slots,
        [
            (base, true),
            (base.wrapping_add(2), false),
            (base.wrapping_add(3), true)
        ]
    );
}

fn tuple_type(width: usize) -> Type {
    let fields = Box::leak(vec![&INTEGER; width].into_boxed_slice());
    Type {
        kind: b'B',
        name: "tuple[wide]",
        variants: Box::leak(vec![Variant { name: "", fields }].into_boxed_slice()),
        ..INTEGER
    }
}

#[test]
fn wide_tuple_equality_order_and_hash_reach_the_last_field() {
    let ty = tuple_type(1024);
    let mut left: Vec<u128> = (0..1024).collect();
    let mut right = left.clone();
    let a = left.as_mut_ptr() as u128;
    let b = right.as_mut_ptr() as u128;
    // SAFETY: both buffers contain all 1024 integer fields of this descriptor.
    unsafe {
        assert!(equal(a, b, &ty));
        assert_eq!(compare(a, b, &ty), Ordering::Equal);
        assert_eq!(
            hashing::hash_with_key(a, &ty, &[0; 16]),
            hashing::hash_with_key(b, &ty, &[0; 16])
        );
        right[1023] += 1;
        assert!(!equal(a, b, &ty));
        assert_eq!(compare(a, b, &ty), Ordering::Less);
        assert_ne!(
            hashing::hash_with_key(a, &ty, &[0; 16]),
            hashing::hash_with_key(b, &ty, &[0; 16])
        );
    }
}

/// Run optimized and single-threaded with --ignored --nocapture. No timing
/// thresholds: this is a reproducible scaling probe, not a CI performance gate.
#[test]
#[ignore = "manual wide-tuple scaling measurement"]
fn wide_tuple_traversal_benchmark() {
    use std::hint::black_box;
    use std::time::Instant;
    const REPEATS: usize = 1000;
    for width in [16, 64, 256, 1024] {
        let ty = tuple_type(width);
        let mut left = vec![42u128; width];
        let mut right = left.clone();
        let a = left.as_mut_ptr() as u128;
        let b = right.as_mut_ptr() as u128;
        let record = ty.record(a).unwrap();
        let start = Instant::now();
        for _ in 0..REPEATS {
            for i in 0..black_box(record.len()) {
                let offset: usize = (0..i).map(|j| black_box(record.ty(j)).slot_words()).sum();
                black_box(record.base.wrapping_add(offset));
            }
        }
        let old = start.elapsed();
        let start = Instant::now();
        for _ in 0..REPEATS {
            for (slot, ty) in black_box(&record).slots() {
                black_box((slot, ty));
            }
        }
        let new = start.elapsed();
        let start = Instant::now();
        for _ in 0..REPEATS {
            // SAFETY: buffers remain live with identical integer tuple layouts.
            unsafe {
                black_box(equal(black_box(a), black_box(b), &ty));
                black_box(hashing::hash_with_key(black_box(a), &ty, &[0; 16]));
            }
        }
        eprintln!(
            "width={width} iterations={REPEATS} legacy_offsets_us={} running_offsets_us={} hash_equal_us={}",
            old.as_micros(),
            new.as_micros(),
            start.elapsed().as_micros()
        );
    }
}
