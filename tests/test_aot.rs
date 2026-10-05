//! Native regression tests for the historical syntax and shared backend.
//! Expected output and diagnostics are specified independently of execution.

use std::process::Command;

fn plenty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_plenty")
}

// Collision-free across parallel test threads: `process::id()` distinguishes
// across processes, `COUNTER` across threads inside one process. Avoids the
// `SystemTime::now().as_nanos()` foot-gun where two tests can land on the
// same nanosecond under `cargo test --test-threads=N`.
fn nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Write `source` to a tempfile, compile to an executable via
/// `plenty --compile` (which embeds the runtime and invokes `cc`
/// internally), run it, and return the captured stdout. Panics with
/// a useful message on any failure — the test harness reports them
/// as failures.
fn run_aot(source: &str, label: &str) -> String {
    let tmp = std::env::temp_dir();
    let n = nonce();
    let src_path = tmp.join(format!("plenty-aot-{label}-{n}.plenty"));
    let exe_path = tmp.join(format!("plenty-aot-{label}-{n}.exe"));
    std::fs::write(&src_path, source).expect("write source");

    let compile = Command::new(plenty_bin())
        .arg("--legacy")
        .args(["--compile"])
        .arg(&src_path)
        .args(["-o"])
        .arg(&exe_path)
        .output()
        .expect("spawn plenty --compile");
    assert!(
        compile.status.success(),
        "compile failed: stderr {:?}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&exe_path).output().expect("run aot binary");
    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&exe_path);
    assert!(run.status.success(), "aot binary exited non-zero");
    String::from_utf8(run.stdout).expect("aot stdout is utf-8")
}

/// An executable's exit status and diagnostic.
struct Outcome {
    code: i32,
    stderr: String,
}

/// Compile `source` and run the AOT binary, capturing exit code and
/// stderr. Like `run_aot` but without the "must succeed" assertion.
fn run_aot_outcome(source: &str, label: &str) -> Outcome {
    let tmp = std::env::temp_dir();
    let n = nonce();
    let src_path = tmp.join(format!("plenty-aot-fail-{label}-{n}.plenty"));
    let exe_path = tmp.join(format!("plenty-aot-fail-{label}-{n}.exe"));
    std::fs::write(&src_path, source).expect("write source");

    let compile = Command::new(plenty_bin())
        .arg("--legacy")
        .args(["--compile"])
        .arg(&src_path)
        .args(["-o"])
        .arg(&exe_path)
        .output()
        .expect("spawn plenty --compile");
    assert!(
        compile.status.success(),
        "compile failed: stderr {:?}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&exe_path).output().expect("run aot binary");
    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&exe_path);
    Outcome {
        code: run.status.code().unwrap_or(-1),
        stderr: String::from_utf8_lossy(&run.stderr).into_owned(),
    }
}

macro_rules! native_output {
    ($name:ident, $label:expr, $expected:expr, $source:expr $(,)?) => {
        #[test]
        fn $name() {
            assert_eq!(run_aot($source, $label), $expected, "{}", $source);
        }
    };
}

macro_rules! native_error {
    ($name:ident, $label:expr, $source:expr $(,)?) => {
        #[test]
        fn $name() {
            let outcome = run_aot_outcome($source, $label);
            assert_eq!(outcome.code, 1);
            let message = if $label.ends_with("zero") {
                "error: division by zero\n"
            } else {
                "error: integer overflow\n"
            };
            assert_eq!(outcome.stderr, message);
        }
    };
}

