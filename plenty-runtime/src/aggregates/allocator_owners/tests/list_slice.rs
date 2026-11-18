use super::*;
use std::sync::atomic::Ordering::Relaxed;

#[test]
fn plain_slices_copy_selected_rows_and_retain_provider_for_growth() {
    let both = SLICE_START | SLICE_STOP;
    for (bounds, expected) in [
        ([1, 4, 1, both], &[11, 12, 13][..]),
        ([0, 5, 2, both], &[10, 12, 14]),
        ([0, 0, -1, 0], &[14, 13, 12, 11, 10]),
        // The explicit stop -1 means the *last* element, not before the first.
        ([0, -1, -1, SLICE_STOP], &[]),
    ] {
        let out = counts();
        let b = allocator(&out, "slice");
        unsafe {
            let source = try_collection_new(&LIST, 5).unwrap();
            for value in 10..15 {
                (*source).insert(value, 0);
            }
            let blocks = if expected.is_empty() { 2 } else { 3 }; // state, object, nonempty rows
            #[cfg(feature = "allocation-checks")]
            crate::accounting::fail_after(Some(blocks));
            let result =
                try_list_slice_stepped(&*source, bounds, &LIST, CopyDestination::Selected(&b));
            #[cfg(feature = "allocation-checks")]
            crate::accounting::fail_after(None);
            let list = result.unwrap() as *mut Collection;
            plenty_release(source.cast());
            drop(b);
            assert_eq!(out.calls.load(Relaxed), blocks);
            assert_eq!(out.drops.load(Relaxed), 0);
            assert_eq!((*list).len(), expected.len());
            for (i, value) in expected.iter().enumerate() {
                assert_eq!((*list).entries.get(i).key, *value);
            }
            let capacity = (*list).entries.capacity();
            assert!(capacity >= expected.len());
            for _ in expected.len()..=capacity {
                (*list).try_adopt(99, 0).unwrap();
            }
            assert_eq!((*list).entries.get(capacity).key, 99);
            assert_eq!(out.calls.load(Relaxed), blocks + 1);
            plenty_release(list.cast());
            assert_eq!(out.releases.load(Relaxed), blocks + 1);
            assert_eq!(out.drops.load(Relaxed), 1);
        }
    }
}

#[test]
fn adjacent_range_slice_rows_point_to_their_own_payloads() {
    static RANGES: Type = Type {
        key: Some(&RANGE),
        ..LIST
    };
    let out = allocator(&counts(), "slice");
    unsafe {
        let source = try_collection_new(&RANGES, 3).unwrap();
        for start in 0..3 {
            let range = ranges::Range::new(start, start + 5, 1, true);
            (*source).insert(&raw const range as u128, 0);
        }
        for destination in [CopyDestination::Ordinary, CopyDestination::Selected(&out)] {
            let result = try_list_slice_stepped(
                &*source,
                [1, 3, 1, SLICE_START | SLICE_STOP],
                &RANGES,
                destination,
            )
            .unwrap() as *mut Collection;
            let rows = &(*result).entries;
            assert_eq!(rows.len(), 2);
            for i in 0..rows.len() {
                // Compare addresses with both owners live, without following the stored pointer.
                let payload = rows.slot_address(i, false).add(1).cast::<ranges::Range>();
                assert_eq!(rows.key(i), payload as u128);
                assert_eq!((*payload).start, i as u64 + 1);
            }
            plenty_release(result.cast());
        }
        plenty_release(source.cast());
    }
}

#[test]
fn ordinary_brackets_do_not_inherit_the_source_provider() {
    let input = counts();
    let a = allocator(&input, "source");
    unsafe {
        let source = try_list_new_in(&a, &LIST, 1).unwrap();
        (*source).insert(42, 0);
        drop(a);
        input.budget.store(0, Relaxed);
        let mut args = [source as u128, 0, 0, 0, 1, 0]; // [:] with default bounds
        operation(LIST_SLICE, args.as_mut_ptr(), &LIST, ptr::null());
        assert_eq!(args[3] >> 64, 0);
        let copy = args[3] as *mut Collection;
        plenty_release(source.cast());
        assert_eq!(input.drops.load(Relaxed), 1);
        assert!((*copy).entries.allocator().is_none());
        assert_eq!((*copy).entries.get(0).key, 42);
        plenty_release(copy.cast());
    }
}

