//! AOT integration tests (DESIGN.md §11.1, §12.3 — phases c.1, c.2).
//!
//! Each test runs a Plenty program two ways — once through the
//! interpreter and once through the AOT pipeline (compile → link →
//! execute) — and asserts that stdout matches. Anything the current
//! AOT phase lowers correctly should produce identical output; that
//! is the strongest end-to-end check we can give the AOT path without
//! re-deriving expected output by hand.
//!
//! The tests are skipped automatically when a C compiler isn't on
//! `PATH`; CI environments without `cc` shouldn't break the build.

use std::path::PathBuf;
use std::process::Command;

fn plenty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_plenty")
}

fn runtime_c() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/plenty_runtime.c")
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

/// Write `source` to a tempfile, compile + link + run through the AOT
/// pipeline, and return the captured stdout. Panics with a useful
/// message on any failure — the test harness reports them as failures.
fn run_aot(source: &str, label: &str) -> String {
    let tmp = std::env::temp_dir();
    let n = nonce();
    let src_path = tmp.join(format!("plenty-aot-{label}-{n}.plenty"));
    let obj_path = tmp.join(format!("plenty-aot-{label}-{n}.o"));
    let exe_path = tmp.join(format!("plenty-aot-{label}-{n}.exe"));
    std::fs::write(&src_path, source).expect("write source");

    let compile = Command::new(plenty_bin())
        .args(["--compile"])
        .arg(&src_path)
        .args(["-o"])
        .arg(&obj_path)
        .output()
        .expect("spawn plenty --compile");
    assert!(
        compile.status.success(),
        "compile failed: stderr {:?}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let link = Command::new("cc")
        .arg(&obj_path)
        .arg(runtime_c())
        .arg("-o")
        .arg(&exe_path)
        .output()
        .expect("spawn cc");
    assert!(
        link.status.success(),
        "cc link failed: stderr {:?}",
        String::from_utf8_lossy(&link.stderr)
    );

    let run = Command::new(&exe_path).output().expect("run aot binary");
    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&obj_path);
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

#[test]
fn unsupported_op_emits_a_helpful_error() {
    // c.2 still rejects string literals (and `match` and `:listdir`).
    // The error must name a later phase so users know it's intentional.
    let tmp = std::env::temp_dir();
    let n = nonce();
    let src = tmp.join(format!("plenty-aot-unsup-{n}.plenty"));
    let obj = tmp.join(format!("plenty-aot-unsup-{n}.o"));
    std::fs::write(&src, "\"hello\" .\n").unwrap();
    let out = Command::new(plenty_bin())
        .args(["--compile"])
        .arg(&src)
        .args(["-o"])
        .arg(&obj)
        .output()
        .expect("spawn");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&obj);
    assert!(!out.status.success(), "strings should be rejected before c.4");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not yet support"),
        "error should explain the limitation; got {stderr:?}"
    );
}
