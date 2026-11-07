use super::*;
use crate::aggregates::Variant;
use crate::ranges::Range;

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
    redacted: false,
    transparent: false,
    debug: None,
    display: None,
    quantity: None,
    finite: None,
};
static RANGE: Type = Type {
    kind: b'R',
    ..INTEGER
};
static NESTED: Type = Type {
    kind: b'C',
    affine: true,
    inline_bytes: 48,
    variants: &[Variant {
        name: "range",
        fields: &[&RANGE],
    }],
    ..INTEGER
};

static SCALARS: [Type; 11] = [
    Type {
        kind: b'1',
        ..INTEGER
    },
    Type {
        kind: b'2',
        ..INTEGER
    },
    Type {
        kind: b'3',
        ..INTEGER
    },
    Type {
        kind: b'4',
        ..INTEGER
    },
    Type {
        kind: b'5',
        ..INTEGER
    },
    Type {
        kind: b'6',
        ..INTEGER
    },
    Type {
        kind: b'7',
        ..INTEGER
    },
    Type {
        kind: b'8',
        ..INTEGER
    },
    Type {
        kind: b'f',
        ..INTEGER
    },
    Type {
        kind: b'd',
        ..INTEGER
    },
    Type {
        kind: b'b',
        ..INTEGER
    },
];

#[test]
fn scalar_rows_use_native_width_and_preserve_neighbors() {
    for (ty, width) in SCALARS.iter().zip([1, 2, 4, 8, 1, 2, 4, 8, 4, 8, 1]) {
        let mut entries = filled(ty, 128);
        assert_eq!(entries.row_bytes(), width);
        assert_eq!(entries.data.len(), 128 * width);
        assert_eq!(entries.data.capacity(), 128 * width);
        // SAFETY: each row is live. Exercise direct element-reference writes
        // followed by canonical runtime reads, including the final row.
        unsafe {
            for index in 0..128 {
                let slot = entries.slot(index, false).cast::<u8>();
                assert_eq!(slot as usize % width, 0);
                write_scalar(slot, u64::MAX as u128, ty);
            }
            entries.set(63, false, 5);
        }
        let bits = u64::MAX;
        let expected = unsafe { read_scalar((&bits as *const u64).cast(), ty) };
        for index in 0..128 {
            assert_eq!(
                entries.get(index).key,
                if index == 63 { 5 } else { expected }
            );
        }
        entries.reverse();
        assert_eq!(entries.get(64).key, 5);
        entries.swap(64, 127);
        assert_eq!(unsafe { entries.remove(127) }.key, 5);
        entries.try_reserve(1).unwrap();
        unsafe { entries.push(Entry { key: 9, value: 0 }) };
        assert_eq!(entries.get(127).key, 9);
        entries.retain(|entry| entry.key == 9);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries.drain().next().unwrap().key, 9);
        assert!(entries.is_empty());
    }
}

#[test]
fn list_rows_omit_dictionary_value_storage() {
    for ty in [&RANGE, &NESTED] {
        let list = filled(ty, 4);
        assert_eq!(list.row_bytes(), ty.slot_words() * 16);
        let dict = Entries::ordered(ty, Some(&INTEGER));
        assert_eq!(dict.row_bytes(), (ty.slot_words() + 1) * 16);
    }
    let set = Entries::ordered(&SCALARS[4], None);
    assert_eq!(set.row_bytes(), 32);
}

