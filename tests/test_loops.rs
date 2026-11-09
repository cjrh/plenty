//! Native loop exits, SSA joins, scope, and source diagnostics.
mod support;
use rstest::rstest;
use std::process::Command;

// Only the fragment entrypoint gains an arithmetic contract; declarations retain
// their independent signatures and the shared fragment splitter stays unchanged.
fn arithmetic_program(source: &str) -> String {
    support::program(source).replace(
        "def main() -> ():",
        "def main() -> Result[(), OverflowError[i64] | ZeroDivisionError]:",
    ) + "    Ok(())\n"
}

fn run(source: &str) -> std::process::Output {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    support::compile_source_to_executable(&arithmetic_program(source), &executable)
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
#[case(
    "mut n = 0\nmut sum = 0\nwhile n < 6:\n    n = n + 1\n    if n % 2 == 0:\n        continue\n    sum = sum + n\nprint(sum).unwrap()",
    "9\n"
)]
#[case(
    "mut visits = 0\nfor n in range(5):\n    visits = visits + 1\n    if visits > 10:\n        break\n    continue\nprint(visits).unwrap()",
    "5\n"
)]
#[case(
    "for n in range(5):\n    if n == 2:\n        break\n    print(n).unwrap()",
    "0\n1\n"
)]
#[case(
    "mut sum = 0\nfor n in [1, 2, 3, 4].unwrap():\n    if n % 2 == 0:\n        continue\n    sum = sum + n\nprint(sum).unwrap()",
    "4\n"
)]
#[case(
    "mut visits = 0\nfor n in range(4):\n    visits = visits + 1\n    if n < 2 and visits < 10:\n        continue\n    else:\n        break\nprint(visits).unwrap()",
    "3\n"
)]
#[case(
    "mut total = 0\nfor outer in range(3):\n    for inner in range(5):\n        if inner == 2:\n            break\n        total = total + 1\nprint(total).unwrap()",
    "6\n"
)]
#[case(
    "mut total = 0\nfor outer in range(3):\n    mut inner = 0\n    while inner < 4:\n        inner = inner + 1\n        if inner == 2:\n            continue\n        if inner == 4:\n            break\n        total = total + 1\nprint(total).unwrap()",
    "6\n"
)]
#[case(
    "mut outer = 0\nmut visits = 0\nwhile outer < 3:\n    outer = outer + 1\n    for inner in range(4):\n        if inner == 1:\n            break\n        visits = visits + 1\n    continue\nprint(visits).unwrap()",
    "3\n"
)]
#[case(
    "mut n = 0\nwhile n < 3:\n    n = n + 1\n    while True:\n        break\n    if n == 2:\n        continue\n    print(n).unwrap()",
    "1\n3\n"
)]
#[case(
    "def limit(n: i64) -> bool:\n    print(n).unwrap()\n    n < 3\nmut n = 0\nwhile limit(n):\n    n = n + 1\nprint(n).unwrap()",
    "0\n1\n2\n3\n3\n"
)]
#[case(
    "def condition() -> bool:\n    print('test').unwrap()\n    True\nwhile condition():\n    break\nprint('done').unwrap()",
    "test\ndone\n"
)]
#[case(
    "mut n = 0\nwhile n < 5:\n    n = n + 1\n    if n < 3:\n        continue\n    break\nprint(n).unwrap()",
    "3\n"
)]
#[case(
    "mut n = 0\nwhile n < 4:\n    local = n + 1\n    n = local\nprint(n).unwrap()",
    "4\n"
)]
#[case(
    "mut n = 0\nmut text = 'before'\nwhile n < 3:\n    n = n + 1\n    if n == 2:\n        text = 'after'\n        break\nprint(text).unwrap()",
    "after\n"
)]
#[case(
    "def find(limit: i64) -> Result[i64, OverflowError[i64]]:\n    mut n = 0\n    while n < limit:\n        n = n + 1\n        if n == 3:\n            return Ok(n)\n    Ok(-1)\nprint(find(5)?).unwrap()\nprint(find(0)?).unwrap()",
    "3\n-1\n"
)]
#[case(
    "def f(flag: bool) -> i64:\n    while True:\n        if flag:\n            return 42\n        else:\n            break\n    7\nprint(f(True)).unwrap()\nprint(f(False)).unwrap()",
    "42\n7\n"
)]
#[case(
    "def f(n: i64) -> Result[i64, OverflowError[i64]]:\n    while n > 0:\n        return f(n - 1)\n    Ok(42)\nprint(f(100_000)?).unwrap()",
    "42\n"
)]
#[case(
    "mut n = 0\nwhile n < 100_000:\n    n = n + 1\nprint(n).unwrap()",
    "100000\n"
)]
#[case(
    "mut n = 0\nwhile n < 3 and len([x for x in range(n + 1)].unwrap()) > 0:\n    n = n + 1\nprint(n).unwrap()",
    "3\n"
)]
#[case(
    "mut xs = [1].unwrap()\nwhile len(xs) < 3:\n    xs.append(len(xs) + 1).unwrap()\nprint(xs).unwrap()",
    "[1, 2, 3]\n"
)]
#[case(
    "mut visits = 0\nfor n in range(5, 0, -2):\n    visits = visits + 1\n    if visits > 10:\n        break\n    if n == 3:\n        continue\n    print(n).unwrap()\nprint(visits).unwrap()",
    "5\n1\n3\n"
)]
#[case("n = 99\nfor n in range(3):\n    break\nprint(n).unwrap()", "99\n")]
#[case(
    "while False:\n    break\nfor n in range(0):\n    continue\nprint(42).unwrap()",
    "42\n"
)]
#[case(
    "mut n = 0\nwhile n < 3:\n    n = n + 1\n    if n > 0:\n        if n < 3:\n            continue\n        else:\n            break\nprint(n).unwrap()",
    "3\n"
)]
// `while True:` with no `break` of its own needs no result after it, even
// when a nested loop has a `break` (issue #896).
#[case(
    "def f() -> i64:\n    while True:\n        return 42\nprint(f()).unwrap()",
    "42\n"
)]
#[case(
    "def f() -> i64:\n    while (True):\n        return 42\nprint(f()).unwrap()",
    "42\n"
)]
#[case(
    "def f(limit: i64) -> i64:\n    while True:\n        for n in range(10):\n            if n == limit:\n                break\n        while True:\n            break\n        match limit:\n            case 0:\n                return -1\n            case _:\n                return limit\nprint(f(3)).unwrap()\nprint(f(0)).unwrap()",
    "3\n-1\n"
)]
#[case(
    "class Res:\n    id: i64\n    def __del__(self) -> ():\n        print(\"drop\", self.id).unwrap()\ndef f(r: Res) -> i64:\n    held = Res(2)\n    while True:\n        inner = Res(3)\n        if r.id > 0:\n            return r.id\n        return held.id\nprint(f(Res(1))).unwrap()",
    "drop 3\ndrop 2\ndrop 1\n1\n"
)]
#[case(
    "def one() -> Generator[i64]:\n    yield 1\ndef two() -> Generator[i64]:\n    yield 2\n    yield 3\ndef pick(flag: bool) -> Generator[i64]:\n    while True:\n        if flag:\n            return one()\n        return two()\nprint(list(pick(True)).unwrap()).unwrap()\nprint(list(pick(False)).unwrap()).unwrap()",
    "[1]\n[2, 3]\n"
)]
#[case(
    "class Holder:\n    v: i64\n    def get(self) -> i64:\n        while True:\n            return self.v\ndef first[T](x: T) -> T:\n    while True:\n        return x\nprint(Holder(4).get()).unwrap()\nprint(first(5)).unwrap()",
    "4\n5\n"
)]
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
    "def f() -> ():\n    break\nfor n in range(1):\n    f()",
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
    "for n in range(3):\n    continue\n    print(n).unwrap()",
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
#[case("for n in range(2):\n    pass\ncontinue", "continue outside a loop")]
#[case("while False:\n    pass\nbreak", "break outside a loop")]
// A `break` anywhere in the body except a nested loop leaves `while True:`,
// so the function still needs a result after the loop (issue #896).
#[case(
    "def f() -> i64:\n    while True:\n        if True:\n            break\n        return 42",
    "expected i64, got ()"
)]
#[case(
    "def f(n: i64) -> i64:\n    while True:\n        match n:\n            case 0:\n                break\n            case _:\n                return n",
    "expected i64, got ()"
)]
#[case(
    "def f(n: i64) -> i64:\n    while True:\n        unchecked:\n            if n > 0:\n                break\n        return n",
    "expected i64, got ()"
)]
// Only the literal `True` makes a loop diverge, not a constant or another
// expression that is always true.
#[case(
    "const FOREVER: bool = True\ndef f() -> i64:\n    while FOREVER:\n        return 42\ndef main() -> ():\n    drop(f())",
    "expected i64, got ()"
)]
#[case(
    "def f() -> i64:\n    while not False:\n        return 42",
    "expected i64, got ()"
)]
#[case(
    "def f() -> i64:\n    while True:\n        return 1\n    2",
    "4:5: unreachable statement"
)]
#[case(
    "def f() -> ():\n    while True:\n        pass\n    print(1).unwrap()",
    "4:5: unreachable statement"
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
    "for n in range(0):\n    pass\nelse:\n    pass",
    "loop else is not supported"
)]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected:?}, got {error:?}"
    );
}

