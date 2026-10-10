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
mut data = Data([1, 2, 3, 4].unwrap())
reverse(&mut data.items)
print(data.items).unwrap()
data.items.reverse()
print(data.items).unwrap()
mut empty: list[i64] = [].unwrap()
empty.reverse()
print(empty).unwrap()
mut one = [42].unwrap()
one.reverse()
print(one).unwrap()
class Custom:
    def reverse(self, n: i64) -> i64:
        n
print(Custom().reverse(7)).unwrap()
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
        print(self.id).unwrap()
mut items = [Guard(1), Guard(2), Guard(3)].unwrap()
items.reverse()
print("reversed").unwrap()
drop(items)
mut nested = [[1].unwrap(), [2].unwrap(), [3].unwrap()].unwrap()
nested.reverse()
print(nested).unwrap()
"#,
        "reversed\n3\n2\n1\n[[3], [2], [1]]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reversal_allocates_nothing_with_managed_elements() {
    native(
        r#"
mut items = [("A" + "da").unwrap(), ("B" + "ea").unwrap(), ("C" + "am").unwrap()].unwrap()
saved = items[0]
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
items.reverse()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(items).unwrap()
drop(items)
print(saved).unwrap()
"#,
        "[\"Cam\", \"Bea\", \"Ada\"]\nAda",
    );
}

#[rstest]
#[case("items = [1].unwrap()\nitems.reverse()", "immutable")]
#[case(
    "print([1].unwrap().reverse()).unwrap()",
    "reverse requires a mutable list"
)]
#[case(
    "mut items = [1].unwrap()\nitems.reverse(1)",
    "reverse takes no arguments"
)]
#[case(
    "mut items = {1}.unwrap()\nitems.reverse()",
    "reverse requires a mutable list"
)]
#[case(
    "mut items = [1].unwrap()\nloan = &items\nitems.reverse()\nprint(loan).unwrap()",
    "borrow"
)]
#[case("items = [1].unwrap()\nloan = &items\nloan.reverse()", "shared")]
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
mut data = Data([1, 2, 3].unwrap())
clear(&mut data.items)
print(data.items).unwrap()
data.items.append(42).unwrap()
print(data.items).unwrap()
mut entries = {"a": 1, "b": 2}.unwrap()
entries.clear()
entries.clear()
print(entries).unwrap()
entries.insert("b", 20).unwrap()
entries.insert("a", 10).unwrap()
print(entries).unwrap()
mut members = {1, 2, 3}.unwrap()
members.clear()
print(len(members)).unwrap()
members.add(4).unwrap()
print(4 in members).unwrap()
class Custom:
    def clear(self, value: i64) -> i64:
        value
print(Custom().clear(7)).unwrap()
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
        print(self.id).unwrap()
class Bag:
    items: list[Guard]
    def __del__(self) -> ():
        print("bag").unwrap()
        self.items.clear()
        print("empty").unwrap()
