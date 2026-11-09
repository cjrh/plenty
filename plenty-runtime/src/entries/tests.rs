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
    destroy: None,
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
                        let relocation = Relocation::of(entries.key);
                        entries
                            .dense_rows()
                            .sort_rows(0, count, 0, relocation, &mut compare);
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
    let mut entries = filled(&NESTED, 257);
    for i in 0..257 {
        entries.swap(i, (i * 97) % 257);
    }
    let mut calls = 0;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        entries.sort_unstable_by(|a, b| {
            calls += 1;
            assert!(calls != 100, "injected comparator panic");
            number(Entry { key: a, value: 0 }, &NESTED)
                .cmp(&number(Entry { key: b, value: 0 }, &NESTED))
        });
    }));
    assert!(result.is_err());
    let mut values = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        assert_addresses(&entries, i, entry);
        values.push(number(entry, &NESTED));
    }
    values.sort_unstable();
    assert_eq!(values, (0..257).collect::<Vec<_>>());
}

#[test]
#[ignore = "release-mode timing comparison; run with --ignored --nocapture"]
fn benchmark_wide_sort() {
    for pattern in 0..7 {
        let mut timings = [Vec::new(), Vec::new()];
        let mut comparisons = [0, 0];
        for repeat in 0..7 {
            for algorithm in [repeat % 2, 1 - repeat % 2] {
                let mut entries = Entries::new(&NESTED, None);
                entries.try_reserve(100_000).unwrap();
                for i in 0..100_000u128 {
                    push(
                        &mut entries,
                        match pattern {
                            0 => (i * 7919 + 97) % 100_003,
                            1 => i,
                            2 => 100_000 - i,
                            3 => i % 5,
                            4 => 7,
                            5 => i.min(100_000 - i),
                            _ => {
                                if i == 50_000 {
                                    50_001
                                } else if i == 50_001 {
                                    50_000
                                } else {
                                    i
                                }
                            }
                        },
                    );
                }
                let mut count = 0;
                let mut compare = |a, b| {
                    count += 1;
                    number(Entry { key: a, value: 0 }, &NESTED)
                        .cmp(&number(Entry { key: b, value: 0 }, &NESTED))
                };
                let time = std::time::Instant::now();
                if algorithm == 0 {
                    let relocation = Relocation::of(entries.key);
                    entries
                        .dense_rows()
                        .heap_sort_rows(0, 100_000, relocation, &mut compare);
                } else {
                    entries.sort_unstable_by(&mut compare);
                }
                timings[algorithm].push(time.elapsed());
                comparisons[algorithm] = count;
                std::hint::black_box(entries);
            }
        }
        for samples in &mut timings {
            samples.sort();
        }
        println!(
            "pattern {pattern}: heap {:?} / {} comparisons; intro {:?} / {} comparisons",
            timings[0][3], comparisons[0], timings[1][3], comparisons[1]
        );
    }
}

#[test]
fn tail_drain_keeps_extracted_scalar_and_inline_rows_in_place() {
    for ty in [&INTEGER, &RANGE, &NESTED] {
        let mut entries = filled(ty, 1024);
        let capacity = entries.capacity();
        let base = entries.data.as_ptr();
        for index in (0..1024).rev() {
            let before = entries.get(index);
            // SAFETY: each tail is live; no owners are retained by these fixtures.
            let removed = unsafe { entries.remove(index) };
            assert_eq!(removed.key, before.key);
            assert_eq!(number(removed, ty), index as u128);
            assert_addresses(&entries, index, removed);
            assert_eq!(entries.len(), index);
            assert_eq!(entries.capacity(), capacity);
            assert_eq!(entries.data.as_ptr(), base);
        }
        // The final scratch row is discarded and the entire buffer reused.
        entries.try_reserve(1024).unwrap();
        for index in 0..1024 {
            push(&mut entries, index);
        }
        assert_eq!(entries.data.as_ptr(), base);
        assert_eq!(number(entries.get(1023), ty), 1023);
    }
}

