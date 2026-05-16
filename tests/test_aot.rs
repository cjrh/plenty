//! AOT integration tests (DESIGN.md §11.1, §12.3 — phases c.1–c.5).
//!
//! Each test runs a Plenty program two ways — once through the
//! interpreter and once through the AOT pipeline (`plenty --compile`
//! then execute) — and asserts that stdout matches. Anything the AOT
//! path lowers correctly should produce identical output; that is the
//! strongest end-to-end check we can give the AOT path without
//! re-deriving expected output by hand.
//!
//! The tests are skipped automatically when a C compiler isn't on
//! `PATH`; CI environments without `cc` shouldn't break the build.

use std::process::Command;

fn plenty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_plenty")
}

fn cc_available() -> bool {
    Command::new("cc").arg("--version").output().is_ok()
}

fn nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
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

/// Run `source` through the interpreter (via the binary) and return
/// stdout. Lets us compare to AOT without re-deriving expected output.
fn run_interpreter(source: &str, label: &str) -> String {
    let path = std::env::temp_dir().join(format!("plenty-interp-{label}-{}.plenty", nonce()));
    std::fs::write(&path, source).expect("write source");
    let out = Command::new(plenty_bin()).arg(&path).output().expect("spawn plenty");
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "interpreter failed: stderr {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("interpreter stdout is utf-8")
}

/// One execution result captured from either backend. Bundles exit
/// code and stderr so the failure-parity macro can compare both
/// without re-running the program.
struct Outcome {
    code: i32,
    stderr: String,
}

