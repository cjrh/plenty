//! Function arithmetic contracts and lexical trapping opt-outs (B14).
mod support;
use support::{check_diagnostic, run_stdout};

#[test]
fn functions_use_the_declared_result_without_wrapping_successes() {
    assert_eq!(
        run_stdout(
            r#"
def scale(a: i64, b: i64) -> Result[i64, OverflowError[i64]]:
    Ok(a * b)
def early(a: i64) -> Result[i64, OverflowError[i64] | ParseError]:
    if a == 0:
        return Ok(42)
    product = a * 2
    return Ok(product)
def caller(a: i64) -> Result[i64, OverflowError[i64]]:
    value = unchecked:
        scale(a, 2)?
    Ok(value)
def main() -> Result[(), IoError]:
    print(scale(3, 4).unwrap(), scale(i64.MAX, 2).is_err())?
    print(early(0).unwrap(), early(i64.MAX).is_err())?
    print(caller(i64.MAX).is_err())?
    Ok(())
"#
        ),
        "12 True\n42 True\nTrue\n"
    );
}

#[test]
fn errors_are_required_at_the_operation_not_inferred_from_callers() {
    for (body, error) in [
        ("a + 1", "OverflowError[i64]"),
        ("a // 2", "ZeroDivisionError"),
        ("a % 2", "ZeroDivisionError"),
    ] {
        let source =
            format!("def bad(a: i64) -> i64:\n    {body}\ndef main() -> ():\n    drop(bad(1))\n");
        let diagnostic = check_diagnostic(&source);
        assert!(diagnostic.contains(error), "{diagnostic}");
        assert!(diagnostic.contains("checked:"), "{diagnostic}");
        assert!(diagnostic.contains("unchecked:"), "{diagnostic}");
    }
    let diagnostic = check_diagnostic(
        "def bad(a: i64) -> Option[i64]:\n    Some(a + 1)\ndef main() -> ():\n    drop(bad(0))\n",
    );
    assert!(diagnostic.contains("OverflowError[i64]"), "{diagnostic}");
    let diagnostic = check_diagnostic(
        "def bad(a: i64) -> Result[i64, OverflowError[i64]]:\n    a + 1\ndef main() -> ():\n    drop(bad(0))\n",
    );
    assert!(diagnostic.contains("expected Result"), "{diagnostic}");
}