#[test]
fn non_tail_removal_repairs_suffix_and_extraction_scratch() {
    for ty in [&INTEGER, &RANGE, &NESTED] {
        for index in 0..4 {
            let mut entries = filled(ty, 5);
            let prefix: Vec<_> = (0..index).map(|i| entries.get(i).key).collect();
            // SAFETY: the selected row is live and extraction is observed before
            // any subsequent mutation of its scratch storage.
            let removed = unsafe { entries.remove(index) };
            assert_eq!(number(removed, ty), index as u128);
            assert_addresses(&entries, entries.len(), removed);
            for (i, entry) in entries.iter().enumerate() {
                assert_eq!(number(entry, ty), (i + usize::from(i >= index)) as u128);
                assert_addresses(&entries, i, entry);
                if i < index {
                    assert_eq!(entry.key, prefix[i]);
                }
            }
            // A second removal truncates the old scratch without touching live
            // ownership, then transfers the next tail from its original address.
            let removed = unsafe { entries.remove(entries.len() - 1) };
            assert_eq!(number(removed, ty), 4);
            assert_addresses(&entries, entries.len(), removed);
        }
    }
}

/// `list.insert` (issue #798): the row lands at its position, later rows
/// shift intact, and every inline address points into its own row, including
/// after a removal left extraction scratch behind.
#[test]
fn insertion_shifts_later_rows_and_repairs_inline_addresses() {
    for ty in [&INTEGER, &RANGE, &NESTED] {
        for index in 0..=4 {
            let mut entries = filled(ty, 4);
            entries.try_reserve(1).unwrap();
            insert(&mut entries, index, 99);
            assert_eq!(entries.len(), 5);
            for (i, entry) in entries.iter().enumerate() {
                let expected = match i.cmp(&index) {
                    Ordering::Less => i as u128,
                    Ordering::Equal => 99,
                    Ordering::Greater => i as u128 - 1,
                };
                assert_eq!(number(entry, ty), expected);
                assert_addresses(&entries, i, entry);
            }
        }
        let mut entries = filled(ty, 4);
        // SAFETY: the row is live; the fixtures retain no owners.
        let removed = unsafe { entries.remove(1) };
        assert_eq!(number(removed, ty), 1);
        let capacity = entries.capacity();
        insert(&mut entries, 0, 7);
        assert_eq!(entries.capacity(), capacity);
        let numbers: Vec<_> = entries.iter().map(|entry| number(entry, ty)).collect();
        assert_eq!(numbers, [7, 0, 2, 3]);
        for (i, entry) in entries.iter().enumerate() {
            assert_addresses(&entries, i, entry);
        }
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn sort_tail_pop_append_and_drain_reuse_capacity_without_allocating() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for ty in [&INTEGER, &RANGE, &NESTED] {
        let mut entries = filled(ty, 128);
        for i in 0..128 {
            entries.swap(i, (i * 97) % 128);
        }
        let base = entries.data.as_ptr();
        let capacity = entries.capacity();
        let restore = Restore;
        crate::accounting::fail_after(Some(0));
        entries.sort_unstable_by(|a, b| {
            number(Entry { key: a, value: 0 }, ty).cmp(&number(Entry { key: b, value: 0 }, ty))
        });
        for index in 128..1024 {
            // SAFETY: the tail is live and its value is read before reuse.
            let removed = unsafe { entries.remove(entries.len() - 1) };
            assert_eq!(number(removed, ty), index - 1);
            entries.try_reserve(1).unwrap();
            push(&mut entries, index);
        }
        while !entries.is_empty() {
            let index = entries.len() - 1;
            let removed = unsafe { entries.remove(index) };
            assert_eq!(
                number(removed, ty),
                if index == 127 { 1023 } else { index as u128 }
            );
        }
        drop(restore);
        assert_eq!(entries.data.as_ptr(), base);
        assert_eq!(entries.capacity(), capacity);
    }
}

#[test]
#[ignore = "release-mode timing benchmark; run with --ignored --nocapture"]
fn benchmark_tail_drain() {
    for ty in [&INTEGER, &NESTED] {
        for count in [40_000, 80_000, 160_000] {
            let mut samples = Vec::new();
            for _ in 0..5 {
                let mut entries = filled(ty, count);
                let start = std::time::Instant::now();
                while !entries.is_empty() {
                    let index = entries.len() - 1;
                    // SAFETY: each extraction is consumed before the next pop.
                    let entry = unsafe { entries.remove(index) };
                    std::hint::black_box(number(entry, ty));
                }
                samples.push(start.elapsed());
            }
            samples.sort();
            println!(
                "{count} {} tail drain median {:?}",
                if ty.kind == b'4' {
                    "scalar"
                } else {
                    "nested inline"
                },
                samples[2]
            );
        }
    }
}

/// Rows are tagged `key * 10_000 + position`; the comparator sees only `key`,
/// so a stable sort must leave positions ascending within each key.
fn stable_values(count: usize, pattern: usize) -> Vec<u128> {
    (0..count)
        .map(|i| {
            let key = match pattern {
                0 => i,
                1 => count - i,
                2 => 7,
                3 => i % 5,
                4 => i.min(count - i),
                _ => (i * 7919 + 97) % 31,
            };
            (key * 10_000 + i) as u128
        })
        .collect()
}

#[test]
fn stable_sort_keeps_equal_rows_in_order_and_inline_addresses() {
    for ty in [&SCALARS[3], &RANGE, &NESTED] {
        for count in [0, 1, 2, 15, 16, 17, 33, 127, 1024] {
            for pattern in 0..6 {
                for descending in [false, true] {
                    let values = stable_values(count, pattern);
                    let mut expected = values.clone();
                    // Rust's slice sort is stable: the reference order.
                    if descending {
                        expected.sort_by_key(|v| std::cmp::Reverse(v / 10_000));
                    } else {
                        expected.sort_by_key(|v| v / 10_000);
                    }
                    let mut entries = Entries::new(ty, None);
                    entries.try_reserve(count).unwrap();
                    for &value in &values {
                        push(&mut entries, value);
                    }
                    let key = |a| number(Entry { key: a, value: 0 }, ty) / 10_000;
                    let mut comparisons = 0usize;
                    entries
                        .sort_stable_rows_by(|a, b| {
                            comparisons += 1;
                            let order = key(a).cmp(&key(b));
                            if descending { order.reverse() } else { order }
                        })
                        .unwrap();
                    for (i, entry) in entries.iter().enumerate() {
                        assert_eq!(number(entry, ty), expected[i], "{count} {pattern}");
                        assert_addresses(&entries, i, entry);
                    }
                    // Probe plus a top-down merge sort: at most n - 1 probe
                    // comparisons and n * ceil(log2 n) for the sort itself,
                    // where 16-row insertion runs cost at most 120 each.
                    let bound = count + count * (usize::BITS - count.leading_zeros()) as usize;
                    assert!(
                        comparisons <= bound + 120 * count.div_ceil(16),
                        "{comparisons}"
                    );
                    let ordered = (pattern == 0 && !descending)
                        || (pattern == 1 && descending)
                        || pattern == 2;
                    if ordered {
                        assert_eq!(comparisons, count.saturating_sub(1));
                    }
                }
            }
        }
    }
}

#[test]
fn stable_sort_reports_allocation_failure_before_moving_rows() {
    let mut entries = filled(&NESTED, 64);
    entries.reverse();
    let compare = |a, b| {
        number(Entry { key: a, value: 0 }, &NESTED)
            .cmp(&number(Entry { key: b, value: 0 }, &NESTED))
    };
    #[cfg(feature = "allocation-checks")]
    {
        let before: Vec<_> = entries.iter().map(|entry| number(entry, &NESTED)).collect();
        crate::accounting::fail_after(Some(0));
        let failed = entries.sort_stable_rows_by(compare);
        crate::accounting::fail_after(None);
        assert_eq!(failed, Err(memory::AllocError::OutOfMemory));
        let after: Vec<_> = entries.iter().map(|entry| number(entry, &NESTED)).collect();
        assert_eq!(after, before);
    }
    entries.sort_stable_rows_by(compare).unwrap();
    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(number(entry, &NESTED), i as u128);
        assert_addresses(&entries, i, entry);
    }
}

