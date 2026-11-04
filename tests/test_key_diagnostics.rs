mod support;
use support::check_error;

const NESTED: &str = "class Measurement:\n    sample: (i64, f64)\nenum Entry:\n    Empty\n    Reading(bool, Measurement)\nclass Key:\n    entry: Entry\n";

#[test]
fn annotations_explain_nested_key_fields() {
    for annotation in ["set[Key]", "dict[Key, str]", "KeyView[Key]"] {
        let error = check_error(&format!(
            "{NESTED}def accept(values: {annotation}) -> ():\n    pass\n"
        ));
        assert!(error.contains("Key -> field entry -> variant Reading payload 2 -> field sample -> tuple element 2: f64 is not a supported key type"), "{error}");
    }
}

#[test]
fn inferred_collections_and_bounds_explain_nested_key_fields() {
    for expression in [
        "{key}.unwrap()",
        "{key: 1}.unwrap()",
        "set([key].unwrap()).unwrap()",
        "need(key)",
        "Holder(key)",
    ] {
        let error = check_error(&format!(
            "{NESTED}def need[K: KeyType](value: K) -> ():\n    pass\nclass Holder[K: KeyType]:\n    value: K\nkey = Key(Entry.Reading(True, Measurement((1, 1.0))))\nvalue = {expression}\n"
        ));
        assert!(error.contains("field entry -> variant Reading payload 2 -> field sample -> tuple element 2: f64 is not a supported key type"), "{expression}: {error}");
    }
}

#[test]
fn anonymous_sums_have_specific_reasons() {
    let error = check_error(
        "class Key:\n    value: i64 | f64\ndef accept(keys: set[Key]) -> ():\n    pass\n",
    );
    assert!(
        error.contains("Key -> field value -> sum member f64: f64 is not a supported key type"),
        "{error}"
    );
}

#[test]
fn anonymous_sum_keys_reject_unsupported_members_at_every_boundary() {
    for member in ["f64", "list[i64]", "Box[i64]"] {
        for annotation in [
            format!("set[i64 | {member}]"),
            format!("dict[i64 | {member}, str]"),
            format!("KeyView[i64 | {member}]"),
        ] {
            let error = check_error(&format!(
                "def accept(keys: {annotation}) -> ():\n    pass\n"
            ));
            assert!(error.contains(&format!("sum member {member}")), "{error}");
        }
        let error = check_error(&format!(
            "def need[K: KeyType]() -> ():\n    pass\nneed[i64 | {member}]()\n"
        ));
        assert!(error.contains("does not satisfy KeyType"), "{error}");
    }
}
