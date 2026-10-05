//! Contract tests for modern Plenty, independent of the historical stack syntax.
use plenty::{input_complete, Vm};
use rstest::rstest;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[rstest]
#[case("1 + 2 * 3", "[7i64]")]
#[case("(1 + 2) * 3", "[9i64]")]
#[case("(1 < 2) == True", "[true]")]
#[case("not (1 > 2)", "[true]")]
#[case("10 - 3 - 2", "[5i64]")]
#[case("-7 // 3", "[-3i64]")]
#[case("7 // -3", "[-3i64]")]
#[case("-7 // -3", "[2i64]")]
#[case("-6 // 3", "[-2i64]")]
#[case("-128i8 // 3i8", "[-43i8]")]
#[case("18446744073709551615u64 // 2u64", "[9223372036854775807u64]")]
#[case("-9223372036854775808", "[-9223372036854775808i64]")]
#[case("i8(257)", "[1i8]")]
#[case("i64(-2i8)", "[-2i64]")]
#[case("not 1 == 2 and True", "[true]")]
#[case("True or False and False", "[true]")]
#[case("False and 1 // 0 == 0", "[false]")]
#[case("True or 1 // 0 == 0", "[true]")]
#[case("42 if True else 1 // 0", "[42i64]")]
#[case("0 if False else 42 if True else 3", "[42i64]")]
#[case("'hello' + \" world\"", "[\"hello world\"]")]
#[case("contains('héllo', 'é')", "[true]")]
#[case("1_000 + 20", "[1020i64]")]
#[case("()", "[]")]
#[case("# a comment\n\n42 # trailing comment", "[42i64]")]
#[case("(1 +\n    # ignored indentation\n 2)", "[3i64]")]
#[case("x: int = 40\nx + 2", "[42i64]")]
#[case("mut x = 1\nx = x + 2\nx", "[3i64]")]
#[case("mut x = 0\nif True:\n    x = 42\nelse:\n    x = 7\nx", "[42i64]")]
#[case("if False:\n    1\nelif True:\n    42\nelse:\n    3", "[42i64]")]
#[case("if True:\n    x = 40\n    x + 2\nelse:\n    x = 1\n    x", "[42i64]")]
#[case("if True:\n    pass", "[]")]
#[case("if True:\n    99\n42", "[42i64]")]
#[case("def double(x: int) -> int:\n    x * 2\ndouble(21)", "[42i64]")]
#[case("double(21)\ndef double(x: int) -> int:\n    return x * 2", "[42i64]")]
#[case("def unit() -> ():\n    return\nunit()", "[]")]
#[case("def text() -> str:\n    return 'hello'\ntext()", "[\"hello\"]")]
#[case("def f(\n    x: int,\n) -> int:\n    x\nf(42,)", "[42i64]")]
#[case("def f(x: int) -> int:\r\n    x + 1\r\nf(41)\r\n", "[42i64]")]
fn expressions(#[case] source: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(source).unwrap_or_else(|e| panic!("{source}\n{e}"));
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
#[case("def f(x) -> int:\n    x", "expected `:`")]
#[case("def f(x: int):\n    x", "expected `->`")]
#[case("def f(x: int) -> str:\n    x", "expected str, got i64")]
#[case("def f(x: int, x: int) -> int:\n    x", "duplicate parameter")]
#[case("def f() -> int:\n    pass", "expected i64, got ()")]
#[case("def f() -> ():\n    42", "expected (), got i64")]
#[case("def f() -> int:\n    return 1\n    2", "early return")]
#[case(
    "def f() -> int:\n    if True:\n        return 1\n    2",
    "early return"
)]
#[case("return 1", "return outside")]
#[case("if True:\n    1\nelse:\n    False", "expected i64, got bool")]
#[case("if True:\n    1", "expected i64, got ()")]
#[case("if 1:\n    pass", "expected bool")]
#[case("1 and 2", "expected bool")]
#[case("True + False", "does not accept bool")]
#[case("'a' < 'b'", "does not accept str")]
#[case("1 < 2 < 3", "chained comparisons")]
#[case("1 / 2", "use `//`")]
#[case("True + 1", "expected bool, got i64")]
#[case("1i8 + 1", "expected i8, got i64")]
#[case("None", "expected an expression")]
#[case("missing", "unknown binding")]
#[case("missing()", "unknown function")]
#[case("def f(x: int) -> int:\n    x\nf()", "expects 1 arguments")]
#[case("def f(x: int) -> int:\n    x\nf(False)", "expected i64, got bool")]
#[case("x = 1\nx = 2", "immutable")]
#[case("mut x = 1\nx = False", "expected i64, got bool")]
#[case("x: bool = 1", "expected bool")]
#[case("x = x + 1", "unknown binding")]
#[case("x = 1\nmut x = 2", "duplicate binding")]
#[case("if True:\n    x = 1\nx", "unknown binding")]
#[case("def f(x: int) -> int:\n    x = 2\n    x", "immutable")]
#[case("x = ()", "expected a value")]
#[case("def f(x: ()) -> ():\n    pass", "unit parameters")]
#[case("def print() -> ():\n    pass", "cannot redefine a builtin")]
#[case("__plenty_entry()", "expected an expression")]
#[case(
    "def f() -> ():\n    def g() -> ():\n        pass",
    "expected an expression"
)]
#[case("def f() -> ():\npass", "expected an indented block")]
#[case("if True:\n    pass\n  pass", "indentation does not match")]
#[case("if True:\n\tpass", "use spaces")]
#[case("1 2 +", "expected the end of the statement")]
#[case("128i8", "out of range")]
#[case("-1u8", "out of range")]
#[case("1__0", "invalid integer separator")]
#[case("'hello", "unterminated string")]
#[case("'a\0b'", "NUL bytes")]
#[case("'\\q'", "unsupported escape")]
#[case("(1 + 2", "unclosed parenthesis")]
#[case("1 + 2)", "unmatched")]
#[case("print()", "print takes one")]
#[case("contains('a', 1)", "expected str")]
fn diagnostics(#[case] source: &str, #[case] message: &str) {
    let error = Vm::new().run(source).expect_err(source).to_string();
    assert!(
        error.contains(message),
        "{source}\nexpected {message:?}, got {error:?}"
    );
    assert!(
        error.chars().next().unwrap().is_ascii_digit(),
        "missing source position: {error}"
    );
}