#[test]
fn inconsistent_or_panicking_stable_comparison_keeps_every_row_and_address() {
    let mut entries = filled(&NESTED, 4096);
    entries.reverse();
    let mut count = 0;
    entries
        .sort_stable_rows_by(|_, _| {
            count += 1;
            if count % 3 == 0 {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        })
        .unwrap();
    let check = |entries: &Entries, rows: u128| {
        let mut values = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            assert_addresses(entries, i, entry);
            values.push(number(entry, &NESTED));
        }
        values.sort_unstable();
        assert_eq!(values, (0..rows).collect::<Vec<_>>());
    };
    check(&entries, 4096);
    // 32 descending rows: the probe stops at its first comparison, each
    // 16-row insertion run costs 120, and the merge check one more, so call
    // 243 is the merge's first comparison, with the whole left run in
    // scratch. By call 251 eight right rows have moved into the hole.
    // The merge takes all 16 right rows first: 258 calls without a panic.
    let mut entries = filled(&NESTED, 32);
    entries.reverse();
    let mut calls = 0;
    entries
        .sort_stable_rows_by(|a, b| {
            calls += 1;
            number(Entry { key: a, value: 0 }, &NESTED)
                .cmp(&number(Entry { key: b, value: 0 }, &NESTED))
        })
        .unwrap();
    assert_eq!(calls, 258);
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
