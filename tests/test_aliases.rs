mod support;
use rstest::rstest;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use support::{check_source, compile_source_to_executable};

#[rstest]
#[case("type int = i32\nx: int = 42i32\nx", "i32", "42\n")]
#[case(
    "type int = i64\ndef f(x: int) -> int:\n    x + 1\nf(41)",
    "i64",
    "42\n"
)]
#[case(
    "type int = i32\ndef f(x: int) -> int:\n    x + int(1)\nf(int(41))",
    "i32",
    "42\n"
)]
#[case(
    "x: Count = 42u16\ntype Count = Word\ntype Word = u16\nx",
    "u16",
    "42\n"
)]
#[case(
    "def f(x: Count) -> Count:\n    x\ntype Count = u8\nf(42u8)",
    "u8",
    "42\n"
)]
#[case("type Byte = u8\nByte(257)", "u8", "1\n")]
#[case("Count(42)\ntype Count = u16", "u16", "42\n")]
#[case("type Count = i32\nCount = 42i32\nx: Count = Count\nx", "i32", "42\n")]
#[case("type Signed = i64\nSigned(-2i8)", "i64", "-2\n")]
#[case(
    "type Flag = bool\ndef yes() -> Flag:\n    True\nyes()",
    "bool",
    "True\n"
)]
#[case(
    "type Text = str\ndef greet() -> Text:\n    return 'hello'\ngreet()",
    "str",
    "hello\n"
)]
#[case("type Done = ()\ndef done() -> Done:\n    return\ndone()", "()", "")]
#[case(
    "type First = Done\ntype Done = ()\ndef done() -> First:\n    pass\ndone()",
    "()",
    ""
)]
#[case(
    "type Count = u32\nmut x: Count = 1u32\nx = Count(42)\nx",
    "u32",
    "42\n"
)]
#[case(
    "type Metres = i64\ntype Seconds = i64\nx: Metres = 20\ny: Seconds = 22\nx + y",
    "i64",
    "42\n"
)]
#[case(
    "type Count = i32\ndef f() -> Count:\n    local: Count = 42i32\n    return local\nf()",
    "i32",
    "42\n"
)]
fn aliases_are_transparent(#[case] source: &str, #[case] ty: &str, #[case] expected: &str) {
    support::assert_value(source, ty, expected);
}

#[rstest]
#[case("x: int = 1", "unknown type `int`")]
#[case("def f(x: int) -> i64:\n    x", "unknown type `int`")]
#[case("int(42)", "unknown function `int`")]
#[case("42int", "invalid integer suffix")]
#[case("type int = i32\nx: int = 42i64", "expected i32, got i64")]
#[case("type int = i32\n42int", "invalid integer suffix")]
#[case("type A = Missing", "unknown type `Missing`")]
#[case("type A = A", "cyclic type alias")]
#[case("type A = B\ntype B = C\ntype C = A", "cyclic type alias")]
#[case("type A = i64\ntype A = i32", "already defined")]
#[case("type i64 = i32", "cannot redefine builtin")]
#[case("type str = u8", "cannot redefine builtin")]
#[case("type print = i32", "cannot redefine builtin")]
#[case(
    "type X = i32\ndef X() -> i32:\n    0i32",
    "conflicts with a type alias"
)]
#[case(
    "def X() -> i32:\n    0i32\ntype X = i32",
    "conflicts with a type alias"
)]
#[case("def f() -> ():\n    type Local = i32", "module scope")]
#[case("if True:\n    type Local = i32", "module scope")]
#[case("type A i32", "expected `=`")]
#[case("type A = 32", "expected a type")]
#[case("type A = i32 + i64", "end of the type alias declaration")]
#[case("type Done = ()\ndef f(x: Done) -> ():\n    pass", "unit parameters")]
#[case("type Done = ()\nx: Done = 42", "unit bindings")]
#[case(
    "type Text = str\nText('hello')",
    "only numeric types support cast syntax"
)]
#[case("type Done = ()\nDone()", "only numeric types support cast syntax")]
fn invalid_aliases_are_diagnosed(#[case] source: &str, #[case] expected: &str) {
    let error = check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error:?}"
    );
    assert!(
        error.chars().next().unwrap().is_ascii_digit(),
        "missing source position: {error}"
    );
}

#[test]
fn long_forward_alias_chains_do_not_require_recursive_resolution() {
    let mut source = (0..10_000)
        .map(|i| format!("type T{i} = T{}\n", i + 1))
        .collect::<String>();
    source.push_str("type T10000 = u8\nx: T0 = 42u8\nx");
    support::assert_value(&source, "u8", "42\n");
}

#[test]
fn native_aliases_have_the_same_abi_and_cast_behavior_as_their_targets() {
    let source = "type int = i32\ntype Byte = u8\ntype Count = int\ntype Text = str\ntype Flag = bool\ntype Done = ()\ndef bump(x: Count) -> int:\n    if x < 0i32:\n        return int(0)\n    x + 1i32\ndef done() -> Done:\n    return\ndef message() -> Text:\n    return 'hello'\ndef yes() -> Flag:\n    True\ndone()\nprint(bump(int(41)))\nprint(Byte(257))\nprint(message())\nprint(yes())";
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = std::env::temp_dir().join(format!(
        "plenty-alias-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let file = base.with_extension("plenty");
    let executable = base.with_extension("exe");
    std::fs::write(&file, support::program(source)).unwrap();
    let run_output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg(&file)
        .output();
    let compiled = compile_source_to_executable(source, &executable);
    let native = compiled
        .as_ref()
        .ok()
        .map(|_| Command::new(&executable).output());
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_file(executable);
    compiled.unwrap();
    let run_output = run_output.unwrap();
    let native = native.unwrap().unwrap();
    assert!(run_output.status.success());
    assert!(native.status.success());
    assert_eq!(run_output.stdout, b"42\n1\nhello\nTrue\n");
    assert_eq!(native.stdout, run_output.stdout);
}
