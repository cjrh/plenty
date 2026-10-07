use std::path::{Path, PathBuf};
use std::process::Command;

fn library(root: &Path, body: &str) -> PathBuf {
    let source = root.join("fixture.c");
    std::fs::write(&source, body).unwrap();
    let library = root.join("libfixture.so");
    let output = Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror"])
        .arg(source)
        .arg("-o")
        .arg(&library)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    library
}

fn run(root: &Path, source: &str) -> std::process::Output {
    let entry = root.join("main.plenty");
    std::fs::write(&entry, source).unwrap();
    let executable = root.join("app");
    plenty::compile_file_to_executable_with_options(&entry, &executable, None, &Default::default())
        .unwrap();
    Command::new(executable).output().unwrap()
}

const RAW: &str = r#"
opaque Lease
opaque Address
extern def open_raw(path: &str as utf8, output: &mut Lease) -> u32 = "plenty_library_open_v1"
extern def symbol_raw(lease: Lease, name: &str as utf8, output: &mut Address) -> u32 = "plenty_library_symbol_v1"
extern def close_raw(lease: Lease) -> () = "plenty_library_close_v1"
extern def invoke(address: Address, a: i32, b: i32) -> i32 = address
pub def calculate(path: &str) -> i32:
    mut lease = Lease.null()
    status = open_raw(path, &mut lease)
    if status != 0:
        return -1
    name = "fixture_add"
    mut address = Address.null()
    lookup = symbol_raw(lease, &name, &mut address)
    close_raw(lease)
    if lookup != 0:
        return -2
    invoke(address, 20, 22)
"#;

#[test]
fn discovery_compares_exact_contract_bytes_and_rejects_bad_descriptors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let lib = library(
        root,
        r#"
#include <stddef.h>
const unsigned char *fixture_good(size_t *len) { *len = 8; return (const unsigned char *)"contract"; }
const unsigned char *fixture_stale(size_t *len) { *len = 8; return (const unsigned char *)"contracX"; }
const unsigned char *fixture_null(size_t *len) { *len = 8; return NULL; }
const unsigned char *fixture_huge(size_t *len) { *len = (size_t)-1; return (const unsigned char *)"x"; }
const unsigned char *fixture_short(size_t *len) { *len = 1; return (const unsigned char *)"x"; }
"#,
    );
    std::fs::write(root.join("native.plentyi"), format!(r#"{RAW}
extern def check_raw(lease: Lease, discovery: &str as utf8, expected: &str as utf8) -> u32 = "plenty_library_contract_v1"
pub def check(path: &str, discovery: &str, expected: &str) -> u32:
    mut lease = Lease.null()
    status = open_raw(path, &mut lease)
    if status != 0:
        return status
    checked = check_raw(lease, discovery, expected)
    close_raw(lease)
    checked
"#)).unwrap();
    let output = run(
        root,
        &format!(
            r#"
import native
def main() -> Result[(), Failure]:
    path = "{}"
    expected = "contract"
    for name in ["fixture_good", "fixture_stale", "fixture_null", "fixture_huge", "fixture_short", "fixture_missing"]?:
        print(native.check(&path, &name, &expected))?
    Ok(())
"#,
            lib.display()
        ),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"0\n7\n7\n7\n7\n6\n");
}

#[test]
fn runtime_helpers_load_without_a_link_time_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let lib = library(root, "int fixture_add(int a, int b) { return a + b; }\n");
    std::fs::write(root.join("native.plentyi"), RAW).unwrap();
    let output = run(
        root,
        &format!(
            "import native\ndef main() -> i32:\n    path = \"{}\"\n    native.calculate(&path)\n",
            lib.display()
        ),
    );
    assert_eq!(output.status.code(), Some(42), "{output:?}");
    let output = run(root, "import native\ndef main() -> i32:\n    path = \"/no/plenty/library.so\"\n    native.calculate(&path)\n");
    assert_eq!(output.status.code(), Some(255), "{output:?}");
}

