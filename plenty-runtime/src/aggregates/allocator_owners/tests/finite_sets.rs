use super::*;
use crate::entries::enum_bits::Bits;
use std::sync::atomic::Ordering::Relaxed;

const UNIT: Variant = Variant {
    name: "V",
    fields: &[],
};
static BOOL: Type = Type {
    kind: b'b',
    ..INTEGER
};
static FLAG: Type = Type {
    kind: b'B',
    variants: &[
        UNIT,
        Variant {
            name: "Flag",
            fields: &[&BOOL],
        },
    ],
    finite: Some(&Finite {
        offsets: &[0, 1],
        values: &[0, 1 << 64, (1 << 64) | 1],
    }),
    ..INTEGER
};
pub(super) static SMALL: Type = Type {
    kind: b'S',
    key: Some(&FLAG),
    ..LIST
};
pub(super) static MEDIUM: Type = Type {
    key: Some(&Type {
        kind: b'B',
        variants: &[UNIT; 65],
        ..INTEGER
    }),
    ..SMALL
};
pub(super) static LARGE: Type = Type {
    key: Some(&Type {
        kind: b'B',
        variants: &[UNIT; 256],
        ..INTEGER
    }),
    ..SMALL
};

#[test]
fn finite_set_rejects_overflow_before_provider_and_reports_owner_failure() {
    let counts = counts();
    let provider = allocator(&counts, "set");
    counts.budget.store(0, Relaxed);
    let too_large = isize::MAX as usize / size_of::<Entry>() + 1;
    assert_eq!(
        dict_set_new_in(&provider, &SMALL, too_large),
        Err(AllocError::CapacityOverflow)
    );
    assert_eq!(counts.calls.load(Relaxed), 0);
    assert_eq!(
        dict_set_new_in(&provider, &SMALL, 0),
        Err(AllocError::OutOfMemory)
    );
    drop(provider);
    assert_eq!(counts.calls.load(Relaxed), 1);
    assert_eq!(counts.releases.load(Relaxed), 0);
    assert_eq!(counts.drops.load(Relaxed), 1);
}

#[test]
fn finite_set_uses_one_selected_block_and_canonical_slots_at_every_width() {
    // Even a request too large for hash backing is valid if the ordinary
    // Entry count fits: capacity is always the complete domain, not the hint.
    let largest_request = isize::MAX as usize / size_of::<Entry>();
    for (ty, domain, request) in [
        (&SMALL, 3, 0),
        (&MEDIUM, 65, 65),
        (&LARGE, 256, largest_request),
    ] {
        let counts = counts();
        let provider = allocator(&counts, "set");
        counts.budget.store(1, Relaxed);
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(Some(1)); // the provider's one system backing block
        let result = dict_set_new_in(&provider, ty, request);
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(None);
        let header = result.unwrap();
        drop(provider);
        assert_eq!(counts.drops.load(Relaxed), 0);
        unsafe {
            let set = &mut *header.cast::<Collection>();
            let ordinary = try_collection_new(ty, request).unwrap();
            let reference = &mut *ordinary;
            let last = domain - 1;
            let key = ty
                .key()
                .finite
                .map_or_else(|| ty.key().pack(0, last), |f| f.values[last]);
            reference.try_insert(key, 0).unwrap();
            let canonical = reference.entries.key_ref(last);

            #[cfg(feature = "allocation-checks")]
            crate::accounting::begin_no_allocations();
            assert_eq!(set.entries.bits(), Some(Bits::new(domain)));
            assert!(set.table.is_empty());
            assert_eq!(set.entries.capacity(), reference.entries.capacity());
            assert_eq!(set.entries.capacity(), domain);
            set.try_reserve(largest_request).unwrap();
            set.try_insert(key, 0).unwrap();
            set.try_insert(key, 0).unwrap();
            assert!(set.entries.indices().eq([last]));
            let borrowed = set.entries.key_ref(last);
            assert_eq!(borrowed, canonical);
            assert_eq!(set.remove(key), Some(0));
            set.try_update(reference).unwrap();
            assert_eq!(set.find(key), Some(last));
            set.entries.clear();
            assert_eq!(set.entries.bits(), Some(Bits::new(domain)));
            #[cfg(feature = "allocation-checks")]
            crate::accounting::end_no_allocations();
            plenty_release(ordinary.cast());
            plenty_release(header);
            assert_eq!(*borrowed, key);
        }
        assert_eq!(counts.calls.load(Relaxed), 1);
        assert_eq!(counts.releases.load(Relaxed), 1);
        assert_eq!(counts.drops.load(Relaxed), 1);
    }
}