native_output!(
    arithmetic,
    "arith",
    "[3i64]\n[3i64 7i64]\n[3i64 7i64 20i64]\n",
    "1 2 + .\n10 3 - .\n4 5 * .\n"
);
native_output!(
    casts_widen_and_narrow,
    "casts",
    "[127i8]\n[127i8 255u8]\n[127i8 255u8 44u8]\n",
    "127 :as-i8 .\n-1 :as-i8 :as-u8 .\n300 :as-u8 .\n",
);
native_output!(
    sized_arithmetic_at_target_width,
    "sized",
    "[150u8]\n[150u8 3i32]\n",
    "100 :as-u8 50 :as-u8 + .\n10 :as-i32 3 :as-i32 / .\n",
);
native_output!(
    comparisons_and_booleans,
    "cmp",
    "[true]\n[true true]\n[true true false]\n[true true false false]\n",
    "1 2 < .\n5 5 = .\ntrue false = .\ntrue not .\n",
);
native_output!(
    multi_value_stack_renders_with_spaces,
    "multistack",
    "[1i64 2i64 3i64 4i64]\n",
    "1 2 3 4 .\n",
);
native_output!(clear_empties_the_stack, "clear", "[]\n", "1 2 3 :clear .\n",);
native_output!(
    stack_words_are_polymorphic,
    "stack-words",
    "[true 1i64]\n",
    "1 true swap dup drop .\n",
);
native_output!(
    print_renders_one_value_without_a_newline,
    "print",
    "1i64\"x\"true",
    "1 :print \"x\" :print true :print\n",
);
native_output!(
    extra_comparisons_and_boolean_ops,
    "more-bools",
    "[true]\n[true true]\n[true true true]\n[true true true false]\n[true true true false true]\n",
    "1 2 != .\n1 2 <= .\n2 1 >= .\ntrue false and .\ntrue false or .\n",
);
native_output!(
    typed_integer_literals,
    "typed-lits",
    "[255u8]\n[255u8 -1i8]\n[255u8 -1i8 42i32]\n[255u8 -1i8 42i32 18446744073709551615u64]\n",
    "255u8 .\n-1i8 .\n42i32 .\n18446744073709551615u64 .\n",
);
native_output!(
    unsigned_comparison_uses_unsigned_predicate,
    "ucmp",
    "[false]\n[false true]\n",
    // -1 as u8 = 255; 1 as u8 = 1. Signed compare would say 255 < 1 (since
    // bit pattern of 255 is -1 in two's complement); unsigned compare says
    // 255 > 1. The AOT path must pick `icmp ult`/`ugt` for u8 to agree.
    "-1 :as-u8 1 :as-u8 < .\n-1 :as-u8 1 :as-u8 > .\n",
);

// --- c.2: functions, calls, locals, tail calls ---------------------------

native_output!(
    single_arg_function,
    "fn-single",
    "[10i64]\n",
    r#": double { x i64 -> i64 } "Double an int." x 2 * ;
       5 :double ."#,
);

native_output!(
    multi_arg_function,
    "fn-multi",
    "[7i64]\n[7i64 8i64]\n",
    r#": addk { a i64 b i64 -> i64 } "Add two ints." a b + ;
       3 4 :addk .
       10 -2 :addk ."#,
);

native_output!(
    function_with_cast_in_body,
    "fn-cast",
    "[44u8]\n[44u8 255u8]\n",
    r#": clip { n i64 -> u8 } "Take low 8 bits as u8." n :as-u8 ;
       300 :clip .
       -1 :clip ."#,
);

native_output!(
    multi_return_function,
    "fn-multi-return",
    "[5i64 6i64]\n",
    r#": split { x i64 -> i64 i64 } "Push x and x+1." x x 1 + ;
       5 :split ."#,
);

native_output!(
    forward_reference_between_functions,
    "fn-forward",
    "[16i64]\n",
    // `caller` is defined before `callee` and calls into it. The
    // two-pass codegen has to declare every function before emitting
    // any body, otherwise this would fail at link time.
    r#": caller { x i64 -> i64 } "Calls callee defined later." x :callee 10 + ;
       : callee { x i64 -> i64 } "Defined after caller." x 2 * ;
       3 :caller ."#,
);

native_output!(
    chained_calls_use_tail_call,
    "fn-chain-tail",
    "[6i64]\n",
    // Every call here sits at the end of its function's body and so is
    // emitted as `return_call`. The chain runs all the way through
    // without needing a base case (no `match` in c.2 yet — TCO under
    // recursion lands once c.3 adds branching).
    r#": a { x i64 -> i64 } "Add 1." x 1 + ;
       : b { x i64 -> i64 } "Chain to a." x :a ;
       : c { x i64 -> i64 } "Chain to b." x :b ;
       5 :c ."#,
);

native_output!(
    non_tail_call_in_body,
    "fn-nontail",
    "[16i64]\n",
    // `:f` is followed by `2 *`, so it is not in tail position and
    // lowers to a regular `call`. The body must still return cleanly
    // with the doubled result on the compile-time stack.
    r#": f { x i64 -> i64 } "Add one." x 1 + ;
       : g { x i64 -> i64 } "Call f, then double." x :f 2 * ;
       7 :g ."#,
);

native_output!(
    nested_function_definition,
    "fn-nested",
    "[5i64]\n",
    // A `:` inside a `: ... ;` body defines a nested function. AOT
    // mode hoists every nested definition into the same module-level
    // symbol table, so calls reach it from anywhere in the source.
    r#": outer { x i64 -> i64 }
         "Defines an inner helper and uses it."
         : inner { y i64 -> i64 } "Inner helper." y 1 + ;
         x :inner ;
       4 :outer ."#,
);

// --- c.3: match ----------------------------------------------------------

native_output!(
    match_on_bool_dispatches_to_true_arm,
    "match-bool-true",
    "[1i64]\n",
    r#"true match
         true  [ 1 ]
         false [ 0 ]
       end ."#,
);

native_output!(
    match_on_bool_dispatches_to_false_arm,
    "match-bool-false",
    "[0i64]\n",
    r#"false match
         true  [ 1 ]
         false [ 0 ]
       end ."#,
);

