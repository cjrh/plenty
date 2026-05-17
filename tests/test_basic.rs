//! End-to-end tests: run Plenty source through the VM and check observable
//! results — never internal representation.

use plenty::Vm;
use rstest::rstest;

#[rstest]
#[case("1 2 + .", "[3i64]")]
#[case("3 4 + .", "[7i64]")]
#[case("5 5 * .", "[25i64]")]
#[case("10 2 - .", "[8i64]")]
#[case("1 2 3 4 5 6 +", "[1i64 2i64 3i64 4i64 11i64]")]
#[case("1 2 +\n3 +", "[6i64]")]
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
#[case(r#": add { a i64 b i64 -> i64 } "Sum two values." a b + ;"#, vec!["add"])]
fn defining_a_function_registers_its_name(#[case] program: &str, #[case] expected: Vec<&str>) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.function_names(), expected);
}

#[rstest]
#[case(
    "\
: add { a i64 b i64 -> i64 } \"Sum two ints.\" a b + ;
1 2 :add",
    "[3i64]"
)]
#[case(
    r#": add { a i64 b i64 -> i64 } "Sum two ints." a b + ; 1 2 :add"#,
    "[3i64]"
)]
// a function body may call other functions
#[case(
    "\
: double { x i64 -> i64 } \"Double an int.\" x 2 * ; \
: quad { x i64 -> i64 } \"Quadruple an int.\" x :double :double ; \
5 :quad",
    "[20i64]"
)]
fn calling_a_function_runs_its_body(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[test]
fn an_operator_alone_on_a_line_consumes_the_prior_lines_values() {
    // The REPL feel: state persists across `run` calls (DESIGN.md §8), and
    // the type checker's view of the stack persists with it. A line with
    // only `+` on it must operate on whatever was left behind, exactly as
    // if it appeared inline.
    let mut vm = Vm::new();
    vm.run("1").unwrap();
    vm.run("2").unwrap();
    vm.run("+").unwrap();
    assert_eq!(vm.stack_repr(), "[3i64]");
}

#[test]
fn cross_line_type_mismatches_are_still_caught_pre_execution() {
    // The other side of the coin: seeding the abstract stack from runtime
    // values must not weaken the checker. `1` then `hello` then `+` is
    // still a type error, caught before any op runs.
    let mut vm = Vm::new();
    vm.run("1").unwrap();
    vm.run("hello").unwrap();
    let before = vm.stack_repr();
    assert!(vm.run("+").is_err());
    assert_eq!(vm.stack_repr(), before, "failed check must leave the stack untouched");
}

#[test]
fn defining_a_function_leaves_the_stack_untouched() {
    // The whole point of the `: name ... ;` redesign: a definition is carved
    // out at compile time, so values already on the stack are never disturbed.
    let mut vm = Vm::new();
    vm.run("99").unwrap();
    vm.run(r#": double { x i64 -> i64 } "Double an int." x 2 * ;"#).unwrap();
    vm.run("21 :double").unwrap();
    assert_eq!(vm.stack_repr(), "[99i64 42i64]");
}

#[test]
fn function_doc_returns_the_captured_docstring() {
    let mut vm = Vm::new();
    vm.run(r#": double { x i64 -> i64 } "Double an integer." x 2 * ;"#)
        .unwrap();
    assert_eq!(vm.function_doc("double"), Some("Double an integer."));
    assert_eq!(vm.function_doc("nonexistent"), None);
}

#[test]
fn function_sig_returns_the_captured_signature() {
    use plenty::Ty;
    let mut vm = Vm::new();
    vm.run(r#": double { x i64 -> i64 } "Double an integer." x 2 * ;"#)
        .unwrap();
    let sig = vm.function_sig("double").expect("function should be defined");
    assert_eq!(sig.inputs, vec![("x".to_string(), Ty::I64)]);
    assert_eq!(sig.outputs, vec![Ty::I64]);
}

#[rstest]
#[case(r#": noop { -> } "Does nothing." ;"#)] // empty inputs and outputs
#[case(r#": zero { -> i64 } "Push zero." 0 ;"#)] // no inputs, one output
#[case(r#": consume { x i64 -> } "Discard an int." ;"#)] // input, no outputs
// named outputs — placeholder body that produces two Ints (no `mod` op yet).
#[case(
    r#": divmod { a i64 b i64 -> q i64 r i64 } "Quot and rem (placeholder)." a b / a b / ;"#
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
#[case(r#": add { a i64 b i64 -> i64 } "doc" +"#)] // ':' with no closing ';'
fn malformed_definitions_are_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

#[rstest]
#[case(": double 2 * ;")] // no header at all
#[case(r#": double "doc" 2 * ;"#)] // docstring where `{` expected
#[case(": double { a i64 2 * ;")] // missing `->`
#[case(r#": double { i64 -> i64 } "doc" ;"#)] // type word in input-name slot
#[case(r#": double { a Floob -> i64 } "doc" ;"#)] // unknown type
#[case(r#": double { a i64 -> Bogus } "doc" ;"#)] // unknown output type
fn malformed_type_headers_are_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

#[rstest]
#[case(": double { x i64 -> i64 } 2 * ;")] // valid header, no docstring
#[case(": double { x i64 -> i64 } ;")] // valid header, only ';' after
fn missing_docstring_is_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

#[rstest]
// One input loaded twice in the body.
#[case(
    r#": dbl { x i64 -> i64 } "Sum x with itself." x x + ; 5 :dbl"#,
    "[10i64]"
)]
// Multiple inputs, reordered and each loaded more than once.
#[case(
    r#": hyp { a i64 b i64 -> i64 } "a*a + b*b." a a * b b * + ; 3 4 :hyp"#,
    "[25i64]"
)]
// A non-commutative operator demonstrates declaration order: `inputs[0]` is
// the deeper value, so `a - b` evaluates with `a` as the minuend.
#[case(
    r#": diff { a i64 b i64 -> i64 } "Subtract b from a." a b - ; 10 3 :diff"#,
    "[7i64]"
)]
// A bare word that doesn't match a local still pushes as text (§11's
// existing fallback — locals just take precedence when the name matches).
#[case(
    r#": tag { n i64 -> Str } "Prefix n with a label." label ; 1 :tag"#,
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
        : id { v i64 -> i64 } "Identity, but goes through a call."  v ;
        : add-via-id { a i64 b i64 -> i64 }
            "a + b, with a nested :id call between the loads of a and b."
            a :id b + ;
        7 5 :add-via-id
        "#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[12i64]");
}

