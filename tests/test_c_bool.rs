//! Exercise C/C++ bool values, exact-width storage and generated adapters.
use std::process::{Command, Output};

fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    output
}

#[test]
fn native_bool_import_export_and_memory() {
    for optimize in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("raw.plentyi"),
            r#"
pub opaque Pointer
pub extern def invert(value: bool) -> bool = "native_invert"
pub extern def flip(value: &mut bool) -> () = "native_flip"
pub extern def inspect(value: &bool) -> bool = "native_inspect"
pub extern def text(value: &str as c_string) -> Result[bool, CStrError] = "native_text"
pub extern def invoke(function: Pointer, value: bool) -> bool = function
pub extern def read(p: Pointer, offset: u64) -> bool = intrinsic "load"
pub extern def write(p: Pointer, offset: u64, value: bool) -> () = intrinsic "store"
"#,
        )
        .unwrap();
        let source = root.join("source.plenty");
        std::fs::write(
            &source,
            r#"
import raw
@thread_safe
export def invert(value: bool) -> bool = "boolean_invert":
    not value
export def borrowed(value: &mut bool, shared: &bool) -> bool = "boolean_borrowed":
    *value = not *value
    *shared
export def imported(value: bool) -> bool = "boolean_imported":
    mut local = value
    raw.flip(&mut local)
    raw.invert(raw.inspect(&local))
export def indirect(function: raw.Pointer, value: bool) -> bool = "boolean_indirect":
    raw.invoke(function, value)
export def memory(p: raw.Pointer) -> bool = "boolean_memory":
    value = raw.read(p, 1u64)
    raw.write(p, 1u64, not value)
    value
export def result(value: bool) -> Result[bool, i32] = "boolean_result":
    if value:
        Ok(True)
    else:
        Err(7i32)
export def text() -> bool = "boolean_text":
    raw.text("yes").unwrap()
"#,
        )
        .unwrap();
        let library = root.join("libboolean.a");
        let mut options = plenty::LibraryOptions::new("boolean", plenty::LibraryKind::Static);
        options.compile.optimize = optimize;
        let artifacts = plenty::compile_file_to_library(&source, &library, None, &options).unwrap();
        plenty::check_module_file(&artifacts.interface, None).unwrap();
        let dynamic = plenty::runtime_interface_source(&source, None, "boolean").unwrap();
        std::fs::write(root.join("dynamic.plentyi"), dynamic).unwrap();
        plenty::check_module_file(&root.join("dynamic.plentyi"), None).unwrap();
        let host = root.join("host.c");
        std::fs::write(
            &host,
            r#"
#include "boolean.h"
#include <assert.h>
#include <string.h>
#ifdef __cplusplus
extern "C" {
#endif
bool native_invert(bool b) { return !b; }
void native_flip(bool *p) { *p = !*p; }
bool native_inspect(const bool *p) { return *p; }
bool native_text(const char *p) { return !strcmp(p, "yes"); }
#ifdef __cplusplus
}
#endif
int main(void) {
    assert(sizeof(bool) == 1);
    bool (*volatile callback)(bool) = boolean_invert;
    for (int n = 0; n != 2; ++n) {
        bool value = n != 0;
        assert(boolean_invert(value) == !value);
        assert(callback(value) == !value);
        assert(boolean_imported(value) == value);
        assert(boolean_indirect((void *)&native_invert, value) == !value);
        struct { unsigned char before; bool value; unsigned char after; } box = {0x45, value, 0xa9};
        bool shared = value;
        assert(boolean_borrowed(&box.value, &shared) == value);
        assert(box.value == !value && box.before == 0x45 && box.after == 0xa9);
        unsigned char raw[] = {0x45, (unsigned char)n, 0xa9};
        assert(boolean_memory(raw) == value);
        assert(raw[0] == 0x45 && raw[1] == !value && raw[2] == 0xa9);
        box.value = false;
        int32_t error = 123;
        assert(boolean_result(value, &box.value, &error) == (value ? 0u : 1u));
        assert(box.value == value && error == (value ? 123 : 7));
        assert(box.before == 0x45 && box.after == 0xa9);
    }
    unsigned char raw[] = {0x45, 0x80, 0xa9};
    assert(boolean_memory(raw));
    assert(raw[0] == 0x45 && raw[1] == 0 && raw[2] == 0xa9);
    assert(boolean_text());
}
"#,
        )
        .unwrap();
        let link_args = std::fs::read_to_string(artifacts.link_args).unwrap();
        for compiler in ["cc", "c++"] {
            let executable = root.join(compiler);
            success(
                Command::new(compiler)
                    .args(["-Wall", "-Wextra", "-Werror", "-O2"])
                    .arg(&host)
                    .arg(&library)
                    .args(link_args.lines())
                    .arg("-o")
                    .arg(&executable)
                    .output()
                    .unwrap(),
            );
            success(Command::new(executable).output().unwrap());
        }
    }
}

