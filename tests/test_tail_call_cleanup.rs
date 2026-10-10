//! A call in tail position evaluates its arguments, drops what the caller still
//! owns in ordinary scope-exit order, and only then enters the callee (issue 7).

mod support;

use rstest::rstest;

const PRELUDE: &str = r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
    def length(self) -> i64:
        print("length").unwrap()
        len(self.name)
def make(name: str) -> Guard:
    Guard(name)
def note(text: str, n: i64) -> i64:
    print(text).unwrap()
    n
def callee(a: i64, b: i64) -> i64:
    print("callee").unwrap()
    a + b
def keep(g: Guard, n: i64) -> i64:
    print("keep").unwrap()
    n
"#;

fn trace(source: &str, expected: &str) {
    let source = format!("{PRELUDE}{source}");
    let output = support::run(&source);
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
#[case("return callee(note('left', 1), note('right', 2))")]
#[case("callee(note('left', 1), note('right', 2))")]
#[case("operation = callee\n    return operation(note('left', 1), note('right', 2))")]
#[case("operation = callee\n    operation(note('left', 1), note('right', 2))")]
fn arguments_run_left_to_right_before_reverse_cleanup_and_callee_entry(#[case] tail: &str) {
    trace(
        &format!(
            "def caller(param: Guard) -> i64:\n    first = Guard('first')\n    second = Guard('second')\n    {tail}\nprint(caller(Guard('param'))).unwrap()\n"
        ),
        "left\nright\nsecond\nfirst\nparam\ncallee\n3\n",
    );
}

#[rstest]
#[case("keep(moved, 1)", "local\nparam\nkeep\nmoved\n1\n")]
#[case("keep(param, 1)", "moved\nlocal\nkeep\nparam\n1\n")]
#[case(
    "operation = keep\n    operation(moved, 1)",
    "local\nparam\nkeep\nmoved\n1\n"
)]
fn moved_arguments_belong_to_the_callee(#[case] tail: &str, #[case] expected: &str) {
    trace(
        &format!(
            "def caller(param: Guard) -> i64:\n    local = Guard('local')\n    moved = Guard('moved')\n    {tail}\nprint(caller(Guard('param'))).unwrap()\n"
        ),
        expected,
    );
}

#[test]
fn explicit_drops_are_not_repeated() {
    trace(
        r#"
def caller() -> i64:
    early = Guard("early")
    late = Guard("late")
    drop(early)
    callee(1, 2)
print(caller()).unwrap()
"#,
        "early\nlate\ncallee\n3\n",
    );
}

#[rstest]
#[case("True", "keep\na\n0\nb\ncallee\n3\n")]
#[case("False", "b\na\ncallee\n3\n")]
fn cleanup_follows_branch_dependent_ownership(#[case] moved: &str, #[case] expected: &str) {
    trace(
        &format!(
            r#"
def caller(moved: bool) -> i64:
    a = Guard("a")
    b = Guard("b")
    if moved:
        print(keep(a, 0)).unwrap()
    callee(1, 2)
print(caller({moved})).unwrap()
"#
        ),
        expected,
    );
}

#[rstest]
#[case("1, Some(2)", "inner\nouter\ncallee\n3\n")]
#[case("1, Nothing", "outer\nkeep\ninner\n1\n")]
#[case("0, Some(2)", "outer\ncallee\n0\n")]
#[case("-1, Some(2)", "outer\ncallee\n1\n")]
#[case("-1, Nothing", "outer\ncallee\n-1\n")]
fn nested_branch_and_match_returns_transfer_after_cleanup(
    #[case] arguments: &str,
    #[case] expected: &str,
) {
    trace(
        &format!(
            r#"
def caller(n: i64, choice: Option[i64]) -> i64:
    outer = Guard("outer")
    if n > 0:
        inner = Guard("inner")
        match choice:
            case Some(value):
                return callee(value, n)
            case Nothing:
                return keep(inner, n)
    if n == 0:
        return callee(0, 0)
    match choice:
        case Some(value):
            callee(value, n)
        case Nothing:
            callee(0, n)
print(caller({arguments})).unwrap()
"#
        ),
        expected,
    );
}

#[rstest]
#[case("return callee(make('temp').length(), 1)")]
#[case("callee(make('temp').length(), 1)")]
fn statement_temporaries_drop_before_the_locals(#[case] tail: &str) {
    trace(
        &format!(
            "def caller() -> i64:\n    local = Guard('local')\n    {tail}\nprint(caller()).unwrap()\n"
        ),
        "length\ntemp\nlocal\ncallee\n5\n",
    );
}