#[test]
fn repl_state_is_checked_before_effects_and_definitions_persist() {
    let mut vm = Vm::new();
    vm.run("def f(x: int) -> int:\n    \"\"\"A rich\n    docstring.\"\"\"\n    x + 1")
        .unwrap();
    assert_eq!(vm.function_doc("f"), Some("A rich\n    docstring."));
    vm.run("f(41)").unwrap();
    assert_eq!(vm.stack_repr(), "[42i64]");
    assert!(vm.run("print('must not print')\nmissing()").is_err());
    assert_eq!(vm.stack_repr(), "[42i64]");
    assert!(vm.run("def bad() -> int:\n    False").is_err());
    assert_eq!(vm.function_names(), vec!["f"]);
    assert!(vm.run("def f() -> bool:\n    True").is_err());
    vm.run("f(9)").unwrap();
    assert_eq!(vm.stack_repr(), "[10i64]");
    assert!(vm.run("1 // 0").is_err());
    vm.run("f(20)").unwrap();
    assert_eq!(vm.stack_repr(), "[21i64]");
    vm.run("local = 1").unwrap();
    assert!(vm.run("local").is_err()); // current submission scope is intentional
}

#[test]
fn local_slot_limit_is_a_diagnostic() {
    let source = (0..257)
        .map(|i| format!("x{i} = {i}\n"))
        .collect::<String>();
    assert!(Vm::new()
        .run(&source)
        .unwrap_err()
        .to_string()
        .contains("256"));
}

#[rstest]
#[case("1 + 2", true)]
#[case("1 if True else 2", true)]
#[case("'if def'", true)]
#[case("def f() -> int:\n    1", false)]
#[case("def f() -> int:\n    1\n\n", true)]
#[case("def f() -> int:\n    1\n    ", true)]
#[case("(1 +", false)]
#[case("'''hello", false)]
#[case("1 + )", true)]
fn repl_completion(#[case] source: &str, #[case] complete: bool) {
    assert_eq!(input_complete(source), complete);
}

struct Artifact {
    source: PathBuf,
    executable: PathBuf,
}
impl Artifact {
    fn new(source: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "plenty-modern-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let artifact = Self {
            source: base.with_extension("plenty"),
            executable: base.with_extension("exe"),
        };
        std::fs::write(&artifact.source, source).unwrap();
        artifact
    }
    fn interpret(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg(&self.source)
            .output()
            .unwrap()
    }
    fn compile(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg("--compile")
            .arg(&self.source)
            .arg("-o")
            .arg(&self.executable)
            .output()
            .unwrap()
    }
}
impl Drop for Artifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.source);
        let _ = std::fs::remove_file(&self.executable);
    }
}

