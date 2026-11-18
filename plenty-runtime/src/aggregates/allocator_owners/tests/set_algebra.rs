use super::*;
use std::sync::atomic::Ordering::Relaxed;

static HASH: Type = Type { kind: b'S', ..LIST };

// Slot identities matter too: a failed operation must leave the sparse index
// and iteration order intact, not just preserve the unordered membership.
fn snapshot(c: &Collection) -> Vec<(usize, u128, u128)> {
    c.entries
        .indices()
        .map(|i| {
            let Entry { key, value } = c.entries.get(i);
            assert_eq!(unsafe { c.find(key) }, Some(i));
            (i, key, value)
        })
        .collect()
}

fn source(ty: &'static Type, keys: [u128; 2], hole: bool) -> (*mut Collection, Arc<Counts>) {
    let counts = counts();
    let provider = allocator(&counts, "source");
    let c = try_dict_set_new_in(&provider, ty, 3).unwrap();
    unsafe {
        if hole {
            (*c).insert(999, 0);
        }
        for (i, key) in keys.into_iter().enumerate() {
            let value = if ty.value.is_some() {
                10 + i as u128
            } else {
                0
            };
            (*c).insert(key, value);
        }
        if hole {
            (*c).remove(999).unwrap();
        }
    }
    counts.budget.store(0, Relaxed);
    (c, counts)
}

#[test]
fn selected_set_algebra_uses_only_the_output_provider() {
    static BYTE: Type = Type {
        kind: b'5',
        ..INTEGER
    };
    static MAP: Type = Type {
        kind: b'D',
        value: Some(&INTEGER),
        ..HASH
    };
    static BYTE_MAP: Type = Type {
        value: Some(&BYTE),
        ..MAP
    };
    static FLAGS: Type = Type {
        key: finite_sets::SMALL.key,
        ..MAP
    };
    static BYTE_FLAGS: Type = Type {
        key: finite_sets::SMALL.key,
        ..BYTE_MAP
    };
    let small = &finite_sets::SMALL;
    let medium = &finite_sets::MEDIUM;
    let large = &finite_sets::LARGE;
    for (ty, left, right) in [
        (&HASH, &HASH, &HASH),
        (&HASH, &HASH, &MAP),
        (&HASH, &MAP, &HASH),
        (&HASH, &MAP, &BYTE_MAP),
        (small, small, small),
        (small, small, &FLAGS),
        (small, &FLAGS, small),
        (small, &FLAGS, &BYTE_FLAGS),
        (medium, medium, medium),
        (large, large, large),
    ] {
        let mask = ty.key().kind == b'B';
        let blocks = if mask { 1 } else { 5 }; // owner, plus hash state, rows, order and buckets
        let last = if mask {
            crate::entries::enum_bits::cardinality(ty.key()) - 1
        } else {
            42
        };
        let key = |i: usize| {
            if !mask {
                i as u128
            } else if let Some(f) = ty.key().finite {
                f.values[i]
            } else {
                ty.key().pack(0, i)
            }
        };
        for op in SET_UNION..=SET_SYMMETRIC_DIFFERENCE {
            let (a, ac) = source(left, [key(last), key(0)], !mask);
            let (b, bc) = source(right, [key(1), key(last)], !mask);
            let calls = (ac.calls.load(Relaxed), bc.calls.load(Relaxed));
            let out = counts();
            let provider = allocator(&out, "output");
            unsafe {
                let before = (snapshot(&*a), snapshot(&*b));
                for budget in 0..blocks {
                    out.budget.store(budget, Relaxed);
                    let released = out.releases.load(Relaxed);
                    assert_eq!(
                        try_set_algebra_in(&provider, &*a, &*b, op, ty),
                        Err(AllocError::OutOfMemory)
                    );
                    assert_eq!(out.releases.load(Relaxed) - released, budget as usize);
                    assert_eq!((snapshot(&*a), snapshot(&*b)), before);
                }
                let hashes = || HASHES.with(|h| h.get());
                let start = hashes();
                let legacy = try_set_algebra(&*a, &*b, op, ty).unwrap() as *mut Collection;
                let cost = hashes() - start;
                let start = hashes();
                let reads = crate::entries::DICT_VALUE_READS.get();
                let allocated = out.calls.load(Relaxed);
                out.budget.store(blocks, Relaxed);
                // Each granted provider request needs exactly one system block.
                // Leave no budget for an additional implicit scratch allocation.
                #[cfg(feature = "allocation-checks")]
                crate::accounting::fail_after(Some(blocks as usize));
                let result = try_set_algebra_in(&provider, &*a, &*b, op, ty);
                #[cfg(feature = "allocation-checks")]
                crate::accounting::fail_after(None);
                let result = result.unwrap() as *mut Collection;
                assert_eq!(crate::entries::DICT_VALUE_READS.get(), reads);
                assert_eq!(hashes() - start, cost);
                assert_eq!(out.calls.load(Relaxed) - allocated, blocks as usize);
                assert_eq!((snapshot(&*a), snapshot(&*b)), before);
                assert_eq!((ac.calls.load(Relaxed), bc.calls.load(Relaxed)), calls);
                drop(provider);
                plenty_release(a.cast());
                plenty_release(b.cast());
                for c in [ac, bc] {
                    assert_eq!(c.releases.load(Relaxed), c.calls.load(Relaxed));
                    assert_eq!(c.drops.load(Relaxed), 1);
                }
                assert_eq!(out.drops.load(Relaxed), 0);
                let output = &mut *result;
                assert!(ptr::eq(output.ty(), ty));
                let expected = &mut *legacy;
                let order: &[usize] = match op {
                    SET_UNION if mask => &[0, 1, last],
                    SET_UNION => &[last, 0, 1],
                    SET_INTERSECTION => &[last],
                    SET_DIFFERENCE => &[0],
                    SET_SYMMETRIC_DIFFERENCE => &[0, 1],
                    _ => unreachable!(),
                };
                assert!(
                    output
                        .entries
                        .iter()
                        .map(|e| e.key)
                        .eq(order.iter().copied().map(key))
                );
                assert_eq!(snapshot(output), snapshot(expected));
                assert_eq!(output.entries.bits(), expected.entries.bits());
                if mask {
                    assert!(output.table.is_empty());
                    for (i, _, _) in snapshot(expected) {
                        assert_eq!(output.entries.key_ref(i), expected.entries.key_ref(i));
                    }
                }
                plenty_release(legacy.cast());
                let released = out.releases.load(Relaxed);
                plenty_release(result.cast());
                assert_eq!(out.releases.load(Relaxed) - released, blocks as usize);
                assert_eq!(out.drops.load(Relaxed), 1);
            }
        }
    }
}

