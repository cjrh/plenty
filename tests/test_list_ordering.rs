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
fn scalar_swap_counts_negative_indices_for_every_width() {
    native(
        r#"
mut small: list[u8] = [1, 2, 3].unwrap()
small.swap(-1, 0)
mut wide: list[i64] = [-7000000000, 5, 9].unwrap()
wide.swap(-3, -2)
wide.swap(2, 2)
mut halves: list[f32] = [1.5f32, -0.0f32, 2.5f32].unwrap()
halves.swap(0, -2)
mut flags = [True, False].unwrap()
flags.swap(1, -2)
mut popped = [1, 2, 3, 4].unwrap()
popped.pop().unwrap()
popped.swap(0, -1)
print(small, wide, halves, flags, popped).unwrap()
"#,
        "[3, 2, 1] [5, -7000000000, 9] [-0.0, 1.5, 2.5] [False, True] [3, 2, 1]",
    );
}

#[rstest]
#[case("items.swap(0, 2)")]
#[case("items.swap(-3, 0)")]
#[case("items.swap(1, -3)")]
fn scalar_swap_traps_on_an_out_of_range_index(#[case] call: &str) {
    let output = support::run(&format!(
        "mut items = [1, 2].unwrap()\n{call}\nprint(items).unwrap()"
    ));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("index out of bounds"));
    assert!(output.stdout.is_empty());
}

/// The runtime symbols that `function` in `source` references.
fn referenced_symbols(source: &str, function: &str) -> Vec<String> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("swap.o");
    plenty::compile_source_to_object(source, &path).unwrap();
    let bytes = std::fs::read(path).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let symbol = file
        .symbols()
        .find(|s| s.name().is_ok_and(|name| name.ends_with(function)))
        .unwrap();
    let section = file
        .section_by_index(symbol.section_index().unwrap())
        .unwrap();
    section
        .relocations()
        .filter(|(offset, _)| {
            *offset >= symbol.address() && *offset < symbol.address() + symbol.size()
        })
        .filter_map(|(_, relocation)| match relocation.target() {
            RelocationTarget::Symbol(index) => Some(
                file.symbol_by_index(index)
                    .unwrap()
                    .name()
                    .unwrap()
                    .to_owned(),
            ),
            _ => None,
        })
        .filter(|name| name.starts_with("plenty_"))
        .collect()
}

/// Issue #714: a scalar swap is inline loads and stores; only an
/// out-of-bounds index calls out. Other rows keep the runtime exchange,
/// which relocates inline payloads.
#[test]
fn scalar_swap_is_inline_without_retain_or_runtime_call() {
    let program = |ty: &str| {
        format!(
            "def exchange(xs: &mut list[{ty}], i: i64, j: i64) -> ():\n    xs.swap(i, j)\ndef main() -> ():\n    mut xs: list[{ty}] = [].unwrap()\n    exchange(&mut xs, 0, 0)\n"
        )
    };
    for ty in ["i64", "u8", "f64", "bool"] {
        let symbols = referenced_symbols(&program(ty), "exchange");
        assert!(
            symbols.iter().all(|name| name == "plenty_trap_list_index"),
            "{ty}: {symbols:?}"
        );
    }
    let symbols = referenced_symbols(&program("str"), "exchange");
    assert!(
        symbols.iter().any(|name| name.starts_with("plenty_op_")),
        "{symbols:?}"
    );
}

/// Issue #729: an exclusive view's scalar swap reuses the inline list swap,
/// on the view's own rows and count. Other rows call the view exchange, which
/// relocates inline payloads as the list exchange does.
#[test]
fn view_swap_is_inline_for_scalar_rows() {
    let program = |ty: &str| {
        format!(
            "def exchange(v: &mut [{ty}], i: i64, j: i64) -> ():\n    v.swap(i, j)\ndef main() -> ():\n    mut xs: list[{ty}] = [].unwrap()\n    exchange(&mut xs, 0, 0)\n"
        )
    };
    for ty in ["i64", "u8", "f32", "bool"] {
        let symbols = referenced_symbols(&program(ty), "exchange");
        assert!(
            symbols.iter().all(|name| name == "plenty_trap_list_index"),
            "{ty}: {symbols:?}"
        );
    }
    for ty in ["str", "tuple[i64, str]"] {
        let symbols = referenced_symbols(&program(ty), "exchange");
        assert!(
            symbols.iter().any(|name| name == "plenty_view_swap"),
            "{ty}: {symbols:?}"
        );
    }
}

#[test]
fn view_swap_allocates_nothing_with_managed_elements() {
    native(
        r#"
mut words = [("p" + "ear tree").unwrap(), "fig", ("banana" + " tree").unwrap()].unwrap()
mut rows = [((3, ("c" + "herry tree").unwrap()), 1), ((1, "a"), 2)].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
w = &mut words[:]
w.swap(0, -1)
w.swap(1, 1)
r = &mut rows[:]
r.swap(0, 1)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(words).unwrap()
print(rows).unwrap()
"#,
        concat!(
            "[\"banana tree\", \"fig\", \"pear tree\"]\n",
            "[((1, \"a\"), 2), ((3, \"cherry tree\"), 1)]",
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
