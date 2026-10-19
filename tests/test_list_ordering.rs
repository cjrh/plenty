//! In-place list reordering: `swap` for any element type and `sort` for ordered ones.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[test]
fn wide_sort_moves_nested_string_owners_in_both_directions() {
    native(
        r#"
mut rows = [((i % 7, str.repr(&i).unwrap()), i) for i in range(127)].unwrap()
rows.sort()
print(len(rows)).unwrap()
print(rows[0]).unwrap()
rows.sort(True)
print(rows[0]).unwrap()
rows.reverse()
print(rows[0]).unwrap()
"#,
        "127\n((0, \"0\"), 0)\n((6, \"97\"), 97)\n((0, \"0\"), 0)\n",
    );
}

#[test]
fn key_sort_borrows_owned_elements_and_supports_descending_and_sorted_copies() {
    native(
        r#"
class Point:
    x: i64
    y: i64
def x(point: &Point) -> i64:
    point.x
mut points = [Point(3, 1), Point(1, 2), Point(2, 4)].unwrap()
points.sort_by_key(x)
for point in &points:
    print(point.x).unwrap()
points.sort_by_key(x, True)
for point in &points:
    print(point.x).unwrap()
mut numbers = [3, 1, 2].unwrap()
print(sorted(&numbers).unwrap()).unwrap()
print(numbers).unwrap()
numbers.sort(True)
print(numbers).unwrap()
print(sorted(&numbers, True).unwrap()).unwrap()
"#,
        "1\n2\n3\n3\n2\n1\n[1, 2, 3]\n[3, 1, 2]\n[3, 2, 1]\n[3, 2, 1]",
    );
}

#[test]
fn swap_exchanges_elements_through_bindings_fields_and_references() {
    native(
        r#"
def ends(items: &mut list[i64]) -> ():
    items.swap(0, -1)
class Data:
    items: list[i64]
mut data = Data([1, 2, 3, 4].unwrap())
ends(&mut data.items)
print(data.items).unwrap()
data.items.swap(1, 2)
print(data.items).unwrap()
data.items.swap(-4, -4)
print(data.items).unwrap()
mut pairs = [(1, "a"), (2, "b"), (3, "c")].unwrap()
pairs.swap(0, 2)
print(pairs).unwrap()
mut indices = [1, 0].unwrap()
indices.swap(indices[0], len(indices) - 2)
print(indices).unwrap()
class Custom:
    def swap(self, a: i64, b: i64) -> i64:
        a - b
print(Custom().swap(7, 2)).unwrap()
"#,
        "[4, 2, 3, 1]\n[4, 3, 2, 1]\n[4, 3, 2, 1]\n[(3, \"c\"), (2, \"b\"), (1, \"a\")]\n[0, 1]\n5",
    );
}

#[test]
fn swap_moves_owned_rows_without_copying_or_early_cleanup() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
mut guards = [Guard(1), Guard(2), Guard(3)].unwrap()
guards.swap(0, 2)
print("swapped").unwrap()
drop(guards)
mut matrix = [[1, 2].unwrap(), [3, 4].unwrap(), [5, 6].unwrap()].unwrap()
matrix.swap(0, 2)
print(matrix).unwrap()
"#,
        "swapped\n3\n2\n1\n[[5, 6], [3, 4], [1, 2]]",
    );
}

#[test]
fn sort_orders_numbers_text_and_tuples() {
    native(
        r#"
def sort(items: &mut list[i64]) -> ():
    items.sort()
class Data:
    items: list[i64]
mut data = Data([5, 3, -1, 9, 3, 0].unwrap())
sort(&mut data.items)
print(data.items).unwrap()
data.items.swap(0, 5)
data.items.sort()
print(data.items).unwrap()
mut bytes: list[u8] = [200, 3, 128].unwrap()
bytes.sort()
print(bytes).unwrap()
mut small: list[i8] = [-5, 100, -128, 0].unwrap()
small.sort()
print(small).unwrap()
mut large: list[u64] = [18446744073709551615, 0, 9223372036854775808].unwrap()
large.sort()
print(large).unwrap()
mut names = ["pear", "apple", "fig", "Apple", "", "figs", "é", "z"].unwrap()
names.sort()
print(names).unwrap()
mut spans = [(5, 8), (1, 3), (2, 6), (1, 2), (15, 18)].unwrap()
spans.sort()
print(spans).unwrap()
mut tagged = [("b", 2.0), ("a", 9.0), ("b", 1.0)].unwrap()
tagged.sort()
print(tagged).unwrap()
mut nested = [((2, "b"), 1), ((1, "z"), 5), ((2, "a"), 0)].unwrap()
nested.sort()
print(nested).unwrap()
mut single = [(3,), (1,), (2,)].unwrap()
single.sort()
print(single).unwrap()
mut empty: list[str] = [].unwrap()
empty.sort()
print(empty).unwrap()
class Custom:
    def sort(self, n: i64) -> i64:
        n
print(Custom().sort(7)).unwrap()
"#,
        concat!(
            "[-1, 0, 3, 3, 5, 9]\n",
            "[-1, 0, 3, 3, 5, 9]\n",
            "[3, 128, 200]\n",
            "[-128, -5, 0, 100]\n",
            "[0, 9223372036854775808, 18446744073709551615]\n",
            "[\"\", \"Apple\", \"apple\", \"fig\", \"figs\", \"pear\", \"z\", \"é\"]\n",
            "[(1, 2), (1, 3), (2, 6), (5, 8), (15, 18)]\n",
            "[(\"a\", 9.0), (\"b\", 1.0), (\"b\", 2.0)]\n",
            "[((1, \"z\"), 5), ((2, \"a\"), 0), ((2, \"b\"), 1)]\n",
            "[(1,), (2,), (3,)]\n",
            "[]\n",
            "7",
        ),
    );
}

