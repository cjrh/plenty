use super::*;
use recursive_copy::{KEY_SET, key};
use std::sync::atomic::Ordering::Relaxed;

#[test]
fn selected_key_view_copies_private_keys_not_custom_cleanup_values() {
    static MAP: Type = Type {
        kind: b'D',
        value: Some(&RECORD),
        ..KEY_SET
    };
    DROPS.with(|drops| drops.borrow_mut().clear());
    let input = counts();
    let output = counts();
    let a = allocator(&input, "dictionary");
    let b = allocator(&output, "set");
    unsafe {
        let map = try_dict_set_new_in(&a, &MAP, 16).unwrap();
        let text = strings::try_new("immutable text in an affine key").unwrap();
        plenty_retain(text.cast());
        for id in [4, 7] {
            let mut payload = [0; 6];
            record(id, &mut payload);
            (*map).insert(
                key(id.into(), text).as_ptr() as u128,
                payload.as_ptr() as u128,
            );
        }
        drop(a);
        let calls = input.calls.load(Relaxed);
        input.budget.store(0, Relaxed);
        let check_source = || {
            assert_eq!((*map).len(), 2);
            for (slot, id) in [4, 7].into_iter().enumerate() {
                assert_eq!((*map).find(key(id, text).as_ptr() as u128), Some(slot));
                let value = (*map).entries.get(slot).value as *const [u128; 6];
                assert_eq!((*value)[1], id);
            }
            DROPS.with(|drops| assert!(drops.borrow().is_empty()));
            assert_eq!(input.calls.load(Relaxed), calls);
        };
        // Owner, hash state, rows, order links, buckets. Fail each request,
        // then retry the very same dictionary with a sufficient budget.
        for budget in 0..5 {
            output.budget.store(budget, Relaxed);
            let released = output.releases.load(Relaxed);
            assert_eq!(
                try_key_view_to_set_in(&b, &*map, &KEY_SET),
                Err(AllocError::OutOfMemory)
            );
            assert_eq!(output.releases.load(Relaxed) - released, budget as usize);
            assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 2);
            check_source();
        }
        output.budget.store(5, Relaxed);
        let allocated = output.calls.load(Relaxed);
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(Some(5)); // exactly the provider's system blocks
        let result = try_key_view_to_set_in(&b, &*map, &KEY_SET);
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(None);
        let set = result.unwrap() as *mut Collection;
        drop(b);
        assert_eq!(output.calls.load(Relaxed) - allocated, 5);
        assert_eq!((*set).ty().kind, b'S');
        assert_eq!((*set).len(), 2);
        for i in 0..2 {
            assert_ne!((*set).entries.key(i), (*map).entries.key(i));
        }
        (*set).remove(key(4, text).as_ptr() as u128).unwrap();
        check_source();
        plenty_release(map.cast());
        assert_eq!(input.drops.load(Relaxed), 1);
        assert_eq!(input.releases.load(Relaxed), calls);
        DROPS.with(|drops| assert_eq!(drops.borrow().as_slice(), &[4, 7]));
        assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 1);
        assert!((*set).find(key(7, text).as_ptr() as u128).is_some());
        let released = output.releases.load(Relaxed);
        plenty_release(set.cast());
        assert_eq!(output.releases.load(Relaxed) - released, 5);
        assert_eq!(output.drops.load(Relaxed), 1);
    }
}

#[test]
fn selected_key_view_entry_converts_hash_dictionary_to_finite_set() {
    static MAP: Type = Type {
        kind: b'D',
        value: Some(&INTEGER),
        ..finite_sets::SMALL
    };
    let set_type = &finite_sets::SMALL;
    unsafe {
        let map = try_collection_new(&MAP, 2).unwrap();
        let keys = set_type.key().finite.unwrap().values;
        (*map).insert(keys[2], 42);
        (*map).insert(keys[0], 0);
        assert!((*map).entries.bits().is_none());
        let mut args = [0, map as u128, 0, 0, 0, 0];
        #[cfg(feature = "allocation-checks")]
        {
            crate::accounting::fail_after(Some(0));
            crate::aggregates::operation(
                ALLOCATOR_KEY_VIEW_TO_SET,
                args.as_mut_ptr(),
                set_type,
                ptr::null(),
            );
            crate::accounting::fail_after(None);
            assert_eq!(args[3], fallible(Err(AllocError::OutOfMemory)));
            assert_eq!((*map).entries.get((*map).find(keys[2]).unwrap()).value, 42);
            crate::accounting::fail_after(Some(1)); // mask fits in the selected owner
        }
        crate::aggregates::operation(
            ALLOCATOR_KEY_VIEW_TO_SET,
            args.as_mut_ptr(),
            set_type,
            ptr::null(),
        );
        #[cfg(feature = "allocation-checks")]
        crate::accounting::fail_after(None);
        assert_eq!(args[3] >> 64, 0);
        let set = args[3] as *mut Collection;
        plenty_release(map.cast());
        assert!((*set).entries.bits().is_some());
        assert!((*set).entries.iter().map(|e| e.key).eq([keys[0], keys[2]]));
        plenty_release(set.cast());
    }
}
