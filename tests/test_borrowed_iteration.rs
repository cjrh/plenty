mod support;

#[test]
fn generic_count_uses_one_reference_rule() {
    let output = support::run(
        r#"
def count_eq[T](values: &list[T], target: &T) -> i64:
    mut n = 0
    for v in values:
        if v == target:
            n = n + 1
    n
class Point:
    x: i64
def main() -> ():
    nums = [1, 2, 1].unwrap()
    texts = ["a", "b", "a"].unwrap()
    target = 1
    text = "a"
    print(count_eq(&nums, &target)).unwrap()
    print(count_eq(&texts, &text)).unwrap()
    for v in &nums:
        print(*v).unwrap()
    print(&target == target).unwrap()
    print(target == &target).unwrap()
    points = [Point(1), Point(2), Point(1)].unwrap()
    point = Point(1)
    print(count_eq(&points, &point)).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "2\n2\n1\n2\n1\nTrue\nTrue\n2\n"
    );
}

#[test]
fn items_always_borrow_keys_and_values() {
    let output = support::run(
        r#"
def inspect[T](value: &T) -> ():
    print(value).unwrap()
def main() -> ():
    mut numbers = {"key": 3}.unwrap()
    for key, value in numbers.items():
        inspect(key)
        inspect(value)
        print(*value).unwrap()
    for key, value in &mut numbers.items():
        print(*key).unwrap()
        *value = 7
    print(numbers["key"]).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "key\n3\n3\nkey\n7\n"
    );
}

#[test]
fn other_borrowed_iterables_yield_shared_references() {
    let output = support::run(
        r#"
def read(value: &i64) -> ():
    print(value).unwrap()
def main() -> ():
    values = {9}.unwrap()
    interval = range(2)
    text = "ab"
    for value in &values:
        read(value)
    for value in &interval:
        read(value)
    for character in &text:
        print(*character).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9\n0\n1\na\nb\n");
}

#[test]
fn element_borrows_prevent_mutation_and_cannot_escape_temporary_slots() {
    for source in [
        "mut xs = [1].unwrap()\nfor value in &xs:\n    xs.append(2).unwrap()\n    print(value).unwrap()",
        "def escape(values: &range[i64]) -> &i64:\n    for value in values:\n        return value\n    escape(values)",
        "mut xs = [1].unwrap()\nfor value in &xs:\n    *value = 2",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn borrowed_iteration_adds_no_allocations() {
    let output = support::run(
        r#"
def main() -> ():
    values = [1, 2, 3].unwrap()
    numbers = {"a": 4, "b": 5}.unwrap()
    interval = range(2)
    text = "ab"
    mut total = 0
    print("__test_begin_no_allocations__").unwrap()
    for value in &values:
        total = total + value
    for key, value in numbers.items():
        total = total + value
    for value in &interval:
        total = total + value
    for character in &text:
        if character == "a":
            total = total + 1
    print("__test_end_no_allocations__").unwrap()
    print(total).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).ends_with("17\n"));
}