#[rstest]
#[case("True", "left\nfail\nmoved\nlocal\nfailed\n")]
#[case("False", "left\nfail\nlocal\nsum\nmoved\n3\n")]
fn failed_argument_propagation_cleans_up_without_entering_the_callee(
    #[case] stop: &str,
    #[case] expected: &str,
) {
    trace(
        &format!(
            r#"
def fail(stop: bool) -> Result[i64, Failure]:
    print("fail").unwrap()
    if stop:
        return Err(Failure.Unspecified)
    Ok(2)
def sum(g: Guard, a: i64, b: i64) -> Result[i64, Failure]:
    print("sum").unwrap()
    Ok(a + b)
def caller(stop: bool) -> Result[i64, Failure]:
    local = Guard("local")
    sum(make("moved"), note("left", 1), fail(stop)?)
match caller({stop}):
    case Ok(value):
        print(value).unwrap()
    case Err(_):
        print("failed").unwrap()
"#
        ),
        expected,
    );
}

#[test]
fn inline_results_keep_the_frame_but_not_its_locals() {
    trace(
        r#"
def span(n: u8) -> range[u8]:
    print("span").unwrap()
    range[u8](n)
def direct() -> range[u8]:
    local = Guard("direct")
    span(2u8)
def indirect() -> range[u8]:
    local = Guard("indirect")
    operation = span
    operation(1u8)
for n in direct():
    print(n).unwrap()
for n in indirect():
    print(n).unwrap()
"#,
        "direct\nspan\n0\n1\nindirect\nspan\n0\n",
    );
}

/// A reference argument may point at a caller local, so the call returns
/// before any of them is dropped.
#[rstest]
#[case("read(&local)", "read\nother\nlocal\n5\n")]
#[case(
    "borrowed = &local\n    return read(borrowed)",
    "read\nother\nlocal\n5\n"
)]
#[case("local.length()", "length\nother\nlocal\n5\n")]
#[case("make('temp').length()", "length\ntemp\nother\nlocal\n4\n")]
#[case("operation = read\n    operation(&local)", "read\nother\nlocal\n5\n")]
fn calls_borrowing_the_caller_clean_up_after_they_return(
    #[case] tail: &str,
    #[case] expected: &str,
) {
    trace(
        &format!(
            r#"
def read(g: &Guard) -> i64:
    print("read").unwrap()
    len(g.name)
def caller() -> i64:
    local = Guard("local")
    other = Guard("other")
    {tail}
print(caller()).unwrap()
"#
        ),
        expected,
    );
}

#[test]
fn context_exits_and_joins_still_precede_cleanup() {
    trace(
        r#"
class Scope:
    name: str
    def __enter__(self: &mut Scope) -> ():
        print("enter").unwrap()
    def __exit__(self: &mut Scope) -> ():
        print(self.name).unwrap()
def increment(value: &mut i64) -> ():
    *value = *value + 1
def checked(a: i64, b: i64) -> Result[i64, Failure]:
    print("checked").unwrap()
    Ok(a + b)
def returns_inside() -> i64:
    local = Guard("local")
    with Scope("exit owned"):
        return callee(1, 2)
def ends_with_borrowed_manager() -> ():
    local = Guard("local")
    mut scope = Scope("exit borrowed")
    with &mut scope:
        print("body").unwrap()
def returns_inside_worker_scope() -> Result[i64, Failure]:
    local = Guard("local")
    mut count = 4
    with spawn(increment, &mut count)?:
        return checked(3, 4)
def main() -> Result[(), Failure]:
    print(returns_inside())?
    ends_with_borrowed_manager()
    print(returns_inside_worker_scope()?)?
    Ok(())
"#,
        "enter\ncallee\nexit owned\nlocal\n3\nenter\nbody\nexit borrowed\nlocal\nchecked\nlocal\n7\n",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn recursion_with_destructor_locals_runs_on_a_small_stack() {
    let source = r#"
class Guard:
    id: i64
    def __del__(self: &mut Guard) -> ():
        self.id = 0
def direct(n: i64, total: i64) -> i64:
    guard = Guard(n)
    names = ["a", "b"].unwrap()
    if n == 0:
        return total
    direct(n - 1, total + guard.id - n + 1)
def indirect(n: i64, total: i64) -> i64:
    guard = Guard(n)
    if n == 0:
        return total
    step = indirect
    step(n - 1, total + 1)
def main() -> Result[(), Failure]:
    print(direct(1000000, 0))?
    print(indirect(1000000, 0))?
    Ok(())
"#;
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("deep");
    support::compile_source_to_executable(source, &executable).unwrap();
    let output = std::process::Command::new("sh")
        .args(["-c", "ulimit -s 256; exec \"$1\"", "tail-call-cleanup-test"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"1000000\n1000000\n");
}