#[test]
fn divisor_proofs_remove_errors_without_creating_an_unbound_failure_edge() {
    assert_eq!(
        run_stdout(
            r#"
def quotient(a: i64, b: Positive[i64]) -> i64:
    a // b
def remainder(a: i64, b: NonZero[i64]) -> i64:
    a % b
def unsigned(a: u64, b: NonZero[u64]) -> u64:
    a // b
def signed(a: i64, b: NonZero[i64]) -> Result[i64, OverflowError[i64]]:
    Ok(a // b)
def main() -> Result[(), IoError | RangeError]:
    positive = Positive.try_from(3)?
    nonzero = NonZero.try_from(-1)?
    other = NonZero.try_from(2u64)?
    print(quotient(14, positive), remainder(i64.MIN, nonzero), unsigned(7u64, other))?
    print(signed(i64.MIN, nonzero).is_err())?
    Ok(())
"#
        ),
        "4 0 3\nTrue\n"
    );
}

#[test]
fn local_recovery_and_nesting_restore_the_outer_mode() {
    assert_eq!(
        run_stdout(
            r#"
def recovered(a: i64) -> i64:
    result = checked:
        a + 1
    match result:
        case Ok(value):
            value
        case Err(_):
            17
def mixed(a: i64) -> Result[i64, OverflowError[i64]]:
    result = unchecked:
        offset = a + 1
        inner = checked:
            offset * 2
        inner
    result
def restored(a: i64) -> Result[i64, OverflowError[i64]]:
    unchecked:
        pass
    Ok(a + 1)
def main() -> Result[(), IoError]:
    print(recovered(2), recovered(i64.MAX))?
    print(mixed(2).unwrap(), mixed(4611686018427387903).is_err())?
    print(restored(i64.MAX).is_err())?
    Ok(())
"#
        ),
        "3 17\n6 True\nTrue\n"
    );
}

#[test]
fn unchecked_supports_value_and_statement_positions_and_contextual_names() {
    assert_eq!(
        run_stdout(
            r#"
def value(a: i64) -> i64:
    return unchecked:
        a * 2
def tail(a: i64) -> i64:
    unchecked:
        unchecked:
            a + 1
def arm(a: i64) -> i64:
    match a:
        case _:
            unchecked:
                a + 2
def main() -> Result[(), IoError]:
    unchecked: i64 = 7
    mut x = 0
    x = unchecked:
        unchecked + 1
    mut n = 0
    while n < 4:
        unchecked:
            n += 1
            if n == 2:
                continue
            if n == 3:
                break
            x += n
    print(value(3), tail(3), arm(3), x, n)?
    Ok(())
"#
        ),
        "6 4 5 9 3\n"
    );
    let diagnostic = check_diagnostic(
        "def main() -> ():\n    unchecked:\n        value: Result[i64, ParseError] = Ok(1)\n        value\n    pass\n",
    );
    assert!(
        diagnostic.contains("cannot implicitly discard a Result"),
        "{diagnostic}"
    );
}

#[test]
fn closure_contracts_are_independent_of_definition_and_call_modes() {
    assert_eq!(
        run_stdout(
            r#"
def make() -> Callable[[i64], Result[i64, OverflowError[i64]]]:
    unchecked:
        (def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + 1))
def main() -> Result[(), IoError]:
    f = make()
    print(f(3).unwrap(), f(i64.MAX).is_err())?
    closure = unchecked:
        offset = 2
        (def [offset](x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + offset))
    print(closure(3).unwrap(), closure(i64.MAX).is_err())?
    result = checked:
        local = (def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + 1))
        print(local(i64.MAX).is_err())?
        1 + 2
    print(result.unwrap())?
    Ok(())
"#
        ),
        "4 True\n5 True\nTrue\n3\n"
    );
    for body in [
        "unchecked:\n        (def(x: i64) -> i64: x + 1)",
        "checked:\n        (def(x: i64) -> i64: x + 1)",
    ] {
        let source = format!(
            "def make() -> Callable[[i64], i64]:\n    {body}\ndef main() -> ():\n    f = make()\n    drop(f(0))\n"
        );
        let diagnostic = check_diagnostic(&source);
        assert!(
            diagnostic.contains("OverflowError[i64]")
                || diagnostic.contains("contains no arithmetic"),
            "{diagnostic}"
        );
    }
    let diagnostic = check_diagnostic(
        r#"
def main() -> Result[(), AllocError]:
    xs = [1, 2]?
    drop(iter(&xs).map(def(x: &i64) -> i64: *x + 1).count())
    Ok(())
"#,
    );
    assert!(diagnostic.contains("OverflowError[i64]"), "{diagnostic}");
}

#[test]
fn implicit_failures_release_locals_and_pending_temporaries_without_writing() {
    assert_eq!(
        run_stdout(
            r#"
class Owner:
    id: i64
    def __del__(self: &mut Owner) -> ():
        print(self.id).unwrap()
def fail(n: &mut i64) -> Result[(), AllocError | OverflowError[i64]]:
    owner = Owner(10)
    items = [1, 2, 3]?
    *n += i64.MAX
    drop(owner)
    drop(items)
    Ok(())
def main() -> Result[(), IoError]:
    mut n = 1
    print(fail(&mut n).is_err(), n)?
    Ok(())
"#
        ),
        "10\nTrue 1\n"
    );
}

#[test]
fn generators_require_local_handling_or_explicit_traps() {
    let diagnostic = check_diagnostic(
        r#"
def values(n: i64) -> Generator[i64]:
    yield n + 1
def main() -> ():
    drop(values(0))
"#,
    );
    assert!(
        diagnostic.contains("generator cannot propagate arithmetic errors"),
        "{diagnostic}"
    );
    assert_eq!(
        run_stdout(
            r#"
def values(n: i64) -> Generator[i64]:
    unchecked:
        yield n + 1
def main() -> Result[(), IoError]:
    for value in values(3):
        print(value)?
    Ok(())
"#
        ),
        "4\n"
    );
}