#[test]
fn generated_static_and_runtime_wrappers_preserve_bool_results() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("source.plenty");
    std::fs::write(
        &source,
        r#"
export def invert(value: bool) -> bool = "boolean_invert":
    not value
export def result(value: bool) -> Result[bool, i32] = "boolean_result":
    if value:
        Ok(False)
    else:
        Err(7i32)
"#,
    )
    .unwrap();
    for shared in [false, true] {
        let library = root.join(if shared {
            "libboolean.so"
        } else {
            "libboolean.a"
        });
        let kind = if shared {
            plenty::LibraryKind::Shared
        } else {
            plenty::LibraryKind::Static
        };
        let artifacts = plenty::compile_file_to_library(
            &source,
            &library,
            None,
            &plenty::LibraryOptions::new("boolean", kind),
        )
        .unwrap();
        let setup = if shared {
            std::fs::write(
                root.join("plugin.plentyi"),
                plenty::runtime_interface_source(&source, None, "boolean").unwrap(),
            )
            .unwrap();
            format!(
                "    path = \"{}\"\n    api = plugin.load(&path)?\n",
                library.display()
            )
        } else {
            std::fs::copy(artifacts.interface, root.join("plugin.plentyi")).unwrap();
            String::new()
        };
        let api = if shared { "api" } else { "plugin" };
        let main = root.join("main.plenty");
        std::fs::write(
            &main,
            format!(
                r#"
import plugin
def main() -> Result[(), AllocError | IoError | LoadError]:
{setup}    print({api}.invert(False))?
    print({api}.invert(True))?
    match {api}.result(True):
        case Ok(value):
            print(value)?
        case Err(code):
            print(code)?
    match {api}.result(False):
        case Ok(value):
            print(value)?
        case Err(code):
            print(code)?
    Ok(())
"#
            ),
        )
        .unwrap();
        let executable = root.join("consumer");
        let mut options = plenty::CompileOptions::default();
        if !shared {
            options.link_args.push(library.into_os_string());
        }
        plenty::compile_file_to_executable_with_options(&main, &executable, None, &options)
            .unwrap();
        let output = success(Command::new(&executable).output().unwrap());
        assert_eq!(output.stdout, b"True\nFalse\nFalse\n7\n");
    }
}

fn c_object(root: &std::path::Path, source: &str) -> std::path::PathBuf {
    let path = root.join("native.c");
    std::fs::write(&path, source).unwrap();
    let object = root.join("native.o");
    success(
        Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c"])
            .arg(&path)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    object
}

fn run_linked(
    root: &std::path::Path,
    main: &str,
    object: &std::path::Path,
    optimize: bool,
) -> Vec<u8> {
    let source = root.join("main.plenty");
    std::fs::write(&source, main).unwrap();
    let executable = root.join("consumer");
    let options = plenty::CompileOptions {
        optimize,
        link_args: vec![object.as_os_str().to_owned()],
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&source, &executable, None, &options).unwrap();
    success(Command::new(&executable).output().unwrap()).stdout
}

// Issue #959: a C callee may store any byte through `bool *`.
#[test]
fn imported_mut_bool_argument_is_normalized_after_call() {
    for optimize in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let object = c_object(
            root,
            "void out2(unsigned char *p) { *p = 2; }\nvoid out0(unsigned char *p) { *p = 0; }\n",
        );
        std::fs::write(
            root.join("raw.plentyi"),
            "pub extern def out2(p: &mut bool) -> () = \"out2\"\npub extern def out0(p: &mut bool) -> () = \"out0\"\n",
        )
        .unwrap();
        let stdout = run_linked(
            root,
            r#"
import raw
def main() -> Result[(), AllocError | IoError]:
    mut b = False
    raw.out2(&mut b)
    n = not b
    print(b, b == True, b == False, n, n == True, n == False)?
    mut c = True
    raw.out0(&mut c)
    print(c, c == False)?
    Ok(())
"#,
            &object,
            optimize,
        );
        assert_eq!(stdout, b"True True False False False True\nFalse True\n");
    }
}

// Issue #959: the generated `Result[bool, E]` wrapper reads `out_ok` through
// `&mut bool`, so a native implementation's nonstandard byte must not leak.
#[test]
fn generated_result_bool_wrapper_normalizes_native_out_slot() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("source.plenty");
    std::fs::write(
        &source,
        r#"
export def result(value: bool) -> Result[bool, i32] = "boolean_result":
    if value:
        Ok(True)
    else:
        Err(7i32)
"#,
    )
    .unwrap();
    let artifacts = plenty::compile_file_to_library(
        &source,
        &root.join("libboolean.a"),
        None,
        &plenty::LibraryOptions::new("boolean", plenty::LibraryKind::Static),
    )
    .unwrap();
    std::fs::copy(&artifacts.interface, root.join("plugin.plentyi")).unwrap();
    // Replace the Plenty implementation with one honoring the generated C
    // ABI except for storing 2 in the `bool` output.
    // The stub also defines the wrapper's no-op contract guard.
    let interface = std::fs::read_to_string(&artifacts.interface).unwrap();
    let guard = interface
        .lines()
        .find_map(|line| line.strip_prefix("extern def _plenty_require_contract() -> () = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap();
    let object = c_object(
        root,
        &format!(
            "#include \"{}\"\nvoid {guard}(void) {{}}\nuint32_t boolean_result(bool value, bool *out_ok, int32_t *out_err) {{\n    if (!value) {{ *out_err = 7; return 1; }}\n    *(unsigned char *)out_ok = 2;\n    return 0;\n}}\n",
            artifacts.header.display()
        ),
    );
    for optimize in [false, true] {
        let stdout = run_linked(
            root,
            r#"
import plugin
def main() -> Result[(), AllocError | IoError]:
    match plugin.result(True):
        case Ok(value):
            print(value, value == True, not value)?
        case Err(code):
            print(code)?
    Ok(())
"#,
            &object,
            optimize,
        );
        assert_eq!(stdout, b"True True False\n");
    }
}