#[test]
fn selected_union_retains_existing_heap_text_without_reallocating_it() {
    static TEXT: Type = Type {
        kind: b's',
        ..INTEGER
    };
    static TEXT_SET: Type = Type {
        key: Some(&TEXT),
        ..HASH
    };
    unsafe {
        let a = try_collection_new(&TEXT_SET, 1).unwrap();
        let b = try_collection_new(&TEXT_SET, 0).unwrap();
        let text = strings::try_new("not an inline text").unwrap();
        (*a).try_adopt(text as u128, 0).unwrap();
        let result = try_set_algebra_in(&Allocator::system(), &*a, &*b, SET_UNION, &TEXT_SET)
            .unwrap() as *mut Collection;
        plenty_release(a.cast());
        plenty_release(b.cast());
        let stored = (*result).entries.get(0).key as *const Text;
        assert_eq!(stored, text.cast_const());
        assert_eq!(strings::utf8(&stored), "not an inline text");
        plenty_release(result.cast());
    }
}

#[test]
fn selected_union_copies_affine_record_keys_into_private_rows() {
    use super::recursive_copy::{KEY_SET, key};
    let input = counts();
    let a = allocator(&input, "input");
    let out = counts();
    let b = allocator(&out, "result");
    unsafe {
        let left = try_dict_set_new_in(&a, &KEY_SET, 1).unwrap();
        let right = try_dict_set_new_in(&a, &KEY_SET, 1).unwrap();
        let text = strings::try_new("shared immutable leaf").unwrap();
        (*left).insert(key(1, text).as_ptr() as u128, 0);
        plenty_retain(text.cast());
        (*right).insert(key(2, text).as_ptr() as u128, 0);
        let before = input.calls.load(Relaxed);
        input.budget.store(0, Relaxed);
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(Some(5));
        let result = try_set_algebra_in(&b, &*left, &*right, SET_UNION, &KEY_SET);
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(None);
        let result = result.unwrap() as *mut Collection;
        assert_eq!(input.calls.load(Relaxed), before);
        assert_eq!(out.calls.load(Relaxed), 5);
        for (i, source) in [left, right].into_iter().enumerate() {
            let orig = (*source).entries.key(0);
            assert_eq!((*result).find(orig), Some(i));
            let stored = (*result).entries.key(i);
            assert_ne!(stored, orig);
            assert_eq!(*(stored as *const [u128; 2]), key(i as u128 + 1, text));
            plenty_release(source.cast());
        }
        assert_eq!(input.releases.load(Relaxed), before);
        assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 2);
        assert_eq!(
            strings::utf8(&(text as *const Text)),
            "shared immutable leaf"
        );
        plenty_release(result.cast());
        assert_eq!(out.releases.load(Relaxed), 5);
    }
}

