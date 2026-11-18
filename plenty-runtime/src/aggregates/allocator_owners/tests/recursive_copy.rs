//! Only hashable fields occur in keys. Exercise allocating children through
//! the same private copy walk on a legal dictionary value, not fictitious
//! set keys containing mutable collections (which would hash as addresses).
use super::*;
use std::sync::atomic::Ordering::Relaxed;

static TEXT: Type = Type {
    kind: b's',
    ..INTEGER
};
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
            name: "name",
            fields: &[&TEXT],
        },
    ],
    ..INTEGER
};
pub(super) fn key(id: u128, text: *mut Text) -> [u128; 2] {
    [id, text as u128]
}
pub(super) static KEY_SET: Type = Type {
    kind: b'S',
    key: Some(&KEY),
    ..LIST
};
static MAYBE_BOX: Type = Type {
    kind: b'B',
    affine: true,
    variants: &[
        Variant {
            name: "Empty",
            fields: &[],
        },
        Variant {
            name: "Some",
            fields: &[&BOX],
        },
    ],
    ..INTEGER
};
static BOXES: Type = Type {
    key: Some(&MAYBE_BOX),
    ..LIST
};
static BOX_LIST: Type = Type {
    key: Some(&BOXES),
    ..BOX
};
static ARRAY: Type = Type {
    kind: b'a',
    inline_bytes: 32,
    affine: true,
    key: Some(&BOX_LIST),
    array: Some(&ArrayLayout { len: 2, stride: 16 }),
    ..INTEGER
};
static SHARED: Type = Type { kind: b'W', ..BOX };
static PAYLOAD: Type = Type {
    kind: b'B',
    inline_bytes: 80,
    affine: true,
    variants: &[
        Variant {
            name: "Empty",
            fields: &[],
        },
        Variant {
            name: "Full",
            fields: &[&SHARED, &ARRAY, &KEY_SET, &finite_sets::LARGE],
        },
    ],
    ..INTEGER
};
static MAP: Type = Type {
    kind: b'D',
    key: Some(&KEY),
    value: Some(&PAYLOAD),
    ..LIST
};

unsafe fn boxed_list(provider: &Allocator, n: u128) -> u128 {
    unsafe {
        let list = try_list_new_in(provider, &BOXES, 1).unwrap();
        let element = try_box_new_in(provider, &BOX, n).unwrap();
        (*list).insert(MAYBE_BOX.pack(element as u128, 1), 0);
        try_box_new_in(provider, &BOX_LIST, list as u128).unwrap() as u128
    }
}
unsafe fn list_in(boxed: u128) -> *mut Collection {
    unsafe { Boxed::content(boxed as *const Boxed, &BOXES).read() as *mut Collection }
}

unsafe fn number_in(list: *const Collection) -> Field {
    unsafe {
        let element = (*list).entries.key(0);
        assert_eq!(MAYBE_BOX.tag(element), 1);
        Boxed::content(MAYBE_BOX.unpack(element) as *const Boxed, &INTEGER)
    }
}

