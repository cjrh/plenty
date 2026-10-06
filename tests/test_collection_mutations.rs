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
