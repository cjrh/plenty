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
#[case(r#": add { a Int b Int -> Int } "Sum two values." a b + ;"#, vec!["add"])]
fn defining_a_function_registers_its_name(#[case] program: &str, #[case] expected: Vec<&str>) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.function_names(), expected);
}

#[rstest]
#[case(
    "\
: add { a Int b Int -> Int } \"Sum two ints.\" a b + ;
1 2 :add",
    "[3]"
)]
#[case(
    r#": add { a Int b Int -> Int } "Sum two ints." a b + ; 1 2 :add"#,
    "[3]"
)]
// a function body may call other functions
#[case(
    "\
: double { x Int -> Int } \"Double an int.\" x 2 * ; \
: quad { x Int -> Int } \"Quadruple an int.\" x :double :double ; \
5 :quad",
    "[20]"
)]
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
    vm.run(r#": double { x Int -> Int } "Double an int." x 2 * ;"#).unwrap();
    vm.run("21 :double").unwrap();
    assert_eq!(vm.stack_repr(), "[99 42]");
}

#[test]
fn function_doc_returns_the_captured_docstring() {
    let mut vm = Vm::new();
    vm.run(r#": double { x Int -> Int } "Double an integer." x 2 * ;"#)
        .unwrap();
    assert_eq!(vm.function_doc("double"), Some("Double an integer."));
    assert_eq!(vm.function_doc("nonexistent"), None);
}

#[test]
fn function_sig_returns_the_captured_signature() {
    use plenty::Ty;
    let mut vm = Vm::new();
    vm.run(r#": double { x Int -> Int } "Double an integer." x 2 * ;"#)
        .unwrap();
    let sig = vm.function_sig("double").expect("function should be defined");
    assert_eq!(sig.inputs, vec![("x".to_string(), Ty::Int)]);
    assert_eq!(sig.outputs, vec![Ty::Int]);
}

#[rstest]
#[case(r#": noop { -> } "Does nothing." ;"#)] // empty inputs and outputs
#[case(r#": zero { -> Int } "Push zero." 0 ;"#)] // no inputs, one output
#[case(r#": consume { x Int -> } "Discard an int." ;"#)] // input, no outputs
// named outputs — placeholder body that produces two Ints (no `mod` op yet).
#[case(
    r#": divmod { a Int b Int -> q Int r Int } "Quot and rem (placeholder)." a b / a b / ;"#
)]
// Bool type — identity body, since there are no Bool literals or ops yet.
#[case(r#": flip { x Bool -> Bool } "Identity, until ops exist." x ;"#)]
#[case(r#": echo { s Str -> Str } "Identity for strings." s ;"#)] // Str type
fn well_formed_type_headers_are_accepted(#[case] program: &str) {
    let mut vm = Vm::new();
    vm.run(program).expect("header should parse and body should type-check");
}

#[rstest]
#[case("1 2 ;")] // ';' with no opening ':'
#[case(":")] // ':' with no name
#[case(r#": add { a Int b Int -> Int } "doc" +"#)] // ':' with no closing ';'
fn malformed_definitions_are_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

#[rstest]
#[case(": double 2 * ;")] // no header at all
#[case(r#": double "doc" 2 * ;"#)] // docstring where `{` expected
#[case(": double { a Int 2 * ;")] // missing `->`
#[case(r#": double { Int -> Int } "doc" ;"#)] // type word in input-name slot
#[case(r#": double { a Floob -> Int } "doc" ;"#)] // unknown type
#[case(r#": double { a Int -> Bogus } "doc" ;"#)] // unknown output type
fn malformed_type_headers_are_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

#[rstest]
#[case(": double { x Int -> Int } 2 * ;")] // valid header, no docstring
#[case(": double { x Int -> Int } ;")] // valid header, only ';' after
fn missing_docstring_is_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