#[rstest]
// Body underflows: declares one i64 input but tries to add two values from
// an empty stack.
#[case(r#": bad { x i64 -> i64 } "Underflows." + ;"#)]
// Body produces the wrong type for the declared output.
#[case(r#": bad { x i64 -> Str } "Wrong output type." x ;"#)]
// Body leaves too many values on the stack.
#[case(r#": bad { -> i64 } "Leaves two ints." 1 2 ;"#)]
// Body leaves no value when an output is declared.
#[case(r#": bad { -> i64 } "Leaves nothing." ;"#)]
// `+` applied to mixed types is a type error, not a runtime error.
#[case(r#": bad { -> i64 } "Mixed-type +." 1 hello + ;"#)]
// `-` applied to a string is a type error.
#[case(r#": bad { s Str -> Str } "Subtract from a string." s 1 - ;"#)]
// Call to an undefined function — caught pre-execution.
#[case(r#": bad { -> i64 } "Calls nothing." :no-such-fn ;"#)]
// Call with wrong argument type.
#[case(r#": id { x i64 -> i64 } "Identity." x ;
          : bad { -> i64 } "Calls id with a Str." hello :id ;"#)]
// Top-level type error: `+` on mixed i64/Str.
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
        : caller { -> i64 } "Calls callee, defined below." :callee ;
        : callee { -> i64 } "Pushes 42." 42 ;
        :caller
        "#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[42i64]");
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
        : good { -> i64 } "Type-correct." 7 ;
        : bad { -> i64 } "Wrong output type." hello ;
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