native_output!(
    match_on_int_with_wildcard,
    "match-int-wild",
    "[99i64]\n[99i64 0i64]\n[99i64 0i64 77i64]\n",
    r#"0 match 0 [ 99 ] _ [ 0 ] end .
       1 match 0 [ 99 ] _ [ 0 ] end .
       7 match 0 [ 99 ] 7 [ 77 ] _ [ 0 ] end ."#,
);

native_output!(
    match_arm_first_match_wins,
    "match-first-wins",
    "[99i64]\n",
    "5 match 5 [ 99 ] 5 [ 88 ] _ [ 0 ] end .",
);

native_output!(
    match_arm_operates_on_surrounding_stack,
    "match-surrounding",
    "[30i64]\n",
    // The values 10 and 20 sit on the stack from before the match; the
    // arm body adds them. Arms are not isolated sub-stacks (§11.8) —
    // they share the data stack with the enclosing context.
    r#"10 20 true match
         true  [ + ]
         false [ * ]
       end ."#,
);

native_output!(
    match_arm_reads_function_locals,
    "match-locals",
    "[1i64]\n[1i64 4i64]\n",
    // The arm bodies reference `x` and `y` — locals declared by the
    // enclosing function. Cranelift `Variable`s defined at function
    // entry are visible across blocks, so each arm block reads the
    // locals without any explicit threading.
    r#": pick { x i64 y i64 flag Bool -> i64 }
         "Return x if flag, else y."
         flag match
           true  [ x ]
           false [ y ]
         end ;
       1 2 true :pick .
       3 4 false :pick ."#,
);

native_output!(
    nested_match_dispatches_correctly,
    "match-nested",
    "[-1i64]\n[-1i64 0i64]\n[-1i64 0i64 1i64]\n",
    // Inner match drives the false branch of the outer match; arm
    // joins compose, since each match's join block becomes the
    // active block before the surrounding arm continues.
    r#": classify { n i64 -> i64 }
         "Return -1/0/1 by sign."
         n 0 = match
           true  [ 0 ]
           false [ n 0 > match
                     true  [ 1 ]
                     false [ 0 1 - ]
                   end ]
         end ;
       -3 :classify .
       0 :classify .
       7 :classify ."#,
);

native_output!(
    simple_tail_recursion,
    "match-tail-simple",
    "[0i64]\n",
    // Both load-bearing pieces — `match` as the base case and
    // `TailCall` in tail position — meeting for the first time. The
    // arm whose body ends in `:countdown` lowers to `return_call` and
    // never jumps to the match's join block.
    r#": countdown { n i64 -> i64 }
         "Recurse to zero."
         n 0 = match
           true  [ n ]
           false [ n 1 - :countdown ]
         end ;
       10 :countdown ."#,
);

native_output!(
    deep_tail_recursion_does_not_overflow,
    "match-deep-tco",
    "[500000500000i64]\n",
    // The TCO stress test: one million tail calls. A naive `call +
    // return` chain would blow the host C stack; `return_call` reuses
    // the caller's frame so the depth stays bounded.
    r#": sum-to { n i64 acc i64 -> i64 }
         "Tail-recursive accumulator: 1+2+...+n + acc."
         n 0 = match
           true  [ acc ]
           false [ n 1 - acc n + :sum-to ]
         end ;
       1000000 0 :sum-to ."#,
);

native_output!(
    mutual_tail_recursion,
    "match-mutual",
    "[true]\n",
    // Mutual TCO across two functions; both functions are `Tail`
    // convention and their tail calls into each other become
    // `return_call`s. Forward declaration (Pass 1) is essential: at
    // the point `even?`'s body is emitted, `odd?` must already be
    // declared.
    r#": even? { n i64 -> Bool }
         "True if n is even."
         n 0 = match
           true  [ true ]
           false [ n 1 - :odd? ]
         end ;
       : odd? { n i64 -> Bool }
         "True if n is odd."
         n 0 = match
           true  [ false ]
           false [ n 1 - :even? ]
         end ;
       100000 :even? ."#,
);

native_output!(
    non_tail_recursive_fibonacci,
    "match-fib",
    "[144i64]\n",
    // Non-tail recursion: each `:fib` is followed by `+` (or `2 -
    // :fib`), so neither call sits at the arm's tail. Both lower to
    // regular `call`s and stack up frames on the host C stack —
    // bounded by depth-12 fib, well within any host's ulimit.
    r#": fib { n i64 -> i64 }
         "Fibonacci via match + double recursion."
         n 2 < match
           true  [ n ]
           false [ n 1 - :fib n 2 - :fib + ]
         end ;
       12 :fib ."#,
);

