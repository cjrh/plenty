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
    destroy: None,
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

#[test]
fn class_keys_hash_fields_instead_of_record_addresses() {
    static CLASS: Type = Type {
        kind: b'C',
        affine: true,
        inline_bytes: 48,
        variants: &[
            Variant {
                name: "first",
                fields: &[&TEXT],
            },
            Variant {
                name: "number",
                fields: &[&INTEGER],
            },
            Variant {
                name: "last",
                fields: &[&TEXT],
            },
        ],
        ..INTEGER
    };
    // SAFETY: records have the descriptor's three slots and independently own
    // their text fields; they stay live through hashing and equality checks.
    unsafe {
        let mut a = [
            strings::new(b"hello") as u128,
            42,
            strings::new(b"world") as u128,
        ];
        let mut b = [
            strings::new(b"hello") as u128,
            42,
            strings::new(b"world") as u128,
        ];
        let first = a.as_mut_ptr() as u128;
        let second = b.as_mut_ptr() as u128;
        for seed in [[0; 16], [1; 16], [0xa5; 16]] {
            assert!(equal(first, second, &CLASS));
            assert_eq!(
                hashing::hash_with_key(first, &CLASS, &seed),
                hashing::hash_with_key(second, &CLASS, &seed)
            );
            b[1] = 43;
            assert!(!equal(first, second, &CLASS));
            assert_ne!(
                hashing::hash_with_key(first, &CLASS, &seed),
                hashing::hash_with_key(second, &CLASS, &seed)
            );
            b[1] = 42;
        }
        release(first, &CLASS);
        release(second, &CLASS);
    }
}

static BYTES: Type = Type {
    kind: b'y',
    ..INTEGER
};

/// The layout of a compiler-emitted immortal literal: a heap-format Text even
/// when its contents would fit inline.
#[repr(C, align(8))]
struct Literal<const N: usize> {
    refs: u64,
    destroy: usize,
    byte_len: u64,
    scalar_len: u64,
    data: [u8; N],
}

fn literal<const N: usize>(data: &[u8; N]) -> Literal<N> {
    Literal {
        refs: u64::MAX,
        destroy: 0,
        byte_len: N as u64,
        scalar_len: N as u64,
        data: *data,
    }
}

// The direct str/bytes path hashes the byte view, so inline, literal and heap
// owners of equal contents must agree with each other and with `equal`, both
// at top level and inside a tuple, which uses the framed encoding.
fn assert_text_storage_agrees<const N: usize>(contents: &[u8; N]) {
    let literal = literal(contents);
    let literal = &literal as *const Literal<N> as u128;
    let owned = strings::new(contents);
    assert_eq!(strings::is_inline(owned), N <= strings::INLINE_MAX);
    let owned_key = owned as u128;
    // SAFETY: both texts stay live through every hash and comparison; packed
    // tuples refer to live local field arrays.
    unsafe {
        let mut literal_fields = [literal, owned_key];
        let mut owned_fields = [owned_key, literal];
        let literal_tuple = PAIR.pack(literal_fields.as_mut_ptr() as u128, 0);
        let owned_tuple = PAIR.pack(owned_fields.as_mut_ptr() as u128, 0);
        for key in [[0; 16], [1; 16], [0xa5; 16]] {
            for ty in [&TEXT, &BYTES] {
                assert!(equal(literal, owned_key, ty));
                assert_eq!(
                    hashing::hash_with_key(literal, ty, &key),
                    hashing::hash_with_key(owned_key, ty, &key),
                    "{contents:?}"
                );
            }
            assert!(equal(literal_tuple, owned_tuple, &PAIR));
            assert_eq!(
                hashing::hash_with_key(literal_tuple, &PAIR, &key),
                hashing::hash_with_key(owned_tuple, &PAIR, &key)
            );
        }
        plenty_release(owned.cast());
    }
}

#[test]
fn text_keys_hash_by_contents_across_storage() {
    assert_text_storage_agrees(b"");
    assert_text_storage_agrees(b"A");
    assert_text_storage_agrees(b"GGTATT");
    assert_text_storage_agrees(b"abcdefg");
    assert_text_storage_agrees(b"abcdefgh");
    assert_text_storage_agrees(b"GGTATTTTAATTTATAGT");
}

#[test]
fn distinct_text_keys_hash_apart() {
    let contents: [&[u8]; 6] = [b"", b"\0", b"\0\0", b"A", b"AC", b"abcdefgh\0"];
    let texts = contents.map(strings::new);
    let mut hashes = std::collections::BTreeSet::new();
    // SAFETY: each text stays live until its release below.
    unsafe {
        for &text in &texts {
            hashes.insert(hashing::hash_with_key(text as u128, &TEXT, &[7; 16]));
        }
        for text in texts {
            plenty_release(text.cast());
        }
    }
    assert_eq!(hashes.len(), contents.len());
}

#[test]
fn scalar_keys_ignore_bits_above_their_width() {
    static I8: Type = Type {
        kind: b'1',
        ..INTEGER
    };
    static U32: Type = Type {
        kind: b'7',
        ..INTEGER
    };
    static BOOL: Type = Type {
        kind: b'b',
        ..INTEGER
    };
    let cases: [(&Type, u128, u128); 4] = [
        (&I8, 0xff, 0xdead_beef_ffff_ff00 | 0xff),
        (&U32, 7, 0x1234_0000_0007),
        (&BOOL, 1, 0x101),
        (&INTEGER, u64::MAX as u128, u128::MAX),
    ];
    for (ty, value, padded) in cases {
        // SAFETY: scalars own nothing; both operands carry the same value.
        unsafe {
            assert!(equal(value, padded, ty));
            for key in [[0; 16], [0xa5; 16]] {
                assert_eq!(
                    hashing::hash_with_key(value, ty, &key),
                    hashing::hash_with_key(padded, ty, &key)
                );
            }
        }
    }
}
