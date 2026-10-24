use super::{Options, emit_file};

fn with_source(source: &str, action: impl FnOnce(&std::path::Path)) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.plenty");
    std::fs::write(&path, source).unwrap();
    action(&path);
}

fn options() -> Options {
    Options {
        exports: [("entry".into(), "entry".into())].into(),
        ..Options::default()
    }
}

#[test]
fn checks_unexported_bodies_before_selecting_roots() {
    with_source(
        "def unused() -> i64:\n    True\ndef entry() -> i64:\n    2\n",
        |path| {
            let error = emit_file(path, &options()).err().unwrap().to_string();
            assert!(error.contains("expected i64"), "{error}");
        },
    );
}

#[test]
fn rejects_native_reference_signature_at_source() {
    with_source("def entry(x: &i64) -> &i64:\n    x\n", |path| {
        let error = emit_file(path, &options()).err().unwrap().to_string();
        assert!(
            error.contains("input.plenty:1:") && error.contains("supports only"),
            "{error}"
        );
    });
}

#[test]
fn stable_source_and_locations_across_compilations() {
    with_source(
        "def entry(x: i64) -> i64:\n    if x == 0:\n        return 1\n    entry(x - 1)\n",
        |path| {
            let a = emit_file(path, &options()).unwrap();
            let b = emit_file(path, &options()).unwrap();
            assert_eq!(a.source, b.source);
            assert_eq!(a.locations, b.locations);
            assert!(a.source.contains("continue 'tail"));
            assert!(a.locations.values().any(|p| p.contains("input.plenty:4:")));
        },
    );
}

#[test]
fn rejects_mutual_recursion_instead_of_losing_tail_guarantee() {
    with_source(
        "def entry(x: i64) -> i64:\n    other(x)\ndef other(x: i64) -> i64:\n    entry(x)\n",
        |path| {
            assert!(
                emit_file(path, &options())
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("mutual recursion")
            );
        },
    );
}

#[test]
fn validates_export_and_companion_mappings() {
    with_source("def entry(x: i64) -> i64:\n    x\n", |path| {
        let mut opts = options();
        opts.exports.insert("entry".into(), "crate".into());
        assert!(emit_file(path, &opts).is_err());
        let mut opts = options();
        opts.companions
            .insert("missing".into(), "crate::host::run".into());
        assert!(emit_file(path, &opts).is_err());
    });
}
