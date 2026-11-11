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

static TEXTS: Type = Type {
    kind: b'S',
    affine: true,
    key: Some(&TEXT),
    ..INTEGER
};

/// `contents` is found at `row` through its stored word and through a
/// literal-format text, another storage of the same bytes.
fn assert_text_found<const N: usize>(c: &Collection, row: usize, contents: &[u8; N]) {
    let other = literal(contents);
    let other = &other as *const Literal<N> as u128;
    // SAFETY: the set and the literal are live.
    unsafe {
        let stored = c.entries.key(row);
        assert_ne!(stored, other);
        assert_eq!(c.find(stored), Some(row), "{contents:?} by word");
        assert_eq!(c.find(other), Some(row), "{contents:?} by bytes");
    }
}

// Issue #931: a text probe answers an equal word without reading the text,
// compares bytes for another storage of the same contents, and passes a
// candidate that shares only the key's hash bits.
#[test]
fn text_probes_find_every_storage_and_pass_hash_collisions() {
    let owner = OwnedValue {
        value: try_collection_new(&TEXTS, 0).unwrap() as u128,
        ty: &TEXTS,
    };
    let contents: [&[u8]; 6] = [
        b"",
        b"A",
        b"GGT",
        b"abcdefg",
        b"abcdefgh",
        b"GGTATTTTAATTTATAGT",
    ];
    // SAFETY: the guard owns the set, which owns its keys.
    unsafe {
        let c = &mut *(owner.value as *mut Collection);
        for bytes in contents {
            c.try_adopt(strings::new(bytes) as u128, 0).unwrap();
        }
        assert_text_found(c, 0, b"");
        assert_text_found(c, 1, b"A");
        assert_text_found(c, 2, b"GGT");
        assert_text_found(c, 3, b"abcdefg");
        assert_text_found(c, 4, b"abcdefgh");
        assert_text_found(c, 5, b"GGTATTTTAATTTATAGT");
        let absent = literal(b"GGA");
        let absent = &absent as *const Literal<3> as u128;
        assert_eq!(c.find(absent), None);

        // Probing with another key's hash meets that key first, as a full
        // hash collision would.
        for (first, later) in [(2, 5), (5, 2), (4, 5)] {
            let hash = hash(c.entries.key(first), &TEXT);
            let later_key = c.entries.key(later);
            assert_eq!(c.table.entry(c.probe(absent, hash)), None);
            // A second bucket with the same hash bits, as a colliding key's
            // insertion leaves it, is found past the first candidate.
            c.table.insert_unique(hash, later);
            assert_eq!(c.table.entry(c.probe(later_key, hash)), Some(later));
            let copy = strings::new(strings::bytes(&(later_key as usize as *const Text)));
            assert_eq!(c.table.entry(c.probe(copy as u128, hash)), Some(later));
            plenty_release(copy.cast());
        }
    }
}

static COUNTS: Type = Type {
    kind: b'D',
    affine: true,
    key: Some(&TEXT),
    value: Some(&INTEGER),
    ..INTEGER
};

// Issue #931: the narrow helper answers an inline key held as the same word
// without a call, and hands every other outcome on with its hash and probed
// bucket. Each outcome must match the general operation and hash once: a
// first insertion, hits by word and by bytes, a candidate sharing only the
// hash bits, and a heap key.
#[test]
fn the_narrow_get_or_insert_agrees_with_the_general_one() {
    let owner = OwnedValue {
        value: try_collection_new(&COUNTS, 0).unwrap() as u128,
        ty: &COUNTS,
    };
    let reference = &owner.value as *const u128;
    let hit = GET_OR_INSERT_HIT as u64;
    // SAFETY: the guard owns the dictionary; keys are inline, immortal
    // literals, or one owned heap text that the dictionary adopts.
    unsafe {
        let narrow = |key: u128, value: u64| {
            let before = HASHES.with(std::cell::Cell::get);
            let words = plenty_dict_get_or_insert_text(reference, key as u64, value, 0);
            assert_eq!(HASHES.with(std::cell::Cell::get), before + 1);
            (words.0 as *mut u128, words.1)
        };
        let inline = |bytes: &[u8]| strings::new(bytes) as u128;
        // The empty dictionary has no table: the general path inserts.
        let (ggt, flag) = narrow(inline(b"GGT"), 1);
        assert_eq!((*ggt as u64, flag), (1, 0));
        // A second bucket for "GGT" under "A"'s hash, as a full collision
        // would leave: "A" passes that candidate, here and on later hits.
        let c = &mut *(owner.value as *mut Collection);
        c.table.insert_unique(hash(inline(b"A"), &TEXT), 0);
        // An inline key that misses is inserted at the probed bucket.
        let (a, flag) = narrow(inline(b"A"), 2);
        assert_eq!((*a as u64, flag), (2, 0));
        // A literal is stored in heap format; an inline key with its bytes
        // hits it by comparison.
        let stored = literal(b"ACG");
        let (acg, flag) = narrow(&stored as *const Literal<3> as u128, 3);
        assert_eq!((*acg as u64, flag), (3, 0));
        assert_eq!(narrow(inline(b"ACG"), 9), (acg, hit));
        // Hits by word leave the stored value and report the same slot.
        assert_eq!(narrow(inline(b"GGT"), 9), (ggt, hit));
        assert_eq!(narrow(inline(b"A"), 9), (a, hit));
        let other = literal(b"GGT");
        assert_eq!(narrow(&other as *const Literal<3> as u128, 9), (ggt, hit));
        assert_eq!((*ggt as u64, *a as u64, *acg as u64), (1, 2, 3));
        let long = strings::new(b"GGTATTTTAATTTATAGT") as u128;
        let (slot, flag) = narrow(long, 4);
        assert_eq!((*slot as u64, flag), (4, 0));
        assert_eq!(c.len(), 4);
        for (row, contents) in [&b"GGT"[..], b"A", b"ACG", b"GGTATTTTAATTTATAGT"]
            .into_iter()
            .enumerate()
        {
            let key = c.entries.key(row) as usize as *const Text;
            assert_eq!(strings::bytes(&key), contents);
        }
    }
}