static TEXT: Type = Type {
    kind: b's',
    ..INTEGER
};
static SHARED: Type = Type { kind: b'W', ..BOX };
static NESTED_BOX: Type = Type {
    key: Some(&BOX),
    ..BOX
};
static ROW: Type = Type {
    kind: b'C',
    affine: true,
    inline_bytes: 48,
    variants: &[
        Variant {
            name: "text",
            fields: &[&TEXT],
        },
        Variant {
            name: "shared",
            fields: &[&SHARED],
        },
        Variant {
            name: "child",
            fields: &[&NESTED_BOX],
        },
    ],
    ..INTEGER
};
static ROWS: Type = Type {
    key: Some(&ROW),
    ..LIST
};

unsafe fn nested_value(outer: u128) -> Field {
    unsafe {
        let inner = Boxed::content(outer as *const Boxed, &BOX).read();
        Boxed::content(inner as *const Boxed, &INTEGER)
    }
}

#[test]
fn managed_slice_unwinds_each_refusal_and_shares_only_leaves() {
    // Three output list blocks, two selected rows each with two levels of boxes.
    const BLOCKS: usize = 3 + 2 * 2;
    let input = counts();
    let out = counts();
    let a = allocator(&input, "input");
    let b = allocator(&out, "slice");
    unsafe {
        let source = try_list_new_in(&a, &ROWS, 3).unwrap();
        let text = strings::try_new("long enough not to be inline")
            .unwrap()
            .cast::<Header>();
        let shared = try_box_new_in(&a, &SHARED, 77).unwrap();
        let mut original = [[0; 3]; 3];
        for (i, row) in original.iter_mut().enumerate() {
            if i != 0 {
                plenty_retain(text);
                plenty_retain(shared.cast());
            }
            let inner = try_box_new_in(&a, &BOX, i as u128).unwrap();
            let outer = try_box_new_in(&a, &NESTED_BOX, inner as u128).unwrap();
            *row = [text as u128, shared as u128, outer as u128];
            (*source).insert(row.as_ptr() as u128, 0);
        }
        let input_blocks = input.calls.load(Relaxed);
        input.budget.store(0, Relaxed);
        drop(a);
        let slice = || {
            try_list_slice_stepped(
                &*source,
                [0, 0, -2, 0],
                &ROWS,
                CopyDestination::Selected(&b),
            )
        };
        let check_source = || {
            assert_eq!((*source).len(), 3);
            for (i, row) in original.iter().enumerate() {
                assert_eq!(&*((*source).entries.key(i) as *const [u128; 3]), row);
                assert_eq!(nested_value(row[2]).read(), i as u128);
            }
            assert_eq!(input.calls.load(Relaxed), input_blocks);
            assert_eq!(input.releases.load(Relaxed), 0);
        };
        for budget in 0..BLOCKS {
            out.budget.store(budget as isize, Relaxed);
            let calls = out.calls.load(Relaxed);
            let released = out.releases.load(Relaxed);
            assert_eq!(slice(), Err(AllocError::OutOfMemory));
            assert_eq!(out.calls.load(Relaxed) - calls, budget + 1);
            assert_eq!(out.releases.load(Relaxed) - released, budget);
            assert_eq!((*text).refs.load(Relaxed), 3);
            assert_eq!((*shared).header.refs.load(Relaxed), 3);
            check_source();
        }
        out.budget.store(BLOCKS as isize, Relaxed);
        let calls = out.calls.load(Relaxed);
        let released = out.releases.load(Relaxed);
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(Some(BLOCKS)); // only the provider's system blocks
        let result = slice();
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(None);
        let list = result.unwrap() as *mut Collection;
        assert_eq!(out.calls.load(Relaxed) - calls, BLOCKS);
        assert_eq!((*list).len(), 2);
        for (k, i) in [2, 0].into_iter().enumerate() {
            let dest = (*list).entries.key(k);
            assert_ne!(dest, (*source).entries.key(i));
            let [t, s, outer] = *(dest as *const [u128; 3]);
            assert_eq!((t, s), (text as u128, shared as u128));
            assert_ne!(outer, original[i][2]);
            let number = nested_value(outer);
            assert_eq!(number.read(), i as u128);
            number.write(99, &INTEGER);
        }
        check_source();
        assert_eq!((*text).refs.load(Relaxed), 5);
        assert_eq!((*shared).header.refs.load(Relaxed), 5);
        plenty_release(source.cast());
        drop(b);
        assert_eq!(input.releases.load(Relaxed), input_blocks - 1); // only Shared survives
        assert_eq!((*text).refs.load(Relaxed), 2);
        assert_eq!((*shared).header.refs.load(Relaxed), 2);
        plenty_release(list.cast());
        assert_eq!(out.releases.load(Relaxed) - released, BLOCKS);
        assert_eq!(out.drops.load(Relaxed), 1);
        assert_eq!(input.releases.load(Relaxed), input_blocks);
        assert_eq!(input.drops.load(Relaxed), 1);
    }
}
