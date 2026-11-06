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
    redacted: false,
    transparent: false,
    debug: None,
    display: None,
    quantity: None,
    finite: None,
};
static MAP: Type = Type {
    kind: b'D',
    affine: true,
    key: Some(&INTEGER),
    value: Some(&INTEGER),
    ..INTEGER
};

fn owned(capacity: usize) -> OwnedValue<'static> {
    OwnedValue {
        value: try_collection_new(&MAP, capacity).unwrap() as u128,
        ty: &MAP,
    }
}

fn assert_model(c: &Collection, model: &[(u128, u128)]) {
    assert_eq!(c.len(), model.len());
    assert_eq!(
        c.entries
            .iter()
            .map(|e| (e.key, e.value))
            .collect::<Vec<_>>(),
        model
    );
    assert_eq!(
        c.entries
            .iter()
            .rev()
            .map(|e| (e.key, e.value))
            .collect::<Vec<_>>(),
        model.iter().copied().rev().collect::<Vec<_>>()
    );
    for &(key, value) in model {
        // SAFETY: all test operands use the integer map descriptor.
        let slot = unsafe { c.find(key) }.expect("live key is reachable");
        assert_eq!(c.entries.get(slot).value, value);
    }
    assert_eq!(c.table.occupied(), model.len());
}

#[test]
fn backward_shift_repairs_wrapped_clusters_and_misses() {
    // Hash to the final two buckets so the cluster crosses bucket zero, with
    // different home buckets to exercise both moving and skipping references.
    let keys: Vec<u128> = (0..10000)
        .filter(|key| unsafe { hash(*key, &INTEGER) } & 15 >= 14)
        .take(8)
        .collect();
    for removed in 0..keys.len() {
        let owner = owned(8);
        // SAFETY: the guard owns the collection throughout all raw ABI access.
        unsafe {
            let c = &mut *(owner.value as *mut Collection);
            let mut model: Vec<_> = keys.iter().map(|&key| (key, key + 1)).collect();
            for &(key, value) in &model {
                c.try_insert(key, value).unwrap();
            }
            let (key, value) = model.remove(removed);
            assert_eq!(c.remove(key), Some(value));
            assert_eq!(c.remove(key), None);
            assert_model(c, &model);
            c.try_insert(key, 0).unwrap();
            model.push((key, 0));
            c.try_insert(model[0].0, 42).unwrap();
            model[0].1 = 42;
            assert_model(c, &model);
            for &(key, value) in &model {
                assert_eq!(c.remove(key), Some(value));
            }
            assert_model(c, &[]);
            c.try_insert(0, 0).unwrap();
            assert_model(c, &[(0, 0)]);
        }
    }
}

#[test]
fn mixed_hash_operations_match_an_ordered_model() {
    let owner = owned(0);
    let mut model = Vec::new();
    let mut random = 1234567u64;
    unsafe {
        let c = &mut *(owner.value as *mut Collection);
        for step in 0..6000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let key = ((random >> 32) % 97) as u128;
            match random % 5 {
                0 | 1 => {
                    let expected = model
                        .iter()
                        .position(|&(k, _)| k == key)
                        .map(|i| model.remove(i).1);
                    assert_eq!(c.remove(key), expected);
                }
                2 if step % 17 == 0 => {
                    c.try_reserve(100).unwrap();
                }
                _ => {
                    c.try_insert(key, step).unwrap();
                    if let Some(entry) = model.iter_mut().find(|(k, _)| *k == key) {
                        entry.1 = step;
                    } else {
                        model.push((key, step));
                    }
                }
            }
            assert_model(c, &model);
        }
    }
}

