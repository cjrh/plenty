//! Copies, set algebra and updates reuse stored hash bits (issue #817).
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
static SET: Type = Type {
    kind: b'S',
    affine: true,
    key: Some(&INTEGER),
    ..INTEGER
};
static MAP: Type = Type {
    kind: b'D',
    affine: true,
    key: Some(&INTEGER),
    value: Some(&INTEGER),
    ..INTEGER
};

fn hashes() -> usize {
    HASHES.with(|count| count.get())
}

/// Keys whose hashes share one home bucket of a 16-bucket table, then others.
fn keys(n: usize) -> Vec<u128> {
    let mut colliding: Vec<u128> = (0..100_000)
        .filter(|key| unsafe { hash(*key, &INTEGER) } & 15 == 5)
        .take(6)
        .collect();
    let others: Vec<u128> = (1000..)
        .filter(|k| !colliding.contains(k))
        .take(n)
        .collect();
    colliding.extend(others);
    colliding.truncate(n);
    colliding
}

/// A collection of `ty` holding `keys` in order, after inserting and removing
/// `holes` extra keys first so that its entry slots are not sequential.
fn build(ty: &'static Type, keys: &[u128], holes: &[u128]) -> OwnedValue<'static> {
    let owner = OwnedValue {
        value: try_collection_new(ty, 0).unwrap() as u128,
        ty,
    };
    // SAFETY: the guard owns the collection, whose keys are integers.
    unsafe {
        let c = &mut *(owner.value as *mut Collection);
        for &key in holes {
            c.try_insert(key, key + 7).unwrap();
        }
        for &key in keys {
            c.try_insert(key, key + 1).unwrap();
        }
        for &key in holes {
            c.remove(key).unwrap();
        }
    }
    owner
}

fn collection(owner: &OwnedValue<'_>) -> &'static mut Collection {
    // SAFETY: test owners hold live collections for the whole test.
    unsafe { &mut *(owner.value as *mut Collection) }
}

/// The collection holds `model` in order, and its index finds each key.
fn assert_members(c: &Collection, model: &[u128]) {
    let keys: Vec<_> = c.entries.iter().map(|e| e.key).collect();
    assert_eq!(keys, model);
    for &key in model {
        // SAFETY: integer keys.
        assert!(unsafe { c.find(key) }.is_some(), "{key} is indexed");
    }
    for key in [3, 999_999, 12_345_678] {
        if !model.contains(&key) {
            assert!(unsafe { c.find(key) }.is_none(), "{key} is absent");
        }
    }
    assert_eq!(c.table.occupied(), model.len());
}

fn algebra(op: i64, left: &[u128], right: &[u128]) -> Vec<u128> {
    let mut out: Vec<u128> = match op {
        SET_UNION => left.to_vec(),
        SET_INTERSECTION => left.iter().copied().filter(|k| right.contains(k)).collect(),
        _ => left
            .iter()
            .copied()
            .filter(|k| !right.contains(k))
            .collect(),
    };
    if matches!(op, SET_UNION | SET_SYMMETRIC_DIFFERENCE) {
        out.extend(right.iter().filter(|k| !left.contains(k)));
    }
    out
}

/// Every operation for operands of several sizes, with and without holes,
/// returning the hashes each result cost.
fn check_algebra() -> Vec<usize> {
    let all = keys(200);
    let mut counts = Vec::new();
    for (l, r) in [(6, 6), (40, 40), (12, 150), (150, 12), (0, 20)] {
        let left = &all[..l];
        let right: Vec<u128> = all[l / 2..l / 2 + r].to_vec();
        for holes in [&[][..], &[77_777, 88_888][..]] {
            let a = build(&SET, left, holes);
            let b = build(&SET, &right, &[]);
            for op in SET_UNION..=SET_SYMMETRIC_DIFFERENCE {
                let before = hashes();
                // SAFETY: borrowed integer sets.
                let out =
                    unsafe { try_set_algebra(collection(&a), collection(&b), op, &SET) }.unwrap();
                counts.push(hashes() - before);
                let out = OwnedValue {
                    value: out,
                    ty: &SET,
                };
                assert_members(collection(&out), &algebra(op, left, &right));
            }
            assert_members(collection(&a), left);
            assert_members(collection(&b), &right);
        }
    }
    counts
}

#[test]
fn set_algebra_reuses_stored_hash_bits() {
    let counts = check_algebra();
    // A union of equal-sized operands hashes no key. Stored bits serve a
    // table only when its slot field is at least as wide as the source's, so
    // a smaller result or a larger probed operand hashes some keys, once.
    let unions: Vec<_> = counts[..16].iter().step_by(4).copied().collect();
    assert_eq!(unions, [0; 4], "{counts:?}");
    assert!(counts.iter().all(|&n| n <= 162), "{counts:?}");
}

#[test]
fn set_algebra_hashes_without_stored_bits() {
    let _limit = buckets::limit_stored_bits(0);
    let counts = check_algebra();
    assert!(counts[..16].iter().all(|&n| n > 0), "{counts:?}");
}

#[test]
fn copies_reuse_bucket_arrays_and_stored_bits() {
    let all = keys(60);
    for holes in [&[][..], &[77_777, 88_888][..]] {
        let set = build(&SET, &all, holes);
        let before = hashes();
        // SAFETY: an integer set.
        let copy = unsafe { try_copy_collection(set.value, &SET) }.unwrap();
        // Slots with holes still map through stored bits: the copy's table
        // is no longer than the source's.
        assert_eq!(hashes() - before, 0);
        let copy = OwnedValue {
            value: copy,
            ty: &SET,
        };
        assert_members(collection(&copy), &all);

        let map = build(&MAP, &all, holes);
        let before = hashes();
        let copy = unsafe { try_copy_collection(map.value, &MAP) }.unwrap();
        let expected = if holes.is_empty() { 0 } else { all.len() };
        assert_eq!(hashes() - before, expected, "dictionary copy");
        let copy = OwnedValue {
            value: copy,
            ty: &MAP,
        };
        let c = collection(&copy);
        assert_members(c, &all);
        for &key in &all {
            let slot = unsafe { c.find(key) }.unwrap();
            assert_eq!(c.entries.get(slot).value, key + 1);
        }
    }
}

#[test]
fn update_hashes_each_key_at_most_once() {
    let all = keys(200);
    for (l, r) in [(6, 6), (40, 40), (12, 150), (150, 12), (0, 20), (20, 0)] {
        for limit in [false, true] {
            let _limit = limit.then(|| buckets::limit_stored_bits(0));
            let left = &all[..l];
            let right: Vec<u128> = all[l / 2..l / 2 + r].to_vec();
            let a = build(&MAP, left, &[77_777]);
            let b = build(&MAP, &right, &[88_888]);
            let before = hashes();
            // SAFETY: integer dictionaries; the update consumes `b`'s entries.
            unsafe { collection(&a).try_update(collection(&b)) }.unwrap();
            let spent = hashes() - before;
            // Without stored bits, growing the target also rehashes its keys.
            let bound = if limit { r + l } else { r };
            assert!(spent <= bound, "{l} {r} {limit}: {spent}");
            if !limit && l == r {
                assert_eq!(spent, 0, "{l} {r}");
            }
            assert_members(collection(&a), &algebra(SET_UNION, left, &right));
            for &key in &right {
                let c = collection(&a);
                let slot = unsafe { c.find(key) }.unwrap();
                assert_eq!(c.entries.get(slot).value, key + 1);
            }
            assert!(collection(&b).entries.is_empty());
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
fn failed_update_and_algebra_leave_operands_intact() {
    let all = keys(60);
    let mut succeeded = false;
    for budget in 0..8 {
        let a = build(&MAP, &all[..20], &[77_777]);
        let b = build(&MAP, &all[10..], &[88_888]);
        let restore = Restore;
        crate::accounting::fail_after(Some(budget));
        // SAFETY: integer dictionaries.
        let result = unsafe { collection(&a).try_update(collection(&b)) };
        drop(restore);
        if result.is_ok() {
            succeeded = true;
            assert_members(collection(&a), &all);
            break;
        }
        assert_members(collection(&a), &all[..20]);
    }
    assert!(succeeded);
    let a = build(&SET, &all[..40], &[77_777]);
    let b = build(&SET, &all[20..], &[]);
    for op in SET_UNION..=SET_SYMMETRIC_DIFFERENCE {
        let restore = Restore;
        crate::accounting::fail_after(Some(0));
        // SAFETY: borrowed integer sets.
        let result = unsafe { try_set_algebra(collection(&a), collection(&b), op, &SET) };
        drop(restore);
        assert!(result.is_err());
        assert_members(collection(&a), &all[..40]);
        assert_members(collection(&b), &all[20..]);
    }
}

#[test]
fn retain_members_repairs_the_index_in_place() {
    let all = keys(200);
    for limit in [false, true] {
        let _limit = limit.then(|| buckets::limit_stored_bits(0));
        for (l, r) in [(6, 3), (40, 40), (150, 12), (12, 150)] {
            for common in [true, false] {
                let left = &all[..l];
                let right: Vec<u128> = all[l / 2..l / 2 + r].to_vec();
                let a = build(&SET, left, &[77_777, 88_888]);
                let b = build(&SET, &right, &[]);
                let before = hashes();
                // SAFETY: integer sets.
                unsafe { collection(&a).retain_members(collection(&b), common) };
                let spent = hashes() - before;
                let op = if common {
                    SET_INTERSECTION
                } else {
                    SET_DIFFERENCE
                };
                assert_members(collection(&a), &algebra(op, left, &right));
                if !limit && collection(&a).table.len() == collection(&b).table.len() {
                    assert_eq!(spent, 0);
                }
                assert!(spent <= 2 * l, "{spent}");
            }
        }
    }
}
