//! Host entry certification reuses transitive worker effects without claiming
//! that a trusted foreign pointer is an ordinary exclusive Plenty reference.
use std::fs;

fn check(source: &str, native: &str) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("native.plentyi"), native).unwrap();
    let path = root.path().join("source.plenty");
    fs::write(&path, source).unwrap();
    plenty::check_module_file(&path, None).map(|_| ())
}

#[test]
fn host_exports_reject_uncertified_calls_through_helpers_and_destructors() {
    let native = "pub extern def touch() -> () = \"foreign_touch\"\n";
    for source in [
        "import native\ndef helper() -> ():\n    native.touch()\n@thread_safe\nexport def entry() -> () = \"host_entry\":\n    helper()\n",
        "import native\nclass Resource:\n    value: i64\n    def __del__(self) -> ():\n        native.touch()\n@thread_safe\nexport def entry() -> () = \"host_entry\":\n    value = Resource(1)\n    drop(value)\n",
    ] {
        let error = check(source, native).unwrap_err().to_string();
        assert!(error.contains("thread-safe export"), "{error}");
        assert!(error.contains("foreign_touch"), "{error}");
    }
}

#[test]
fn host_exports_keep_managed_transfer_and_pointer_thread_contracts_explicit() {
    let borrowed =
        "@thread_safe\nexport def entry(value: &mut i64) -> () = \"host_entry\":\n    *value = 1\n";
    assert!(
        check(borrowed, "")
            .unwrap_err()
            .to_string()
            .contains("separate host transfer contract")
    );
    let source = "from native import Pointer\n@thread_safe\nexport def entry(value: Pointer) -> Pointer = \"host_entry\":\n    value\n";
    assert!(
        check(source, "pub opaque Pointer\n")
            .unwrap_err()
            .to_string()
            .contains("no thread contract")
    );
    check(source, "@thread_movable\npub opaque Pointer\n").unwrap();
}

#[test]
fn host_exports_reject_process_global_effects() {
    let source = "import std.env\n@thread_safe\nexport def entry() -> () = \"host_entry\":\n    path = std.env.current_dir().unwrap()\n    std.env.set_current_dir(&path).unwrap()\n";
    let error = check(source, "").unwrap_err().to_string();
    assert!(error.contains("thread-safe export"), "{error}");
    assert!(error.contains("working directory"), "{error}");
}

#[test]
fn host_exports_allow_certified_foreign_calls_and_ordinary_serial_exports() {
    let source = "import native\n@thread_safe\nexport def entry() -> () = \"host_entry\":\n    native.touch()\n";
    check(
        source,
        "@thread_safe\npub extern def touch() -> () = \"foreign_touch\"\n",
    )
    .unwrap();
    check(
        &source.replace("@thread_safe\n", ""),
        "pub extern def touch() -> () = \"foreign_touch\"\n",
    )
    .unwrap();
}