#[test]
fn runtime_loader_prototypes_cannot_be_redeclared_with_another_abi() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let entry = root.join("main.plenty");
    std::fs::write(&entry, "import native\ndef main() -> ():\n    pass\n").unwrap();
    for declaration in [
        "extern def bad(path: i64, output: &mut i64) -> u32 = \"plenty_library_open_v1\"",
        "opaque Lease\nextern def bad(lease: Lease) -> i32 = \"plenty_library_close_v1\"",
        "opaque Lease\nextern def bad(lease: Lease, name: &str as utf8, output: &mut Lease) -> i64 = \"plenty_library_symbol_v1\"",
    ] {
        std::fs::write(root.join("native.plentyi"), format!("{declaration}\n")).unwrap();
        let error = plenty::compile_file_to_executable_with_options(&entry, &root.join("app"), None, &Default::default()).unwrap_err().to_string();
        assert!(error.contains("fixed runtime loader signature"), "{error}");
    }
}

#[test]
fn generated_loader_checks_contract_and_calls_cached_scalar_addresses() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let producer = root.join("producer.plenty");
    std::fs::write(
        &producer,
        r#"
export def add(a: i32, b: i32) -> i32 = "calc_add":
    a + b
export def scale(value: f32) -> f32 = "calc_scale":
    value * 2.0
export def nothing() -> () = "calc_nothing":
    pass
"#,
    )
    .unwrap();
    let native = root.join("libcalc.so");
    let options = plenty::LibraryOptions::new("calc", plenty::LibraryKind::Shared);
    plenty::compile_file_to_library(&producer, &native, None, &options).unwrap();
    let source = plenty::runtime_interface_source(&producer, None, "calc").unwrap();
    std::fs::write(root.join("plugin.plentyi"), source).unwrap();
    let output = run(
        root,
        &format!(
            r#"
import plugin
def main() -> Result[(), Failure]:
    path = "{}"
    library = plugin.load(&path)?
    print(library.add(20, 22))?
    print(library.scale(1.25))?
    library.nothing()
    missing = "/no/plenty/library.so"
    match plugin.load(&missing):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error)?
    Ok(())
"#,
            native.display()
        ),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"42\n2.5\nLoadError.OpenFailed\n");
    // A changed return signature is refused before any exported function runs.
    std::fs::write(
        &producer,
        "export def add(a: i32, b: i32) -> i64 = \"calc_add\":\n    i64(a + b)\n",
    )
    .unwrap();
    plenty::compile_file_to_library(&producer, &native, None, &options).unwrap();
    let output = run(
        root,
        &format!(
            r#"
import plugin
def main() -> Result[(), Failure]:
    path = "{}"
    match plugin.load(&path):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error)?
    Ok(())
"#,
            native.display()
        ),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"LoadError.IncompatibleContract\n");
}

#[test]
fn loader_error_markers_preserve_all_tags_in_nested_values() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(
        dir.path(),
        r#"
def code(error: LoadError) -> i64:
    match error:
        case LoadError.OutOfMemory:
            1
        case LoadError.CapacityOverflow:
            2
        case LoadError.InvalidPath:
            3
        case LoadError.OpenFailed:
            4
        case LoadError.InvalidSymbol:
            5
        case LoadError.MissingSymbol:
            6
        case LoadError.IncompatibleContract:
            7
def main() -> Result[(), Failure]:
    errors = [LoadError.OutOfMemory, LoadError.CapacityOverflow, LoadError.InvalidPath, LoadError.OpenFailed, LoadError.InvalidSymbol, LoadError.MissingSymbol, LoadError.IncompatibleContract]?
    for error in &errors:
        print(code(error))?
    left: Result[Option[LoadError], LoadError] = Ok(Some(LoadError.InvalidPath))
    right: Result[Option[LoadError], LoadError] = Ok(Some(LoadError.IncompatibleContract))
    print(left == right)?
    print(right)?
    Ok(())
"#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n2\n3\n4\n5\n6\n7\nFalse\nResult[Option[LoadError], LoadError].Ok(Option[LoadError].Some(LoadError.IncompatibleContract))\n");
}
