use std::process::Command;

fn run(source: &str) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.plenty");
    let binary = dir.path().join("program");
    std::fs::write(&file, source).unwrap();
    plenty::compile_file_to_executable(&file, &binary, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    Command::new(binary).output().unwrap()
}
fn output(source: &str, expected: &str) {
    let result = run(source);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let visible = String::from_utf8_lossy(&result.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected);
}

#[test]
fn eager_and_lazy_defaults_preserve_evaluation_order() {
    output(
        r#"
def value(label: str, n: i64) -> i64:
    print(label).unwrap()
    n
def recover(error: str) -> i64:
    print(error).unwrap()
    9
def fallback() -> i64:
    print("fallback").unwrap()
    8
def main() -> ():
    good: Result[i64, str] = Ok(value("receiver", 7))
    print(good.unwrap_or(value("eager", 2))).unwrap()
    bad: Result[i64, str] = Err("cause")
    print(good.unwrap_or_else(recover), bad.unwrap_or_else(recover)).unwrap()
    present = Some(6)
    absent: Option[i64] = Nothing
    print(present.unwrap_or_else(fallback), absent.unwrap_or_else(fallback)).unwrap()
    print(present.ok_or_else(def() -> str: "missing").unwrap()).unwrap()
    drop(absent.ok_or_else(def() -> str: "missing"))
    print(good.expect("good"), present.expect("present")).unwrap()
"#,
        "receiver\neager\n7\ncause\n7 9\nfallback\n6 8\n6\n7 6",
    );
}

#[test]
fn mappings_compose_fallible_defaults_without_eager_propagation() {
    output(
        r#"
def fallback() -> Result[i64, ParseError]:
    print("fallback").unwrap()
    Err(ParseError.Invalid)
def main() -> ():
    yes = Some(4)
    no: Option[i64] = Nothing
    good: Result[i64, str] = Ok(5)
    bad: Result[i64, str] = Err("oops")
    print(yes.map_or(9, def(n: i64) -> i64: n), no.map_or(9, def(n: i64) -> i64: n)).unwrap()
    print(good.map_or(9, def(n: i64) -> i64: n), bad.map_or(9, def(n: i64) -> i64: n)).unwrap()
    print(yes.map_or_else(fallback, Ok).unwrap()).unwrap()
    print(no.map_or_else(fallback, Ok).is_err()).unwrap()
    print(good.map_or_else(def(e: str) -> str: e, def(n: i64) -> str: "success")).unwrap()
    print(bad.map_or_else(def(e: str) -> str: e, def(n: i64) -> str: "success")).unwrap()
"#,
        "4 9\n5 9\n4\nfallback\nTrue\nsuccess\noops",
    );
}

#[test]
fn expect_reports_message_and_call_site() {
    let result = run(
        "def main() -> ():\n    result: Result[i64, str] = Err(\"cause\")\n    drop(result.expect(\"configuration required\"))\n",
    );
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("configuration required"), "{stderr}");
    assert!(stderr.contains("main.plenty:3:"), "{stderr}");
}

#[test]
fn owned_callbacks_and_payloads_are_dropped_exactly_once() {
    output(
        r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
def main() -> ():
    unused = Guard("unused callback")
    some = Some(Guard("selected value"))
    selected = some.unwrap_or_else(def once [unused]() -> Guard: unused)
    print("returned").unwrap()
    drop(selected)
    captured = Guard("fallback value")
    bad: Result[Guard, Guard] = Err(Guard("error"))
    recover = def once [captured](error: Guard) -> Guard:
        print("recover").unwrap()
        captured
    recovered = bad.unwrap_or_else(recover)
    drop(recovered)
    good: Result[Guard, Guard] = Ok(Guard("eager success"))
    result = good.unwrap_or(Guard("eager unused"))
    drop(result)
    failed: Result[Guard, Guard] = Err(Guard("discarded error"))
    fallback = failed.unwrap_or(Guard("eager fallback"))
    drop(fallback)
"#,
        "unused callback\nreturned\nselected value\nrecover\nerror\nfallback value\neager unused\neager success\ndiscarded error\neager fallback",
    );
}

#[test]
fn unit_callbacks_and_payloads() {
    output(
        r#"
def absent() -> ():
    print("absent").unwrap()
def use(value: i64) -> ():
    print("some").unwrap()
def main() -> ():
    Some(()).unwrap_or_else(absent)
    no: Option[()] = Nothing
    no.unwrap_or_else(absent)
    good: Result[(), ()] = Ok(())
    good.unwrap_or(())
    good.expect("unit")
    Some(1).map_or((), use)
    Some(1).map_or_else(absent, use)
"#,
        "absent\nsome\nsome",
    );
}