#[test]
fn recursive_copy_routes_allocations_retains_leaves_and_unwinds_each_failure() {
    // Map, two boxed lists each holding Some(Box(n)), member set, mask. Retains of the
    // text and Shared box must cost no blocks, from any provider.
    const BLOCKS: usize = 5 + 2 * (1 + 3 + 1) + 5 + 1;
    let input = counts();
    let a = allocator(&input, "input");
    let out = counts();
    let b = allocator(&out, "result");
    unsafe {
        let source = try_dict_set_new_in(&a, &MAP, 1).unwrap();
        let text = strings::try_new("long enough not to be inline").unwrap();
        let key = key(42, text);
        let shared = try_box_new_in(&a, &SHARED, 73).unwrap();
        let members = try_dict_set_new_in(&a, &KEY_SET, 1).unwrap();
        (*members).insert(key.as_ptr() as u128, 0);
        let mask = try_dict_set_new_in(&a, &finite_sets::LARGE, 0).unwrap();
        let flag = finite_sets::LARGE.key().pack(0, 250);
        (*mask).insert(flag, 0);
        let fields = [
            shared as u128,
            boxed_list(&a, 11),
            boxed_list(&a, 22),
            members as u128,
            mask as u128,
        ];
        plenty_retain(text.cast());
        (*source).insert(
            key.as_ptr() as u128,
            PAYLOAD.pack(fields.as_ptr() as u128, 1),
        );
        let row = (*source).entries.get(0);
        let before = input.calls.load(Relaxed);
        input.budget.store(0, Relaxed);
        drop(a);
        for budget in 0..=BLOCKS {
            out.budget.store(budget as isize, Relaxed);
            let calls = out.calls.load(Relaxed);
            let frees = out.releases.load(Relaxed);
            // Release callbacks log in a Vec. Pre-reserve so the *whole*
            // walk, including rollback, has only the provider's system calls.
            out.events.lock().unwrap().reserve(BLOCKS);
            let mut copy = source as u128;
            #[cfg(feature = "allocation-checks")]
            crate::accounting::fail_after(Some(budget));
            let result = try_copy_into(&mut copy, &MAP, CopyDestination::Selected(&b));
            #[cfg(feature = "allocation-checks")]
            crate::accounting::fail_after(None);
            assert_eq!(input.calls.load(Relaxed), before);
            assert_eq!(input.releases.load(Relaxed), 0);
            assert_eq!((*source).find(key.as_ptr() as u128), Some(0));
            assert_eq!((*source).entries.get(0).key, row.key);
            assert_eq!((*source).entries.get(0).value, row.value);
            assert_eq!(*(row.value as *const [u128; 5]), fields);
            assert_eq!((*members).find(key.as_ptr() as u128), Some(0));
            assert_eq!((*mask).find(flag), Some(250));
            for (i, n) in [(1, 11), (2, 22)] {
                assert_eq!(number_in(list_in(fields[i])).read(), n);
            }
            if budget < BLOCKS {
                assert_eq!(result, Err(AllocError::OutOfMemory));
                assert_eq!(copy, 0);
                assert_eq!(out.calls.load(Relaxed) - calls, budget + 1);
                assert_eq!(out.releases.load(Relaxed) - frees, budget);
                assert_eq!((*shared).header.refs.load(Relaxed), 1);
                assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 2);
                continue;
            }
            result.unwrap();
            assert_eq!(out.calls.load(Relaxed) - calls, BLOCKS);
            assert_eq!(out.releases.load(Relaxed), frees);
            assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 4);
            assert_eq!((*shared).header.refs.load(Relaxed), 2);
            let dict = &*(copy as *const Collection);
            let copied = dict.entries.get(dict.find(key.as_ptr() as u128).unwrap());
            assert_ne!(copied.key, row.key);
            assert_ne!(copied.value, row.value);
            assert_eq!(PAYLOAD.tag(copied.value), 1);
            let dest = *(copied.value as *const [u128; 5]);
            assert_eq!(dest[0], fields[0]); // Shared, not a copied box
            for i in 1..5 {
                assert_ne!(dest[i], fields[i]);
            }
            for (i, n) in [(1, 11), (2, 22)] {
                let list = list_in(dest[i]);
                assert_ne!(list, list_in(fields[i]));
                let number = number_in(list);
                assert_eq!(number.read(), n);
                assert_ne!(number.at, number_in(list_in(fields[i])).at);
                number.write(99, &INTEGER);
                assert_eq!(number_in(list_in(fields[i])).read(), n);
            }
            let keys = &mut *(dest[3] as *mut Collection);
            assert_ne!(keys.entries.key(0), (*members).entries.key(0));
            assert_eq!(keys.remove(key.as_ptr() as u128), Some(0));
            assert_eq!((*members).len(), 1);
            assert_eq!((*(dest[4] as *mut Collection)).remove(flag), Some(0));
            assert_eq!((*mask).len(), 1);
            plenty_release(source.cast());
            assert_eq!(input.releases.load(Relaxed), before - 1); // only Shared remains
            assert_eq!(Boxed::content(shared, &INTEGER).read(), 73);
            assert_eq!(*(copied.key as *const [u128; 2]), key);
            assert_eq!((*text.cast::<Header>()).refs.load(Relaxed), 1);
            release(copy, &MAP);
            assert_eq!(out.releases.load(Relaxed) - frees, BLOCKS);
        }
        assert_eq!(input.releases.load(Relaxed), before);
        assert_eq!(input.drops.load(Relaxed), 1);
    }
    drop(b);
    assert_eq!(out.drops.load(Relaxed), 1);
}
