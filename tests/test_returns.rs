//! Early exits are checked before native code generation.
mod support;
use rstest::rstest;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use support::{check_source, compile_source_to_executable};

/// Exercise return values and caller-frame cleanup in native code.
fn assert_result(source: &str, expression: &str, printed: &str) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "plenty-returns-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let source = format!("{source}\nprint({expression})\n");
    compile_source_to_executable(&source, &path).unwrap_or_else(|e| panic!("{source}\n{e}"));
    let result = Command::new(&path).output();
    let _ = std::fs::remove_file(&path);
    let result = result.unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), printed);
}

#[rstest]
#[case("f(0)", "42\n")]
#[case("f(2)", "12\n")]
#[case("f(-2)", "8\n")]
fn guard_return_and_implicit_tail(#[case] expression: &str, #[case] printed: &str) {
    assert_result(
        "def f(n: i64) -> i64:\n    if n == 0:\n        return 42\n    n + 10",
        expression,
        printed,
    );
}

#[rstest]
#[case("f(0)", "42\n")]
#[case("f(1)", "43\n")]
#[case("f(2)", "44\n")]
#[case("f(3)", "45\n")]
fn nested_and_sibling_return_branches(#[case] expression: &str, #[case] printed: &str) {
    assert_result("def f(n: i64) -> i64:\n    if n < 2:\n        if n == 0:\n            local = 42\n            return local\n        return 43\n    elif n == 2:\n        return 44\n    else:\n        return 45", expression, printed);
}

#[rstest]
#[case("f(True)", "42\n")]
#[case("f(False)", "10\n")]
fn mutation_survives_only_on_continuing_path(#[case] expression: &str, #[case] printed: &str) {
    assert_result("def f(flag: bool) -> i64:\n    mut x = 1\n    if flag:\n        x = 40\n    else:\n        return x + 9\n    return x + 2", expression, printed);
}

#[rstest]
#[case("f(True)", "1\n")]
#[case("f(False)", "2\n")]
fn final_branch_can_return_or_produce_a_value(#[case] expression: &str, #[case] printed: &str) {
    assert_result(
        "def f(flag: bool) -> i64:\n    if flag:\n        return 1\n    else:\n        2",
        expression,
        printed,
    );
}

#[rstest]
#[case("f(True)", "42\n")]
#[case("f(False)", "12\n")]
fn discarded_branch_value_does_not_join_with_a_return(
    #[case] expression: &str,
    #[case] printed: &str,
) {
    assert_result("def f(flag: bool) -> i64:\n    if flag:\n        return 42\n    else:\n        'discarded'\n    return 12", expression, printed);
}

#[test]
fn nested_calls_preserve_caller_operands_and_locals() {
    assert_result("def early(n: i64) -> i64:\n    if n > 0:\n        return n + 1\n    0\ndef caller(n: i64) -> i64:\n    local = 100\n    n + early(1) * early(2) + local", "caller(7)", "113\n");
}

#[test]
fn explicit_return_operands_evaluate_once_in_order() {
    assert_result("def value(n: i64) -> i64:\n    print(n)\n    return n\ndef caller(flag: bool) -> i64:\n    if flag:\n        return value(1) + value(2)\n    return 0", "caller(True)", "1\n2\n3\n");
}

#[test]
fn returned_strings_survive_frame_cleanup() {
    assert_result("def text(flag: bool) -> str:\n    if flag:\n        local = 'hel' + 'lo'\n        return local\n    return 'world'", "text(True) + ' ' + text(False)", "hello world\n");
}

#[test]
fn return_skips_runtime_errors_on_later_paths() {
    assert_result(
        "def safe(flag: bool) -> i64:\n    if flag:\n        return 42\n    return 1 // 0",
        "safe(True)",
        "42\n",
    );
}

#[test]
fn unit_guard_returns_to_the_caller() {
    assert_result("def stop(flag: bool) -> ():\n    if flag:\n        return\n    1 // 0\n    ()\ndef caller() -> i64:\n    stop(True)\n    42", "caller()", "42\n");
}

#[test]
fn explicit_early_tail_calls_use_bounded_stack_space() {
    assert_result("def count(n: i64, total: i64) -> i64:\n    if n > 0:\n        return count(n - 1, total + 1)\n    return total", "count(100_000, 0)", "100000\n");
}

#[test]
fn returned_conditional_preserves_mutual_tail_calls() {
    assert_result("def even(n: i64) -> bool:\n    return True if n == 0 else odd(n - 1)\ndef odd(n: i64) -> bool:\n    return False if n == 0 else even(n - 1)", "even(100_000)", "True\n");
}

#[test]
fn unit_early_tail_calls_use_bounded_stack_space() {
    assert_result("def stop(n: i64) -> ():\n    if n > 0:\n        return stop(n - 1)\n    return\ndef caller() -> i64:\n    stop(100_000)\n    42", "caller()", "42\n");
}

#[test]
fn short_circuit_return_terminates_each_path() {
    assert_result(
        "def yes() -> bool:\n    return True\ndef f(flag: bool) -> bool:\n    return flag or yes()",
        "f(False) and f(True)",
        "True\n",
    );
}

#[rstest]
#[case("def f() -> i64:\n    return", "expected i64, got ()")]
#[case("def f() -> ():\n    return 42", "expected (), got i64")]
#[case(
    "def f() -> i64:\n    if False:\n        return 'wrong'\n    42",
    "expected i64, got str"
)]
#[case(
    "def f(flag: bool) -> i64:\n    if flag:\n        return 42",
    "expected i64, got ()"
)]
#[case(
    "def f(flag: bool) -> i64:\n    if flag:\n        return 42\n    else:\n        False",
    "expected i64, got bool"
)]
#[case(
    "def f() -> i64:\n    return 42\n    print('dead')",
    "3:5: unreachable statement"
)]
#[case(
    "def f(flag: bool) -> i64:\n    if flag:\n        return 1\n    else:\n        return 2\n    3",
    "6:5: unreachable statement"
)]
#[case(
    "def f() -> ():\n    if True:\n        return\n        pass",
    "4:9: unreachable statement"
)]
#[case(
    "def main() -> ():\n    if True:\n        return 1",
    "expected (), got i64"
)]
fn invalid_returns_are_rejected_before_execution(#[case] source: &str, #[case] expected: &str) {
    let error = check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error:?}"
    );
}