/// Runs a complete program with and without `--release`, returning the exit
/// code and output of each, which must agree.
fn run_both(source: &str) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let outputs: Vec<_> = [&[][..], &["--release"]]
        .iter()
        .map(|flags| {
            let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
                .args(*flags)
                .arg(&path)
                .output()
                .unwrap();
            (
                output.status.code(),
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr)
                    .replace(&*path.to_string_lossy(), "main.plenty"),
            )
        })
        .collect();
    assert_eq!(outputs[0], outputs[1], "{source}");
    outputs[0].clone()
}

/// A rotated loop tests its condition in a guard and again at the end of each
/// pass (issue #726). A failure in either copy reports the same error, and
/// `and` still stops at its first false or failing operand.
#[test]
fn rotated_conditions_keep_their_failures_and_order() {
    let (code, stdout, stderr) = run_both(
        r#"
def climb(start: i64, step: i64) -> Result[i64, OverflowError[i64]]:
    mut n = start
    mut passes = 0
    while n * step > 0:
        n += 1
        passes += 1
    Ok(passes)

def both(a: i64, b: i64) -> Result[i64, OverflowError[i64]]:
    mut x = a
    mut y = b
    mut passes = 0
    while x * 2 > 0 and y * 3 > 0:
        x += 1
        y += 1
        passes += 1
    Ok(passes)

def show(r: Result[i64, OverflowError[i64]]) -> Result[(), IoError]:
    match r:
        case Ok(n):
            print(n)?
        case Err(e):
            print(e.wrapped)?
    Ok(())

def main() -> Result[(), IoError]:
    # The first test fails.
    show(climb(4611686018427387904, 2))?
    # A later pass fails.
    show(climb(4611686018427387900, 2))?
    # Both operands would fail; the first one reports.
    show(both(4611686018427387904, 3074457345618258603))?
    # The second operand fails on a later pass.
    show(both(1, 3074457345618258600))?
    # The condition is false at once.
    show(both(-1, 1))?
    Ok(())
"#,
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "-9223372036854775808\n-9223372036854775808\n-9223372036854775808\n-9223372036854775807\n0\n"
    );
}

