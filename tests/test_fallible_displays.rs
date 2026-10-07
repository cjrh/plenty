//! Visible allocation boundaries for collection displays.
mod support;

fn native(source: &str, expected: &str) {
    let out = support::run(source);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.lines()
            .filter(|s| !s.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n"),
        expected
    );
}

#[test]
fn literals_preserve_types_order_and_duplicate_rules() {
    native(r#"
print(try [1, 2, 3])
print(try {3, 1, 3})
print(try {"a": 1, "a": 2})
empty: Result[list[u8], AllocError] = try []
print(empty)
def nested() -> Result[list[list[i64]], AllocError]:
    try [(try [1, 2])?, (try [3])?]
print(nested())
"#, "Result[list[i64], AllocError].Ok([1, 2, 3])\nResult[set[i64], AllocError].Ok({3, 1})\nResult[dict[str, i64], AllocError].Ok({\"a\": 2})\nResult[list[u8], AllocError].Ok([])\nResult[list[list[i64]], AllocError].Ok([[1, 2], [3]])");
}

#[test]
fn comprehensions_support_nested_loops_filters_and_all_collection_kinds() {
    native(r#"
print(try [a * b for a in range(3) for b in range(3) if b != 1])
print(try {a % 2 for a in range(5)})
print(try {a: a * a for a in range(3)})
"#, "Result[list[i64], AllocError].Ok([0, 0, 0, 2, 0, 4])\nResult[set[i64], AllocError].Ok({0, 1})\nResult[dict[i64, i64], AllocError].Ok({0: 0, 1: 1, 2: 4})");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_comprehension_stops_before_advancing_its_generator() {
    native(
        r#"
def values() -> Generator[i64]:
    yield 1
    print("unexpected resume")
    yield 2
source = values()
print("__test_fail_allocations_after_1__")
result = try [n for n in source]
print("__test_restore_allocations__")
print(result)
"#,
        "Result[list[i64], AllocError].Err(AllocError.OutOfMemory)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn nested_failure_stops_both_loops() {
    native(
        r#"
def mark(n: i64) -> i64:
    print("entry")
    n
outer = [1, 2]
inner = [3, 4]
print("__test_fail_allocations_after_1__")
result = try [mark(a + b) for a in outer for b in &inner]
print("__test_restore_allocations__")
print(result)
"#,
        "entry\nResult[list[i64], AllocError].Err(AllocError.OutOfMemory)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn comprehension_growth_failure_reclaims_owned_elements() {
    for budget in 0..=4 {
        native(
            &format!(
                r#"
source = [[1], [2], [3], [4], [5], [6], [7], [8], [9]]
print("__test_fail_allocations_after_{budget}__")
result = try [value for value in source]
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

#[cfg(feature = "runtime-checks")]
#[test]
fn failure_at_each_literal_allocation_is_recoverable() {
    for (literal, allocations, value) in [
        (
            "[1, 2, 3, 4, 5, 6, 7, 8, 9]",
            4,
            "Result[list[i64], AllocError].Ok([1, 2, 3, 4, 5, 6, 7, 8, 9])",
        ),
        ("{1, 2}", 3, "Result[set[i64], AllocError].Ok({1, 2})"),
        (
            "{1: 2, 3: 4}",
            3,
            "Result[dict[i64, i64], AllocError].Ok({1: 2, 3: 4})",
        ),
    ] {
        for budget in 0..=allocations {
            let source = format!("print(\"__test_fail_allocations_after_{budget}__\")\nresult = try {literal}\nprint(\"__test_restore_allocations__\")\nprint(result)");
            let failure = format!(
                "{}.Err(AllocError.OutOfMemory)",
                value.split(".Ok").next().unwrap()
            );
            native(
                &source,
                if budget == allocations {
                    value
                } else {
                    &failure
                },
            );
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_growth_skips_later_entry_expressions() {
    native(
        r#"
def unexpected() -> i64:
    print("unexpected")
    2
print("__test_fail_allocations_after_1__")
result = try [1, unexpected()]
print("__test_restore_allocations__")
print(result)
"#,
        "Result[list[i64], AllocError].Err(AllocError.OutOfMemory)",
    );
}