// Issue #931: a heap or literal key's hit is answered in one function, which
// compares another heap candidate's bytes itself. Each length class of that
// comparison (under eight bytes, whole words, an overlapping last word) must
// hit an equal key held by another object, pass a candidate of the same
// length that differs in its first or last byte, and hash once.
fn assert_heap_key_hits<const N: usize>() {
    let owner = OwnedValue {
        value: try_collection_new(&COUNTS, 0).unwrap() as u128,
        ty: &COUNTS,
    };
    let reference = &owner.value as *const u128;
    let hit = GET_OR_INSERT_HIT as u64;
    let contents: [u8; N] = std::array::from_fn(|i| b'a' + (i % 26) as u8);
    let changed = |index: usize| {
        let mut bytes = contents;
        bytes[index] ^= 1;
        bytes
    };
    // Immortal literals outlive the dictionary that stores them.
    let leak = |bytes: &[u8; N]| Box::leak(Box::new(literal(bytes))) as *const Literal<N> as u128;
    // SAFETY: the guard owns the dictionary; every key is an immortal literal.
    unsafe {
        let narrow = |key: u128, value: u64| {
            let before = HASHES.with(std::cell::Cell::get);
            let words = plenty_dict_get_or_insert_text(reference, key as u64, value, 0);
            assert_eq!(HASHES.with(std::cell::Cell::get), before + 1);
            (words.0 as *mut u128, words.1)
        };
        let stored = leak(&contents);
        let (slot, flag) = narrow(stored, 1);
        assert_eq!((*slot as u64, flag), (1, 0));
        assert_eq!(narrow(stored, 9), (slot, hit), "the same object");
        assert_eq!(narrow(leak(&contents), 9), (slot, hit), "equal bytes");
        // A bucket for the stored row under each differing key's hash, as a
        // full collision would leave: the comparison must reject it.
        let c = &mut *(owner.value as *mut Collection);
        for (value, index) in [(2, 0), (3, N - 1)] {
            let other = leak(&changed(index));
            c.table.insert_unique(hash(other, &TEXT), 0);
            let (inserted, flag) = narrow(other, value);
            assert_eq!((*inserted as u64, flag), (value, 0), "byte {index}");
            assert_ne!(inserted, slot);
            assert_eq!(narrow(leak(&changed(index)), 9), (inserted, hit));
            if N == 1 {
                break;
            }
        }
        assert_eq!(*slot as u64, 1);
        assert_eq!(narrow(leak(&contents), 9), (slot, hit));
    }
}

#[test]
fn a_heap_key_hit_compares_every_length_class() {
    assert_heap_key_hits::<1>();
    assert_heap_key_hits::<3>();
    assert_heap_key_hits::<7>();
    assert_heap_key_hits::<8>();
    assert_heap_key_hits::<9>();
    assert_heap_key_hits::<15>();
    assert_heap_key_hits::<16>();
    assert_heap_key_hits::<18>();
    assert_heap_key_hits::<40>();
}