#[rstest]
#[case(include_str!("../examples/sum.plenty"), "5051\n")]
#[case("def plenty_main() -> int:\n    42\ndef plenty_println() -> str:\n    return 'safe'\nprint(plenty_main())\nprint(plenty_println())", "42\nsafe\n")]
#[case("print('héllo\\nworld')\nprint(True)\nprint(False)\nprint(-128i8)\nprint(18446744073709551615u64)", "héllo\nworld\nTrue\nFalse\n-128\n18446744073709551615\n")]
#[case(
    "mut x = 1\nif True:\n    x = 40\nelse:\n    x = 0\nx = x + 2\nprint(x)",
    "42\n"
)]
#[case("mut x = 1\nif False:\n    x = 40\nelse:\n    x = 2\nprint(x)", "2\n")]
#[case("def f(flag: bool) -> int:\n    if flag:\n        a = 42\n        a\n    else:\n        b = 7\n        b\nprint(f(True))\nprint(f(False))", "42\n7\n")]
#[case("def count(n: int, total: int) -> int:\n    if n == 0:\n        total\n    else:\n        x = total + 1\n        count(n - 1, x)\nprint(count(100_000, 0))", "100000\n")]
#[case("def even(n: int) -> bool:\n    True if n == 0 else odd(n - 1)\ndef odd(n: int) -> bool:\n    False if n == 0 else even(n - 1)\nprint(even(100_000))", "True\n")]
#[case("def unit(n: int) -> ():\n    if n == 0:\n        pass\n    else:\n        unit(n - 1)\nunit(100_000)\nprint(42)", "42\n")]
#[case("def f(n: int) -> int:\n    if n == 0:\n        0\n    else:\n        1 + f(n - 1)\nprint(f(30))", "30\n")]
#[case("def side() -> bool:\n    print('side')\n    True\nprint(False and side())\nprint(True or side())\nprint(True and side())", "False\nTrue\nside\nTrue\n")]
#[case(
    "def value(n: int) -> int:\n    print(n)\n    n\nprint(value(1) + value(2))",
    "1\n2\n3\n"
)]
#[case("print(-7 // 3)\nprint(7 // -3)\nprint(-7 // -3)\nprint(-128i8 // 3i8)\nprint(-9223372036854775808 // 1)", "-3\n-3\n2\n-43\n-9223372036854775808\n")]
#[case(
    "print(42 if True else 1 // 0)\nprint(False and 1 // 0 == 0)",
    "42\nFalse\n"
)]
fn native_and_interpreter_agree(#[case] source: &str, #[case] expected: &str) {
    let artifact = Artifact::new(source);
    let interpreted = artifact.interpret();
    assert!(
        interpreted.status.success(),
        "interpreter: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&interpreted.stdout), expected);
    let compiled = artifact.compile();
    assert!(
        compiled.status.success(),
        "compiler: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let native = Command::new(&artifact.executable).output().unwrap();
    assert!(
        native.status.success(),
        "native: {}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(native.stdout, interpreted.stdout);
}

#[rstest]
#[case("print(127i8 + 1i8)", "integer overflow")]
#[case("print(-128i8 // -1i8)", "integer overflow")]
#[case("print(1 // 0)", "division by zero")]
fn runtime_errors_agree(#[case] source: &str, #[case] expected: &str) {
    let artifact = Artifact::new(source);
    let interpreted = artifact.interpret();
    let compiled = artifact.compile();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let native = Command::new(&artifact.executable).output().unwrap();
    assert_eq!(interpreted.status.code(), Some(1));
    assert_eq!(native.status.code(), Some(1));
    assert_eq!(native.stderr, interpreted.stderr);
    assert!(String::from_utf8_lossy(&native.stderr).contains(expected));
}

#[test]
fn rejected_program_has_no_effects_or_output_artifact() {
    let artifact = Artifact::new("print('must not execute')\nx = 1\nx = False");
    let interpreted = artifact.interpret();
    let compiled = artifact.compile();
    assert!(!interpreted.status.success());
    assert!(!compiled.status.success());
    assert!(interpreted.stdout.is_empty());
    assert!(!artifact.executable.exists());
    assert_eq!(interpreted.stderr, compiled.stderr);
}

#[test]
fn modern_library_aot_entry_point_uses_modern_syntax() {
    let artifact = Artifact::new("");
    plenty::compile_source_to_executable("print(40 + 2)", &artifact.executable).unwrap();
    let output = Command::new(&artifact.executable).output().unwrap();
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn check_mode_does_not_execute_valid_programs() {
    let source = "print('must not print')\n1 // 0";
    plenty::check_source(source).unwrap();
    let artifact = Artifact::new(source);
    let checked = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--check")
        .arg(&artifact.source)
        .output()
        .unwrap();
    assert!(checked.status.success());
    assert!(checked.stdout.is_empty());
    assert!(checked.stderr.is_empty());
    assert!(plenty::check_source("mut x = True\nx = 1").is_err());
}
