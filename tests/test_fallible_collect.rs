//! Eager collection conversion with recoverable output growth.
mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        text.lines()
            .filter(|s| !s.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n"),
        expected
    );
}

#[test]
fn collect_ranges_generators_and_owned_elements() {
    native(r#"
def numbers() -> Generator[i64]:
    yield 2
    yield 4
print(str.repr(list[i64].from(range(3))).unwrap()).unwrap()
print(str.repr(list[i64].from(numbers())).unwrap()).unwrap()
print(str.repr(list[list[i64]].from([[1].unwrap(), [2].unwrap()].unwrap())).unwrap()).unwrap()
"#, "Result[list[i64], AllocError].Ok([0, 1, 2])\nResult[list[i64], AllocError].Ok([2, 4])\nResult[list[list[i64]], AllocError].Ok([[1], [2]])");
}

#[test]
fn collection_conversion_checks_types_and_consumes_its_source() {
    for (source, message) in [
        ("list[i64].from(1)", "requires an owned collection"),
        ("list[u8].from(range(2))", "expected u8"),
        (
            "values = [1].unwrap()\ndrop(list[i64].from(values))\nprint(values).unwrap()",
            "use of moved binding `values`",
        ),
        (
            "values = [1].unwrap()\nlist[i64].from(&values)",
            "requires an owned collection",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn set_conversion_deduplicates_and_preserves_first_seen_order() {
    native(r#"
print(str.repr(set[i64].from([3, 1, 3, 2].unwrap())).unwrap()).unwrap()
print(str.repr(set[str].from(["a", "b", "a"].unwrap())).unwrap()).unwrap()
print(str.repr(set[i64].from(range(0))).unwrap()).unwrap()
print(str.repr(list[str].from({"a": 1, "b": 2}.unwrap())).unwrap()).unwrap()
"#, "Result[set[i64], AllocError].Ok({3, 1, 2})\nResult[set[str], AllocError].Ok({\"a\", \"b\"})\nResult[set[i64], AllocError].Ok(set())\nResult[list[str], AllocError].Ok([\"a\", \"b\"])");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn set_conversion_recovers_from_entry_and_hash_table_growth_failure() {
    for budget in 0..=6 {
        native(
            &format!(
                r#"
source = [0, 1, 2, 3, 4, 5, 6, 7, 8, 0].unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = set[i64].from(source)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
"#
            ),
            if budget == 6 {
                "Result[set[i64], AllocError].Ok({0, 1, 2, 3, 4, 5, 6, 7, 8})"
            } else {
                "Result[set[i64], AllocError].Err(AllocError.OutOfMemory)"
            },
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_collection_abandons_generator_before_its_next_effect() {
    native(
        r#"
class Resource:
    n: i64
    def __del__(self: &mut Resource) -> ():
        print("__test_restore_allocations__").unwrap()
        print(self.n).unwrap()
def numbers(r: Resource) -> Generator[i64]:
    yield 4
    print("unexpected continuation").unwrap()
    yield r.n
source = numbers(Resource(7))
print("__test_fail_allocations_after_1__").unwrap()
result = set[i64].from(source)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
"#,
        "7\nResult[set[i64], AllocError].Err(AllocError.OutOfMemory)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn output_failure_reclaims_partial_collection_and_remaining_source() {
    for budget in 0..=4 {
        native(
            &format!(
                r#"
source = [[1].unwrap(), [2].unwrap(), [3].unwrap(), [4].unwrap(), [5].unwrap(), [6].unwrap(), [7].unwrap(), [8].unwrap(), [9].unwrap()].unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = list[list[i64]].from(source)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
"#
            ),
            if budget == 4 {
                "Result[list[list[i64]], AllocError].Ok([[1], [2], [3], [4], [5], [6], [7], [8], [9]])"
            } else {
                "Result[list[list[i64]], AllocError].Err(AllocError.OutOfMemory)"
            },
        );
    }
}
