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