#[test]
fn packed_scalar_growth_sort_and_extraction_keep_every_value() {
    for ty in &SCALARS[..10] {
        let mut entries = Entries::new(ty, None);
        for n in (0..120).rev() {
            entries.try_reserve(1).unwrap();
            unsafe { entries.push(Entry { key: n, value: 0 }) };
        }
        entries.sort_unstable_by(|a, b| a.cmp(&b));
        for n in 0..120 {
            assert_eq!(entries.get(n).key, n as u128);
        }
        assert_eq!(unsafe { entries.remove(37) }.key, 37);
        assert_eq!(entries.get(37).key, 38);
        let values: Vec<_> = entries.drain().map(|entry| entry.key).collect();
        assert_eq!(values, (0..120).filter(|n| *n != 37).collect::<Vec<_>>());
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn packed_reserve_failure_preserves_contents_and_reuses_remaining_capacity() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    let mut entries = filled(&SCALARS[4], 7);
    let base = entries.rows();
    let capacity = entries.capacity();
    let restore = Restore;
    crate::accounting::fail_after(Some(0));
    assert!(entries.try_reserve(1000).is_err());
    assert!(entries.try_reserve(usize::MAX).is_err());
    assert_eq!(entries.len(), 7);
    assert_eq!(entries.rows(), base);
    assert_eq!(entries.capacity(), capacity);
    for n in 0..7 {
        assert_eq!(entries.get(n).key, n as u128);
    }
    // A failed growth must not consume spare capacity or alter the byte length.
    entries.try_reserve(1).unwrap();
    unsafe { entries.push(Entry { key: 7, value: 0 }) };
    assert_eq!(entries.get(7).key, 7);
    drop(restore);
}

fn push(entries: &mut Entries, number: u128) {
    let range = Range::new(number as u64, number as u64 + 3, 1, false);
    let mut record = [0; 3];
    // SAFETY: the record reserves the range's entire three-word storage slot.
    unsafe {
        ranges::store(record.as_mut_ptr(), &range as *const Range as u128, &RANGE);
    }
    let key = match entries.key.kind {
        b'R' => &range as *const Range as u128,
        b'C' => record.as_ptr() as u128,
        _ => number,
    };
    // SAFETY: the source's complete payload remains live until push copies it;
    // the caller reserves capacity before transferring each row.
    unsafe { entries.push(Entry { key, value: 0 }) };
}

fn insert(entries: &mut Entries, index: usize, number: u128) {
    let range = Range::new(number as u64, number as u64 + 3, 1, false);
    let mut record = [0; 3];
    // SAFETY: as for `push`.
    unsafe {
        ranges::store(record.as_mut_ptr(), &range as *const Range as u128, &RANGE);
    }
    let key = match entries.key.kind {
        b'R' => &range as *const Range as u128,
        b'C' => record.as_ptr() as u128,
        _ => number,
    };
    // SAFETY: as for `push`.
    unsafe { entries.insert(index, Entry { key, value: 0 }) };
}

fn filled(ty: &'static Type, count: usize) -> Entries {
    let mut entries = Entries::new(ty, None);
    entries.try_reserve(count).unwrap();
    for number in 0..count as u128 {
        push(&mut entries, number);
    }
    entries
}

fn number(entry: Entry, ty: &Type) -> u128 {
    // SAFETY: all inline rows in these tests hold the descriptor's complete
    // payload, and callers read extracted rows before the next mutation.
    unsafe {
        match ty.kind {
            b'R' => (*(entry.key as *const Range)).start as u128,
            b'C' => (*(*(entry.key as *const u128) as *const Range)).start as u128,
            _ => entry.key,
        }
    }
}

fn assert_addresses(entries: &Entries, index: usize, entry: Entry) {
    if entries.key.payload_bytes() == 0 {
        return;
    }
    // SAFETY: both live rows and initialized extraction scratch have a complete
    // inline payload. Reading their addresses does not mutate the collection.
    unsafe {
        let slot = entries
            .data
            .as_ptr()
            .add(index * entries.stride)
            .cast::<u128>();
        assert_eq!(entry.key, slot.add(1) as u128);
        if entries.key.kind == b'C' {
            assert_eq!(*(entry.key as *const u128), slot.add(2) as u128);
        }
    }
}

#[test]
fn wide_sort_preserves_rows_and_inline_addresses() {
    for ty in [&RANGE, &NESTED] {
        for count in [0, 1, 2, 15, 16, 17, 127, 1024] {
            for pattern in 0..7 {
                let values: Vec<_> = (0..count)
                    .map(|i| match pattern {
                        0 => i,
                        1 => count - i,
                        2 => 7,
                        3 => i % 5,
                        4 => i.min(count - i),
                        5 => (i * 7919 + 97) % 1009,
                        _ => {
                            if i == count / 2 && i + 1 < count {
                                i + 1
                            } else if i == count / 2 + 1 {
                                i - 1
                            } else {
                                i
                            }
                        }
                    } as u128)
                    .collect();
                let mut expected = values.clone();
                expected.sort_unstable();
                for fallback in [false, true] {
                    let mut entries = Entries::new(ty, None);
                    entries.try_reserve(count).unwrap();
                    for &value in &values {
                        push(&mut entries, value);
                    }
                    let base = entries.data.as_ptr();
                    let capacity = entries.capacity();
                    let mut compare = |a, b| {
                        number(Entry { key: a, value: 0 }, ty)
                            .cmp(&number(Entry { key: b, value: 0 }, ty))
                    };
                    if fallback {
                        entries.sort_rows(0, count, 0, Relocation::of(entries.key), &mut compare);
                    } else {
                        entries.sort_unstable_by(compare);
                    }
                    assert_eq!(entries.data.as_ptr(), base);
                    assert_eq!(entries.capacity(), capacity);
                    for (i, entry) in entries.iter().enumerate() {
                        assert_eq!(number(entry, ty), expected[i]);
                        assert_addresses(&entries, i, entry);
                    }
                }
            }
        }
    }
}

#[test]
fn inconsistent_sort_comparison_keeps_every_row_and_address() {
    let mut entries = filled(&NESTED, 4096);
    let mut count = 0;
    entries.sort_unstable_by(|_, _| {
        count += 1;
        if count % 3 == 0 {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    });
    let mut values = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        assert_addresses(&entries, i, entry);
        values.push(number(entry, &NESTED));
    }
    values.sort_unstable();
    assert_eq!(values, (0..4096).collect::<Vec<_>>());
    assert!(count < 4096 * 100);
}

#[test]
fn panicking_sort_comparison_preserves_inline_owners() {
    // 32 descending rows: the probe stops at its first comparison, each
    // 16-row insertion run costs 120, and the merge check one more, so call
    // 243 is the merge's first comparison, with the whole left run in
    // scratch. By call 251 eight right rows have moved into the hole.
    for panic_at in [243, 251] {
        let mut entries = filled(&NESTED, 32);
        entries.reverse();
        let mut calls = 0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            entries.sort_stable_rows_by(|a, b| {
                calls += 1;
                assert!(calls != panic_at, "injected comparator panic");
                number(Entry { key: a, value: 0 }, &NESTED)
                    .cmp(&number(Entry { key: b, value: 0 }, &NESTED))
            })
        }));
        assert!(result.is_err());
        assert_eq!(calls, panic_at);
        check(&entries, 32);
    }
}
