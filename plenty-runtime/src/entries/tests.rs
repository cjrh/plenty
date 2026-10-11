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
        let slot = entries.data.as_ptr().add(index * entries.stride);
        assert_eq!(entry.key, slot.add(1) as u128);
        if entries.key.kind == b'C' {
            assert_eq!(*(entry.key as *const u128), slot.add(2) as u128);
        }
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

#[cfg(feature = "allocation-checks")]
#[test]
fn tail_pop_append_and_drain_reuse_capacity_without_allocating() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::accounting::fail_after(None);
        }
    }
    for ty in [&INTEGER, &RANGE, &NESTED] {
        let mut entries = filled(ty, 128);
        let base = entries.data.as_ptr();
        let capacity = entries.capacity();
        let restore = Restore;
        crate::accounting::fail_after(Some(0));
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
