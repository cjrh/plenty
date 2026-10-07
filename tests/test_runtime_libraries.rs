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
