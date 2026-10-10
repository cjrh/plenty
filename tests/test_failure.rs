//! Explicit error erasure must preserve success types, cleanup, and allocation guarantees.
mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn mixed_errors_and_contextual_success_types() {
    native(
        r#"
type AppFailure = Failure
def build() -> Result[list[u8], AppFailure]:
    Ok([n * n for n in range(4)]?)
def main() -> Result[(), Failure]:
    values: list[u8] = build()?
    print(values)?
    empty: dict[str, u16] = {}?
    print(empty)?
    pair: tuple[u8, str] = (7, "seven")
    print(pair)?
    n: u8 = u8.parse("42")?
    print(n)?
    print("é"[0])?
    Ok(())
"#,
        "[0, 1, 4, 9]\n{}\n(7, \"seven\")\n42\né\n",
    );
}

#[test]
fn different_errors_erase_in_helpers_and_marker_is_an_ordinary_inline_value() {
    native(r#"
def parse(text: str) -> Result[u8, Failure]:
    Ok(u8.parse(text)?)
def io() -> Result[(), IoError]:
    Err(IoError.System(5))
def work() -> Result[(), Failure]:
    io()?
    print("unreachable")?
    Ok(())
def main() -> Result[(), Failure]:
    print(str.repr(parse("256")).unwrap())?
    print(str.repr(work()).unwrap())?
    marker = Failure.Unspecified
    print(marker == copy(marker)?)?
    match marker:
        case Failure.Unspecified:
            print("discarded")?
    Ok(())
"#,
        "Result[u8, Failure].Err(Failure.Unspecified)\nResult[(), Failure].Err(Failure.Unspecified)\nTrue\ndiscarded\n");
}

#[test]
fn owned_error_drops_before_pending_arguments_contexts_and_locals() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
class Context:
    def __enter__(self: &mut Context) -> i64:
        1
    def __exit__(self: &mut Context) -> ():
        print("exit").unwrap()
def fail() -> Result[i64, Option[Guard]]:
    Err(Some(Guard("error")))
def later() -> i64:
    print("unreachable").unwrap()
    0
def consume(g: Guard, a: i64, b: i64) -> i64:
    a + b
def work() -> Result[i64, Failure]:
    first = Guard("first")
    with Context() as context:
        second = Guard("second")
        return Ok(consume(Guard("argument"), fail()?, later()))
def main() -> Result[(), IoError]:
    print(str.repr(work()).unwrap())?
    Ok(())
"#,
        "error\nargument\nsecond\nexit\nfirst\nResult[i64, Failure].Err(Failure.Unspecified)\n",
    );
}

#[test]
fn discarding_existing_owned_errors_never_allocates() {
    native(r#"
class Guard:
    def __del__(self) -> ():
        print("dropped").unwrap()
def discard(error: Guard) -> Result[(), Failure]:
    result: Result[i64, Guard] = Err(error)
    result?
    Ok(())
def forward(value: Result[(), Failure]) -> Result[(), Failure]:
    value?
    Ok(())
def main() -> Result[(), Failure]:
    guard = Guard()
    print("__test_begin_no_allocations__")?
    bad = forward(discard(guard))
    good = forward(Ok(()))
    expected: Result[(), Failure] = Err(Failure.Unspecified)
    same = bad == expected
    print("__test_end_no_allocations__")?
    print(str.repr(good).unwrap())?
    print(same)?
    Ok(())
"#, "__test_begin_no_allocations__\ndropped\n__test_end_no_allocations__\nResult[(), Failure].Ok(())\nTrue\n");
}

#[test]
fn main_exit_status_and_cleanup() {
    let output = support::run(
        r#"
class Guard:
    def __del__(self) -> ():
        print("cleaned").unwrap()
def main() -> Result[(), Failure]:
    guard = Guard()
    u8.parse("bad")?
    print("unreachable")?
    Ok(())
"#,
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"cleaned\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: main returned Failure.Unspecified: Failure keeps no details of the original error\n"
    );
    let output = support::run("def main() -> Result[i32, Failure]:\n    Ok(7)");
    assert_eq!(output.status.code(), Some(7));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn actual_allocation_failure_can_be_erased_without_allocating() {
    native(r#"
def work() -> Result[(), Failure]:
    values: list[u8] = [1, 2]?
    print("unreachable")?
    Ok(())
def main() -> Result[(), IoError]:
    print("__test_fail_allocations_after_0__")?
    failed = work()
    print("__test_restore_allocations__")?
    print(str.repr(failed).unwrap())?
    Ok(())
"#, "__test_fail_allocations_after_0__\n__test_restore_allocations__\nResult[(), Failure].Err(Failure.Unspecified)\n");
}

#[test]
fn erasure_is_only_for_result_propagation_and_requires_explicit_success() {
    for (source, expected) in [
        ("def f() -> Result[i64, Failure]:\n    Ok(Some(1)?)", "same Result or Option family"),
        ("def f() -> Result[(), IoError]:\n    r: Result[(), Failure] = Err(Failure.Unspecified)\n    r?\n    Ok(())", "identical Result error types"),
        ("def f() -> Result[(), Failure]:\n    Err(\"details\")", "expected Failure"),
        ("def f() -> Result[list[i64], Failure]:\n    [1, 2]", "expected Result[list[i64], Failure]"),
        ("def f() -> Result[(), Failure]:\n    print(\"hello\")?", "expected Result[(), Failure]"),
        ("def f() -> Result[(), Failure]:\n    Failure.Unspecified?\n    Ok(())", "same Result or Option family"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}
