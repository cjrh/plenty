//! In-place collection operations with exclusive borrowing and deterministic cleanup.
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
fn reverse_mutates_in_place_and_returns_unit() {
    native(
        r#"
def reverse(items: &mut list[i64]) -> ():
    items.reverse()
class Data:
    items: list[i64]
mut data = Data([1, 2, 3, 4])
reverse(&mut data.items)
print(data.items)
data.items.reverse()
print(data.items)
mut empty: list[i64] = []
empty.reverse()
print(empty)
mut one = [42]
one.reverse()
print(one)
class Custom:
    def reverse(self, n: i64) -> i64:
        n
print(Custom().reverse(7))
"#,
        "[4, 3, 2, 1]\n[1, 2, 3, 4]\n[]\n[42]\n7",
    );
}

#[test]
fn reversal_moves_owned_slots_without_early_cleanup() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print(self.id)
mut items = [Guard(1), Guard(2), Guard(3)]
items.reverse()
print("reversed")
drop(items)
mut nested = [[1], [2], [3]]
nested.reverse()
print(nested)
"#,
        "reversed\n3\n2\n1\n[[3], [2], [1]]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reversal_allocates_nothing_with_managed_elements() {
    native(
        r#"
mut items = ["A" + "da", "B" + "ea", "C" + "am"]
saved = items[0]
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
items.reverse()
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(items)
drop(items)
print(saved)
"#,
        "[\"Cam\", \"Bea\", \"Ada\"]\nAda",
    );
}

#[rstest]
#[case("items = [1]\nitems.reverse()", "immutable")]
#[case("print([1].reverse())", "reverse requires a mutable list")]
#[case("mut items = [1]\nitems.reverse(1)", "reverse takes no arguments")]
#[case("mut items = {1}\nitems.reverse()", "reverse requires a mutable list")]
#[case(
    "mut items = [1]\nloan = &items\nitems.reverse()\nprint(loan)",
    "borrow"
)]
#[case("items = [1]\nloan = &items\nloan.reverse()", "shared")]
fn invalid_reverse(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn clear_empties_collections_and_allows_reuse_through_fields_and_references() {
    native(
        r#"
def clear(items: &mut list[i64]) -> ():
    items.clear()
class Data:
    items: list[i64]
mut data = Data([1, 2, 3])
clear(&mut data.items)
print(data.items)
data.items.append(42)
print(data.items)
mut entries = {"a": 1, "b": 2}
entries.clear()
entries.clear()
print(entries)
entries["b"] = 20
entries["a"] = 10
print(entries)
mut members = {1, 2, 3}
members.clear()
print(len(members))
members.add(4)
print(4 in members)
class Custom:
    def clear(self, value: i64) -> i64:
        value
print(Custom().clear(7))
"#,
        "[]\n[42]\n{}\n{\"b\": 20, \"a\": 10}\n0\nTrue\n7",
    );
}

#[test]
fn clear_runs_nested_cleanup_once_in_element_or_insertion_order() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print(self.id)
class Bag:
    items: list[Guard]
    def __del__(self) -> ():
        print("bag")
        self.items.clear()
        print("empty")
mut items = [[Guard(1), Guard(2)], [Guard(3)]]
items.clear()
print("list empty")
mut entries = {2: Guard(20), 1: Guard(10)}
entries.clear()
print("dict empty")
drop(Bag([Guard(4), Guard(5)]))
"#,
        "1\n2\n3\nlist empty\n20\n10\ndict empty\nbag\n4\n5\nempty",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn clear_and_reuse_keep_capacity_and_allocate_nothing() {
    native(r#"
mut items = ["A" + "da", "B" + "ea"]
mut entries = {"a": "Ada", "b": "Bea"}
mut members = {"a", "b"}
saved = items[0]
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
items.clear()
entries.clear()
members.clear()
items.clear()
a = items.try_append(saved)
b = entries.try_insert("c", saved)
c = members.try_add(saved)
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(items)
print(entries)
print(len(members))
print(a)
print(b)
print(c)
"#, "[\"Ada\"]\n{\"c\": \"Ada\"}\n1\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())");
}

#[rstest]
#[case("items = [1]\nitems.clear()", "immutable")]
#[case("[1].clear()", "clear requires a mutable")]
#[case("mut items = {1: 2}\nitems.clear(1)", "clear takes no arguments")]
#[case("mut text = \"x\"\ntext.clear()", "clear requires a mutable")]
#[case("mut items = {1}\nloan = &items\nitems.clear()\nprint(loan)", "borrow")]
#[case("items = [1]\nloan = &items\nloan.clear()", "shared")]
fn invalid_clear(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