#[cfg(feature = "allocation-checks")]
struct Restore;
#[cfg(feature = "allocation-checks")]
impl Drop for Restore {
    fn drop(&mut self) {
        crate::accounting::fail_after(None);
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn drain_and_churn_reuse_all_buffers_without_allocating() {
    let owner = owned(128);
    unsafe {
        let c = &mut *(owner.value as *mut Collection);
        for key in 0..128 {
            c.try_insert(key, key).unwrap();
        }
        let capacity = c.entries.capacity();
        let table_capacity = c.table.capacity_bytes();
        let restore = Restore;
        crate::accounting::fail_after(Some(0));
        for key in 0..4096 {
            assert_eq!(c.remove(key), Some(key));
            c.try_insert(key + 128, key + 128).unwrap();
        }
        for key in 4096..4224 {
            assert_eq!(c.remove(key), Some(key));
        }
        assert_eq!(c.remove(0), None);
        for key in 0..128 {
            c.try_insert(key, key).unwrap();
        }
        drop(restore);
        assert_eq!(c.entries.capacity(), capacity);
        assert_eq!(c.table.capacity_bytes(), table_capacity);
        assert_model(c, &(0..128).map(|k| (k, k)).collect::<Vec<_>>());
    }
}

#[cfg(feature = "allocation-checks")]
#[test]
fn failed_sparse_reservation_preserves_order_and_bucket_ids() {
    for budget in 0..=3 {
        let owner = owned(8);
        unsafe {
            let c = &mut *(owner.value as *mut Collection);
            for key in 0..8 {
                c.try_insert(key, key).unwrap();
            }
            for key in [0, 2, 7] {
                c.remove(key);
            }
            c.try_insert(20, 20).unwrap();
            let restore = Restore;
            crate::accounting::fail_after(Some(budget));
            let result = c.try_reserve(100);
            drop(restore);
            assert_eq!(
                result,
                if budget == 3 {
                    Ok(())
                } else {
                    Err(AllocError::OutOfMemory)
                }
            );
            assert_model(c, &[(1, 1), (3, 3), (4, 4), (5, 5), (6, 6), (20, 20)]);
            c.try_insert(21, 21).unwrap();
            assert_eq!(c.remove(20), Some(20));
        }
    }
}

#[test]
#[ignore = "release-mode timing benchmark; run with --ignored --nocapture"]
fn benchmark_hash_removal() {
    for count in [40_000, 80_000, 160_000] {
        for churn in [false, true] {
            let mut samples = Vec::new();
            for _ in 0..5 {
                let owner = owned(count);
                unsafe {
                    let c = &mut *(owner.value as *mut Collection);
                    for key in 0..count as u128 {
                        c.try_insert(key, key).unwrap();
                    }
                    let start = std::time::Instant::now();
                    for key in 0..count as u128 {
                        assert_eq!(c.remove(key), Some(key));
                        if churn {
                            c.try_insert(key + count as u128, key).unwrap();
                        }
                    }
                    samples.push(start.elapsed());
                }
            }
            samples.sort();
            println!(
                "{count} {} median {:?}",
                if churn { "churn" } else { "drain" },
                samples[2]
            );
        }
        let owner = owned(count);
        unsafe {
            let c = &mut *(owner.value as *mut Collection);
            for key in 0..count as u128 {
                c.try_insert(key, key).unwrap();
            }
            for key in 0..count as u128 - 10 {
                c.remove(key);
            }
            let start = std::time::Instant::now();
            for _ in 0..10000 {
                std::hint::black_box(c.entries.iter().map(|e| e.key).sum::<u128>());
            }
            println!(
                "{count} capacity, 10 live, 10000 traversals {:?}",
                start.elapsed()
            );
        }
    }
}

fn hashes() -> usize {
    HASHES.with(|count| count.get())
}

#[test]
fn insert_hashes_each_key_once() {
    let owner = owned(4);
    // SAFETY: the guard owns the collection throughout all raw ABI access.
    unsafe {
        let c = &mut *(owner.value as *mut Collection);
        // New keys within capacity, a replacement, then a key that rebuilds
        // the table from stored hash bits, hashing no stored key (issue #761).
        for key in 0..4 {
            let before = hashes();
            c.insert(key, key + 1);
            assert_eq!(hashes() - before, 1, "new key {key}");
        }
        let before = hashes();
        c.insert(2, 30);
        assert_eq!(hashes() - before, 1, "replacement");
        let table = c.table.len();
        let mut key = 4;
        loop {
            let before = hashes();
            let stored = c.len();
            c.insert(key, key + 1);
            if c.table.len() != table {
                assert!(stored > 4, "the table grew");
                assert_eq!(hashes() - before, 1, "rebuild at key {key}");
                break;
            }
            assert_eq!(hashes() - before, 1, "new key {key}");
            key += 1;
        }
        let model: Vec<_> = (0..=key)
            .map(|k| (k, if k == 2 { 30 } else { k + 1 }))
            .collect();
        assert_model(c, &model);
    }
}

#[test]
fn get_or_insert_hashes_once_and_keeps_existing_values() {
    let owner = owned(4);
    // SAFETY: the guard owns the collection throughout all raw ABI access.
    unsafe {
        let c = &mut *(owner.value as *mut Collection);
        for key in 0..12 {
            let before = hashes();
            let row = c.try_get_or_insert(key, key + 1).unwrap();
            assert_eq!(hashes() - before, 1, "new key {key}");
            assert_eq!(c.entries.get(row).value, key + 1);
        }
        for key in 0..12 {
            let before = hashes();
            let row = c.try_get_or_insert(key, 99).unwrap();
            assert_eq!(hashes() - before, 1, "present key {key}");
            assert_eq!(c.entries.get(row).value, key + 1);
        }
        let model: Vec<_> = (0..12).map(|k| (k, k + 1)).collect();
        assert_model(c, &model);
    }
}

#[test]
fn removal_and_growth_reuse_stored_hash_bits() {
    // Keys sharing one home bucket of the 16-bucket table form a cluster, so
    // removal shifts references and growth must separate them (issue #761).
    let keys: Vec<u128> = (0..100_000)
        .filter(|key| unsafe { hash(*key, &INTEGER) } & 15 == 5)
        .take(64)
        .collect();
    let owner = owned(4);
    // SAFETY: the guard owns the collection throughout all raw ABI access.
    unsafe {
        let c = &mut *(owner.value as *mut Collection);
        let mut model = Vec::new();
        for &key in &keys[..6] {
            c.try_insert(key, key).unwrap();
            model.push((key, key));
        }
        assert_eq!(c.table.len(), 16);
        for &(key, value) in &model[..3] {
            let before = hashes();
            assert_eq!(c.remove(key), Some(value));
            assert_eq!(hashes() - before, 1, "removal of {key}");
        }
        model.drain(..3);
        assert_model(c, &model);
        // Reinsertion reuses the freed slots; later keys grow the table.
        for &key in &keys[..3] {
            c.try_insert(key, key + 1).unwrap();
            model.push((key, key + 1));
        }
        for &key in &keys[6..] {
            let before = hashes();
            c.try_insert(key, key).unwrap();
            assert_eq!(hashes() - before, 1, "insertion of {key}");
            model.push((key, key));
        }
        assert!(c.table.len() >= 128);
        assert_model(c, &model);
        for &(key, value) in model.iter().step_by(2) {
            assert_eq!(c.remove(key), Some(value));
        }
        let model: Vec<_> = model.iter().copied().skip(1).step_by(2).collect();
        assert_model(c, &model);
    }
}

#[test]
fn growth_and_removal_fall_back_to_hashing_keys_without_stored_bits() {
    // Tables over 2^32 buckets lack the stored bits growth and removal need;
    // the cap makes a 16-bucket table take those fallbacks.
    let _limit = buckets::limit_stored_bits(0);
    let keys: Vec<u128> = (0..100_000)
        .filter(|key| unsafe { hash(*key, &INTEGER) } & 15 == 5)
        .take(64)
        .collect();
    let owner = owned(4);
    // SAFETY: the guard owns the collection throughout all raw ABI access.
    unsafe {
        let c = &mut *(owner.value as *mut Collection);
        let mut model = Vec::new();
        for &key in &keys[..6] {
            c.try_insert(key, key).unwrap();
            model.push((key, key));
        }
        assert_eq!(c.table.len(), 16);
        // Removal rehashes each later reference in the cluster.
        let before = hashes();
        assert_eq!(c.remove(keys[0]), Some(keys[0]));
        assert_eq!(hashes() - before, 1 + 5, "removal rehashes the cluster");
        model.remove(0);
        assert_model(c, &model);
        let mut rebuilt = false;
        for &key in &keys[6..] {
            let (before, stored, table) = (hashes(), c.len(), c.table.len());
            c.try_insert(key, key).unwrap();
            let rebuild = if c.table.len() == table {
                0
            } else {
                rebuilt = true;
                stored
            };
            assert_eq!(hashes() - before, 1 + rebuild, "insertion of {key}");
            model.push((key, key));
        }
        assert!(rebuilt && c.table.len() >= 128);
        assert_model(c, &model);
        for &(key, value) in model.iter().step_by(2) {
            assert_eq!(c.remove(key), Some(value));
        }
        let model: Vec<_> = model.iter().copied().skip(1).step_by(2).collect();
        assert_model(c, &model);
    }
}