#[test]
fn function_arithmetic_keeps_native_recursive_tail_returns() {
    assert_eq!(
        run_stdout(
            r#"
def count(n: i64) -> Result[i64, OverflowError[i64]]:
    if n == 0:
        return Ok(n)
    return count(n - 1)
def main() -> Result[(), IoError]:
    print(count(100000).unwrap())?
    Ok(())
"#
        ),
        "0\n"
    );
}

#[test]
fn every_integer_width_uses_the_default_boundary() {
    let mut source = String::new();
    let mut calls = String::new();
    let mut expected = String::new();
    for ty in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        for (name, operation, args) in [
            ("add", "a + b", format!("{ty}.MAX, 1{ty}")),
            ("sub", "a - b", format!("{ty}.MIN, 1{ty}")),
            ("mul", "a * b", format!("{ty}.MAX, 2{ty}")),
            ("pow", "a.pow(b)", format!("2{ty}, 64{ty}")),
        ] {
            source.push_str(&format!("def {name}_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    Ok({operation})\n"));
            calls.push_str(&format!("    print({name}_{ty}({args}).is_err())?\n"));
            expected.push_str("True\n");
        }
        let signed = ty.starts_with('i');
        let errors = if signed {
            format!("ZeroDivisionError | OverflowError[{ty}]")
        } else {
            "ZeroDivisionError".into()
        };
        source.push_str(&format!(
            "def div_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, {errors}]:\n    Ok(a // b)\n"
        ));
        source.push_str(&format!(
            "def rem_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, ZeroDivisionError]:\n    Ok(a % b)\n"
        ));
        calls.push_str(&format!(
            "    print(div_{ty}(1{ty}, 0{ty}).is_err(), rem_{ty}(1{ty}, 0{ty}).is_err())?\n"
        ));
        expected.push_str("True True\n");
        if signed {
            source.push_str(&format!(
                "def neg_{ty}(a: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    Ok(-a)\n"
            ));
            calls.push_str(&format!("    print(neg_{ty}({ty}.MIN).is_err(), div_{ty}({ty}.MIN, -1{ty}).is_err(), rem_{ty}({ty}.MIN, -1{ty}).unwrap())?\n"));
            expected.push_str("True True 0\n");
        }
    }
    source.push_str(&format!(
        "def main() -> Result[(), IoError]:\n{calls}    Ok(())\n"
    ));
    assert_eq!(run_stdout(&source), expected);
}

#[cfg(feature = "runtime-checks")]
#[test]
fn implicit_arithmetic_failures_do_not_allocate() {
    let output = support::run(
        r#"
def step(a: i64, b: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    Ok(a * 2 // b)
def main() -> ():
    print("__test_begin_no_allocations__").unwrap()
    for divisor in range(-2, 3):
        drop(step(i64.MAX, divisor))
        drop(step(1, divisor))
    print("__test_end_no_allocations__").unwrap()
"#,
    );
    support::assert_success(output);
}

#[test]
fn unchecked_failure_terminates_instead_of_reaching_an_outer_boundary() {
    let source = r#"
def fail(n: i64) -> Result[i64, OverflowError[i64]]:
    result = checked:
        unchecked:
            drop(n + 1)
        n + 2
    result
def main() -> ():
    drop(fail(i64.MAX))
"#;
    for release in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trapping.plenty");
        let executable = dir.path().join("trapping");
        std::fs::write(&path, source).unwrap();
        let mut compiler = std::process::Command::new(support::plenty_bin());
        compiler.env("PLENTY_CACHE", "off");
        if release {
            compiler.arg("--release");
        }
        let output = compiler
            .arg("--compile")
            .arg(&path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        support::assert_success(output);
        let output = std::process::Command::new(&executable).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "release={release}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("integer overflow"),
            "release={release}: {error}"
        );
    }
}
