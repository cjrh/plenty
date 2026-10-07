//! Native loop exits, SSA joins, scope, and source diagnostics.
mod support;
use rstest::rstest;
use std::process::Command;

fn run(source: &str) -> std::process::Output {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    Command::new(executable).output().unwrap()
}

#[rstest]
#[case("mut n = 0\nwhile n < 5:\n    n = n + 1\nprint(n).unwrap()", "5\n")]
#[case("mut n = 0\nwhile False:\n    n = 1 // 0\nprint(n).unwrap()", "0\n")]
#[case(
    "mut n = 0\nwhile n < 10:\n    n = n + 1\n    if n == 3:\n        break\nprint(n).unwrap()",
    "3\n"
)]
#[case("mut n = 0\nmut sum = 0\nwhile n < 6:\n    n = n + 1\n    if n % 2 == 0:\n        continue\n    sum = sum + n\nprint(sum).unwrap()", "9\n")]
#[case("mut visits = 0\nfor n in range(5).unwrap():\n    visits = visits + 1\n    if visits > 10:\n        break\n    continue\nprint(visits).unwrap()", "5\n")]
#[case(
    "for n in range(5).unwrap():\n    if n == 2:\n        break\n    print(n).unwrap()",
    "0\n1\n"
)]
#[case("mut sum = 0\nfor n in [1, 2, 3, 4].unwrap():\n    if n % 2 == 0:\n        continue\n    sum = sum + n\nprint(sum).unwrap()", "4\n")]
#[case("mut visits = 0\nfor n in range(4).unwrap():\n    visits = visits + 1\n    if n < 2 and visits < 10:\n        continue\n    else:\n        break\nprint(visits).unwrap()", "3\n")]
#[case("mut total = 0\nfor outer in range(3).unwrap():\n    for inner in range(5).unwrap():\n        if inner == 2:\n            break\n        total = total + 1\nprint(total).unwrap()", "6\n")]
#[case("mut total = 0\nfor outer in range(3).unwrap():\n    mut inner = 0\n    while inner < 4:\n        inner = inner + 1\n        if inner == 2:\n            continue\n        if inner == 4:\n            break\n        total = total + 1\nprint(total).unwrap()", "6\n")]
#[case("mut outer = 0\nmut visits = 0\nwhile outer < 3:\n    outer = outer + 1\n    for inner in range(4).unwrap():\n        if inner == 1:\n            break\n        visits = visits + 1\n    continue\nprint(visits).unwrap()", "3\n")]
#[case("mut n = 0\nwhile n < 3:\n    n = n + 1\n    while True:\n        break\n    if n == 2:\n        continue\n    print(n).unwrap()", "1\n3\n")]
#[case("def limit(n: i64) -> bool:\n    print(n).unwrap()\n    n < 3\nmut n = 0\nwhile limit(n):\n    n = n + 1\nprint(n).unwrap()", "0\n1\n2\n3\n3\n")]
#[case("def condition() -> bool:\n    print('test').unwrap()\n    True\nwhile condition():\n    break\nprint('done').unwrap()", "test\ndone\n")]
#[case(
    "mut n = 0\nwhile n < 5:\n    n = n + 1\n    if n < 3:\n        continue\n    break\nprint(n).unwrap()",
    "3\n"
)]
#[case(
    "mut n = 0\nwhile n < 4:\n    local = n + 1\n    n = local\nprint(n).unwrap()",
    "4\n"
)]
#[case("mut n = 0\nmut text = 'before'\nwhile n < 3:\n    n = n + 1\n    if n == 2:\n        text = 'after'\n        break\nprint(text).unwrap()", "after\n")]
#[case("def find(limit: i64) -> i64:\n    mut n = 0\n    while n < limit:\n        n = n + 1\n        if n == 3:\n            return n\n    -1\nprint(find(5)).unwrap()\nprint(find(0)).unwrap()", "3\n-1\n")]
#[case("def f(flag: bool) -> i64:\n    while True:\n        if flag:\n            return 42\n        else:\n            break\n    7\nprint(f(True)).unwrap()\nprint(f(False)).unwrap()", "42\n7\n")]
#[case(
    "def f(n: i64) -> i64:\n    while n > 0:\n        return f(n - 1)\n    42\nprint(f(100_000)).unwrap()",
    "42\n"
)]
#[case(
    "mut n = 0\nwhile n < 100_000:\n    n = n + 1\nprint(n).unwrap()",
    "100000\n"
)]
#[case(
    "mut n = 0\nwhile n < 3 and len([x for x in range(n + 1).unwrap()].unwrap()) > 0:\n    n = n + 1\nprint(n).unwrap()",
    "3\n"
)]
#[case(
    "mut xs = [1].unwrap()\nwhile len(xs) < 3:\n    xs.append(len(xs) + 1).unwrap()\nprint(xs).unwrap()",
    "[1, 2, 3]\n"
)]
#[case("mut visits = 0\nfor n in range(5, 0, -2).unwrap():\n    visits = visits + 1\n    if visits > 10:\n        break\n    if n == 3:\n        continue\n    print(n).unwrap()\nprint(visits).unwrap()", "5\n1\n3\n")]
#[case(
    "n = 99\nfor n in range(3).unwrap():\n    break\nprint(n).unwrap()",
    "99\n"
)]
#[case(
    "while False:\n    break\nfor n in range(0).unwrap():\n    continue\nprint(42).unwrap()",
    "42\n"
)]
#[case("mut n = 0\nwhile n < 3:\n    n = n + 1\n    if n > 0:\n        if n < 3:\n            continue\n        else:\n            break\nprint(n).unwrap()", "3\n")]
fn native_loops(#[case] source: &str, #[case] expected: &str) {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}

#[rstest]
#[case("break", "break outside a loop")]
#[case("continue", "continue outside a loop")]
#[case(
    "def f() -> ():\n    break\nfor n in range(1).unwrap():\n    f()",
    "break outside a loop"
)]
#[case(
    "def f() -> ():\n    continue\nwhile True:\n    f()",
    "continue outside a loop"
)]
#[case("while 1:\n    pass", "expected bool")]
#[case(
    "while True:\n    break\n    print(42).unwrap()",
    "5:9: unreachable statement"
)]
#[case(
    "for n in range(3).unwrap():\n    continue\n    print(n).unwrap()",
    "5:9: unreachable statement"
)]
#[case(
    "while True:\n    if True:\n        break\n    else:\n        continue\n    pass",
    "8:9: unreachable statement"
)]
#[case(
    "while False:\n    local = 1\nprint(local).unwrap()",
    "unknown binding"
)]
#[case("mut n = 0\nwhile n < 1:\n    n = True", "expected i64")]
#[case("while missing:\n    missing = True", "unknown binding")]
#[case(
    "for n in range(2).unwrap():\n    pass\ncontinue",
    "continue outside a loop"
)]
#[case("while False:\n    pass\nbreak", "break outside a loop")]
#[case(
    "def f() -> i64:\n    while True:\n        return 42",
    "expected i64, got ()"
)]
#[case(
    "def f() -> i64:\n    while False:\n        return True\n    42",
    "expected i64, got bool"
)]
#[case("while True:\n    break 42", "end of the statement")]
#[case(
    "while False:\n    pass\nelse:\n    pass",
    "loop else is not supported"
)]
#[case(
    "for n in range(0).unwrap():\n    pass\nelse:\n    pass",
    "loop else is not supported"
)]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected:?}, got {error:?}"
    );
}
