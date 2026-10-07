//! Ordered, allocation-free list removal with ownership transfer.
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
fn pop_supports_default_positive_negative_and_extreme_indices() {
    native(r#"
mut items = [10, 20, 30, 40, 50].unwrap()
print(items.pop()).unwrap()
print(items.pop(1)).unwrap()
print(items.pop(-2)).unwrap()
print(items).unwrap()
print(items.pop(-3)).unwrap()
print(items.pop(2)).unwrap()
print(items.pop(-9223372036854775808)).unwrap()
print(items.pop(9223372036854775807)).unwrap()
print(items).unwrap()
print(items.pop(-2)).unwrap()
print(items.pop(0)).unwrap()
print(items.pop()).unwrap()
print(items.pop(0)).unwrap()
print(items).unwrap()
"#, "Option[i64].Some(50)\nOption[i64].Some(20)\nOption[i64].Some(30)\n[10, 40]\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Nothing\n[10, 40]\nOption[i64].Some(10)\nOption[i64].Some(40)\nOption[i64].Nothing\nOption[i64].Nothing\n[]");
}

#[test]
fn pop_transfers_nested_values_and_preserves_inline_payloads() {
    native(r#"
mut rows = [[1].unwrap(), [2].unwrap(), [3].unwrap()].unwrap()
match rows.pop(1):
    case Some(row):
        mut changed = row
        changed.append(4).unwrap()
        rows.append(changed).unwrap()
    case Nothing:
        print("missing").unwrap()
print(rows).unwrap()
mut options: list[Option[list[i64]]] = [Nothing, Some([42].unwrap())].unwrap()
print(options.pop()).unwrap()
print(options.pop()).unwrap()
print(options.pop()).unwrap()
mut numbers = [2.5f32, 3.5f32].unwrap()
print(numbers.pop(0)).unwrap()
mut words = [("é" + "🙂").unwrap()].unwrap()
word = words.pop()
drop(words)
print(word).unwrap()
"#, "[[1], [3], [2, 4]]\nOption[Option[list[i64]]].Some(Option[list[i64]].Some([42]))\nOption[Option[list[i64]]].Some(Option[list[i64]].Nothing)\nOption[Option[list[i64]]].Nothing\nOption[f32].Some(2.5)\nOption[str].Some(\"é🙂\")");
}

#[test]
fn removed_custom_cleanup_values_drop_once_under_the_new_owner() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self: &mut Guard) -> ():
        print(self.id).unwrap()
mut guards = [Guard(10).unwrap(), Guard(20).unwrap(), Guard(30).unwrap()].unwrap()
removed = guards.pop(1)
print("removed").unwrap()
drop(guards)
print("list dropped").unwrap()
drop(removed)
mut discarded = [Guard(40).unwrap()].unwrap()
discarded.pop()
print("done").unwrap()
"#,
        "removed\n10\n30\nlist dropped\n20\n40\ndone",
    );
}

#[test]
fn borrowed_fields_and_index_evaluation_precede_exclusive_receiver_loan() {
    native(
        r#"
class Storage:
    items: list[i64]
def take(items: &mut list[i64], index: &i64) -> Option[i64]:
    items.pop(index)
def index(items: &mut list[i64]) -> i64:
    print("index").unwrap()
    items.append(30).unwrap()
    0
class Custom:
    def pop(self) -> i64:
        42
mut storage = Storage([10, 20].unwrap()).unwrap()
print(storage.items.pop(index(&mut storage.items))).unwrap()
position = -1
print(take(&mut storage.items, &position)).unwrap()
print(storage.items.pop(len(storage.items) - 1)).unwrap()
print(storage.items).unwrap()
print(Custom().unwrap().pop()).unwrap()
"#,
        "index\nOption[i64].Some(10)\nOption[i64].Some(30)\nOption[i64].Some(20)\n[]\n42",
    );
}

#[test]
fn missing_pop_propagates_and_skips_later_operations() {
    native(
        r#"
def take_two(items: &mut list[list[i64]]) -> Option[list[i64]]:
    first = items.pop()?
    second = items.pop()?
    print("unreachable").unwrap()
    Some(second)
mut rows = [[1].unwrap()].unwrap()
print(take_two(&mut rows)).unwrap()
print(rows).unwrap()
"#,
        "Option[list[i64]].Nothing\n[]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn removal_and_append_reuse_storage_without_allocating() {
    native(
        r#"
mut rows = [[1].unwrap(), [2].unwrap(), [3].unwrap()].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
missing = rows.pop(3)
removed = rows.pop(1)
match removed:
    case Some(row):
        rows.append(row).unwrap()
    case Nothing:
        print("unexpected").unwrap()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(missing).unwrap()
print(rows).unwrap()
"#,
        "Option[list[i64]].Nothing\n[[1], [3], [2]]",
    );
}

#[rstest]
#[case("items = [1].unwrap()\nitems.pop()", "mutable")]
#[case(
    "mut items = [1].unwrap()\nitems.pop(0, 1)",
    "list pop takes at most one i64 index"
)]
#[case("mut items = [1].unwrap()\nitems.pop(0u8)", "expected i64")]
#[case("mut items = [1].unwrap()\nitems.pop(0.0)", "expected i64")]
#[case("[1].unwrap().pop()", "pop requires a mutable list or dictionary")]
#[case(
    "mut items = [[1].unwrap()].unwrap()\nloan = &items\nitems.pop()\nprint(loan).unwrap()",
    "borrow"
)]
#[case(
    "mut items = [[1].unwrap()].unwrap()\nremoved = items.pop()\ndrop(removed)\nprint(removed).unwrap()",
    "moved"
)]
fn rejects_invalid_removal(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
