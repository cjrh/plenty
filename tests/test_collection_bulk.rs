//! Bulk mutation reserves first and explicitly consumes its source collection.
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

#[test]
fn extend_moves_elements_in_order_through_exclusive_references_and_fields() {
    native(r#"
def extend(items: &mut list[i64], other: list[i64]) -> Result[(), AllocError]:
    items.try_extend(other)?
    Ok(())
class Data:
    items: list[i64]
mut data = Data([1, 2])
other = [3, 4, 5]
print(extend(&mut data.items, other))
print(data.items.try_extend([]))
print(data.items)
mut nested = [[1]]
print(nested.try_extend([[2], [3]]))
print(nested)
class Custom:
    def try_extend(self, n: i64) -> i64:
        n
print(Custom().try_extend(42))
"#, "Result[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\n[1, 2, 3, 4, 5]\nResult[(), AllocError].Ok(())\n[[1], [2], [3]]\n42");
}

#[test]
fn fallible_copy_preserves_the_source_and_arguments_run_before_mutation() {
    native(
        r#"
def append_copy(items: &mut list[str], source: &list[str]) -> Result[(), AllocError]:
    items.try_extend(try_copy(source)?)
def more(items: &list[i64]) -> list[i64]:
    print(len(items))
    [len(items), len(items) + 1]
mut items = [10]
print(items.try_extend(more(&items)))
print(items)
source = ["A" + "da"]
mut names = ["Bea"]
print(append_copy(&mut names, &source))
drop(names)
print(source)
"#,
        "1\nResult[(), AllocError].Ok(())\n[10, 1, 2]\nResult[(), AllocError].Ok(())\n[\"Ada\"]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn extension_failure_preserves_destination_and_drops_consumed_owners_once() {
    for budget in 0..=1 {
        native(
            &format!(
                r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print("__test_restore_allocations__")
        print(self.id)
mut destination = list[Guard]()
source = [Guard(1), Guard(2), Guard(3)]
print("__test_fail_allocations_after_{budget}__")
result = destination.try_extend(source)
print("__test_restore_allocations__")
print(result)
print(len(destination))
print("drop destination")
drop(destination)
"#
            ),
            if budget == 0 {
                "1\n2\n3\nResult[(), AllocError].Err(AllocError.OutOfMemory)\n0\ndrop destination"
            } else {
                "Result[(), AllocError].Ok(())\n3\ndrop destination\n1\n2\n3"
            },
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn extension_reuses_reserved_capacity_and_empty_sources_allocate_nothing() {
    native(
        r#"
def run() -> Result[list[str], AllocError]:
    mut items = list[str].try_with_capacity(3)?
    source = ["A" + "da", "B" + "ea"]
    empty = list[str]()
    print("__test_fail_allocations_after_0__")
    print("__test_begin_no_allocations__")
    items.try_extend(source)?
    items.try_extend(empty)?
    print("__test_restore_allocations__")
    print("__test_end_no_allocations__")
    Ok(items)
print(run())
"#,
        "Result[list[str], AllocError].Ok([\"Ada\", \"Bea\"])",
    );
}

#[rstest]
#[case("items = [1]\nitems.try_extend([2])", "immutable")]
#[case(
    "mut items = [1]\nsource = [2]\nitems.try_extend(source)\nprint(source)",
    "moved"
)]
#[case("mut items = [1]\nitems.try_extend(items)", "moved")]
#[case(
    "mut items = [1]\nsource = [2]\nitems.try_extend(&source)",
    "expected list[i64]"
)]
#[case("mut items = [1]\nitems.try_extend([2u8])", "expected list[i64]")]
#[case(
    "mut items = [1]\nitems.try_extend()",
    "try_extend requires 1 argument"
)]
#[case("[1].try_extend([2])", "try_extend requires a mutable list")]
#[case(
    "mut items = [1]\nloan = &items\nitems.try_extend([2])\nprint(loan)",
    "borrow"
)]
fn invalid_extension(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