#[rstest]
// Widening signed -> signed: sign-extend preserves value.
#[case("-1 :as-i32", "[-1i32]")]
// Widening signed -> larger signed.
#[case("127 :as-i8 :as-i64", "[127i64]")]
// Narrowing: truncate to low bits.
#[case("300 :as-u8", "[44u8]")]
// Same-width signedness change: reinterpret bit pattern.
#[case("-1 :as-i8 :as-u8", "[255u8]")]
// Round-trip through several widths.
#[case("-1 :as-i8 :as-u16 :as-i32", "[65535i32]")]
// Cast literal directly to unsigned and add at the target width.
#[case("100 :as-u8 100 :as-u8 +", "[200u8]")]
fn cast_words_convert_integers_to_the_target_width(
    #[case] program: &str,
    #[case] expected: &str,
) {
    let mut vm = Vm::new();
    vm.run(program).expect("cast program should type-check and run");
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
// u8 overflow: 200 + 100 wraps at 255.
#[case("200 :as-u8 100 :as-u8 +")]
// u8 underflow: 0 - 1.
#[case("0 :as-u8 1 :as-u8 -")]
// i8 overflow: 127 + 1.
#[case("127 :as-i8 1 :as-i8 +")]
// Division by zero is still caught after a cast.
#[case("1 :as-i32 0 :as-i32 /")]
fn checked_arithmetic_fires_at_the_target_width(#[case] program: &str) {
    let mut vm = Vm::new();
    let err = vm.run(program).expect_err("arithmetic should fail at the target width");
    assert!(!err.to_string().is_empty());
}

#[rstest]
// Mixed widths are a type error, not silent promotion.
#[case("1 :as-i32 2 :as-i64 +")]
#[case("100 :as-u8 100 :as-u16 +")]
// Comparison too: same-width only.
#[case("1 :as-i32 2 :as-i64 <")]
// `=` rejects mixed widths the same way (it requires same type).
#[case("1 :as-i32 1 :as-i64 =")]
// Casting a Str is not legal.
#[case(r#""hello" :as-i64"#)]
// Casting a Bool is not legal.
#[case("true :as-i8")]
fn mixed_widths_are_rejected_at_check_time(#[case] program: &str) {
    let mut vm = Vm::new();
    let err = vm.run(program).expect_err("checker should reject mixed widths or non-int casts");
    assert!(!err.to_string().is_empty());
}

#[test]
fn function_signatures_can_use_any_integer_width() {
    use plenty::Ty;
    let mut vm = Vm::new();
    vm.run(
        r#": narrow { x i32 -> i8 } "Truncate to i8." x :as-i8 ; 300 :as-i32 :narrow"#,
    )
    .unwrap();
    let sig = vm.function_sig("narrow").expect("function should be defined");
    assert_eq!(sig.inputs, vec![("x".to_string(), Ty::I32)]);
    assert_eq!(sig.outputs, vec![Ty::I8]);
    assert_eq!(vm.stack_repr(), "[44i8]");
}

#[test]
fn match_against_a_narrow_int_requires_in_range_patterns() {
    let mut vm = Vm::new();
    // 300 cannot fit in i8 — must be rejected at check time, not silently
    // narrowed to 44.
    let err = vm
        .run("100 :as-i8 match 300 [ 1 ] _ [ 0 ] end")
        .expect_err("pattern out of range should be rejected");
    assert!(err.to_string().contains("out of range"));
}

#[test]
fn a_stack_slot_stays_small() {
    // The point of the memory model: a stack slot never grows past 16 bytes,
    // whatever kind of value it holds.
    assert!(std::mem::size_of::<plenty::Value>() <= 16);
}
