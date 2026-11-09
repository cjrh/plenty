//! Native hosts discover initialized objects without invoking Plenty first.
use object::{Object, ObjectSymbol};
use std::process::Command;

#[test]
fn native_initialized_objects_static_and_shared() {
    for kind in [plenty::LibraryKind::Static, plenty::LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("raw.plentyi"),
            r#"
pub opaque Pointer
pub extern def descriptor() -> Pointer = object "native_descriptor"(size=32, align=8):
    0: u32 = 7
    8: callback = "native_apply"
    16: object = "native_name"
    24: object = "native_descriptor"
pub extern def name() -> Pointer = object "native_name"(size=6, align=1):
    0: bytes = b"hello\0"
"#,
        )
        .unwrap();
        let source = root.join("source.plenty");
        std::fs::write(
            &source,
            r#"
import raw
export def apply(value: i32) -> i32 = "native_apply":
    value
export def address() -> raw.Pointer = "native_address":
    raw.descriptor()
"#,
        )
        .unwrap();
        let library = root.join(if kind == plenty::LibraryKind::Static {
            "libnative.a"
        } else {
            "libnative.so"
        });
        let mut options = plenty::LibraryOptions::new("native", kind);
        options.compile.link_args.push("-Wl,--gc-sections".into());
        let artifacts = plenty::compile_file_to_library(&source, &library, None, &options).unwrap();
        plenty::check_module_file(&artifacts.interface, None).unwrap();
        if kind == plenty::LibraryKind::Shared {
            let bytes = std::fs::read(&library).unwrap();
            let file = object::File::parse(bytes.as_slice()).unwrap();
            let symbol = file
                .dynamic_symbols()
                .find(|symbol| symbol.name().ok() == Some("native_descriptor"))
                .unwrap();
            assert_eq!(symbol.kind(), object::SymbolKind::Data);
            assert_eq!(symbol.size(), 32);
        }
        let c = root.join("host.c");
        std::fs::write(&c, r#"
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <stddef.h>
#include <dlfcn.h>
#include "native.h"
struct descriptor { uint32_t version; int32_t (*apply)(int32_t); const char *name; const void *self; };
_Static_assert(sizeof(struct descriptor) == 32, "size");
_Static_assert(offsetof(struct descriptor, apply) == 8, "callback offset");
#ifndef SHARED
extern const struct descriptor native_descriptor;
#endif
int main(int argc, char **argv) {
#ifdef SHARED
    assert(argc == 2);
    void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    assert(library);
    const struct descriptor *d = dlsym(library, "native_descriptor");
    void *(*address)(void) = dlsym(library, "native_address");
    assert(address);
#else
    (void)argc; (void)argv;
    const struct descriptor *d = &native_descriptor;
    void *(*address)(void) = native_address;
#endif
    assert(d && d->version == 7);
    assert(d->apply(-17) == -17);
    assert(!strcmp(d->name, "hello"));
    assert(d->self == d && address() == d);
    const unsigned char *bytes = (const unsigned char *)d;
    for (int i = 4; i < 8; ++i) assert(bytes[i] == 0);
#ifdef SHARED
    assert(dlclose(library) == 0);
#endif
}
"#).unwrap();
        let executable = root.join("host");
        let mut cc = Command::new("cc");
        cc.args(["-Wall", "-Wextra", "-Werror"]).arg(c);
        if kind == plenty::LibraryKind::Static {
            cc.arg(&library).args(
                std::fs::read_to_string(artifacts.link_args)
                    .unwrap()
                    .lines(),
            );
        } else {
            cc.args(["-DSHARED", "-ldl"]);
        }
        let output = cc.arg("-o").arg(&executable).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            Command::new(executable)
                .arg(library)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[test]
fn object_layout_diagnostics() {
    for (declaration, expected) in [
        (
            "object \"entry\"(size=8, align=3):\n    0: u64 = 0",
            "power-of-two",
        ),
        (
            "object \"entry\"(size=8, align=8):\n    8: u8 = 0",
            "outside",
        ),
        (
            "object \"entry\"(size=8, align=8):\n    0: u64 = 0\n    4: u32 = 0",
            "overlap",
        ),
        (
            "object \"entry\"(size=8, align=8):\n    1: u32 = 0",
            "aligned",
        ),
        (
            "object \"entry\"(size=8, align=8):\n    0: u8 = 256",
            "out of range",
        ),
        (
            "object \"entry\"(size=8, align=8):\n    0: callback = \"missing\"",
            "exported function",
        ),
        (
            "object \"entry\"(size=8, align=8):\n    0: object = \"missing\"",
            "exported object",
        ),
        (
            "object \"native_apply\"(size=8, align=8):\n    0: u64 = 0",
            "duplicate",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("raw.plentyi"),
            format!("pub opaque Pointer\npub extern def entry() -> Pointer = {declaration}\n"),
        )
        .unwrap();
        let source = temp.path().join("source.plenty");
        std::fs::write(
            &source,
            "import raw\nexport def apply() -> () = \"native_apply\":\n    pass\n",
        )
        .unwrap();
        let error = plenty::check_module_file(&source, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
}

#[test]
fn data_only_library_keeps_unused_object() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("raw.plentyi"), "pub opaque Pointer\npub extern def entry() -> Pointer = object \"entry\"(size=8, align=8):\n    0: u64 = 123\n").unwrap();
    let source = temp.path().join("source.plenty");
    std::fs::write(&source, "import raw\n").unwrap();
    let artifacts = plenty::compile_file_to_library(
        &source,
        &temp.path().join("libnative.so"),
        None,
        &plenty::LibraryOptions::new("native", plenty::LibraryKind::Shared),
    )
    .unwrap();
    plenty::check_module_file(&artifacts.interface, None).unwrap();
    let output = Command::new("nm")
        .args(["-D", "--defined-only"])
        .arg(temp.path().join("libnative.so"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.ends_with(" entry"))
    );
}

#[test]
fn object_contract_fingerprints_contents_and_rejects_generated_symbols() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("source.plenty");
    std::fs::write(&source, "import raw\n").unwrap();
    let declaration = |symbol: &str, value: u32| {
        std::fs::write(root.join("raw.plentyi"), format!("pub opaque Pointer\npub extern def entry() -> Pointer = object \"{symbol}\"(size=8, align=8):\n    0: u32 = {value}\n")).unwrap();
    };
    declaration("entry", 1);
    let first = plenty::runtime_interface_source(&source, None, "native").unwrap();
    declaration("entry", 2);
    let second = plenty::runtime_interface_source(&source, None, "native").unwrap();
    assert_ne!(first, second);
    declaration("native_plenty_interface_v1", 1);
    let error = plenty::runtime_interface_source(&source, None, "native")
        .unwrap_err()
        .to_string();
    assert!(error.contains("generated library symbol"), "{error}");
}
