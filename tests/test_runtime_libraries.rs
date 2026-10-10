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

#[test]
fn generated_methods_preserve_result_payloads_and_scalar_borrow_updates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let producer = root.join("producer.plenty");
    std::fs::write(
        &producer,
        r#"
export def change(value: &mut i16, fail: i32) -> Result[f32, ParseError] = "calc_change":
    *value = *value + 1
    if fail != 0:
        Err(ParseError.OutOfRange)
    else:
        Ok(1.25)
export def numeric(fail: i32) -> Result[(), u64] = "calc_numeric":
    if fail != 0:
        Err(18446744073709551615u64)
    else:
        Ok(())
export def allocation() -> Result[i32, AllocError] = "calc_allocation":
    Err(AllocError.CapacityOverflow)
export def failure() -> Result[(), Failure] = "calc_failure":
    Err(Failure.Unspecified)
export def read(a: &f64, b: &f64) -> f64 = "calc_read":
    *a + *b
"#,
    )
    .unwrap();
    let native = root.join("libcalc.so");
    plenty::compile_file_to_library(
        &producer,
        &native,
        None,
        &plenty::LibraryOptions::new("calc", plenty::LibraryKind::Shared),
    )
    .unwrap();
    std::fs::write(
        root.join("plugin.plentyi"),
        plenty::runtime_interface_source(&producer, None, "calc").unwrap(),
    )
    .unwrap();
    let output = run(
        root,
        &format!(
            r#"
import plugin
def main() -> Result[(), Failure]:
    path = "{}"
    library = plugin.load(&path)?
    mut value: i16 = 10
    print(library.change(&mut value, 0))?
    print(library.change(&mut value, 1))?
    print(value)?
    print(library.numeric(0))?
    print(library.numeric(1))?
    print(library.allocation())?
    print(library.failure())?
    number: f64 = 2.5
    print(library.read(&number, &number))?
    Ok(())
"#,
            native.display()
        ),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "Result[f32, ParseError].Ok(1.25)\nResult[f32, ParseError].Err(ParseError.OutOfRange)\n12\nResult[(), u64].Ok(())\nResult[(), u64].Err(18446744073709551615)\nResult[i32, AllocError].Err(AllocError.CapacityOverflow)\nResult[(), Failure].Err(Failure.Unspecified)\n5.0\n");
}

const OWNERS: &str = r#"
class Counter:
    value: i64
    def __del__(self) -> ():
        print("released").unwrap()
export def create(value: i64) -> Result[Counter, AllocError] = "calc_create":
    Ok(Counter(value))
export def read(counter: &Counter) -> i64 = "calc_read":
    counter.value
export def bump(counter: &mut Counter) -> Result[(), i32] = "calc_bump":
    counter.value = counter.value + 1
    Err(-1)
export def transfer(counter: Counter) -> Result[Counter, AllocError] = "calc_transfer":
    Ok(counter)
export def finish(counter: Counter) -> Result[(), i32] = "calc_finish":
    Err(-2)
"#;

