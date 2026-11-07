//! `list.insert(index, value)`: checked, allocating insertion at any position
//! (issue #798).
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[test]
fn insert_places_values_at_the_start_middle_end_and_negative_positions() {
    native(
        r#"
mut items = [10, 20].unwrap()
items.insert(1, 15).unwrap()
items.insert(0, 5).unwrap()
items.insert(4, 25).unwrap()
items.insert(len(items), 30).unwrap()
items.insert(-1, 27).unwrap()
items.insert(-7, 0).unwrap()
print(items).unwrap()
mut empty = list[i64]().unwrap()
empty.insert(0, 1).unwrap()
empty.insert(-1, 0).unwrap()
print(empty).unwrap()
"#,
        "[0, 5, 10, 15, 20, 25, 27, 30]\n[0, 1]",
    );
}

#[test]
fn insert_supports_every_scalar_width_and_reuses_popped_capacity() {
    native(
        r#"
mut small: list[u8] = [1, 3].unwrap()
small.insert(1, 2).unwrap()
mut halves: list[f32] = [1.5f32, 3.5f32].unwrap()
halves.insert(-1, 2.5f32).unwrap()
mut flags = [True, True].unwrap()
flags.insert(1, False).unwrap()
mut wide: list[i16] = [-300, 300].unwrap()
wide.insert(0, 0).unwrap()
mut popped = [1, 2, 3, 4].unwrap()
popped.pop().unwrap()
popped.insert(0, 0).unwrap()
popped.pop(1).unwrap()
popped.insert(1, 9).unwrap()
print(small, halves, flags, wide, popped).unwrap()
"#,
        "[1, 2, 3] [1.5, 2.5, 3.5] [True, False, True] [0, -300, 300] [0, 9, 2, 3]",
    );
}

#[test]
fn insert_moves_inline_and_owned_rows_and_repairs_their_payloads() {
    native(
        r#"
mut words = [("a" + "lpha").unwrap(), ("g" + "amma").unwrap()].unwrap()
words.insert(1, ("b" + "eta").unwrap()).unwrap()
mut pairs = [(1, ("o" + "ne").unwrap()), (3, ("th" + "ree").unwrap())].unwrap()
pairs.insert(1, (2, ("t" + "wo").unwrap())).unwrap()
pairs.insert(0, (0, "zero")).unwrap()
mut options: list[Option[list[i64]]] = [Nothing, Some([3].unwrap())].unwrap()
options.insert(1, Some([1, 2].unwrap())).unwrap()
mut rows = [[1].unwrap(), [3].unwrap()].unwrap()
rows.insert(1, [2].unwrap()).unwrap()
rows[0].append(10).unwrap()
rows[2].append(30).unwrap()
print(words).unwrap()
print(pairs).unwrap()
print(options).unwrap()
print(rows).unwrap()
print(pairs[2][1], pairs[3][1]).unwrap()
"#,
        concat!(
            "[\"alpha\", \"beta\", \"gamma\"]\n",
            "[(0, \"zero\"), (1, \"one\"), (2, \"two\"), (3, \"three\")]\n",
            "[Option[list[i64]].Nothing, Option[list[i64]].Some([1, 2]), Option[list[i64]].Some([3])]\n",
            "[[1, 10], [2], [3, 30]]\n",
            "two three",
        ),
    );
}

#[test]
fn inserted_and_shifted_owners_drop_once_in_list_order() {
    native(
        r#"
class Guard:
    id: i64
    rows: list[i64]
    def __del__(self: &mut Guard) -> ():
        print(self.id).unwrap()
mut guards = [Guard(1, [1].unwrap()), Guard(3, [3].unwrap())].unwrap()
guards.insert(1, Guard(2, [2].unwrap())).unwrap()
guards.insert(0, Guard(0, [0].unwrap())).unwrap()
guards.insert(len(guards), Guard(4, [4].unwrap())).unwrap()
print("inserted").unwrap()
print(guards[1].rows, guards[3].rows).unwrap()
taken = guards.pop(2).unwrap()
print("popped").unwrap()
drop(guards)
print("list dropped").unwrap()
print(taken.rows).unwrap()
"#,
        "inserted\n[1] [3]\npopped\n0\n1\n3\n4\nlist dropped\n[2]\n2",
    );
}