native_output!(
    match_at_top_level,
    "match-toplevel",
    "[101i64]\n",
    // Top-level `match` is allowed; tail-call marking only runs inside
    // function bodies, so any top-level arm's last op stays a regular
    // op (or `Call` rather than `TailCall`). The join block falls
    // through to the rest of the program.
    r#"true match
         true  [ 1 ]
         false [ 0 ]
       end
       100 + ."#,
);

// --- c.4: strings + heap -------------------------------------------------

native_output!(
    string_literal_prints,
    "str-lit",
    "[\"hello\"]\n",
    r#""hello" ."#,
);

native_output!(
    string_concat,
    "str-concat",
    "[\"helloworld\"]\n",
    r#""hello" "world" + ."#,
);

native_output!(
    string_concat_three_ways,
    "str-concat-3",
    "[\"abc\"]\n[\"abc\" \"x\"]\n[\"abc\" \"x\" \"x\"]\n",
    // Multiple concatenations in a row; each call allocates a fresh
    // buffer in the runtime's heap (malloc, never freed — same
    // append-only allocation policy).
    r#""a" "b" + "c" + .
       "" "x" + .
       "x" "" + ."#,
);

native_output!(
    string_equality,
    "str-eq",
    "[true]\n[true false]\n[true false true]\n",
    r#""hi" "hi" = .
       "hi" "bye" = .
       "" "" = ."#,
);

native_output!(
    match_on_str_with_wildcard,
    "match-str-wild",
    "[1i64]\n[1i64 0i64]\n",
    r#""hello" match
         "hello" [ 1 ]
         _       [ 0 ]
       end .
       "xyz" match
         "hello" [ 1 ]
         _       [ 0 ]
       end ."#,
);

native_output!(
    function_with_str_input_and_output,
    "fn-str-io",
    "[\"hello world\"]\n[\"hello world\" \"hello there\"]\n",
    // Strings flow through function parameters and returns: the CLIF
    // signature uses the pointer type for each Str slot.
    r#": greet { who Str -> Str } "Build a greeting." "hello " who + ;
       "world" :greet .
       "there" :greet ."#,
);

native_output!(
    same_literal_used_twice,
    "str-share",
    "[true]\n[true \"xx\"]\n",
    // Two `PushStr` ops with the same `StrId` should share a single
    // data symbol (collect_str_ids dedups), so the resulting compares
    // are pointer-distinct but content-equal — and the runtime
    // `plenty_str_eq` works on either.
    r#""x" "x" = .
       "x" "x" + ."#,
);

native_output!(
    classify_via_str_match,
    "str-classify",
    "[\"negative\"]\n[\"negative\" \"zero\"]\n[\"negative\" \"zero\" \"positive\"]\n",
    // The fully-worked nested-control-flow example from
    // tests/test_control_flow.rs, ported through the AOT path.
    r#": classify { n i64 -> Str }
         "Return a sign label for n."
         n 0 = match
           true  [ "zero" ]
           false [ n 0 > match
                     true  [ "positive" ]
                     false [ "negative" ]
                   end ]
         end ;
       -3 :classify .
       0 :classify .
       7 :classify ."#,
);

native_output!(
    describe_bool_returns_str,
    "str-describe",
    "[\"yes\"]\n[\"yes\" \"no\"]\n",
    r#": describe { flag Bool -> Str }
         "Render a Bool as text."
         flag match
           true  [ "yes" ]
           false [ "no" ]
         end ;
       true :describe .
       false :describe ."#,
);

// Checked arithmetic errors.

native_error!(
    i64_add_overflows_at_max,
    "trap-i64-add",
    "9223372036854775807 1 + .",
);

native_error!(
    i32_sub_overflows_at_min,
    "trap-i32-sub",
    "-2147483648 :as-i32 1 :as-i32 - .",
);

native_error!(
    i64_mul_overflows,
    "trap-i64-mul",
    "9223372036854775807 2 * .",
);

native_error!(
    u8_add_wraps_past_255,
    "trap-u8-add",
    // 200 + 100 = 300, doesn't fit in u8.
    "200 :as-u8 100 :as-u8 + .",
);

native_error!(
    u32_sub_underflows_below_zero,
    "trap-u32-sub",
    // 0u32 - 1 underflows; uint subtraction has no negative result.
    "0 :as-u32 1 :as-u32 - .",
);

native_error!(signed_div_by_zero, "trap-sdiv-zero", "10 0 / .",);

native_error!(
    unsigned_div_by_zero,
    "trap-udiv-zero",
    "10 :as-u32 0 :as-u32 / .",
);

native_error!(
    signed_div_int_min_by_neg_one,
    "trap-sdiv-intmin",
    // The classic signed-division overflow: -INT_MIN is not
    // representable. Cranelift's `sdiv` would normally hardware-trap
    // here; the explicit pre-check routes this through the same
    // runtime diagnostic for integer overflow.
    "-2147483648 :as-i32 -1 :as-i32 / .",
);