#[test]
fn selected_view_union_entry_copies_affine_keys_not_dictionary_values() {
    use super::recursive_copy::{KEY_SET, key};
    static LEFT: Type = Type {
        kind: b'D',
        value: Some(&RECORD),
        ..KEY_SET
    };
    static RIGHT: Type = Type {
        value: Some(&INTEGER),
        ..LEFT
    };
    DROPS.with(|drops| drops.borrow_mut().clear());
    unsafe {
        let left = try_collection_new(&LEFT, 1).unwrap();
        let right = try_collection_new(&RIGHT, 1).unwrap();
        let text = strings::try_new("immutable leaf in both affine keys").unwrap();
        let mut payload = [0; 6];
        record(53, &mut payload);
        (*left).insert(key(1, text).as_ptr() as u128, payload.as_ptr() as u128);
        plenty_retain(text.cast());
        (*right).insert(key(2, text).as_ptr() as u128, 17);

        let mut args = [0, left as u128, right as u128, 0, 0, 0];
        let union = |args: &mut [u128; 6]| {
            crate::aggregates::operation(
                ALLOCATOR_SET_UNION,
                args.as_mut_ptr(),
                &KEY_SET,
                ptr::null(),
            );
        };
        #[cfg(feature = "allocation-checks")]
        {
            crate::accounting::fail_after(Some(4));
            union(&mut args);
            crate::accounting::fail_after(None);
            assert_eq!(args[3], fallible(Err(AllocError::OutOfMemory)));
            assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 2);
            crate::accounting::fail_after(Some(5));
        }
        union(&mut args);
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(None);
        assert_eq!(args[3] >> 64, 0);
        let result = args[3] as *mut Collection;
        assert!(ptr::eq((*result).ty(), &KEY_SET));
        assert_eq!((*result).len(), 2);
        assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 4);
        for (i, source) in [left, right].into_iter().enumerate() {
            let probe = key(i as u128 + 1, text);
            assert_eq!((*source).find(probe.as_ptr() as u128), Some(0));
            assert_eq!((*result).find(probe.as_ptr() as u128), Some(i));
            assert_ne!((*source).entries.key(0), (*result).entries.key(i));
        }
        let retained = (*left).entries.get(0).value as *const [u128; 6];
        assert_eq!((*retained)[1], 53);
        assert_eq!((*right).entries.get(0).value, 17);
        DROPS.with(|drops| assert!(drops.borrow().is_empty()));
        plenty_release(left.cast());
        plenty_release(right.cast());
        DROPS.with(|drops| assert_eq!(drops.borrow().as_slice(), &[53]));
        assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 2);
        assert_eq!(
            strings::utf8(&(text as *const Text)),
            "immutable leaf in both affine keys"
        );
        plenty_release(result.cast());
        DROPS.with(|drops| assert_eq!(drops.borrow().as_slice(), &[53]));
    }
}