#[test]
fn insert_evaluates_its_operands_before_borrowing_the_list() {
    native(
        r#"
class Data:
    items: list[i64]
def front(items: &mut list[i64], value: i64) -> Result[(), AllocError]:
    items.insert(0, value)?
    Ok(())
mut items = [1, 2].unwrap()
items.insert(len(items), len(items)).unwrap()
i = 1
items.insert(&i, items[0]).unwrap()
front(&mut items, 0).unwrap()
mut data = Data([7].unwrap())
data.items.insert(0, 6).unwrap()
front(&mut data.items, 5).unwrap()
mut n = 0
while n < 3:
    items.insert(0, items[0]).unwrap()
    unchecked:
        n = n + 1
print(items, data.items).unwrap()
"#,
        "[0, 0, 0, 0, 1, 1, 2, 2] [5, 6, 7]",
    );
}

#[test]
fn an_out_of_range_insertion_index_traps_before_changing_the_list() {
    let source = r#"
class Guard:
    id: i64
    def __del__(self: &mut Guard) -> ():
        print(self.id).unwrap()
def main() -> Result[(), AllocError | IoError]:
    mut items = [1, 2]?
    mut guards = [Guard(1)]?
    which = args()?[1]
    if which == "0":
        items.insert(3, 0)?
    elif which == "1":
        items.insert(-3, 0)?
    elif which == "2":
        items.insert(9223372036854775807, 0)?
    elif which == "3":
        items.insert(-9223372036854775808, 0)?
    elif which == "4":
        guards.insert(2, Guard(2))?
    print(items)?
    Ok(())
"#;
    support::assert_trap_cases(
        source,
        &[
            "list.insert: index 3 is outside -2..=2".into(),
            "list.insert: index -3 is outside -2..=2".into(),
            "list.insert: index 9223372036854775807 is outside -2..=2".into(),
            "list.insert: index -9223372036854775808 is outside -2..=2".into(),
            "list.insert: index 2 is outside -1..=1".into(),
        ],
    );
}

#[rstest]
#[case("items = [1].unwrap()\nitems.insert(0, 0).unwrap()", "immutable")]
#[case(
    "[1].unwrap().insert(0, 0).unwrap()",
    "mutation requires a named binding"
)]
#[case(
    "mut items = [1].unwrap()\nitems.insert(0).unwrap()",
    "insert requires an i64 index and an element"
)]
#[case(
    "mut items = [1].unwrap()\nitems.insert(0, 1, 2).unwrap()",
    "insert requires an i64 index and an element"
)]
#[case(
    "mut items = [1].unwrap()\nitems.insert(\"a\", 0).unwrap()",
    "expected i64, got str"
)]
#[case(
    "mut items = [1].unwrap()\nitems.insert(0, \"a\").unwrap()",
    "expected i64, got str"
)]
#[case("mut items = {1}.unwrap()\nitems.insert(0, 0).unwrap()", "insert")]
#[case(
    "mut items = [1].unwrap()\nloan = &items\nitems.insert(0, 0).unwrap()\nprint(loan).unwrap()",
    "borrow"
)]
#[case(
    "items = [1].unwrap()\nloan = &items\nloan.insert(0, 0).unwrap()",
    "shared"
)]
fn invalid_insertions_are_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_insertion_drops_the_value_and_keeps_the_list_then_succeeds_on_retry() {
    native(
        r#"
class Guard:
    id: str
    def __del__(self: &mut Guard) -> ():
        # A literal prints without allocating, so the failed insertion's
        # drop can report itself while allocation is failing.
        print(self.id).unwrap()
mut guards = [Guard("1"), Guard("2"), Guard("3"), Guard("4")].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = guards.insert(1, Guard("9"))
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(len(guards), guards[0].id, guards[1].id, guards[3].id).unwrap()
print(str.repr(guards.insert(1, Guard("5"))).unwrap()).unwrap()
print(len(guards), guards[1].id, guards[2].id).unwrap()
drop(guards)
"#,
        concat!(
            "9\n",
            "Result[(), AllocError].Err(AllocError.OutOfMemory)\n",
            "4 1 2 4\n",
            "Result[(), AllocError].Ok(())\n",
            "5 5 2\n",
            "1\n5\n2\n3\n4",
        ),
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn insertion_into_reserved_capacity_allocates_nothing() {
    native(
        r#"
def fill(words: &mut list[str], rows: &mut list[list[i64]], row: list[i64]) -> Result[(), AllocError]:
    words.insert(0, "a")?
    words.insert(1, "b")?
    words.insert(-1, "c")?
    rows.insert(0, row)?
    Ok(())
mut words = ["z"].unwrap()
mut rows: list[list[i64]] = [].unwrap()
row = [1, 2].unwrap()
words.reserve(3).unwrap()
rows.reserve(1).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
result = fill(&mut words, &mut rows, row)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(words, rows).unwrap()
"#,
        "Result[(), AllocError].Ok(())\n[\"a\", \"b\", \"c\", \"z\"] [[1, 2]]",
    );
}
