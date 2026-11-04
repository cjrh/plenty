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
fn custom_cleanup_and_anonymous_sums_have_specific_reasons() {
    let error = check_error(
        "class Guard:\n    n: i64\n    def __del__(self) -> ():\n        pass\nclass Key:\n    guard: Guard\ndef accept(keys: set[Key]) -> ():\n    pass\n",
    );
    assert!(
        error.contains("Key -> field guard: Guard declares custom cleanup (__del__)"),
        "{error}"
    );
    let error = check_error(
        "class Key:\n    value: i64 | str\ndef accept(keys: set[Key]) -> ():\n    pass\n",
    );
    assert!(
        error.contains("Key -> field value: anonymous sums are not supported as keys"),
        "{error}"
    );
}
