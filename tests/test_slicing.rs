//! Checked forward slices with explicit allocation failure and ownership.
mod support;
use rstest::rstest;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let visible = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim_end(), "{source}");
}

#[rstest]
#[case("0", "4", "[10, 20, 30, 40]")]
#[case("1", "3", "[20, 30]")]
#[case("-3", "-1", "[20, 30]")]
#[case("-100", "100", "[10, 20, 30, 40]")]
#[case("3", "1", "[]")]
#[case("100", "200", "[]")]
#[case("-100", "-99", "[]")]
#[case("-9223372036854775808", "9223372036854775807", "[10, 20, 30, 40]")]
#[case("9223372036854775807", "-9223372036854775808", "[]")]
fn list_bounds_are_clamped(#[case] start: &str, #[case] stop: &str, #[case] expected: &str) {
    native(
        &format!(
            "items = [10, 20, 30, 40]\nprint(items.try_slice({start}, {stop}))\nprint(len(items))"
        ),
        &format!("Result[list[i64], AllocError].Ok({expected})\n4"),
    );
}

#[test]
fn list_slice_retains_strings_and_nested_sum_tags_after_source_drop() {
    native(r#"
mut items = ["A" + "da", "B" + "ea", "C" + "am"]
slice = items.try_slice(1, 3)
items[1] = "changed"
drop(items)
print(slice)
values: list[Option[str]] = [Nothing, Some("é" + "🙂")]
result = values.try_slice(0, 2)
drop(values)
print(result)
print(list[i64]().try_slice(-10, 10))
print([1.5f32, 2.5f32].try_slice(1, 2))
"#, "Result[list[str], AllocError].Ok([\"Bea\", \"Cam\"])\nResult[list[Option[str]], AllocError].Ok([Option[str].Nothing, Option[str].Some(\"é🙂\")])\nResult[list[i64], AllocError].Ok([])\nResult[list[f32], AllocError].Ok([2.5])");
}

#[test]
fn slicing_borrows_fields_and_indices_and_evaluates_once_left_to_right() {
    native(r#"
class Data:
    items: list[i64]
def slice(items: &list[i64], start: &i64, stop: &i64) -> Result[list[i64], AllocError]:
    result = items.try_slice(start, stop)?
    Ok(result)
def data() -> list[i64]:
    print("receiver")
    [1, 2, 3]
def index(n: i64) -> i64:
    print(n)
    n
mut record = Data([10, 20, 30])
start = 0
stop = 2
print(slice(&record.items, &start, &stop))
print(data().try_slice(index(1), index(3)))
"#, "Result[list[i64], AllocError].Ok([10, 20])\nreceiver\n1\n3\nResult[list[i64], AllocError].Ok([2, 3])");
}

#[test]
fn explicit_copy_of_owned_elements_preserves_the_original() {
    native(
        r#"
def middle(items: &list[list[i64]]) -> Result[list[list[i64]], AllocError]:
    try_copy(items)?.try_slice(1, 2)
items = [[1], [2], [3]]
print(middle(&items))
print(items)
class Custom:
    def try_slice(self, n: i64) -> i64:
        n
print(Custom().try_slice(42))
"#,
        "Result[list[list[i64]], AllocError].Ok([[2]])\n[[1], [2], [3]]\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn every_list_slice_allocation_failure_preserves_borrowed_sources() {
    for (start, stop, allocations, selected) in [(0, 2, 2, "[\"Ada\", \"Bea\"]"), (2, 0, 1, "[]")] {
        for budget in 0..=allocations {
            native(&format!(r#"
items = ["A" + "da", "B" + "ea"]
print("__test_fail_allocations_after_{budget}__")
result = items.try_slice({start}, {stop})
print("__test_restore_allocations__")
print(result)
print(items)
print(items.try_slice({start}, {stop}))
"#), &format!("Result[list[str], AllocError].{}\n[\"Ada\", \"Bea\"]\nResult[list[str], AllocError].Ok({selected})", if budget < allocations { "Err(AllocError.OutOfMemory)".to_owned() } else { format!("Ok({selected})") }));
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn temporary_slice_transfers_selected_owners_and_cleans_every_failure() {
    for budget in 0..=2 {
        native(
            &format!(
                r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print("__test_restore_allocations__")
        print(self.id)
def consume(items: list[Guard]) -> list[Guard]:
    items
def sliced(items: list[Guard]) -> Result[list[Guard], AllocError]:
    print("__test_fail_allocations_after_{budget}__")
    result = consume(items).try_slice(1, 2)?
    print("__test_restore_allocations__")
    print("success")
    Ok(result)
result = sliced([Guard(10), Guard(20), Guard(30)])
print("returned")
drop(result)
"#
            ),
            if budget < 2 {
                "10\n20\n30\nreturned"
            } else {
                "10\n30\nsuccess\nreturned\n20"
            },
        );
    }
}

#[rstest]
#[case(
    "print([1].try_slice(0))",
    "try_slice requires start and stop arguments"
)]
#[case(
    "print([1].try_slice(0, 1, 1))",
    "try_slice requires start and stop arguments"
)]
#[case("print([1].try_slice(0u8, 1))", "expected i64")]
#[case("print({1: 2}.try_slice(0, 1))", "try_slice requires a list receiver")]
#[case(
    "items = [[1]]\nprint(items.try_slice(0, 1))",
    "try_slice with owned elements requires an owned temporary"
)]
#[case(
    "mut items = [[1]]\nloan = &mut items\nprint(loan.try_slice(0, 1))",
    "try_slice with owned elements requires an owned temporary"
)]
#[case(
    "mut items = [1]\nloan = &mut items\nprint(items.try_slice(0, 1))\nloan.append(2)",
    "borrow"
)]
fn invalid_list_slices_are_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