#[test]
fn dynamic_owners_keep_destruction_and_instance_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let producer = root.join("producer.plenty");
    std::fs::write(&producer, OWNERS).unwrap();
    let options = plenty::LibraryOptions::new("calc", plenty::LibraryKind::Shared);
    let first = root.join("first.so");
    let second = root.join("second.so");
    plenty::compile_file_to_library(&producer, &first, None, &options).unwrap();
    plenty::compile_file_to_library(&producer, &second, None, &options).unwrap();
    std::fs::write(
        root.join("plugin.plentyi"),
        plenty::runtime_interface_source(&producer, None, "calc").unwrap(),
    )
    .unwrap();
    let output = run(
        root,
        &format!(
            r#"
import plugin
def acquire(path: &str) -> Result[plugin.Counter, Failure]:
    library = plugin.load(path)?
    Ok(library.create(20)?)
def main() -> Result[(), Failure]:
    path = "{}"
    library = plugin.load(&path)?
    mut counter = acquire(&path)?
    print(library.bump(&mut counter))?
    print(library.read(&counter))?
    counter = library.transfer(counter)?
    print(library.finish(counter))?
    last = library.create(42)?
    drop(library)
    drop(last)
    Ok(())
"#,
            first.display()
        ),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"Result[(), i32].Err(-1)\n21\nreleased\nResult[(), i32].Err(-2)\nreleased\n"
    );
    let output = run(
        root,
        &format!(
            r#"
import plugin
def main() -> Result[(), Failure]:
    a = "{}"
    b = "{}"
    first = plugin.load(&a)?
    second = plugin.load(&b)?
    owner = first.create(42)?
    print(second.read(&owner))?
    Ok(())
"#,
            first.display(),
            second.display()
        ),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("different loaded instance"),
        "{output:?}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn injected_failures_preserve_loading_and_owner_cleanup_without_allocating_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let producer = root.join("producer.plenty");
    std::fs::write(
        &producer,
        format!(
            r#"{OWNERS}
export def warm() -> () = "calc_warm":
    print("warm").unwrap()
export def fail_allocations() -> () = "calc_fail_allocations":
    print("__test_fail_allocations_after_0__").unwrap()
export def restore_allocations() -> () = "calc_restore_allocations":
    print("__test_restore_allocations__").unwrap()
"#
        ),
    )
    .unwrap();
    let native = root.join("libcalc.so");
    plenty::compile_file_to_library(
        &producer,
        &native,
        None,
        &plenty::LibraryOptions::new("calc", plenty::LibraryKind::Shared),
    )
    .unwrap();
    std::fs::write(
        root.join("plugin.plentyi"),
        plenty::runtime_interface_source(&producer, None, "calc").unwrap(),
    )
    .unwrap();
    let mut source = format!(
        "import plugin\ndef main() -> Result[(), Failure]:\n    path = \"{}\"\n",
        native.display()
    );
    for budget in 0..5 {
        source.push_str(&format!("    print(\"__test_fail_allocations_after_{budget}__\").unwrap()\n    attempt{budget} = plugin.load(&path)\n    print(\"__test_restore_allocations__\").unwrap()\n    match attempt{budget}:\n        case Ok(value):\n            drop(value)\n            print(\"loaded\")?\n        case Err(error):\n            print(error)?\n"));
    }
    source.push_str(
        r#"
    library = plugin.load(&path)?
    library.warm()
    mut owner = library.create(10)?
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    value = library.read(&owner)
    changed = library.bump(&mut owner)
    error: Result[i64, LoadError] = Err(LoadError.IncompatibleContract)
    same = error == Result[i64, LoadError].Err(LoadError.IncompatibleContract)
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(value)?
    print(changed)?
    print(same)?
    print("__test_fail_allocations_after_0__").unwrap()
    # The wrapper needs no allocation, and this library has its own runtime.
    transferred = library.transfer(owner)
    print("__test_restore_allocations__").unwrap()
    match transferred:
        case Ok(_):
            print("transferred")?
        case Err(error):
            print(error)?
    library.fail_allocations()
    failed_native = library.create(42)
    library.restore_allocations()
    match failed_native:
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error)?
    Ok(())
"#,
    );
    let output = run(root, &source);
    assert!(output.status.success(), "{output:?}");
    let output = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = output
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect();
    assert_eq!(
        lines,
        [
            // Loading allocates once in this process: the library's records
            // are inline.
            "LoadError.OutOfMemory",
            "loaded",
            "loaded",
            "loaded",
            "loaded",
            "warm",
            "10",
            "Result[(), i32].Err(-1)",
            "True",
            "transferred",
            "released",
            "AllocError.OutOfMemory"
        ]
    );
}

#[test]
fn partial_resolution_never_publishes_a_callable_table() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let producer = root.join("producer.plenty");
    std::fs::write(&producer, "export def first() -> i32 = \"calc_first\":\n    1\nexport def second() -> i32 = \"calc_second\":\n    2\n").unwrap();
    let native = root.join("libcalc.so");
    let artifacts = plenty::compile_file_to_library(
        &producer,
        &native,
        None,
        &plenty::LibraryOptions::new("calc", plenty::LibraryKind::Shared),
    )
    .unwrap();
    std::fs::write(
        root.join("plugin.plentyi"),
        plenty::runtime_interface_source(&producer, None, "calc").unwrap(),
    )
    .unwrap();
    let contract = std::fs::read_to_string(artifacts.interface).unwrap();
    let hash = plenty::read_library_interfaces(&native)
        .unwrap()
        .remove(0)
        .fingerprint
        .unwrap();
    let bytes = contract
        .bytes()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(",");
    for guard in [false, true] {
        let body = format!("#include <stddef.h>\n#include <stdio.h>\nstatic const unsigned char contract[] = {{{bytes}}};\nconst unsigned char *calc_plenty_interface_v1(size_t *len) {{ *len = sizeof(contract); return contract; }}\nint calc_first(void) {{ puts(\"must not run\"); return 1; }}\n{}", if guard { format!("void calc_plenty_contract_v1_{hash}(void) {{}}\n") } else { String::new() });
        let fake = library(root, &body);
        let output = run(
            root,
            &format!(
                r#"
import plugin
def main() -> Result[(), Failure]:
    path = "{}"
    for n in range(20):
        match plugin.load(&path):
            case Ok(library):
                print(library.first())?
            case Err(error):
                if n == 19:
                    print(error)?
    Ok(())
"#,
                fake.display()
            ),
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"LoadError.MissingSymbol\n");
    }
}
