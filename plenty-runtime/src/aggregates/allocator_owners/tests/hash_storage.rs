use super::*;
use std::sync::atomic::Ordering::Relaxed;

static MAP: Type = Type {
    kind: b'D',
    value: Some(&INTEGER),
    ..LIST
};
static SET: Type = Type { kind: b'S', ..LIST };
static INLINE_MAP: Type = Type {
    value: Some(&RECORD),
    ..MAP
};

#[test]
fn hash_construction_preflights_capacity_and_unwinds_every_partial_owner() {
    for ty in [&SET, &INLINE_MAP] {
        let counts = counts();
        let provider = allocator(&counts, "hash");
        assert_eq!(
            try_hash_new_in(&provider, ty, usize::MAX),
            Err(AllocError::CapacityOverflow)
        );
        // Entry count fits the ordinary check, but the value payload does not.
        // For scalar set rows the payload alone fits, but its backing header does not.
        assert_eq!(
            try_hash_new_in(&provider, ty, isize::MAX as usize / size_of::<Entry>()),
            Err(AllocError::CapacityOverflow)
        );
        assert_eq!(counts.calls.load(Relaxed), 0);
        for (cap, allocations) in [(0, 2), (8, 5)] {
            for budget in 0..allocations {
                let before = counts.releases.load(Relaxed);
                counts.budget.store(budget, Relaxed);
                assert_eq!(
                    try_hash_new_in(&provider, ty, cap),
                    Err(AllocError::OutOfMemory)
                );
                assert_eq!(counts.releases.load(Relaxed) - before, budget as usize);
            }
        }
        drop(provider);
        assert_eq!(counts.drops.load(Relaxed), 1);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn empty_and_reserved_hash_owners_grow_without_any_ambient_allocation() {
    struct Pool(Vec<Block>);
    impl Provider for Pool {
        fn allocate(&mut self, _: Layout) -> Result<Block, AllocError> {
            self.0.pop().ok_or(AllocError::OutOfMemory)
        }
        fn release(&mut self, block: Block) {
            self.0.push(block);
        }
    }
    for ty in [&MAP, &SET] {
        for cap in [0, 8] {
            let blocks = (0..10)
                .map(|_| {
                    Allocator::system()
                        .allocate(Layout::from_size_align(4096, 16).unwrap())
                        .unwrap()
                })
                .collect();
            let provider = Allocator::new(&Allocator::system(), Pool(blocks))
                .unwrap_or_else(|_| panic!("bootstrap"));
            crate::accounting::fail_after(Some(0));
            // Don't assert/panic while even the panic handler cannot allocate.
            let result = (|| unsafe {
                let c = try_hash_new_in(&provider, ty, cap)?;
                drop(provider);
                let owner = OwnedValue {
                    value: c as u128,
                    ty,
                };
                for k in 0..24 {
                    (*c).try_adopt(k, k)?;
                }
                let found = (*c).find(23);
                drop(owner);
                Ok::<_, AllocError>(found)
            })();
            crate::accounting::fail_after(None);
            assert_eq!(result, Ok(Some(23)));
        }
    }
}

#[test]
fn sparse_hash_growth_failure_retains_wrapped_lookups_order_and_slot_ids() {
    let _limit = buckets::limit_stored_bits(0);
    let keys: Vec<u128> = (0..)
        .filter(|k| unsafe { hash(*k, &INTEGER) } & 15 == 15)
        .take(10)
        .collect();
    for ty in [&MAP, &SET] {
        for budget in 0..=3 {
            let counts = counts();
            let provider = allocator(&counts, "hash");
            unsafe {
                let c = try_hash_new_in(&provider, ty, 8).unwrap();
                drop(provider);
                for &k in &keys[..8] {
                    (*c).insert(k, k + 1);
                }
                counts.budget.store(0, Relaxed);
                for i in [1, 4] {
                    assert!((*c).remove(keys[i]).is_some());
                }
                (*c).insert(keys[8], keys[8] + 1); // reuse slot 4, leave slot 1 dead
                (*c).insert(keys[0], 99); // duplicate even with provider exhausted
                let before = counts.calls.load(Relaxed);
                counts.budget.store(budget, Relaxed);
                let result = (*c).try_reserve(16);
                assert_eq!(
                    result,
                    if budget == 3 {
                        Ok(())
                    } else {
                        Err(AllocError::OutOfMemory)
                    }
                );
                assert_eq!(
                    counts.calls.load(Relaxed) - before,
                    (budget as usize + 1).min(3)
                );
                assert_eq!((*c).table.len(), if budget == 3 { 64 } else { 16 });
                let order = [0, 2, 3, 5, 6, 7, 4];
                assert!((*c).entries.indices().eq(order));
                for (i, slot) in [0, 2, 3, 5, 6, 7, 8].into_iter().zip(order) {
                    let key = keys[i];
                    assert_eq!((*c).find(key), Some(slot));
                    assert_eq!(
                        (*c).entries.get(slot).value,
                        if ty.kind == b'S' {
                            0
                        } else if i == 0 {
                            99
                        } else {
                            key + 1
                        }
                    );
                }
                assert_eq!((*c).find(keys[1]), None);
                counts.budget.store(0, Relaxed);
                (*c).insert(keys[9], 44);
                assert_eq!((*c).find(keys[9]), Some(1));
                if ty.kind == b'S' {
                    let other = try_collection_new(&SET, 1).unwrap();
                    (*other).insert(keys[3], 0);
                    (*c).retain_members(&*other, false);
                    assert_eq!((*c).find(keys[3]), None);
                    plenty_release(other.cast());
                }
                collection(58, c as u128, 0, 0, ptr::null());
                assert_eq!((*c).len(), 0);
                (*c).insert(18, 19);
                assert_eq!((*c).find(18), Some(0));
                plenty_release(c.cast());
            }
            assert_eq!(counts.drops.load(Relaxed), 1);
            assert_eq!(
                counts.calls.load(Relaxed),
                counts.releases.load(Relaxed) + usize::from(budget != 3)
            );
        }
    }
}

#[test]
fn sparse_hash_inline_relocation_preserves_live_children_and_extracted_value() {
    static KEY: Type = Type {
        kind: b'C',
        affine: true,
        inline_bytes: 32,
        variants: &[
            Variant {
                name: "id",
                fields: &[&INTEGER],
            },
            Variant {
                name: "index",
                fields: &[&INTEGER],
            },
        ],
        ..INTEGER
    };
    static T: Type = Type {
        key: Some(&KEY),
        ..INLINE_MAP
    };
    let counts = counts();
    let provider = allocator(&counts, "hash");
    DROPS.with(|d| d.borrow_mut().clear());
    unsafe {
        let c = try_hash_new_in(&provider, &T, 5).unwrap();
        drop(provider);
        for id in 0u128..5 {
            let key = [id, id + 10];
            let mut value = [0; 6];
            record(id as u64, &mut value);
            (*c).insert(key.as_ptr() as u128, value.as_ptr() as u128);
        }
        let mut snapshot = [0u128; 7];
        let popped = (*c).remove([1u128, 11].as_ptr() as u128).unwrap();
        ranges::store(snapshot.as_mut_ptr(), popped, &RECORD);
        release((*c).remove([3u128, 13].as_ptr() as u128).unwrap(), &RECORD);
        let old = (*c).entries.slot_address(0, true);
        (*c).try_reserve(32).unwrap();
        assert_ne!((*c).entries.slot_address(0, true), old);
        for i in [0u128, 2, 4] {
            assert_eq!((*c).find([i, i + 10].as_ptr() as u128), Some(i as usize));
            let key = (*c).entries.slot_address(i as usize, false);
            assert_eq!(key.read(), key.add(1) as u128);
        }
        for id in 5u128..7 {
            let mut value = [0; 6];
            record(id as u64, &mut value);
            (*c).insert([id, id + 10].as_ptr() as u128, value.as_ptr() as u128);
        }
        release(snapshot[0], &RECORD);
        plenty_release(c.cast());
    }
    DROPS.with(|d| assert_eq!(d.borrow().as_slice(), [3, 1, 0, 2, 4, 5, 6]));
    assert_eq!(counts.calls.load(Relaxed), counts.releases.load(Relaxed));
}

#[test]
fn hash_worker_drop_finishes_child_before_owner_storage() {
    static T: Type = Type {
        value: Some(&BOX),
        ..MAP
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let parent = Counts::new(events.clone());
    let child = Counts::new(events.clone());
    let a = allocator(&parent, "parent");
    let b = allocator(&child, "child");
    let ptr = unsafe {
        let c = try_hash_new_in(&a, &T, 2).unwrap();
        (*c).insert(42, try_box_new_in(&b, &BOX, 7).unwrap() as u128);
        c.expose_provenance()
    };
    drop((a, b));
    std::thread::spawn(move || unsafe { plenty_release(ptr::with_exposed_provenance_mut(ptr)) })
        .join()
        .unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        ["child", "parent", "parent", "parent", "parent", "parent"]
    );
    assert_eq!(parent.drops.load(Relaxed), 1);
    assert_eq!(child.drops.load(Relaxed), 1);
}

static TEXT: Type = Type {
    kind: b's',
    ..INTEGER
};
static KEY: Type = Type {
    kind: b'C',
    affine: true,
    inline_bytes: 32,
    variants: &[Variant {
        name: "text",
        fields: &[&TEXT],
    }],
    drop: Some(key_drop),
    ..INTEGER
};
unsafe extern "C" fn key_drop(receiver: *mut u128) {
    let identity = unsafe { receiver.add(2).read() };
    DROPS.with(|d| d.borrow_mut().push(identity));
}

#[test]
fn consuming_update_uses_destination_storage_and_drops_duplicate_identity_once() {
    static T: Type = Type {
        key: Some(&KEY),
        ..INLINE_MAP
    };
    for (missing, fail) in [(true, false), (true, true), (false, false)] {
        DROPS.with(|d| d.borrow_mut().clear());
        let (left, right) = (counts(), counts());
        let (a, b) = (allocator(&left, "dest"), allocator(&right, "src"));
        unsafe {
            let dest = try_hash_new_in(&a, &T, 1).unwrap();
            let src = try_hash_new_in(&b, &T, 4).unwrap();
            drop((a, b));
            let insert = |c: *mut Collection, text: &[u8], id| {
                let text = strings::new(text) as u128;
                let mut key = [1, text];
                let mut value = [0; 6];
                record(id, &mut value);
                (*c).insert(key.as_mut_ptr() as u128, value.as_mut_ptr() as u128);
                text
            };
            let first = insert(dest, b"duplicate", 101);
            let duplicate = insert(src, b"duplicate", 102);
            let hole = insert(src, b"dead row!", 103);
            let unique = missing.then(|| insert(src, b"a new key", 104));
            release((*src).remove([1, hole].as_ptr() as u128).unwrap(), &RECORD);
            right.budget.store(0, Relaxed);
            if fail || !missing {
                left.budget.store(0, Relaxed);
            }
            let result = (*dest).try_update(&mut *src);
            let failed = Err(AllocError::OutOfMemory);
            assert_eq!(result, if fail { failed } else { Ok(()) });
            plenty_release(src.cast()); // compiler drops the consumed source on both paths
            assert_eq!(right.calls.load(Relaxed), 5);
            assert_eq!(right.drops.load(Relaxed), 1);
            let entry = (*dest).entries.get(0);
            assert_eq!((entry.key as *const u128).add(1).read(), first);
            assert_eq!(
                (entry.value as *const u128).add(1).read(),
                if fail { 101 } else { 102 }
            );
            assert_eq!((*dest).find([1, first].as_ptr() as u128), Some(0));
            assert_eq!((*dest).len(), 1 + usize::from(missing && !fail));
            if let Some(text) = unique
                && !fail
            {
                assert_eq!((*dest).find([1, text].as_ptr() as u128), Some(1));
            }
            plenty_release(dest.cast());
            let mut expected = vec![hole, 103, first, duplicate, 101, 102];
            if let Some(text) = unique {
                expected.extend([text, 104]);
            }
            expected.sort_unstable();
            DROPS.with(|d| {
                let mut actual = d.borrow_mut();
                actual.sort_unstable();
                assert_eq!(*actual, expected);
            });
            assert_eq!(
                left.calls.load(Relaxed),
                left.releases.load(Relaxed) + usize::from(fail)
            );
        }
    }
}

#[test]
fn selected_dictionary_helper_misses_and_hits_preserve_slot_and_operand_ownership() {
    static T: Type = Type {
        key: Some(&TEXT),
        ..INLINE_MAP
    };
    DROPS.with(|d| d.borrow_mut().clear());
    let policy = counts();
    let a = allocator(&policy, "helpers");
    unsafe {
        let owner = try_hash_new_in(&a, &T, 1).unwrap() as u128;
        drop(a);
        let reference = &owner as *const u128;
        let key = strings::pack(b"key") as u64;
        let mut storage = [0; 7]; // include the destructor's virtual receiver slot
        let mut default = |id| {
            let payload: &mut [u128; 6] = (&mut storage[1..]).try_into().unwrap();
            record(id, payload);
            payload.as_mut_ptr() as u64
        };
        let generic = |key, v| plenty_dict_get_or_insert(reference, key, 0, v, 0);
        let bytes = |key: &[u8], v| {
            plenty_dict_get_or_insert_bytes(reference, key.as_ptr(), key.len() as u64, v, 0)
        };
        let slot = generic(key, default(0));
        assert_eq!(slot.1, 0);
        assert_eq!(
            (slot.0 as *const u128).read(),
            (slot.0 as *const u128).add(1) as u128
        );
        policy.budget.store(0, Relaxed);
        let hit = generic(key, default(1));
        assert_eq!((hit.0, hit.1), (slot.0, 2)); // generic helper destroys affine default
        let v = default(2);
        let hit = bytes(b"key", v);
        assert_eq!((hit.0, hit.1), (slot.0, 2));
        DROPS.with(|d| assert_eq!(d.borrow().as_slice(), [1]));
        release(v as u128, &RECORD); // bytes helper always leaves a hit's default to caller
        let invalid = bytes(b"\xff", default(3));
        assert_eq!((invalid.0, invalid.1), (0, 1));
        let failure = bytes(b"new owned heap key", default(4));
        assert_eq!((failure.0, failure.1), (0, 3));
        let failure = generic(strings::new(b"another heap key") as u64, default(5));
        assert_eq!((failure.0, failure.1), (0, 1));
        let mut state = [0u128; 4];
        let state_ptr = state.as_mut_ptr() as u128;
        let probe = |key| {
            collection(
                DICT_LAZY_PROBE,
                reference as u128,
                key,
                state_ptr,
                ptr::null(),
            )
        };
        let miss = strings::new(b"factory failed") as u128;
        probe(miss);
        assert_eq!(state[0], 0);
        release(miss, &TEXT); // factory error: caller drops key without calling finish
        let miss = strings::new(b"factory succeeded") as u128;
        probe(miss);
        assert_eq!(
            lazy_finish(reference as u128, miss, default(6) as u128, state_ptr),
            1u128 << 64
        );
        probe(key as u128);
        assert_eq!(state[0], slot.0 as u128);
        assert_eq!((*(owner as *mut Collection)).len(), 1);
        plenty_release(owner as *mut Header);
    }
    DROPS.with(|d| assert_eq!(d.borrow().as_slice(), [1, 2, 3, 4, 5, 6, 0]));
}

#[test]
fn text_helper_reports_selected_growth_failure_and_preserves_writable_slot() {
    static T: Type = Type {
        key: Some(&TEXT),
        ..MAP
    };
    let counts = counts();
    let a = allocator(&counts, "text");
    unsafe {
        let owner = try_hash_new_in(&a, &T, 1).unwrap() as u128;
        drop(a);
        counts.budget.store(0, Relaxed);
        let insert = |key| plenty_dict_get_or_insert_text(&owner, key, 1, 0);
        let key = strings::pack(b"first") as u64;
        let slot = insert(key);
        assert_eq!(slot.1, 0);
        (slot.0 as *mut u128).write(42);
        let failure = insert(strings::new(b"new heap key") as u64);
        assert_eq!((failure.0, failure.1), (0, 1));
        let hit = insert(key);
        assert_eq!((hit.0, hit.1), (slot.0, 2));
        assert_eq!((hit.0 as *const u128).read(), 42);
        plenty_release(owner as *mut Header);
    }
}
