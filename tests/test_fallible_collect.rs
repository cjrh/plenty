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
print(list[i64].try_from(range(3)))
print(list[i64].try_from(numbers()))
print(list[list[i64]].try_from([[1], [2]]))
"#, "Result[list[i64], AllocError].Ok([0, 1, 2])\nResult[list[i64], AllocError].Ok([2, 4])\nResult[list[list[i64]], AllocError].Ok([[1], [2]])");
}

#[test]
fn collection_conversion_checks_types_and_consumes_its_source() {
    for (source, message) in [
        ("list[i64].try_from(1)", "requires an owned collection"),
        ("list[u8].try_from(range(2))", "expected u8"),
        (
            "values = [1]\nlist[i64].try_from(values)\nprint(values)",
            "moved or possibly moved",
        ),
        (
            "values = [1]\nlist[i64].try_from(&values)",
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
print(set[i64].try_from([3, 1, 3, 2]))
print(set[str].try_from(["a", "b", "a"]))
print(set[i64].try_from(range(0)))
print(list[str].try_from({"a": 1, "b": 2}))
"#, "Result[set[i64], AllocError].Ok({3, 1, 2})\nResult[set[str], AllocError].Ok({\"a\", \"b\"})\nResult[set[i64], AllocError].Ok(set())\nResult[list[str], AllocError].Ok([\"a\", \"b\"])");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn set_conversion_recovers_from_entry_and_hash_table_growth_failure() {
    for budget in 0..=6 {
        native(
            &format!(
                r#"
source = [0, 1, 2, 3, 4, 5, 6, 7, 8, 0]
print("__test_fail_allocations_after_{budget}__")
result = set[i64].try_from(source)
print("__test_restore_allocations__")
print(result)
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
        print("__test_restore_allocations__")
        print(self.n)
def numbers(r: Resource) -> Generator[i64]:
    yield 4
    print("unexpected continuation")
    yield r.n
source = numbers(Resource(7))
print("__test_fail_allocations_after_1__")
result = set[i64].try_from(source)
print("__test_restore_allocations__")
print(result)
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
source = [[1], [2], [3], [4], [5], [6], [7], [8], [9]]
print("__test_fail_allocations_after_{budget}__")
result = list[list[i64]].try_from(source)
print("__test_restore_allocations__")
print(result)
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