#[test]
fn callbacks_have_independent_return_contracts() {
    let source = r#"
def fallback() -> Result[i64, IoError]:
    Ok(2)
def main() -> ():
    drop(Some(1).unwrap_or_else(fallback))
"#;
    let error = plenty::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains("expected") && error.contains("i64"),
        "{error}"
    );
}

#[test]
fn contextual_types_temporaries_and_error_widening() {
    output(
        r#"
def fallback() -> Result[i64, ParseError]:
    Ok(3)
def widened(value: Option[i64]) -> Result[i64, ParseError | IoError]:
    value.map_or_else(fallback, Ok)
def main() -> ():
    print(i64.parse("bad").unwrap_or(8)).unwrap()
    print(Result[i64, str].Err("bad").unwrap_or(9)).unwrap()
    byte: u8 = Nothing.unwrap_or_else(def() -> u8: 7u8)
    print(byte, widened(Some(4)).unwrap()).unwrap()
    unit: Option[()] = Nothing
    drop(unit.ok_or_else(def() -> str: "absent"))
"#,
        "8\n9\n7 4",
    );
}

#[test]
fn user_methods_with_the_same_names_keep_dispatch() {
    output(
        r#"
class Custom:
    def expect(self, a: i64, b: i64) -> i64:
        a
    def map_or_else(self) -> i64:
        5
    def unwrap_or_else(self, n: i64) -> i64:
        n
    def ok_or_else(self) -> i64:
        6
def main() -> ():
    c = Custom()
    print(c.expect(3, 4), Custom().map_or_else(), c.unwrap_or_else(7), Custom().ok_or_else()).unwrap()
"#,
        "3 5 7 6",
    );
}

#[test]
fn failed_argument_cleans_up_receiver_before_returning() {
    output(
        r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
def missing() -> Result[Guard, ParseError]:
    Err(ParseError.Invalid)
def recover() -> Result[Guard, ParseError]:
    value: Result[Guard, str] = Ok(Guard("receiver"))
    Ok(value.unwrap_or(missing()?))
def main() -> ():
    drop(recover())
"#,
        "receiver",
    );
}

#[test]
fn expect_evaluates_dynamic_message_on_success_and_reports_option_failure() {
    output(
        r#"
def message() -> str:
    print("message").unwrap()
    "must exist"
def main() -> ():
    print(Some(3).expect(message())).unwrap()
"#,
        "message\n3",
    );
    let result = run(
        "def main() -> ():\n    value: Option[i64] = Nothing\n    drop(value.expect(\"required option\"))\n",
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("required option"));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn helpers_allocate_nothing_and_preserve_callback_allocation_failures() {
    output(
        r#"
def fallback() -> Result[str, AllocError]:
    "a sufficiently long default ".concat("that must allocate storage")
def main() -> Result[(), AllocError | IoError]:
    value: Result[list[i64], ParseError] = Ok([4, 5]?)
    backup = [8]?
    no: Option[str] = Nothing
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    selected = value.unwrap_or_else(def once [backup](error: ParseError) -> list[i64]: backup)
    kept = Some("present").map_or_else(fallback, Ok)
    print("__test_end_no_allocations__")?
    failed = no.map_or_else(fallback, Ok)
    print("__test_restore_allocations__")?
    print(selected, kept?, failed.is_err())?
    Ok(())
"#,
        "[4, 5] present True",
    );
}

#[test]
fn expect_preserves_returned_reference_loans() {
    output(
        r#"
def first(values: &list[i64]) -> Option[&i64]:
    if len(values) == 0:
        return Nothing
    Some(&values[0])
def main() -> ():
    values = [7].unwrap()
    found = first(&values).expect("nonempty")
    print(*found).unwrap()
"#,
        "7",
    );
    let bad = r#"
def first(values: &list[i64]) -> Option[&i64]:
    Some(&values[0])
def main() -> ():
    mut values = [7].unwrap()
    found = first(&values).expect("nonempty")
    values.append(8).unwrap()
    print(*found).unwrap()
"#;
    assert!(plenty::check_source(bad).is_err());
}

#[test]
fn invalid_callbacks_and_arity_are_diagnosed() {
    for (source, message) in [
        (
            "drop(Some(1).unwrap_or_else(2))",
            "requires an owned callable",
        ),
        (
            "drop(Some(1).unwrap_or_else(def(n: i64) -> i64: n))",
            "callback must take ()",
        ),
        (
            "drop(Some(1).map_or_else(def() -> i64: 2))",
            "takes 2 argument(s)",
        ),
        ("drop(Some(1).expect(2))", "expected str"),
    ] {
        let error = plenty::check_source(&format!("def main() -> ():\n    {source}\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{source}\n{error}");
    }
}
