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
};
static ID: Type = Type {
    kind: b'B',
    name: "Id",
    variants: &[
        Variant {
            name: "User",
            fields: &[&INTEGER],
        },
        Variant {
            name: "Group",
            fields: &[&INTEGER],
        },
    ],
    ..INTEGER
};
static UNIT: Type = Type {
    kind: b'B',
    name: "Unit",
    variants: &[
        Variant {
            name: "A",
            fields: &[],
        },
        Variant {
            name: "B",
            fields: &[],
        },
    ],
    ..INTEGER
};

#[test]
fn variants_with_equal_payloads_hash_apart() {
    let user = ID.pack(1, 0);
    let group = ID.pack(1, 1);
    // SAFETY: each operand is a packed value of its descriptor.
    unsafe {
        assert!(!equal(user, group, &ID));
        assert_ne!(hash(user, &ID), hash(group, &ID));
        assert_ne!(hash(UNIT.pack(0, 0), &UNIT), hash(UNIT.pack(0, 1), &UNIT));
    }
}

static TEXT: Type = Type {
    kind: b's',
    ..INTEGER
};
static PAIR: Type = Type {
    kind: b'B',
    variants: &[Variant {
        name: "",
        fields: &[&TEXT, &TEXT],
    }],
    ..INTEGER
};

#[test]
fn keyed_hashes_mix_old_low_bit_collisions() {
    let mut buckets = std::collections::BTreeSet::new();
    for bytes in [b"a", b"q", b"A", b"Q", b"1", b"!"] {
        // SAFETY: each temporary owns one live string for all hash calls.
        unsafe {
            let text = strings::new(bytes);
            let first = hashing::hash_with_key(text as u128, &TEXT, &[0; 16]);
            let second = hashing::hash_with_key(text as u128, &TEXT, &[1; 16]);
            assert_ne!(first, second);
            buckets.insert(first & 15);
            plenty_release(text.cast());
        }
    }
    assert!(
        buckets.len() >= 4,
        "fixed regression keys must spread: {buckets:?}"
    );
}

#[test]
fn structural_hashes_frame_fields_and_ignore_storage_addresses() {
    // SAFETY: all packed records refer to live local field arrays; their text
    // owners stay live until every hash/comparison has completed.
    unsafe {
        let text: Vec<_> = ["ab", "c", "a", "bc", "ab", "c"]
            .map(|s| strings::new(s.as_bytes()))
            .into();
        let mut first = [text[0] as u128, text[1] as u128];
        let mut split = [text[2] as u128, text[3] as u128];
        let mut equal_copy = [text[4] as u128, text[5] as u128];
        let first = PAIR.pack(first.as_mut_ptr() as u128, 0);
        let split = PAIR.pack(split.as_mut_ptr() as u128, 0);
        let equal_copy = PAIR.pack(equal_copy.as_mut_ptr() as u128, 0);
        for key in [[0; 16], [1; 16], [0xa5; 16]] {
            assert!(equal(first, equal_copy, &PAIR));
            assert_eq!(
                hashing::hash_with_key(first, &PAIR, &key),
                hashing::hash_with_key(equal_copy, &PAIR, &key)
            );
            assert_ne!(
                hashing::hash_with_key(first, &PAIR, &key),
                hashing::hash_with_key(split, &PAIR, &key)
            );
        }
        for text in text {
            plenty_release(text.cast());
        }
    }
}