#[rstest]
// One input loaded twice in the body.
#[case(
    r#": dbl { x Int -> Int } "Sum x with itself." x x + ; 5 :dbl"#,
    "[10]"
)]
// Multiple inputs, reordered and each loaded more than once.
#[case(
    r#": hyp { a Int b Int -> Int } "a*a + b*b." a a * b b * + ; 3 4 :hyp"#,
    "[25]"
)]
// A non-commutative operator demonstrates declaration order: `inputs[0]` is
// the deeper value, so `a - b` evaluates with `a` as the minuend.
#[case(
    r#": diff { a Int b Int -> Int } "Subtract b from a." a b - ; 10 3 :diff"#,
    "[7]"
)]
// A bare word that doesn't match a local still pushes as text (§11's
// existing fallback — locals just take precedence when the name matches).
#[case(
    r#": tag { n Int -> Str } "Prefix n with a label." label ; 1 :tag"#,
    r#"["label"]"#
)]
fn local_names_resolve_to_call_inputs(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[test]
fn nested_calls_use_independent_locals_frames() {
    // The outer call's `a` must survive an inner call's full frame setup and
    // teardown — otherwise the second load of `a` would see the wrong value.
    let mut vm = Vm::new();
    vm.run(
        r#"
        : id { v Int -> Int } "Identity, but goes through a call."  v ;
        : add-via-id { a Int b Int -> Int }
            "a + b, with a nested :id call between the loads of a and b."
            a :id b + ;
        7 5 :add-via-id
        "#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[12]");
}

#[rstest]
// Body underflows: declares one Int input but tries to add two values from
// an empty stack.
#[case(r#": bad { x Int -> Int } "Underflows." + ;"#)]
// Body produces the wrong type for the declared output.
#[case(r#": bad { x Int -> Str } "Wrong output type." x ;"#)]
// Body leaves too many values on the stack.
#[case(r#": bad { -> Int } "Leaves two ints." 1 2 ;"#)]
// Body leaves no value when an output is declared.
#[case(r#": bad { -> Int } "Leaves nothing." ;"#)]
// `+` applied to mixed types is a type error, not a runtime error.
#[case(r#": bad { -> Int } "Mixed-type +." 1 hello + ;"#)]
// `-` applied to a string is a type error.
#[case(r#": bad { s Str -> Str } "Subtract from a string." s 1 - ;"#)]
// Call to an undefined function — caught pre-execution.
#[case(r#": bad { -> Int } "Calls nothing." :no-such-fn ;"#)]
// Call with wrong argument type.
#[case(r#": id { x Int -> Int } "Identity." x ;
          : bad { -> Int } "Calls id with a Str." hello :id ;"#)]
// Top-level type error: `+` on mixed Int/Str.
#[case(r#"1 hello +"#)]
// Top-level type error: `*` on a Str.
#[case(r#"hello 2 *"#)]
fn type_errors_are_caught_before_execution(#[case] program: &str) {
    let mut vm = Vm::new();
    let err = vm.run(program).expect_err("checker should reject this program");
    // Smoke check that we got something usable — not a panic, not a runtime
    // surprise. Specific wording is tested elsewhere; here we only care
    // that the type pass rejected the source.
    assert!(!err.to_string().is_empty());
}

#[test]
fn forward_reference_within_one_source_type_checks() {
    // `caller` is defined before `callee` in source order, but the checker
    // sees both before any op runs and resolves the forward reference.
    let mut vm = Vm::new();
    vm.run(
        r#"
        : caller { -> Int } "Calls callee, defined below." :callee ;
        : callee { -> Int } "Pushes 42." 42 ;
        :caller
        "#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[42]");
}

#[test]
fn type_check_failure_leaves_the_vm_unchanged() {
    // Atomicity: a check failure must not partially register definitions
    // or push partially-evaluated values onto the stack.
    let mut vm = Vm::new();
    vm.run("99").unwrap();
    let before = vm.stack_repr();
    let names_before = vm.function_names().len();

    let err = vm.run(
        r#"
        : good { -> Int } "Type-correct." 7 ;
        : bad { -> Int } "Wrong output type." hello ;
        "#,
    );
    assert!(err.is_err(), "checker should reject the bad function");
    assert_eq!(vm.stack_repr(), before, "stack must not change on a check failure");
    assert_eq!(
        vm.function_names().len(),
        names_before,
        "no definition (even the type-correct one) should register when any sibling fails check",
    );
}

#[test]
fn a_stack_slot_stays_small() {
    // The point of the memory model: a stack slot never grows past 16 bytes,
    // whatever kind of value it holds.
    assert!(std::mem::size_of::<plenty::Value>() <= 16);
}
