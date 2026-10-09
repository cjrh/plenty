mod support;

#[test]
fn literals_propagate_with_context_and_no_prefix() {
    let output = support::run(
        r#"
def build() -> Result[list[list[u8]], AllocError]:
    [[1, 2]?, [3]?]
def main() -> Result[(), AllocError]:
    values: list[list[u8]] = build()?
    print(values).unwrap()
    squares: list[u8] = [n * n for n in range(6)]?
    print(squares).unwrap()
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "[[1, 2], [3]]\n[0, 1, 4, 9, 16, 25]\n"
    );
}

#[test]
fn ordinary_names_return_results() {
    let output = support::run(
        r#"
class Item:
    value: i64
enum Choice:
    Value(i64)
def main() -> Result[(), AllocError]:
    mut values = list[i64]()?
    values.append(42)?
    values.reserve(4)?
    print(copy(values)).unwrap()
    print(Item(7)).unwrap()
    print(Choice.Value(9)).unwrap()
    print(str.from(42)).unwrap()
    print("a" + "b").unwrap()
    print({"answer": 42}).unwrap()
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Result[list[i64], AllocError].Ok([42])")
    );
}

#[test]
fn result_entrypoint_returns_failure_status() {
    let output =
        support::run("def main() -> Result[(), AllocError]:\n    Err(AllocError.OutOfMemory)\n");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn old_prefixes_are_rejected() {
    for source in ["print(try [1])", "try_print(1)", "x = list[i64].try_new()"] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
    assert!(support::check_source("print = 1\nprint(42)")
        .unwrap_err()
        .to_string()
        .contains("not callable"));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn ranges_text_index_and_iteration_succeed_without_allocation() {
    let output = support::run(
        r#"
def main() -> ():
    print("__test_fail_allocations_after_0__").unwrap()
    sequence = range[u8](4)
    character = "é🙂"[1]
    mut count = 0
    for item in "é🙂":
        if item == "🙂":
            count = count + 1
    print("__test_restore_allocations__").unwrap()
    print(sequence).unwrap()
    print(character).unwrap()
    print(count).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("range(0, 4, 1)\n🙂\n1\n"), "{text}");
}

#[test]
fn main_propagates_io_errors_and_drops_owned_errors() {
    let output = support::run(
        r#"
class OwnedError:
    def __del__(self) -> ():
        print("cleaned").unwrap()
def main() -> Result[(), OwnedError]:
    Err(OwnedError().unwrap())
"#,
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"cleaned\n");
    let output = support::run(
        r#"
def main() -> Result[(), IoError]:
    print("success")?
    Ok(())
"#,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"success\n");
}

#[test]
fn dictionary_assignment_replaces_but_insertion_is_checked() {
    let output = support::run(
        r#"
def main() -> Result[(), AllocError]:
    mut values = {"a": 1}?
    values["a"] = 2
    values.insert("b", 3)?
    print(values).unwrap()
    Ok(())
"#,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"{\"a\": 2, \"b\": 3}\n");
    let output = support::run("mut values = {\"a\": 1}.unwrap()\nvalues[\"b\"] = 2");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("use insert"));
}

#[test]
fn builtin_unwrap_preserves_inherent_methods_and_type_depth_limits() {
    let output = support::run(
        r#"
class Wrapper:
    value: i64
    def unwrap(self) -> i64:
        self.value
def main() -> ():
    wrapper = Wrapper(42).unwrap()
    value: i64 = wrapper.unwrap()
    print(value).unwrap()
"#,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"42\n");
    let mut source = String::from("type T0 = i64\n");
    for n in 1..=64 {
        source.push_str(&format!("type T{n} = Option[T{}]\n", n - 1));
    }
    source.push_str("T64.Nothing.new()\n");
    assert!(support::check_source(&source)
        .unwrap_err()
        .to_string()
        .contains("type nesting exceeds"));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn structural_equality_needs_no_allocation() {
    let output = support::run(
        r#"
def main() -> Result[(), AllocError]:
    left = [[1.5, 2.0]?, [3.0]?]?
    right = copy(left)?
    print("__test_fail_allocations_after_0__").unwrap()
    equal = left == right
    print("__test_restore_allocations__").unwrap()
    print(equal).unwrap()
    Ok(())
"#,
    );
    assert!(output.status.success());
    assert!(output.stdout.ends_with(b"True\n"));
}
