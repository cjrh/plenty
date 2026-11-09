//! Trusted raw access keeps host aliases outside ordinary Plenty references.
use std::process::Command;

#[test]
fn native_callbacks_static_storage_and_aliasing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(
        root.join("raw.plentyi"),
        r#"
pub opaque Pointer
pub extern def read(p: Pointer, offset: u64) -> f32 = intrinsic "load"
pub extern def write(p: Pointer, offset: u64, value: f32) -> () = intrinsic "store"
pub extern def write_pointer(p: Pointer, offset: u64, value: Pointer) -> () = intrinsic "store"
pub extern def callback() -> Pointer = address "process"
pub extern def text() -> Pointer = static "native callback"
pub extern def table() -> Pointer = storage 16
"#,
    )
    .unwrap();
    let source = root.join("source.plenty");
    std::fs::write(
        &source,
        r#"
import raw
export def process(input: raw.Pointer, output: raw.Pointer) -> () = exact "process":
    first = raw.read(input, 0u64)
    second = raw.read(input, 4u64)
    raw.write(output, 0u64, second)
    raw.write(output, 4u64, first)
export def descriptor() -> raw.Pointer = exact "descriptor":
    p = raw.table()
    raw.write_pointer(p, 0u64, raw.text())
    raw.write_pointer(p, 8u64, raw.callback())
    p
"#,
    )
    .unwrap();
    let library = root.join("libnative.a");
    let options = plenty::LibraryOptions::new("native", plenty::LibraryKind::Static);
    let artifacts = plenty::compile_file_to_library(&source, &library, None, &options).unwrap();
    plenty::check_module_file(&artifacts.interface, None).unwrap();
    plenty::runtime_interface_source(&source, None, "native").unwrap();
    let c = root.join("host.c");
    std::fs::write(
        &c,
        r#"
#include <assert.h>
#include <string.h>
#include "native.h"
struct descriptor { const char *text; void (*process)(void *, void *); };
int main(void) {
    const struct descriptor *d = descriptor();
    assert(d == descriptor());
    assert(!strcmp(d->text, "native callback"));
    float values[] = { 1, 2, 3 };
    d->process(values, values);
    assert(values[0] == 2 && values[1] == 1);
    d->process(values, values + 1);
    assert(values[0] == 2 && values[1] == 1 && values[2] == 2);
    return 0;
}
"#,
    )
    .unwrap();
    let executable = root.join("host");
    let args = std::fs::read_to_string(artifacts.link_args).unwrap();
    let output = Command::new("cc")
        .args(["-Wall", "-Wextra", "-Werror"])
        .arg(c)
        .arg(library)
        .args(args.lines())
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(Command::new(executable).status().unwrap().success());
}

#[test]
fn raw_operations_reject_invalid_signatures_and_unknown_callback_targets() {
    for (declaration, expected) in [
        (
            "pub extern def bad(p: Pointer) -> f32 = intrinsic \"load\"",
            "invalid trusted C intrinsic signature",
        ),
        (
            "pub extern def bad() -> Pointer = address \"not_an_export\"",
            "must name an export",
        ),
        (
            "pub extern def bad() -> Pointer = storage 0",
            "storage size",
        ),
        (
            "pub extern def bad(p: Pointer, offset: u64, value: str) -> () = intrinsic \"store\"",
            "invalid trusted C intrinsic signature",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("raw.plentyi"),
            format!("pub opaque Pointer\n{declaration}\n"),
        )
        .unwrap();
        let source = temp.path().join("source.plenty");
        std::fs::write(
            &source,
            "import raw\nexport def entry() -> () = \"native_entry\":\n    pass\n",
        )
        .unwrap();
        let error = plenty::check_module_file(&source, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
}
