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
    let output = if std::env::var_os("PLENTY_TEST_VALGRIND").is_some() {
        Command::new("valgrind")
            .args([
                "--quiet",
                "--error-exitcode=99",
                "--leak-check=full",
                "--errors-for-leak-kinds=definite,indirect,possible",
            ])
            .arg(executable)
            .output()
            .unwrap()
    } else {
        Command::new(executable).output().unwrap()
    };
    success(output)
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
    for ty in [
        "str",
        "bool",
        "list[i64]",
        "Option[i64]",
        "&str",
        "&mut list[u8]",
    ] {
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

fn adapters(root: &Path) {
    std::fs::write(
        root.join("native.plentyi"),
        include_str!("fixtures/ffi.plentyi"),
    )
    .unwrap();
}

fn visible(output: Output) -> String {
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn borrowed_scalar_outputs_and_text_adapters_match_c() {
    let temp = tempfile::tempdir().unwrap();
    let library = fixture(temp.path(), false);
    adapters(temp.path());
    let output = run(
        temp.path(),
        r#"
import native
def main() -> Result[(), Failure]:
    mut a = 0i8
    mut b = 0u16
    mut c = 0.0f32
    mut d = 0.0f64
    mut e = 0i64
    native.outputs(&mut a, &mut b, &mut c, &mut d, &mut e)
    print(a)?
    print(b)?
    print(c)?
    print(d)?
    print(native.read(&e))?
    text = "é\0A"
    print(native.bytes(&text))?
    empty = ""
    print(native.bytes(&empty))?
    character = text[0]
    print(native.bytes(&character))?
    short = text.slice(0, 3)?
    print(native.bytes(&short))?
    print(str.repr(native.strings(&character, &empty)).unwrap())?
    good = "é"
    print(str.repr(native.strings(&good, &empty)).unwrap())?
    print(str.repr(native.strings(&good, &text)).unwrap())?
    print(str.repr(native.strings(&text, &good)).unwrap())?
    native.string_void(&good)?
    print(native.text_calls())?
    print(native.string_float(&good)?)?
    print(native.pointer_value(&good)?)?
    Ok(())
"#,
        library,
    );
    assert_eq!(visible(output), "-128\n65535\n1.5\n-2.25\n-9223372036854775808\n429\n0\n364\n429\nResult[u64, CStrError].Ok(2)\nResult[u64, CStrError].Ok(2)\nResult[u64, CStrError].Err(CStrError.EmbeddedNul)\nResult[u64, CStrError].Err(CStrError.EmbeddedNul)\n4\n2.5\n42");
}

#[test]
fn owned_foreign_handles_drop_on_success_failure_and_transfer() {
    let temp = tempfile::tempdir().unwrap();
    let library = fixture(temp.path(), true);
    adapters(temp.path());
    let output = run(
        temp.path(),
        r#"
import native
def main() -> Result[(), Failure]:
    mut first = native.create(0)?
    first.set(17)
    print(first.value())?
    moved = first
    print(native.live())?
    drop(moved)
    print(native.live())?
    drop(native.create(1))
    drop(native.create(2))
    print(native.live())?
    drop(native.acquire_pair(2))
    invalid = "bad\0text"
    drop(native.convert_owned(native.create(0)?, &invalid))
    print(native.live())?
    match native.transfer(native.create(0)?, 1):
        case Ok(_):
            print("unexpected")?
        case Err(owner):
            print(owner.value())?
    print(native.live())?
    native.transfer(native.create(0)?, 0)?
    print(native.live())?
    drop(native.consume(native.create(0)?, 1))
    native.consume(native.create(0)?, 0)?
    print(native.live())?
    Ok(())
"#,
        library,
    );
    assert_eq!(visible(output), "17\n1\n0\n0\n0\n42\n0\n0\n0");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn c_string_allocation_failure_reclaims_earlier_buffers_and_skips_c_call() {
    for budget in 0..=2 {
        let temp = tempfile::tempdir().unwrap();
        let library = fixture(temp.path(), false);
        adapters(temp.path());
        let source = format!(
            r#"
import native
def main() -> Result[(), Failure]:
    a = "hello"
    b = "é"
    print("__test_fail_allocations_after_{budget}__")?
    result = native.strings(&a, &b)
    print("__test_restore_allocations__")?
    print(str.repr(result).unwrap())?
    print(native.text_calls())?
    print("__test_fail_allocations_after_0__")?
    owner = native.create(0)
    print("__test_restore_allocations__")?
    drop(owner)
    print(native.opened())?
    print(native.live())?
    print("__test_begin_no_allocations__")?
    count = native.bytes(&a)
    print("__test_end_no_allocations__")?
    print(count)?
    owned = native.create(0)?
    print("__test_fail_allocations_after_0__")?
    converted = native.convert_owned(owned, &a)
    print("__test_restore_allocations__")?
    print(str.repr(converted).unwrap())?
    print(native.live())?
    Ok(())
"#
        );
        let actual = visible(run(temp.path(), &source, library));
        let result = if budget == 2 {
            "Result[u64, CStrError].Ok(7)\n1"
        } else {
            "Result[u64, CStrError].Err(CStrError.Allocation(AllocError.OutOfMemory))\n0"
        };
        // Constructing the owner needs no allocation, so the handle opens and closes.
        assert_eq!(actual, format!("{result}\n1\n0\n532\nResult[(), CStrError].Err(CStrError.Allocation(AllocError.OutOfMemory))\n0"));
    }
}

#[test]
fn adapters_reject_invalid_types_and_preserve_borrow_rules() {
    let main = "import native\ndef main() -> ():\n    pass\n";
    rejected(
        "extern def bad(text: &str as c_string) -> i64 = \"bad\"\n",
        main,
        "Result[T, CStrError]",
    );
    rejected(
        "extern def bad(text: &mut str as utf8) -> i64 = \"bad\"\n",
        main,
        "text adapters require &str",
    );
    rejected(
        "extern def bad(text: &str as bytes) -> i64 = \"bad\"\n",
        main,
        "expected utf8 or c_string",
    );
    rejected(
        "pub extern def write(a: &mut i64, b: &i64) -> () = \"write_both\"\n",
        "import native\ndef main() -> ():\n    mut n = 1\n    native.write(&mut n, &n)\n",
        "borrow",
    );
    rejected(include_str!("fixtures/ffi.plentyi"), "import native\ndef main() -> ():\n    source = native.create(0).unwrap()\n    moved = source\n    print(source.value()).unwrap()\n", "moved");
    rejected(include_str!("fixtures/ffi.plentyi"), "import native\ndef main() -> ():\n    source = native.create(0).unwrap()\n    copy(source).unwrap()\n", "cannot be copied");
}
