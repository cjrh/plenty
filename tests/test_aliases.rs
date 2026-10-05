use plenty::{check_source, compile_source_to_executable, input_complete, Ty, Vm};
use rstest::rstest;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[rstest]
#[case("type int = i32\nx: int = 42i32\nx", "[42i32]")]
#[case("type int = i64\ndef f(x: int) -> int:\n    x + 1\nf(41)", "[42i64]")]
#[case(
    "type int = i32\ndef f(x: int) -> int:\n    x + int(1)\nf(int(41))",
    "[42i32]"
)]
#[case("x: Count = 42u16\ntype Count = Word\ntype Word = u16\nx", "[42u16]")]
#[case("def f(x: Count) -> Count:\n    x\ntype Count = u8\nf(42u8)", "[42u8]")]
#[case("type Byte = u8\nByte(257)", "[1u8]")]
#[case("Count(42)\ntype Count = u16", "[42u16]")]
#[case("type Count = i32\nCount = 42i32\nx: Count = Count\nx", "[42i32]")]
#[case("type Signed = i64\nSigned(-2i8)", "[-2i64]")]
#[case("type Flag = bool\ndef yes() -> Flag:\n    True\nyes()", "[true]")]
#[case(
    "type Text = str\ndef greet() -> Text:\n    return 'hello'\ngreet()",
    "[\"hello\"]"
)]
#[case("type Done = ()\ndef done() -> Done:\n    return\ndone()", "[]")]
#[case(
    "type First = Done\ntype Done = ()\ndef done() -> First:\n    pass\ndone()",
    "[]"
)]
#[case("type Count = u32\nmut x: Count = 1u32\nx = Count(42)\nx", "[42u32]")]
#[case(
    "type Metres = i64\ntype Seconds = i64\nx: Metres = 20\ny: Seconds = 22\nx + y",
    "[42i64]"
)]
#[case(
    "type Count = i32\ndef f() -> Count:\n    local: Count = 42i32\n    return local\nf()",
    "[42i32]"
)]
fn aliases_are_transparent(#[case] source: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(source)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
#[case("x: int = 1", "unknown type `int`")]
#[case("def f(x: int) -> i64:\n    x", "unknown type `int`")]
#[case("int(42)", "unknown function `int`")]
#[case("42int", "invalid integer suffix")]
#[case("type int = i32\nx: int = 42", "expected i32, got i64")]
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
    "only integer types support cast syntax"
)]
#[case("type Done = ()\nDone()", "only integer types support cast syntax")]
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
fn aliases_persist_in_the_repl_without_redefinition_or_partial_updates() {
    let mut vm = Vm::new();
    vm.run("type int = i32").unwrap();
    vm.run("type Count = int\ndef f(x: Count) -> int:\n    x + 1i32")
        .unwrap();
    assert_eq!(vm.type_alias_names(), ["Count", "int"]);
    assert_eq!(vm.function_sig("f").unwrap().outputs, [Ty::I32]);
    vm.run("f(int(41))").unwrap();
    assert_eq!(vm.stack_repr(), "[42i32]");
    for source in [
        "type int = i64",
        "type Added = i16\ntype Invalid = Missing",
        "type Added = i16\nunknown()",
        "type Added = i16\ndef wrong() -> bool:\n    42",
        "type f = i64",
        "def int() -> i64:\n    1",
    ] {
        assert!(vm.run(source).is_err(), "unexpectedly accepted {source}");
        assert_eq!(vm.type_alias_names(), ["Count", "int"]);
        assert_eq!(vm.function_names(), ["f"]);
        assert_eq!(vm.stack_repr(), "[42i32]");
    }
    vm.run("f(0i32)").unwrap();
    assert_eq!(vm.stack_repr(), "[1i32]");
    assert!(input_complete("type int = i32"));
}

#[test]
fn aliases_are_session_local_and_do_not_change_literal_defaults() {
    let mut first = Vm::new();
    let mut second = Vm::new();
    first.run("type int = i32").unwrap();
    second.run("type int = i64").unwrap();
    first.run("int(42)").unwrap();
    second.run("int(42)").unwrap();
    assert_eq!(first.stack_repr(), "[42i32]");
    assert_eq!(second.stack_repr(), "[42i64]");
    first.run("42").unwrap();
    assert_eq!(first.stack_repr(), "[42i64]");
    first.clear();
    assert_eq!(first.type_alias_names(), ["int"]);
}

#[test]
fn successful_declarations_survive_runtime_errors_like_functions() {
    let mut vm = Vm::new();
    assert!(vm
        .run("type Count = i32\ndef f() -> Count:\n    42i32\n1 // 0")
        .is_err());
    assert_eq!(vm.type_alias_names(), ["Count"]);
    vm.run("Count(f())").unwrap();
    assert_eq!(vm.stack_repr(), "[42i32]");
}

#[test]
fn long_forward_alias_chains_do_not_require_recursive_resolution() {
    let mut source = (0..10_000)
        .map(|i| format!("type T{i} = T{}\n", i + 1))
        .collect::<String>();
    source.push_str("type T10000 = u8\nx: T0 = 42u8\nx");
    let mut vm = Vm::new();
    vm.run(&source).unwrap();
    assert_eq!(vm.stack_repr(), "[42u8]");
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
    std::fs::write(&file, source).unwrap();
    let interpreted = Command::new(env!("CARGO_BIN_EXE_plenty"))
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
    let interpreted = interpreted.unwrap();
    let native = native.unwrap().unwrap();
    assert!(interpreted.status.success());
    assert!(native.status.success());
    assert_eq!(interpreted.stdout, b"42\n1\nhello\nTrue\n");
    assert_eq!(native.stdout, interpreted.stdout);
}
