//! Results must be handled before discarding or directly printing them.
mod support;
use rstest::rstest;

#[rstest]
#[case("print(\"lost\")")]
#[case("[1, 2]")]
#[case("\"hello\" + \"world\"")]
#[case("if True:\n        print(\"lost\")")]
#[case("match Some(1):\n        case Some(_):\n            print(\"lost\")\n        case Nothing:\n            pass")]
#[case("for n in range(1):\n        print(n)")]
#[case("while False:\n        print(\"lost\")")]
#[case("with open(\"unused.txt\")? as file:\n        print(\"lost\")")]
fn rejects_discarded_results(#[case] statement: &str) {
    let source = format!("def main() -> Result[(), Failure]:\n    {statement}\n    Ok(())\n");
    let error = plenty::check_source(&source).unwrap_err().to_string();
    assert!(
        error.contains("cannot implicitly discard a Result"),
        "{error}"
    );
    assert!(error.contains("use `?`"), "{error}");
    assert!(error.contains("drop(result)"), "{error}");
}

#[test]
fn aliases_and_generator_final_statements_are_checked() {
    for source in [
        "type Outcome = Result[i64, str]\ndef f(value: Outcome) -> ():\n    value\n    pass\n",
        "def f() -> Generator[i64]:\n    yield 1\n    print(\"lost\")\n",
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(
            error.contains("cannot implicitly discard a Result"),
            "{error}"
        );
        assert!(!error.contains("use `?`"), "{error}");
    }
}

#[rstest]
#[case("Result[(), Failure]", "Result[i64, str]", "value", true)]
#[case("Result[(), str]", "Result[i64, str]", "value", true)]
#[case("Result[(), IoError]", "Result[i64, str]", "value", false)]
#[case("()", "Result[i64, str]", "value", false)]
#[case("Result[(), Failure]", "Result[Result[i64, str], str]", "value", false)]
#[case("Option[i64]", "Result[i64, str]", "value", false)]
#[case(
    "Result[(), Failure]",
    "Result[(), str]",
    "print(value).unwrap()",
    false
)]
#[case(
    "Result[(), Failure]",
    "Result[Result[i64, str], str]",
    "print(value).unwrap()",
    false
)]
#[case(
    "Result[(), Failure]",
    "Result[i64, str]",
    "print(value).unwrap()",
    true
)]
#[case(
    "Result[(), IoError]",
    "Result[i64, str]",
    "print(value).unwrap()",
    false
)]
#[case("()", "Result[i64, str]", "print(value).unwrap()", false)]
#[case(
    "Result[(), Failure]",
    "&Result[i64, str]",
    "print(value).unwrap()",
    false
)]
#[case("Result[(), Failure]", "Result[i64, str]", "takes_int(value)", true)]
#[case("Result[(), Failure]", "Result[str, str]", "takes_int(value)", false)]
#[case("Result[(), IoError]", "Result[i64, str]", "takes_int(value)", false)]
fn propagation_advice_requires_compatible_success_and_error_types(
    #[case] returns: &str,
    #[case] argument: &str,
    #[case] statement: &str,
    #[case] hint: bool,
) {
    let source = format!(
        "def takes_int(n: i64) -> ():\n    pass\ndef f(value: {argument}) -> {returns}:\n    {statement}\n    pass\n"
    );
    let error = support::check_source(&source).unwrap_err().to_string();
    assert_eq!(error.contains("use `?`"), hint, "{error}");
}

#[test]
fn missing_inner_and_outer_propagation_have_distinct_diagnostics() {
    let inner = plenty::check_source(
        "def main() -> Result[(), Failure]:\n    print(\"hello\" + \"world\")?\n    Ok(())\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        inner.contains("print cannot print a Result directly"),
        "{inner}"
    );
    assert!(inner.contains("on the argument"), "{inner}");
    assert!(inner.contains("str.repr"), "{inner}");
    assert!(inner.starts_with("2:11:"), "{inner}");
    let outer = plenty::check_source(
        "def main() -> Result[(), Failure]:\n    print((\"hello\" + \"world\")?)\n    Ok(())\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        outer.contains("cannot implicitly discard a Result"),
        "{outer}"
    );
    assert!(outer.starts_with("2:5:"), "{outer}");
}

#[test]
fn printing_a_borrowed_result_alias_is_also_rejected() {
    let error = support::check_source(
        "type Outcome = Result[i64, str]\ndef inspect(value: &Outcome) -> ():\n    print(*value).unwrap()\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("print cannot print a Result directly"),
        "{error}"
    );
}

#[test]
fn explicit_handling_returns_and_nested_printable_values_remain_valid() {
    let output = support::run(
        r#"
type Outcome = Result[i64, str]
class Record:
    result: Outcome
def final(value: Outcome) -> Outcome:
    value
def returned(value: Outcome) -> Outcome:
    return value
def branched(value: Outcome) -> Outcome:
    if True:
        value
    else:
        Err("unused")
def matched(value: Outcome) -> Outcome:
    match Some(1):
        case Some(_):
            value
        case Nothing:
            Err("unused")
def consume(value: Outcome) -> ():
    drop(value)
def main() -> Result[(), Failure]:
    drop(final(Ok(1)))
    consume(returned(Ok(2)))
    print(branched(Ok(3))?)?
    match matched(Ok(4)):
        case Ok(n):
            print(n)?
        case Err(message):
            print(message)?
    value: Outcome = Err("debug")
    print(str.repr(value)?)?
    print(Some(5))?
    print(Record(Ok(6)))?
    values: list[Outcome] = [Ok(7)]?
    print(values)?
    unused: Outcome = Ok(8)
    Some(9)
    10
    Ok(())
"#,
    );
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "3\n4\nResult[i64, str].Err(\"debug\")\nOption[i64].Some(5)\nRecord(result=Result[i64, str].Ok(6))\n[Result[i64, str].Ok(7)]\n");
}

#[test]
fn explicit_drop_matching_formatting_and_propagation_drop_owned_errors_once() {
    let output = support::run(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def fail(label: str) -> Result[(), Guard]:
    Err(Guard(label))
def propagate() -> Result[(), Failure]:
    fail("propagated")?
    Ok(())
def main() -> ():
    drop(fail("dropped"))
    match fail("matched"):
        case Ok(_):
            pass
        case Err(error):
            drop(error)
    error = fail("formatted")
    text = str.repr(error).unwrap()
    print(len(text) > 0).unwrap()
    drop(error)
    drop(propagate())
"#,
    );
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "dropped\nmatched\nTrue\nformatted\npropagated\n"
    );
}
