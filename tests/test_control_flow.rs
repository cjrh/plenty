//! Control-flow and tail-call tests — §11.8.
//!
//! `match` is Plenty's only branching primitive and there are no looping
//! primitives; iteration is recursion plus mandatory tail-call optimisation.
//! These tests pin the contract end-to-end: that the surface syntax compiles
//! and runs, that the type checker rejects misuse, that exhaustiveness is
//! enforced, that branch joins agree, and that deep tail recursion does not
//! grow the host call stack.
//!
//! Like `test_basic.rs`, every assertion is against observable behaviour
//! (`stack_repr`, error presence) rather than internal representation.

use plenty::Vm;
use rstest::rstest;

// --- Bool literals --------------------------------------------------------

#[rstest]
#[case("true", "[true]")]
#[case("false", "[false]")]
#[case("true false", "[true false]")]
fn bool_literals_push_a_bool(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[test]
fn a_function_can_return_a_bool_literal() {
    let mut vm = Vm::new();
    vm.run(r#": yes { -> Bool } "Always true." true ;"#).unwrap();
    vm.run(":yes").unwrap();
    assert_eq!(vm.stack_repr(), "[true]");
}

// --- Comparison operators -------------------------------------------------

#[rstest]
#[case("1 1 =", "[true]")]
#[case("1 2 =", "[false]")]
#[case("0 0 =", "[true]")]
#[case("-5 -5 =", "[true]")]
fn integer_equality_pushes_bool(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
#[case("true true =", "[true]")]
#[case("true false =", "[false]")]
#[case("false false =", "[true]")]
fn bool_equality_pushes_bool(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
#[case(r#""hi" "hi" ="#, "[true]")]
#[case(r#""hi" "bye" ="#, "[false]")]
#[case(r#""" "" ="#, "[true]")]
fn string_equality_compares_contents(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
#[case("3 5 <", "[true]")]
#[case("5 3 <", "[false]")]
#[case("5 5 <", "[false]")]
#[case("5 3 >", "[true]")]
#[case("3 5 >", "[false]")]
#[case("5 5 >", "[false]")]
fn integer_inequality_pushes_bool(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[rstest]
#[case("true not", "[false]")]
#[case("false not", "[true]")]
fn not_negates_a_bool(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

// --- Type errors on comparisons -------------------------------------------

#[rstest]
// `=` requires same-typed operands; the checker rejects mixed pairs even
// though the runtime would too.
#[case("1 true =")]
#[case(r#"1 "x" ="#)]
#[case(r#"true "x" ="#)]
// `<` and `>` are integer-only.
#[case(r#""a" "b" <"#)]
#[case("true false <")]
#[case("1 true <")]
// `not` requires a Bool.
#[case("1 not")]
#[case(r#""x" not"#)]
// Underflow on each.
#[case("1 =")]
#[case("1 <")]
#[case("not")]
fn comparison_type_errors_are_caught_before_execution(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

// --- Match on Bool --------------------------------------------------------

#[test]
fn match_on_bool_dispatches_to_the_true_arm() {
    let mut vm = Vm::new();
    vm.run(
        r#"true match
             true  [ 1 ]
             false [ 0 ]
           end"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[1]");
}

#[test]
fn match_on_bool_dispatches_to_the_false_arm() {
    let mut vm = Vm::new();
    vm.run(
        r#"false match
             true  [ 1 ]
             false [ 0 ]
           end"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[0]");
}

#[test]
fn match_on_bool_accepts_a_wildcard_arm_for_exhaustiveness() {
    // Only one named arm + wildcard is exhaustive too.
    let mut vm = Vm::new();
    vm.run(
        r#"true match
             true [ 42 ]
             _    [ 0  ]
           end"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[42]");
}

#[rstest]
// Missing `false` arm and no wildcard.
#[case(r#"true match true [ 1 ] end"#)]
// Missing `true` arm and no wildcard.
#[case(r#"false match false [ 0 ] end"#)]
fn non_exhaustive_bool_match_is_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

// --- Match on Int ---------------------------------------------------------

#[rstest]
#[case("0 match  0 [ 99 ] _ [ 0 ] end", "[99]")]
#[case("1 match  0 [ 99 ] _ [ 0 ] end", "[0]")]
#[case("7 match  0 [ 99 ] 7 [ 77 ] _ [ 0 ] end", "[77]")]
fn match_on_int_dispatches_on_value(#[case] program: &str, #[case] expected: &str) {
    let mut vm = Vm::new();
    vm.run(program).unwrap();
    assert_eq!(vm.stack_repr(), expected);
}

#[test]
fn match_on_int_uses_first_matching_arm() {
    // Even with redundant patterns, the first matching arm wins.
    let mut vm = Vm::new();
    vm.run("5 match 5 [ 99 ] 5 [ 88 ] _ [ 0 ] end").unwrap();
    assert_eq!(vm.stack_repr(), "[99]");
}

#[test]
fn match_on_int_without_wildcard_is_rejected() {
    let mut vm = Vm::new();
    let err = vm.run("3 match 0 [ 99 ] 1 [ 88 ] end").unwrap_err();
    assert!(err.to_string().contains("non-exhaustive"));
}

#[rstest]
// Bool pattern against Int value.
#[case("1 match true [ 0 ] false [ 0 ] _ [ 0 ] end")]
// String pattern against Int value.
#[case(r#"1 match "hi" [ 0 ] _ [ 0 ] end"#)]
fn pattern_type_must_match_value_type(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

// --- Match on Str ---------------------------------------------------------

#[test]
fn match_on_str_dispatches_on_contents() {
    let mut vm = Vm::new();
    vm.run(
        r#""hello" match
             "hello" [ 1 ]
             _       [ 0 ]
           end"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[1]");
}

#[test]
fn match_on_str_falls_through_to_wildcard() {
    let mut vm = Vm::new();
    vm.run(
        r#""xyz" match
             "hello" [ 1 ]
             _       [ 0 ]
           end"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[0]");
}

#[test]
fn match_on_str_without_wildcard_is_rejected() {
    let mut vm = Vm::new();
    let err = vm
        .run(r#""hi" match "hi" [ 1 ] end"#)
        .unwrap_err();
    assert!(err.to_string().contains("non-exhaustive"));
}

// --- Match structure ------------------------------------------------------

#[test]
fn match_consumes_the_matched_value() {
    // The matched value is popped before the arm body runs: only `99` remains.
    let mut vm = Vm::new();
    vm.run("7 match 7 [ 99 ] _ [ 0 ] end").unwrap();
    assert_eq!(vm.stack_repr(), "[99]");
}

#[test]
fn arm_body_operates_on_the_surrounding_stack() {
    // The values 10 and 20 sit on the stack from before the match; the arm
    // body adds them. Match arms are not isolated sub-stacks (§11.8).
    let mut vm = Vm::new();
    vm.run(
        r#"10 20 true match
             true  [ + ]
             false [ * ]
           end"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[30]");
}

#[test]
fn arm_body_sees_function_locals() {
    let mut vm = Vm::new();
    vm.run(
        r#": pick { x Int y Int flag Bool -> Int }
             "Return x if flag is true, otherwise y."
             flag match
               true  [ x ]
               false [ y ]
             end ;
           1 2 true :pick
           3 4 false :pick"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[1 4]");
}

#[test]
fn empty_match_is_rejected() {
    let mut vm = Vm::new();
    assert!(vm.run("1 match end").is_err());
}

#[rstest]
// `match` with no `end`.
#[case("1 match _ [ 0 ]")]
// `match` with no open bracket on an arm.
#[case("1 match _ 0 end")]
// `match` with no close bracket on an arm.
#[case("1 match _ [ 0 end")]
// Pattern that isn't a literal or wildcard.
#[case("1 match foo [ 0 ] _ [ 0 ] end")]
// `]` with no matching `[`.
#[case("1 ]")]
// `[` outside any match.
#[case("[ 0 ]")]
// `end` outside any match.
#[case("end")]
fn malformed_match_syntax_is_rejected(#[case] program: &str) {
    let mut vm = Vm::new();
    assert!(vm.run(program).is_err());
}

// --- Branch joins ---------------------------------------------------------

#[test]
fn arms_that_disagree_on_stack_effect_are_rejected() {
    // True arm leaves one Int; false arm leaves nothing. The join requires
    // both arms to produce the same shape.
    let mut vm = Vm::new();
    let err = vm
        .run(
            r#"true match
                 true  [ 1 ]
                 false [ ]
               end"#,
        )
        .unwrap_err();
    assert!(err.to_string().contains("same stack effect"));
}

#[test]
fn arms_that_disagree_on_output_type_are_rejected() {
    let mut vm = Vm::new();
    let err = vm
        .run(
            r#"true match
                 true  [ 1 ]
                 false [ "no" ]
               end"#,
        )
        .unwrap_err();
    assert!(err.to_string().contains("same stack effect"));
}

#[test]
fn arms_may_take_different_paths_if_they_agree_at_the_end() {
    // Wildly different bodies are fine so long as they leave the same shape.
    let mut vm = Vm::new();
    vm.run(
        r#"true match
             true  [ 1 2 + ]
             false [ 10 7 - ]
           end"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[3]");
}

// --- Nested matches -------------------------------------------------------

#[test]
fn nested_match_dispatches_correctly() {
    let mut vm = Vm::new();
    vm.run(
        r#": classify { n Int -> Str }
             "Return zero/positive/negative for a sign classification."
             n 0 = match
               true  [ "zero" ]
               false [ n 0 > match
                         true  [ "positive" ]
                         false [ "negative" ]
                       end ]
             end ;
           -3 :classify
           0 :classify
           7 :classify"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), r#"["negative" "zero" "positive"]"#);
}

#[test]
fn nested_match_branch_join_works_independently_of_outer() {
    // The inner match has its own join check; an inner mismatch is a type
    // error even if the outer arms happen to be uniform.
    let mut vm = Vm::new();
    let err = vm
        .run(
            r#": bad { n Int -> Int }
                 "Inner arms disagree."
                 n 0 = match
                   true  [ 0 ]
                   false [ n 0 > match
                             true  [ 1 ]
                             false [ "no" ]
                           end ]
                 end ;"#,
        )
        .unwrap_err();
    assert!(err.to_string().contains("same stack effect"));
}

// --- Recursion + tail-call optimisation -----------------------------------

#[test]
fn simple_tail_recursion_works() {
    let mut vm = Vm::new();
    vm.run(
        r#": countdown { n Int -> Int }
             "Recurse down to zero, returning zero."
             n 0 = match
               true  [ n ]
               false [ n 1 - :countdown ]
             end ;
           10 :countdown"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[0]");
}

#[test]
fn tail_recursive_accumulator_works() {
    // Classic tail-recursive sum-to-n.
    let mut vm = Vm::new();
    vm.run(
        r#": sum-to { n Int acc Int -> Int }
             "Tail-recursive accumulator: 1+2+...+n + acc."
             n 0 = match
               true  [ acc ]
               false [ n 1 - acc n + :sum-to ]
             end ;
           100 0 :sum-to"#,
    )
    .unwrap();
    // 1+2+...+100 = 5050
    assert_eq!(vm.stack_repr(), "[5050]");
}

#[test]
fn deep_tail_recursion_does_not_overflow_the_call_stack() {
    // This is the load-bearing TCO test (§13 invariant). One million tail
    // calls would blow any reasonable Rust call stack if frames were stacked
    // on the host; under TCO the explicit frames vec stays at a small
    // constant size for the entire run.
    let mut vm = Vm::new();
    vm.run(
        r#": sum-to { n Int acc Int -> Int }
             "Tail-recursive accumulator over a deep recursion."
             n 0 = match
               true  [ acc ]
               false [ n 1 - acc n + :sum-to ]
             end ;
           1000000 0 :sum-to"#,
    )
    .unwrap();
    // 1 + 2 + ... + 1_000_000 = 500_000_500_000
    assert_eq!(vm.stack_repr(), "[500000500000]");
}

#[test]
fn mutual_tail_recursion_works() {
    // even? and odd? tail-call each other; both must be optimised for this
    // to run without growing the call stack.
    let mut vm = Vm::new();
    vm.run(
        r#": even? { n Int -> Bool }
             "True if n is even (mutually recursive with odd?)."
             n 0 = match
               true  [ true ]
               false [ n 1 - :odd? ]
             end ;
           : odd? { n Int -> Bool }
             "True if n is odd (mutually recursive with even?)."
             n 0 = match
               true  [ false ]
               false [ n 1 - :even? ]
             end ;
           100000 :even?"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[true]");
}

#[test]
fn non_tail_recursion_works_for_moderate_depth() {
    // Standard non-tail factorial — the multiplication after the recursive
    // call defeats tail-call detection. Frames stack on the explicit frames
    // vec (not the host stack), so even non-tail recursion is bounded by
    // heap rather than by the host ulimit, but we only test moderate depth
    // here to avoid huge integer outputs.
    let mut vm = Vm::new();
    vm.run(
        r#": fact { n Int -> Int }
             "Non-tail-recursive factorial."
             n 1 = match
               true  [ 1 ]
               false [ n n 1 - :fact * ]
             end ;
           10 :fact"#,
    )
    .unwrap();
    // 10! = 3628800
    assert_eq!(vm.stack_repr(), "[3628800]");
}

#[test]
fn tail_call_inside_outer_match_is_optimised() {
    // The recursive call sits two levels deep: inside an inner match arm,
    // which is inside an outer match arm, which is the last op of the body.
    // The compiler's tail-call detection must recurse through both matches.
    let mut vm = Vm::new();
    vm.run(
        r#": squeeze { n Int -> Int }
             "Recurse until n hits 0, branching via two nested matches."
             n 0 = match
               true  [ n ]
               false [ n 1 > match
                         true  [ n 1 - :squeeze ]
                         false [ n 1 - :squeeze ]
                       end ]
             end ;
           500000 :squeeze"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[0]");
}

// --- Integration with the rest of the language ---------------------------

#[test]
fn a_function_returning_bool_via_comparison_type_checks() {
    let mut vm = Vm::new();
    vm.run(
        r#": positive? { n Int -> Bool } "True if n > 0." n 0 > ;
           5 :positive?
           -1 :positive?"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[true false]");
}

#[test]
fn a_function_taking_a_bool_uses_it_in_match() {
    let mut vm = Vm::new();
    vm.run(
        r#": describe { flag Bool -> Str }
             "Render a Bool as text."
             flag match
               true  [ "yes" ]
               false [ "no" ]
             end ;
           true :describe
           false :describe"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), r#"["yes" "no"]"#);
}

#[test]
fn fibonacci_via_match_and_recursion() {
    // The cleanest non-tail recursive function in the test suite: each call
    // splits into two recursive sub-calls. fib(12) = 144.
    let mut vm = Vm::new();
    vm.run(
        r#": fib { n Int -> Int }
             "Fibonacci, demonstrating two-arm match plus double recursion."
             n 2 < match
               true  [ n ]
               false [ n 1 - :fib n 2 - :fib + ]
             end ;
           12 :fib"#,
    )
    .unwrap();
    assert_eq!(vm.stack_repr(), "[144]");
}

// --- Atomicity preserves invariants across failing match programs --------

#[test]
fn a_type_failing_match_does_not_register_its_definition() {
    let mut vm = Vm::new();
    vm.run("42").unwrap();
    let before = vm.stack_repr();
    let names_before = vm.function_names().len();

    // The body type-checks except that arms disagree; the whole `run` is
    // rejected pre-execution and the VM is left unchanged.
    let err = vm.run(
        r#": bad { n Int -> Int }
             "Arms disagree."
             n 0 = match
               true  [ 1 ]
               false [ "no" ]
             end ;"#,
    );
    assert!(err.is_err());
    assert_eq!(vm.stack_repr(), before);
    assert_eq!(vm.function_names().len(), names_before);
}