#[test]
fn sort_places_every_float_with_negative_zero_first_and_nan_last() {
    native(
        r#"
nan = 0.0 / 0.0
mut wide = [2.5, nan, 0.0, -0.0, -3.0, 1.0 / 0.0, nan, -1.0 / 0.0].unwrap()
wide.sort()
print(wide).unwrap()
mut narrow: list[f32] = [1.5, 0.0, -0.0, -2.0].unwrap()
narrow.sort()
print(narrow).unwrap()
"#,
        "[-inf, -3.0, -0.0, 0.0, 2.5, inf, NaN, NaN]\n[-2.0, -0.0, 0.0, 1.5]",
    );
}

#[test]
fn sort_orders_many_tuple_rows_and_keeps_every_element() {
    native(
        r#"
def ordered(items: &list[(i64, i64)]) -> bool:
    for i in range(1, len(items)):
        if items[i - 1][0] > items[i][0]:
            return False
        if items[i - 1][0] == items[i][0] and items[i - 1][1] > items[i][1]:
            return False
    True
def total(items: &list[(i64, i64)]) -> i64:
    mut sum = 0
    for i in range(len(items)):
        sum = sum + items[i][0] * 1000 + items[i][1]
    sum
mut seed = 7
mut items: list[(i64, i64)] = [].unwrap()
for i in range(500):
    seed = (seed * 1103515245 + 12345) % 2147483648
    items.append((seed % 40, seed % 7)).unwrap()
before = total(&items)
print(ordered(&items)).unwrap()
items.sort()
print(ordered(&items)).unwrap()
print(len(items)).unwrap()
print(total(&items) == before).unwrap()
items.reverse()
items.sort()
print(ordered(&items)).unwrap()
"#,
        "False\nTrue\n500\nTrue\nTrue",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn swap_and_sort_allocate_nothing_with_managed_elements() {
    native(
        r#"
mut words = [("p" + "ear tree").unwrap(), ("a" + "pple tree").unwrap(), "fig", ("banana" + " tree").unwrap()].unwrap()
saved = words[0]
mut rows = [((3, ("c" + "herry tree").unwrap()), 1), ((1, "a"), 2), ((2, "b"), 3)].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
words.sort()
words.swap(0, -1)
rows.sort()
rows.swap(0, 1)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(words).unwrap()
print(rows).unwrap()
drop(words)
print(saved).unwrap()
"#,
        concat!(
            "[\"pear tree\", \"banana tree\", \"fig\", \"apple tree\"]\n",
            "[((2, \"b\"), 3), ((1, \"a\"), 2), ((3, \"cherry tree\"), 1)]\n",
            "pear tree",
        ),
    );
}

#[test]
fn swap_traps_on_a_missing_index() {
    let output = support::run("mut items = [1, 2].unwrap()\nitems.swap(0, 2)");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("index out of bounds"));
}

#[rstest]
#[case("items = [1].unwrap()\nitems.swap(0, 0)", "immutable")]
#[case("[1].unwrap().swap(0, 0)", "swap requires a mutable list")]
#[case(
    "mut items = [1].unwrap()\nitems.swap(0)",
    "swap requires two i64 indices"
)]
#[case(
    "mut items = [1].unwrap()\nitems.swap(0, 0, 0)",
    "swap requires two i64 indices"
)]
#[case(
    "mut items = [1].unwrap()\nitems.swap(0, \"a\")",
    "expected i64, got str"
)]
#[case("mut items = {1}.unwrap()\nitems.swap(0, 0)", "swap")]
#[case(
    "mut items = [1].unwrap()\nloan = &items\nitems.swap(0, 0)\nprint(loan).unwrap()",
    "borrow"
)]
#[case("items = [1].unwrap()\nloan = &items\nloan.swap(0, 0)", "shared")]
#[case("items = [1].unwrap()\nitems.sort()", "immutable")]
#[case("[1].unwrap().sort()", "sort requires a mutable list")]
#[case("mut items = [1].unwrap()\nitems.sort(1)", "expected bool, got i64")]
#[case(
    "mut items = [True].unwrap()\nitems.sort()",
    "sort does not order bool elements"
)]
#[case(
    "mut items = [[1].unwrap()].unwrap()\nitems.sort()",
    "sort does not order list[i64] elements"
)]
#[case(
    "mut items = [(1, True)].unwrap()\nitems.sort()",
    "sort does not order tuple[i64, bool] elements"
)]
#[case(
    "class Point:\n    x: i64\nmut items = [Point(1)].unwrap()\nitems.sort()",
    "sort does not order Point elements"
)]
#[case(
    "class Point:\n    x: i64\nitems = [Point(1)].unwrap()\nsorted(&items).unwrap()",
    "sorted does not order Point elements"
)]
#[case(
    "items = [True].unwrap()\nshared = &items\nsorted(shared, True).unwrap()",
    "sorted does not order bool elements"
)]
#[case(
    "mut items = [1].unwrap()\nloan = &items\nitems.sort()\nprint(loan).unwrap()",
    "borrow"
)]
#[case("items = [1].unwrap()\nloan = &items\nloan.sort()", "shared")]
fn invalid_reordering(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