/// `continue` in a rotated loop runs the latch's test; a condition with a
/// call stays at the header and runs once per test.
#[test]
fn rotated_loops_continue_to_the_latch() {
    let (code, stdout, stderr) = run_both(
        r#"
def skip_sum(n: i64) -> Result[i64, OverflowError[i64]]:
    mut i = 0
    mut t = 0
    while i < n:
        i += 1
        if i == 2 or i == 4:
            continue
        t += i
    Ok(t)

def below(i: i64, n: i64, calls: &mut i64) -> Result[bool, OverflowError[i64]]:
    calls += 1
    Ok(i < n)

def counted(n: i64) -> Result[i64, OverflowError[i64]]:
    mut i = 0
    mut calls = 0
    while below(i, n, &mut calls)?:
        i += 1
    Ok(calls)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(skip_sum(10)?)?
    print(skip_sum(0)?)?
    print(counted(4)?)?
    Ok(())
"#,
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "49\n0\n5\n");
}

/// A loop's index facts about `lo < hi` hold in its body, not after a loop
/// that never ran: `s[hi]` here is out of bounds and must trap, at its own
/// position (issue #816; it read past the list before loops were rotated).
#[test]
fn loop_index_facts_stay_in_the_body() {
    let (code, stdout, stderr) = run_both(
        r#"
def h(s: &mut [i64], first: i64) -> Result[i64, OverflowError[i64]]:
    mut lo = 1
    mut hi = first - 1
    while lo < hi:
        s.swap(lo, hi)
        lo += 1
        hi -= 1
    Ok(s[hi])

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut a = [7]?
    print(h(&mut a, 2)?)?
    Ok(())
"#,
    );
    assert_eq!(code, Some(1), "{stdout}");
    assert!(
        stderr.contains("main.plenty:9:8: index out of bounds: index 1, length 1"),
        "{stderr}"
    );
}

/// A rotated condition's index check traps at the condition's position with
/// the same report, whether the guard or the latch fails (issue #726).
#[test]
fn rotated_condition_traps_keep_their_position() {
    for (start, index) in [(5, 5), (0, 3)] {
        let (code, _, stderr) = run_both(&format!(
            r#"
def scan(xs: &[i64], start: i64) -> Result[i64, OverflowError[i64]]:
    mut i = start
    while xs[i] > 0:
        i += 1
    Ok(i)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    xs = [1, 2, 3]?
    print(scan(&xs, {start})?)?
    Ok(())
"#
        ));
        assert_eq!(code, Some(1), "{stderr}");
        assert!(
            stderr.contains(&format!(
                "main.plenty:4:11: index out of bounds: index {index}, length 3"
            )),
            "{stderr}"
        );
    }
}
