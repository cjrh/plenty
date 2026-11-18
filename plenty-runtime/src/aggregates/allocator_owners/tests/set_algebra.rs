use super::*;
use std::sync::atomic::Ordering::Relaxed;

static HASH: Type = Type { kind: b'S', ..LIST };

// Slot identities matter too: a failed operation must leave the sparse index
// and iteration order intact, not just preserve the unordered membership.
fn snapshot(c: &Collection) -> Vec<(usize, u128)> {
    c.entries
        .indices()
        .map(|i| {
            let key = c.entries.get(i).key;
            assert_eq!(unsafe { c.find(key) }, Some(i));
            (i, key)
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
        for key in keys {
            (*c).insert(key, 0);
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
    for ty in [
        &HASH,
        &finite_sets::SMALL,
        &finite_sets::MEDIUM,
        &finite_sets::LARGE,
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
            let (a, ac) = source(ty, [key(last), key(0)], !mask);
            let (b, bc) = source(ty, [key(1), key(last)], !mask);
            let calls = (ac.calls.load(Relaxed), bc.calls.load(Relaxed));
            let out = counts();
            let provider = allocator(&out, "output");
            unsafe {
                let before = (snapshot(&*a), snapshot(&*b));
                for budget in 0..blocks {
                    out.budget.store(budget, Relaxed);
                    let released = out.releases.load(Relaxed);
                    assert_eq!(
                        try_set_algebra_in(&provider, &*a, &*b, op),
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
                let allocated = out.calls.load(Relaxed);
                out.budget.store(blocks, Relaxed);
                // Each granted provider request needs exactly one system block.
                // Leave no budget for an additional implicit scratch allocation.
                #[cfg(feature = "allocation-checks")]
                crate::accounting::fail_after(Some(blocks as usize));
                let result = try_set_algebra_in(&provider, &*a, &*b, op);
                #[cfg(feature = "allocation-checks")]
                crate::accounting::fail_after(None);
                let result = result.unwrap() as *mut Collection;
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
                    for (i, _) in snapshot(expected) {
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
        let result = try_set_algebra_in(&Allocator::system(), &*a, &*b, SET_UNION).unwrap()
            as *mut Collection;
        plenty_release(a.cast());
        plenty_release(b.cast());
        let stored = (*result).entries.get(0).key as *const Text;
        assert_eq!(stored, text.cast_const());
        assert_eq!(strings::utf8(&stored), "not an inline text");
        plenty_release(result.cast());
    }
}

#[test]
#[should_panic(expected = "selected algebra requires non-affine keys")]
fn selected_algebra_rejects_affine_keys_before_member_copy() {
    static TY: Type = Type {
        key: Some(&RECORD),
        ..HASH
    };
    let c = try_collection_new(&TY, 0).unwrap();
    let _owner = OwnedValue {
        value: c as u128,
        ty: &TY,
    };
    unsafe { try_set_algebra_in(&Allocator::system(), &*c, &*c, SET_UNION) }.unwrap();
}
