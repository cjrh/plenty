//! C fixtures validate actual ABI calls, not just interface parsing.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn fixture(root: &Path, shared: bool) -> PathBuf {
    let source = root.join("fixture.c");
    std::fs::write(&source, include_str!("fixtures/ffi.c")).unwrap();
    let object = root.join("fixture.o");
    success(
        Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-fPIC", "-c"])
            .arg(source)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    let library = root.join(if shared {
        "libfixture.so"
    } else {
        "libfixture.a"
    });
    if shared {
        success(
            Command::new("cc")
                .arg("-shared")
                .arg(object)
                .arg("-o")
                .arg(&library)
                .output()
                .unwrap(),
        );
    } else {
        success(
            Command::new("ar")
                .arg("rcs")
                .arg(&library)
                .arg(object)
                .output()
                .unwrap(),
        );
    }
    library
}

fn run(root: &Path, source: &str, library: PathBuf) -> Output {
    let path = root.join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let executable = root.join("program");
    let options = plenty::CompileOptions {
        link_args: vec![library.into_os_string()],
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&path, &executable, None, &options).unwrap();
    success(Command::new(executable).output().unwrap())
}

const RAW: &str = r#"
pub opaque Token
pub extern def narrow(a: i8, b: u8, c: i16, d: u16, e: i32, f: u32, g: i64, h: u64) -> i64 = "fixture_narrow"
pub extern def small(value: i8) -> i8 = "fixture_small"
pub extern def unsigned(value: u64) -> u64 = "fixture_unsigned"
pub extern def real(a: f32, b: f64) -> f64 = "fixture_real"
pub extern def single(value: f32) -> f32 = "fixture_single"
pub extern def token() -> Token = "fixture_token"
pub extern def value(token: Token) -> i64 = "fixture_value"
pub extern def reset() -> () = "fixture_reset"
"#;

#[test]
fn scalar_and_opaque_calls_link_statically_and_dynamically() {
    for shared in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let library = fixture(temp.path(), shared);
        std::fs::write(temp.path().join("native.plentyi"), RAW).unwrap();
        let output = run(
            temp.path(),
            r#"
import native
def main() -> Result[(), Failure]:
    native.reset()
    print(native.narrow(-2, 250, -300, 65000, -70000, 4000000000, -9, 42))?
    print(native.small(-127))?
    print(native.unsigned(18446744073709551615u64))?
    print(native.real(1.5f32, 2.25))?
    print(native.single(3.5))?
    token = native.token()
    print(token.is_null())?
    print(native.Token.null().is_null())?
    print(native.value(token))?
    Ok(())
"#,
            library,
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "3999994981\n-127\n18446744073709551615\n3.75\n3.5\nFalse\nTrue\n42\n"
        );
    }
}

fn rejected(interface: &str, source: &str, message: &str) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("native.plentyi"), interface).unwrap();
    let source_path = temp.path().join("main.plenty");
    std::fs::write(&source_path, source).unwrap();
    let error = plenty::check_file(&source_path, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains(message), "expected {message:?}, got {error}");
}

#[test]
fn declarations_are_explicit_and_private_layouts_do_not_cross_c() {
    let main = "import native\ndef main() -> ():\n    pass\n";
    rejected(
        "extern def first(x: i32) -> i32 = \"same\"\nextern def second(x: f64) -> i32 = \"same\"\n",
        main,
        "conflicting C signatures",
    );
    for ty in ["str", "bool", "list[i64]", "Option[i64]", "&i64"] {
        rejected(
            &format!("extern def bad(value: {ty}) -> () = \"fixture_bad\"\n"),
            main,
            "no supported C ABI",
        );
    }
    rejected(
        "extern def bad[T](value: T) -> T = \"bad\"\n",
        main,
        "cannot be generic",
    );
    rejected(
        "extern def bad() -> () = \"plenty_release\"\n",
        main,
        "reserved",
    );
    rejected(
        "opaque Secret\npub extern def leak() -> Secret = \"leak\"\n",
        main,
        "public signature exposes private type",
    );
    rejected(
        "extern def hidden() -> i64 = \"hidden\"\n",
        "import native\ndef main() -> i64:\n    native.hidden()\n",
        "private",
    );
    rejected(
        "pub opaque A\npub opaque B\npub extern def use(value: A) -> () = \"use\"\n",
        "import native\ndef main() -> ():\n    native.use(native.B.null())\n",
        "expected",
    );
    let error =
        plenty::check_source("extern def call() -> () = \"call\"\ndef main() -> ():\n    pass\n")
            .unwrap_err()
            .to_string();
    assert!(error.contains("trusted .plentyi"), "{error}");
}

#[test]
fn ambiguous_source_and_interface_modules_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["native.plenty", "native.plentyi"] {
        std::fs::write(temp.path().join(name), "").unwrap();
    }
    let main = temp.path().join("main.plenty");
    std::fs::write(&main, "import native\ndef main() -> ():\n    pass\n").unwrap();
    assert!(plenty::check_file(&main, None)
        .unwrap_err()
        .to_string()
        .contains("ambiguous module"));
}