/// Run the interpreter on `source` regardless of whether it succeeds
/// or fails. Returns the captured outcome so the failure-parity
/// macro can compare it to AOT.
fn run_interpreter_outcome(source: &str, label: &str) -> Outcome {
    let path = std::env::temp_dir().join(format!("plenty-interp-fail-{label}-{}.plenty", nonce()));
    std::fs::write(&path, source).expect("write source");
    let out = Command::new(plenty_bin()).arg(&path).output().expect("spawn plenty");
    let _ = std::fs::remove_file(&path);
    Outcome {
        code: out.status.code().unwrap_or(-1),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
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

macro_rules! aot_matches_interpreter {
    ($name:ident, $label:expr, $source:expr $(,)?) => {
        #[test]
        fn $name() {
            if !cc_available() {
                eprintln!("skipping {}: no `cc` on PATH", stringify!($name));
                return;
            }
            let source = $source;
            let interp = run_interpreter(source, $label);
            let aot = run_aot(source, $label);
            assert_eq!(
                aot, interp,
                "AOT output disagrees with interpreter for:\n{source}"
            );
        }
    };
}

/// Generate a `#[test]` that runs `source` through both backends and
/// asserts that both fail with matching exit code and stderr. Used
/// to verify the AOT trap path agrees with the interpreter's
/// `checked_*` errors (integer overflow, division by zero).
macro_rules! aot_failure_matches_interpreter {
    ($name:ident, $label:expr, $source:expr $(,)?) => {
        #[test]
        fn $name() {
            if !cc_available() {
                eprintln!("skipping {}: no `cc` on PATH", stringify!($name));
                return;
            }
            let source = $source;
            let interp = run_interpreter_outcome(source, $label);
            let aot = run_aot_outcome(source, $label);
            assert!(
                interp.code != 0,
                "expected interpreter to fail for:\n{source}\nexit={} stderr={:?}",
                interp.code,
                interp.stderr
            );
            assert_eq!(
                aot.code, interp.code,
                "exit code disagrees for:\n{source}"
            );
            assert_eq!(
                aot.stderr, interp.stderr,
                "stderr disagrees for:\n{source}"
            );
        }
    };
}

aot_matches_interpreter!(arithmetic, "arith", "1 2 + .\n10 3 - .\n4 5 * .\n");
aot_matches_interpreter!(
    casts_widen_and_narrow,
    "casts",
    "127 :as-i8 .\n-1 :as-i8 :as-u8 .\n300 :as-u8 .\n",
);
aot_matches_interpreter!(
    sized_arithmetic_at_target_width,
    "sized",
    "100 :as-u8 50 :as-u8 + .\n10 :as-i32 3 :as-i32 / .\n",
);
aot_matches_interpreter!(
    comparisons_and_booleans,
    "cmp",
    "1 2 < .\n5 5 = .\ntrue false = .\ntrue not .\n",
);
aot_matches_interpreter!(
    multi_value_stack_renders_with_spaces,
    "multistack",
    "1 2 3 4 .\n",
);
aot_matches_interpreter!(
    clear_empties_the_stack,
    "clear",
    "1 2 3 :clear .\n",
);
aot_matches_interpreter!(
    unsigned_comparison_uses_unsigned_predicate,
    "ucmp",
    // -1 as u8 = 255; 1 as u8 = 1. Signed compare would say 255 < 1 (since
    // bit pattern of 255 is -1 in two's complement); unsigned compare says
    // 255 > 1. The AOT path must pick `icmp ult`/`ugt` for u8 to agree.
    "-1 :as-u8 1 :as-u8 < .\n-1 :as-u8 1 :as-u8 > .\n",
);

// --- c.2: functions, calls, locals, tail calls ---------------------------

aot_matches_interpreter!(
    single_arg_function,
    "fn-single",
    r#": double { x i64 -> i64 } "Double an int." x 2 * ;
       5 :double ."#,
);

aot_matches_interpreter!(
    multi_arg_function,
    "fn-multi",
    r#": addk { a i64 b i64 -> i64 } "Add two ints." a b + ;
       3 4 :addk .
       10 -2 :addk ."#,
);

aot_matches_interpreter!(
    function_with_cast_in_body,
    "fn-cast",
    r#": clip { n i64 -> u8 } "Take low 8 bits as u8." n :as-u8 ;
       300 :clip .
       -1 :clip ."#,
);

aot_matches_interpreter!(
    multi_return_function,
    "fn-multi-return",
    r#": split { x i64 -> i64 i64 } "Push x and x+1." x x 1 + ;
       5 :split ."#,
);

aot_matches_interpreter!(
    forward_reference_between_functions,
    "fn-forward",
    // `caller` is defined before `callee` and calls into it. The
    // two-pass codegen has to declare every function before emitting
    // any body, otherwise this would fail at link time.
    r#": caller { x i64 -> i64 } "Calls callee defined later." x :callee 10 + ;
       : callee { x i64 -> i64 } "Defined after caller." x 2 * ;
       3 :caller ."#,
);

aot_matches_interpreter!(
    chained_calls_use_tail_call,
    "fn-chain-tail",
    // Every call here sits at the end of its function's body and so is
    // emitted as `return_call`. The chain runs all the way through
    // without needing a base case (no `match` in c.2 yet — TCO under
    // recursion lands once c.3 adds branching).
    r#": a { x i64 -> i64 } "Add 1." x 1 + ;
       : b { x i64 -> i64 } "Chain to a." x :a ;
       : c { x i64 -> i64 } "Chain to b." x :b ;
       5 :c ."#,
);

aot_matches_interpreter!(
    non_tail_call_in_body,
    "fn-nontail",
    // `:f` is followed by `2 *`, so it is not in tail position and
    // lowers to a regular `call`. The body must still return cleanly
    // with the doubled result on the compile-time stack.
    r#": f { x i64 -> i64 } "Add one." x 1 + ;
       : g { x i64 -> i64 } "Call f, then double." x :f 2 * ;
       7 :g ."#,
);

aot_matches_interpreter!(
    nested_function_definition,
    "fn-nested",
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

aot_matches_interpreter!(
    match_on_bool_dispatches_to_true_arm,
    "match-bool-true",
    r#"true match
         true  [ 1 ]
         false [ 0 ]
       end ."#,
);

aot_matches_interpreter!(
    match_on_bool_dispatches_to_false_arm,
    "match-bool-false",
    r#"false match
         true  [ 1 ]
         false [ 0 ]
       end ."#,
);

aot_matches_interpreter!(
    match_on_int_with_wildcard,
    "match-int-wild",
    r#"0 match 0 [ 99 ] _ [ 0 ] end .
       1 match 0 [ 99 ] _ [ 0 ] end .
       7 match 0 [ 99 ] 7 [ 77 ] _ [ 0 ] end ."#,
);

aot_matches_interpreter!(
    match_arm_first_match_wins,
    "match-first-wins",
    "5 match 5 [ 99 ] 5 [ 88 ] _ [ 0 ] end .",
);

aot_matches_interpreter!(
    match_arm_operates_on_surrounding_stack,
    "match-surrounding",
    // The values 10 and 20 sit on the stack from before the match; the
    // arm body adds them. Arms are not isolated sub-stacks (§11.8) —
    // they share the data stack with the enclosing context.
    r#"10 20 true match
         true  [ + ]
         false [ * ]
       end ."#,
);

aot_matches_interpreter!(
    match_arm_reads_function_locals,
    "match-locals",
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

aot_matches_interpreter!(
    nested_match_dispatches_correctly,
    "match-nested",
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

aot_matches_interpreter!(
    simple_tail_recursion,
    "match-tail-simple",
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

aot_matches_interpreter!(
    deep_tail_recursion_does_not_overflow,
    "match-deep-tco",
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

aot_matches_interpreter!(
    mutual_tail_recursion,
    "match-mutual",
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

aot_matches_interpreter!(
    non_tail_recursive_fibonacci,
    "match-fib",
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

aot_matches_interpreter!(
    match_at_top_level,
    "match-toplevel",
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

aot_matches_interpreter!(
    string_literal_prints,
    "str-lit",
    r#""hello" ."#,
);

aot_matches_interpreter!(
    string_concat,
    "str-concat",
    r#""hello" "world" + ."#,
);

aot_matches_interpreter!(
    string_concat_three_ways,
    "str-concat-3",
    // Multiple concatenations in a row; each call allocates a fresh
    // buffer in the runtime's heap (malloc, never freed — same
    // policy as the interpreter's append-only Heap).
    r#""a" "b" + "c" + .
       "" "x" + .
       "x" "" + ."#,
);

aot_matches_interpreter!(
    string_equality,
    "str-eq",
    r#""hi" "hi" = .
       "hi" "bye" = .
       "" "" = ."#,
);

aot_matches_interpreter!(
    match_on_str_with_wildcard,
    "match-str-wild",
    r#""hello" match
         "hello" [ 1 ]
         _       [ 0 ]
       end .
       "xyz" match
         "hello" [ 1 ]
         _       [ 0 ]
       end ."#,
);

aot_matches_interpreter!(
    function_with_str_input_and_output,
    "fn-str-io",
    // Strings flow through function parameters and returns: the CLIF
    // signature uses the pointer type for each Str slot.
    r#": greet { who Str -> Str } "Build a greeting." "hello " who + ;
       "world" :greet .
       "there" :greet ."#,
);

aot_matches_interpreter!(
    same_literal_used_twice,
    "str-share",
    // Two `PushStr` ops with the same `StrId` should share a single
    // data symbol (collect_str_ids dedups), so the resulting compares
    // are pointer-distinct but content-equal — and the runtime
    // `plenty_str_eq` works on either.
    r#""x" "x" = .
       "x" "x" + ."#,
);

aot_matches_interpreter!(
    classify_via_str_match,
    "str-classify",
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

aot_matches_interpreter!(
    describe_bool_returns_str,
    "str-describe",
    r#": describe { flag Bool -> Str }
         "Render a Bool as text."
         flag match
           true  [ "yes" ]
           false [ "no" ]
         end ;
       true :describe .
       false :describe ."#,
);

// --- c.5.5: overflow / div-zero parity with the interpreter -------------
//
// The interpreter's arithmetic uses `checked_*` and errors with
// `"integer overflow"` or `"division by zero"`; `main.rs` prefixes
// those with `error: ` and exits 1. The AOT path now matches via
// `plenty_trap_overflow` / `plenty_trap_div_zero` in the runtime,
// reached through CLIF `*_overflow` instructions and explicit
// zero/INT_MIN checks for `Op::Div`. These tests assert the exit
// code and stderr line agree on both backends.

aot_failure_matches_interpreter!(
    i64_add_overflows_at_max,
    "trap-i64-add",
    "9223372036854775807 1 + .",
);

aot_failure_matches_interpreter!(
    i32_sub_overflows_at_min,
    "trap-i32-sub",
    "-2147483648 :as-i32 1 :as-i32 - .",
);

aot_failure_matches_interpreter!(
    i64_mul_overflows,
    "trap-i64-mul",
    "9223372036854775807 2 * .",
);

aot_failure_matches_interpreter!(
    u8_add_wraps_past_255,
    "trap-u8-add",
    // 200 + 100 = 300, doesn't fit in u8.
    "200 :as-u8 100 :as-u8 + .",
);

aot_failure_matches_interpreter!(
    u32_sub_underflows_below_zero,
    "trap-u32-sub",
    // 0u32 - 1 underflows; uint subtraction has no negative result.
    "0 :as-u32 1 :as-u32 - .",
);

aot_failure_matches_interpreter!(
    signed_div_by_zero,
    "trap-sdiv-zero",
    "10 0 / .",
);

aot_failure_matches_interpreter!(
    unsigned_div_by_zero,
    "trap-udiv-zero",
    "10 :as-u32 0 :as-u32 / .",
);

aot_failure_matches_interpreter!(
    signed_div_int_min_by_neg_one,
    "trap-sdiv-intmin",
    // The classic signed-division overflow: -INT_MIN is not
    // representable. Cranelift's `sdiv` would normally hardware-trap
    // here; the explicit pre-check routes this through the same
    // `"integer overflow"` message the interpreter emits.
    "-2147483648 :as-i32 -1 :as-i32 / .",
);
