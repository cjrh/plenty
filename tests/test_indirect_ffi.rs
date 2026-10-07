use std::process::Command;

#[test]
fn trusted_addresses_use_c_signatures_and_adapters() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("fixture.c"),
        r#"#include <stdint.h>
#include <stddef.h>
static double calculate(int8_t a, uint8_t b, float c, double d, int64_t *out,
                        const uint8_t *text, size_t len) {
    *out += a + b + (int64_t)len + text[0];
    return c + d;
}
void *fixture_address(void) { return (void *)&calculate; }
"#,
    )
    .unwrap();
    let object = root.join("fixture.o");
    assert!(Command::new("cc")
        .args(["-Wall", "-Wextra", "-Werror", "-c"])
        .arg(root.join("fixture.c"))
        .arg("-o")
        .arg(&object)
        .status()
        .unwrap()
        .success());
    std::fs::write(root.join("native.plentyi"), r#"
opaque Function
extern def address() -> Function = "fixture_address"
extern def invoke(a: i8, function: Function, b: u8, c: f32, d: f64, out: &mut i64, text: &str as utf8) -> f64 = function
pub def calculate(out: &mut i64) -> f64:
    text = "abc"
    invoke(-2, address(), 250, 1.25, 2.5, out, &text)
"#).unwrap();
    let source = root.join("main.plenty");
    std::fs::write(
        &source,
        r#"
import native
def main() -> Result[(), Failure]:
    mut out = 4
    print(native.calculate(&mut out))?
    print(out)?
    Ok(())
"#,
    )
    .unwrap();
    let executable = root.join("app");
    let options = plenty::CompileOptions {
        link_args: vec![object.into_os_string()],
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&source, &executable, None, &options).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"3.75\n352\n");
}

#[test]
fn indirect_targets_require_declared_opaque_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, "import native\ndef main() -> ():\n    pass\n").unwrap();
    for (declaration, expected) in [
        (
            "extern def invoke(code: i64) -> i32 = code\n",
            "opaque pointer type",
        ),
        (
            "opaque Code\nextern def invoke(code: &Code) -> i32 = code\n",
            "opaque pointer type",
        ),
        (
            "extern def invoke() -> i32 = missing\n",
            "function-address parameter",
        ),
    ] {
        std::fs::write(dir.path().join("native.plentyi"), declaration).unwrap();
        let error = plenty::compile_file_to_executable_with_options(
            &source,
            &dir.path().join("app"),
            None,
            &Default::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{error}");
    }
}
