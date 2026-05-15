//! End-to-end tests: run Plenty source through the VM and check observable
//! results — never internal representation.

use plenty::Vm;
use rstest::rstest;

#[rstest]
#[case("1 2 + .", "[3]")]
#[case("3 4 + .", "[7]")]
#[case("5 5 * .", "[25]")]
#[case("10 2 - .", "[8]")]
#[case("1 2 3 4 5 6 +", "[1 2 3 4 11]")]
#[case("1 2 +\n3 +", "[6]")]
fn arithmetic_leaves_expected_stack(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
#[case(r#""hello""#, r#"["hello"]"#)]
#[case(r#""hello world""#, r#"["hello world"]"#)]
#[case(r#""a\"b""#, r#"["a\"b"]"#)]
#[case(r#""a\\b""#, r#"["a\\b"]"#)]
fn a_quoted_string_pushes_text(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
#[case(r#""hello"#)] // unterminated string literal
#[case(r#""bad \z escape""#)] // unrecognised escape sequence
fn malformed_string_literals_are_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

#[rstest]
#[case(": add + ;", vec!["add"])]
fn defining_a_function_registers_its_name(#[case] program: &str, #[case] expected: Vec<&str>) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.function_names(), expected);
}

#[rstest]
// define a function, then call it — newline and space are interchangeable
#[case(": add + ;\n1 2 :add", "[3]")]
#[case(": add + ; 1 2 :add", "[3]")]
// a function body may call other functions
#[case(": double 2 * ; : quad :double :double ; 5 :quad", "[20]")]
fn calling_a_function_runs_its_body(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[test]
fn defining_a_function_leaves_the_stack_untouched() {
    // The whole point of the `: name ... ;` redesign: a definition is carved
    // out at compile time, so values already on the stack are never disturbed.
    let mut vm = Vm::new();
    vm.run("99").unwrap();
    vm.run(": double 2 * ;").unwrap();
    vm.run("21 :double").unwrap();
    assert_eq!(vm.stack_repr(), "[99 42]");
}

#[rstest]
#[case("1 2 ;")] // ';' with no opening ':'
#[case(": add +")] // ':' with no closing ';'
#[case(":")] // ':' with no name
fn malformed_definitions_are_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

#[test]
fn a_stack_slot_stays_small() {
    // The point of the memory model: a stack slot never grows past 16 bytes,
    // whatever kind of value it holds.
    assert!(std::mem::size_of::<plenty::Value>() <= 16);
}