mut items = [[Guard(1), Guard(2)].unwrap(), [Guard(3)].unwrap()].unwrap()
items.clear()
print("list empty").unwrap()
mut entries = {2: Guard(20), 1: Guard(10)}.unwrap()
entries.clear()
print("dict empty").unwrap()
drop(Bag([Guard(4), Guard(5)].unwrap()))
"#,
        "1\n2\n3\nlist empty\n20\n10\ndict empty\nbag\n4\n5\nempty",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn clear_and_reuse_keep_capacity_and_allocate_nothing() {
    native(r#"
mut items = [("A" + "da").unwrap(), ("B" + "ea").unwrap()].unwrap()
mut entries = {"a": "Ada", "b": "Bea"}.unwrap()
mut members = {"a", "b"}.unwrap()
saved = items[0]
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
items.clear()
entries.clear()
members.clear()
items.clear()
a = items.append(saved)
b = entries.insert("c", saved)
c = members.add(saved)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(items).unwrap()
print(entries).unwrap()
print(len(members)).unwrap()
print(str.repr(a).unwrap()).unwrap()
print(str.repr(b).unwrap()).unwrap()
print(str.repr(c).unwrap()).unwrap()
"#, "[\"Ada\"]\n{\"c\": \"Ada\"}\n1\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())");
}

#[rstest]
#[case("items = [1].unwrap()\nitems.clear()", "immutable")]
#[case("[1].unwrap().clear()", "clear requires a mutable")]
#[case(
    "mut items = {1: 2}.unwrap()\nitems.clear(1)",
    "clear takes no arguments"
)]
#[case("mut text = \"x\"\ntext.clear()", "clear requires a mutable")]
#[case(
    "mut items = {1}.unwrap()\nloan = &items\nitems.clear()\nprint(loan).unwrap()",
    "borrow"
)]
#[case("items = [1].unwrap()\nloan = &items\nloan.clear()", "shared")]
fn invalid_clear(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn sparse_dictionary_traversal_snapshots_copy_and_update_preserve_order() {
    native(r#"
mut data = {"a": 1, "b": 2, "c": 3, "d": 4}.unwrap()
drop(data.pop("a"))
drop(data.pop("c"))
data.insert("b", 20).unwrap()
data.insert("a", 10).unwrap()
print(data).unwrap()
print(data.keys().unwrap()).unwrap()
print(data.values().unwrap()).unwrap()
print(copy(data).unwrap() == data).unwrap()
print([k for k in &data].unwrap()).unwrap()
for key, value in data.items():
    print(key).unwrap()
    print(value).unwrap()
mut total = 0
for key in &data:
    for inner in &data:
        if inner == "d":
            continue
        total = total + data[key]
print(total).unwrap()
mut source = {"a": 100, "discard": 0, "z": 26}.unwrap()
drop(source.pop("discard"))
data.update(source).unwrap()
print(data).unwrap()
data.clear()
print(len(data)).unwrap()
data.insert("fresh", 7).unwrap()
print(data).unwrap()
"#, "{\"b\": 20, \"d\": 4, \"a\": 10}\n[\"b\", \"d\", \"a\"]\n[20, 4, 10]\nTrue\n[\"b\", \"d\", \"a\"]\nb\n20\nd\n4\na\n10\n68\n{\"b\": 20, \"d\": 4, \"a\": 100, \"z\": 26}\n0\n{\"fresh\": 7}");
}

#[test]
fn sparse_sets_support_iteration_bulk_operations_and_equality() {
    native(
        r#"
mut values = {0, 1, 2, 3, 4, 5}.unwrap()
values.discard(0)
values.discard(2)
values.discard(4)
values.add(0).unwrap()
mut total = 0
for value in &values:
    total = total + value
print(total).unwrap()
print(values == {0, 1, 3, 5}.unwrap()).unwrap()
print(copy(values).unwrap() == values).unwrap()
values.intersection_update({0, 1, 5}.unwrap())
print(values == {0, 1, 5}.unwrap()).unwrap()
values.difference_update({1}.unwrap())
values.update({7, 0}.unwrap()).unwrap()
print(values == {0, 5, 7}.unwrap()).unwrap()
print(len(values.union({7, 8}.unwrap()).unwrap())).unwrap()
values.clear()
print(len(values)).unwrap()
values.add(0).unwrap()
print(0 in values).unwrap()
"#,
        "9\nTrue\nTrue\nTrue\nTrue\n4\n0\nTrue",
    );
}

#[test]
fn sparse_inline_values_survive_slot_reuse_growth_and_owned_snapshots() {
    native(
        r#"
class Guard:
    id: i64
    data: range[i64]
    def __del__(self) -> ():
        print(self.id).unwrap()
def take(data: dict[i64, Guard]) -> dict[i64, Guard]:
    data
mut data = {0: Guard(0, range(10, 13)), 1: Guard(1, range(20, 24)), 2: Guard(2, range(30, 35))}.unwrap()
saved = data.pop(1).unwrap()
data.insert(3, Guard(3, range(40, 46))).unwrap()
data.reserve(100).unwrap()
print(len(saved.data)).unwrap()
print(len(data[3].data)).unwrap()
drop(saved)
drop(data.pop(0))
data.reserve(300).unwrap()
print("snapshot").unwrap()
items = take(data).values().unwrap()
drop(items)
print("clear").unwrap()
mut other = {4: Guard(4, range(4)), 5: Guard(5, range(5))}.unwrap()
drop(other.pop(4))
other.clear()
print("done").unwrap()
"#,
        "4\n6\n1\n0\nsnapshot\n2\n3\nclear\n4\n5\ndone",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn hash_removal_and_reinsertion_allocate_nothing_and_keep_string_owners() {
    native(
        r#"
mut data = {"a": range(10, 13), "b": range(20, 24), "c": range(30, 35)}.unwrap()
mut names = {"a", "b", "c"}.unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
saved = data.pop("b").unwrap()
names.discard("b")
names.discard("missing")
names.add("d").unwrap()
data.insert("d", range(40, 46)).unwrap()
data.insert("a", range(50, 57)).unwrap()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(len(saved)).unwrap()
print(data.keys().unwrap()).unwrap()
print("d" in names).unwrap()
"#,
        "4\n[\"a\", \"c\", \"d\"]\nTrue",
    );
}