// Issue #949: `get_or_insert_bytes` hashes a key's bytes where they lie. The
// hash must be the one the owned `str` gets, inline or heap, at every length
// class of the block loop and the final block.
#[test]
fn key_bytes_hash_as_the_owned_text_does() {
    for length in 0..=40usize {
        let bytes: Vec<u8> = (0..length).map(|i| b'a' + (i % 26) as u8).collect();
        let owned = OwnedValue {
            value: strings::new(&bytes) as u128,
            ty: &TEXT,
        };
        // SAFETY: the guard owns a text of these bytes.
        let expected = unsafe { hash(owned.value, &TEXT) };
        assert_eq!(hashing::hash_text_bytes(&bytes), expected, "{length}");
        assert_eq!(hashing::text_bytes(&bytes), Some(expected), "{length}");
    }
}

// Issue #949: for every key length the borrowed form inserts on a miss,
// hits its own entry and the owned form's, and is hit by the owned form;
// each call hashes once. A miss that is not UTF-8 stores nothing, also when
// a bucket with its hash bits already exists.
#[test]
fn get_or_insert_bytes_agrees_with_the_owned_key_at_every_length() {
    let owner = OwnedValue {
        value: try_collection_new(&COUNTS, 0).unwrap() as u128,
        ty: &COUNTS,
    };
    let reference = &owner.value as *const u128;
    let hit = GET_OR_INSERT_HIT as u64;
    // SAFETY: the guard owns the dictionary; each owned key is adopted by a
    // miss or released here after a hit, as generated code does.
    unsafe {
        let borrowed = |bytes: &[u8], value: u64| {
            let before = HASHES.with(std::cell::Cell::get);
            let words = plenty_dict_get_or_insert_bytes(
                reference,
                bytes.as_ptr(),
                bytes.len() as u64,
                value,
                0,
            );
            assert_eq!(HASHES.with(std::cell::Cell::get), before + 1);
            (words.0 as *mut u128, words.1)
        };
        let owned = |bytes: &[u8], value: u64| {
            let key = strings::new(bytes) as u128;
            let words = plenty_dict_get_or_insert_text(reference, key as u64, value, 0);
            if words.1 == hit {
                release(key, &TEXT);
            }
            (words.0 as *mut u128, words.1)
        };
        let contents = |length: usize, first: u8| -> Vec<u8> {
            (0..length).map(|i| first + (i % 26) as u8).collect()
        };
        let c = &mut *(owner.value as *mut Collection);
        for length in 0..=40usize {
            // Inserted by bytes, found by bytes and by an owned key.
            let key = contents(length, b'a');
            let (slot, flag) = borrowed(&key, length as u64);
            assert_eq!((*slot as u64, flag), (length as u64, 0), "{length}");
            assert_eq!(borrowed(&key, 99), (slot, hit), "{length}");
            assert_eq!(owned(&key, 99), (slot, hit), "{length}");
            assert_eq!(*slot as u64, length as u64);
            if length == 0 {
                continue;
            }
            // Inserted by an owned key, found by bytes.
            let other = contents(length, b'A');
            let (slot, flag) = owned(&other, 1000 + length as u64);
            assert_eq!(flag, 0);
            assert_eq!(borrowed(&other, 99), (slot, hit), "{length}");
            // The same length differing in the last byte is another key.
            let mut changed = key.clone();
            changed[length - 1] ^= 1;
            let (inserted, flag) = borrowed(&changed, 7);
            assert_eq!(flag, 0, "{length}");
            assert_ne!(inserted, slot);
        }
        assert_eq!(c.len(), 1 + 40 * 3);
        // A stored literal is heap text at any length; short bytes hit it.
        let stored = literal(b"ACG");
        let words =
            plenty_dict_get_or_insert_text(reference, &stored as *const Literal<3> as u64, 5, 0);
        assert_eq!(words.1, 0);
        assert_eq!(borrowed(b"ACG", 99), (words.0 as *mut u128, hit));

        // Invalid UTF-8 matches no key: an error, nothing stored. The second
        // probe meets a bucket carrying its own hash bits, as a full
        // collision would leave, and still compares bytes.
        let length = c.len();
        for invalid in [&b"\xff"[..], b"ab\xc3", b"0123456789abcdef\xe2\x82"] {
            assert_eq!(borrowed(invalid, 9), (std::ptr::null_mut(), 1));
            c.table.insert_unique(hashing::hash_text_bytes(invalid), 0);
            assert_eq!(borrowed(invalid, 9), (std::ptr::null_mut(), 1));
            assert_eq!(c.len(), length);
        }
        for (row, _) in (0..c.len()).enumerate() {
            let key = c.entries.key(row) as usize as *const Text;
            assert!(std::str::from_utf8(strings::bytes(&key)).is_ok());
        }
    }
}
