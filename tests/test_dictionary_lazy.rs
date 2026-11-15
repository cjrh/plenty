//! Lazy dictionary factories run only after one missing-key probe (issue #966).
mod support;

fn run(source: &str, expected: &str) {
    for optimize in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("lazy-dict");
        plenty::compile_source_to_executable_with_options(
            source,
            &binary,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
        let output = std::process::Command::new(binary).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn hit_skips_factories_and_misses_append_once() {
    run(
        r#"
def make() -> i64:
    print("factory").unwrap()
    7
def main() -> ():
    mut values = {"a": 3}.unwrap()
    value = values.get_or_insert_with("a", make).unwrap()
    *value = 4
    print(values).unwrap()
    second = values.get_or_insert_with("b", make).unwrap()
    *second = 8
    print(values).unwrap()
"#,
        "{\"a\": 4}\nfactory\n{\"a\": 4, \"b\": 8}\n",
    );
}

#[test]
fn factory_errors_leave_contents_and_order_unchanged() {
    run(
        r#"
@copy
enum Fault:
    Failed(i64)
def fail() -> Result[i64, Fault]:
    Err(Fault.Failed(9))
def main() -> ():
    mut values = {"a": 3}.unwrap()
    print(*values.get_or_insert_with("a", fail).unwrap()).unwrap()
    match values.get_or_insert_with("b", fail):
        case Ok(value):
            print(*value).unwrap()
        case Err(DictInitError.Factory(Fault.Failed(n))):
            print("failed", n).unwrap()
        case Err(DictInitError.Allocation(_)):
            print("allocation").unwrap()
    values.insert("c", 4).unwrap()
    print(values).unwrap()
"#,
        "3\nfailed 9\n{\"a\": 3, \"c\": 4}\n",
    );
}

#[test]
fn owned_once_and_borrowed_mutable_factories_preserve_cleanup_and_loans() {
    run(
        r#"
def main() -> ():
    mut values: dict[str, list[i64]] = {}.unwrap()
    payload = [1, 2].unwrap()
    make = def once [payload]() -> list[i64]:
        payload
    first = values.get_or_insert_with("a", make).unwrap()
    first.append(3).unwrap()
    unused = [4].unwrap()
    skip = def once [unused]() -> list[i64]:
        unused
    print(values.get_or_insert_with("a", skip).unwrap()).unwrap()
    mut calls = 0
    mut borrowed = def [&mut calls]() -> list[i64]:
        unchecked:
            calls += 1
        [9].unwrap()
    second = values.get_or_insert_with("b", &mut borrowed).unwrap()
    unchecked:
        calls += 1
    second.append(10).unwrap()
    print(calls, values).unwrap()
"#,
        "[1, 2, 3]\n2 {\"a\": [1, 2, 3], \"b\": [9, 10]}\n",
    );
}

#[test]
fn factory_cannot_alias_dictionary_and_returned_loan_protects_receiver() {
    let aliased = r#"
def main() -> ():
    mut values = {"a": 1}.unwrap()
    make = def [&values]() -> i64:
        len(values)
    print(values.get_or_insert_with("b", make).unwrap()).unwrap()
"#;
    let error = support::check_diagnostic(aliased);
    assert!(error.contains("conflicting borrow"), "{error}");
    let held = r#"
def make() -> i64:
    0
def main() -> ():
    mut values = {"a": 1}.unwrap()
    value = values.get_or_insert_with("b", make).unwrap()
    print(values).unwrap()
    print(*value).unwrap()
"#;
    let error = support::check_diagnostic(held);
    assert!(error.contains("conflicting borrow"), "{error}");
}

#[test]
fn key_and_factory_expressions_evaluate_in_order_even_on_hit() {
    run(
        r#"
def key() -> str:
    print("key").unwrap()
    "a"
def make() -> i64:
    print("body").unwrap()
    7
def factory() -> Callable[[], i64]:
    print("factory expression").unwrap()
    make
def main() -> ():
    mut values = {"a": 1}.unwrap()
    print(*values.get_or_insert_with(key(), factory()).unwrap()).unwrap()
"#,
        "key\nfactory expression\n1\n",
    );
}

#[test]
fn owned_factory_captures_drop_on_hit_and_value_drops_once() {
    run(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print("drop", self.label).unwrap()
def main() -> ():
    mut values = {"a": Guard("stored")}.unwrap()
    unused = Guard("unused")
    skip = def once [unused]() -> Guard:
        unused
    print(values.get_or_insert_with("a", skip).unwrap().label).unwrap()
    made = Guard("inserted")
    create = def once [made]() -> Guard:
        made
    print(values.get_or_insert_with("b", create).unwrap().label).unwrap()
    print("done").unwrap()
"#,
        "drop unused\nstored\ninserted\ndone\ndrop stored\ndrop inserted\n",
    );
}

#[test]
fn growth_failure_after_factory_drops_value_and_leaves_dictionary_empty() {
    support::assert_visible_stdout(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print("drop").unwrap()
def make() -> Guard:
    print("made").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    Guard("new value")
def main() -> ():
    mut values: dict[str, Guard] = {}.unwrap()
    match values.get_or_insert_with(("long owned incoming key" + "").unwrap(), make):
        case Ok(_):
            print("unexpected success").unwrap()
        case Err(_):
            print("growth failed").unwrap()
    print("__test_restore_allocations__").unwrap()
    print(len(values)).unwrap()
    print("__test_small_live_heap__").unwrap()
"#,
        if cfg!(feature = "runtime-checks") {
            "made\ndrop\ngrowth failed\n0"
        } else {
            "made\nunexpected success\n1\ndrop"
        },
    );
}

#[test]
fn result_values_and_allocation_error_factories_have_distinct_types() {
    run(
        r#"
def result_value() -> Result[i64, AllocError]:
    Ok(7)
def empty() -> Result[list[i64], AllocError]:
    []
def main() -> Result[(), AllocError | DictInitError[AllocError] | IoError]:
    mut results: dict[str, Result[i64, AllocError]] = {}?
    value: Result[&mut Result[i64, AllocError], AllocError] = results.get_or_insert_with("a", result_value)
    print(str.repr(value.unwrap())?)?
    mut lists: dict[str, list[i64]] = {}?
    group = lists.get_or_insert_with("a", empty)?
    group.append(2)?
    print(lists)?
    Ok(())
"#,
        "Result[i64, AllocError].Ok(7)\n{\"a\": [2]}\n",
    );
}

#[test]
fn lazy_dictionary_documentation_examples_run() {
    for page in [
        include_str!("../book/src/design/facilities/01-builtins/dictionaries.md"),
        include_str!("../book/src/tutorial/10-functions/named-callbacks.md"),
    ] {
        for fence in page.split("```plenty\n").skip(1) {
            let (source, rest) = fence.split_once("\n```").unwrap();
            if source.contains("get_or_insert_with") {
                let expected = rest
                    .split_once("```output\n")
                    .unwrap()
                    .1
                    .split_once("\n```")
                    .unwrap()
                    .0;
                run(source, &format!("{expected}\n"));
            }
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn factory_allocation_error_is_preserved_without_inserting() {
    support::assert_visible_stdout(
        r#"
def make() -> Result[list[i64], AllocError]:
    print("__test_fail_allocations_after_0__").unwrap()
    []
def main() -> ():
    mut values: dict[str, list[i64]] = {}.unwrap()
    match values.get_or_insert_with("a", make):
        case Ok(_):
            print("unexpected success").unwrap()
        case Err(DictInitError.Factory(_)):
            print("factory failed").unwrap()
        case Err(DictInitError.Allocation(_)):
            print("unexpected growth error").unwrap()
    print("__test_restore_allocations__").unwrap()
    print(len(values)).unwrap()
    print("__test_small_live_heap__").unwrap()
"#,
        "factory failed\n0",
    );
}

#[test]
fn generic_factory_works_through_mutable_dictionary_reference() {
    run(
        r#"
def insert[F: OnceCallable[[], i64]](values: &mut dict[str, i64], factory: F) -> Result[&mut i64, AllocError]:
    values.get_or_insert_with("a", factory)
def main() -> ():
    mut values: dict[str, i64] = {}.unwrap()
    captured = 8
    make = def once [captured]() -> i64:
        captured
    value = insert(&mut values, make).unwrap()
    *value = 9
    print(values).unwrap()
"#,
        "{\"a\": 9}\n",
    );
}
