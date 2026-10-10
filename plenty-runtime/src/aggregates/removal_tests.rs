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
    assert_eq!(
        c.table.iter().filter(|slot| **slot != 0).count(),
        model.len()
    );
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
        let table_capacity = c.table.capacity();
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
        assert_eq!(c.table.capacity(), table_capacity);
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
