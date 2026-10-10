//! Assignment to several targets: the right side is evaluated first, then each
//! component is assigned to its target from left to right.
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
fn names_unpack_a_tuple_written_without_parentheses() {
    native(
        r#"
a, b = 1, "one"
print((a, b)).unwrap()
mut x, y = 1, 2
x, y = y, x
print((x, y)).unwrap()
x, y = (y, x + y)
print((x, y)).unwrap()
mut low: u8 = 1
mut high: u8 = 2
low, high = 250, 251
print((low, high)).unwrap()
mut left = [1].unwrap()
mut right = [2, 3].unwrap()
left, right = right, left
print(left).unwrap()
print(right).unwrap()
_, kept = 1, 2
print(kept).unwrap()
"#,
        "(1, \"one\")\n(2, 1)\n(1, 3)\n(250, 251)\n[2, 3]\n[1]\n2",
    );
}

#[test]
fn elements_fields_and_references_exchange_values() {
    native(
        r#"
class Pair:
    left: i64
    right: i64
def exchange(n: &mut i64, items: &mut list[i64]) -> ():
    *n, items[0] = items[0], *n
def pair() -> (i64, str):
    (7, "seven")
mut items = [10, 20, 30].unwrap()
items[0], items[2] = items[2], items[0]
print(items).unwrap()
items[0], items[1], items[2] = items[1], items[2], items[0]
print(items).unwrap()
mut a = 1
a, items[1] = items[1], a
print(a).unwrap()
print(items).unwrap()
mut p = Pair(1, 2)
p.left, p.right = p.right, p.left
print((p.left, p.right)).unwrap()
mut n = 5
exchange(&mut n, &mut items)
print(n).unwrap()
print(items).unwrap()
mut names = ["x", "y"].unwrap()
count, names[0] = pair()
print(count).unwrap()
print(names).unwrap()
mut bytes: list[u8] = [1, 2].unwrap()
bytes[0], bytes[1] = 200, 100
print(bytes).unwrap()
_, items[0] = 1, 99
print(items).unwrap()
mut table = {"a": 1, "b": 2}.unwrap()
table["a"], table["b"] = table["b"], table["a"]
print(table).unwrap()
"#,
        concat!(
            "[30, 20, 10]\n",
            "[20, 10, 30]\n",
            "10\n",
            "[20, 1, 30]\n",
            "(2, 1)\n",
            "20\n",
            "[5, 1, 30]\n",
            "7\n",
            "[\"seven\", \"y\"]\n",
            "[200, 100]\n",
            "[99, 1, 30]\n",
            "{\"a\": 2, \"b\": 1}",
        ),
    );
}

#[test]
fn targets_are_assigned_from_left_to_right_after_the_right_side() {
    native(
        r#"
def index(label: str, value: i64) -> i64:
    print(label).unwrap()
    value
mut items = [0, 0, 0].unwrap()
mut i = 0
i, items[i] = 2, 5
print(i).unwrap()
print(items).unwrap()
items[index("first target", 0)], items[index("second target", 1)] = index("first value", 7), index("second value", 8)
print(items).unwrap()
items[0], items[0] = 1, 2
print(items).unwrap()
for step in range(2):
    items[step], items[step + 1] = items[step + 1], items[step]
print(items).unwrap()
"#,
        concat!(
            "2\n",
            "[0, 0, 5]\n",
            "first value\nsecond value\nfirst target\nsecond target\n",
            "[7, 8, 5]\n",
            "[2, 8, 5]\n",
            "[8, 5, 2]",
        ),
    );
}

#[test]
fn owned_components_move_into_their_targets_and_replaced_values_drop() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
mut guards = [Guard(1), Guard(2)].unwrap()
mut spare = Guard(3)
print("before").unwrap()
spare, guards[0] = Guard(4), spare
print("after").unwrap()
drop(spare)
drop(guards)
mut rows = [[1].unwrap(), [2].unwrap()].unwrap()
mut row = [9].unwrap()
row, rows[0] = [8].unwrap(), row
print(row).unwrap()
print(rows).unwrap()
"#,
        "before\n1\nafter\n4\n3\n2\n[8]\n[[9], [2]]",
    );
}

#[test]
fn constructors_closures_and_names_of_declarations_accept_several_targets() {
    native(
        r#"
class Pair:
    left: i64
    right: list[i64]
    def __init__(self, a: i64, b: i64) -> Result[(), AllocError]:
        self.left, self.right = a, [b, a]?
        self.left, self.right[0] = self.right[0], self.left
        Ok(())
def count() -> i64:
    7
mut items = [1, 2, 3].unwrap()
count, items[0] = 5, 6
print(count).unwrap()
p = Pair(1, 2).unwrap()
print(p.left).unwrap()
print(p.right).unwrap()
mut exchange = def [&mut items](i: i64, j: i64) -> ():
    items[i], items[j] = items[j], items[i]
    fresh, items[1] = i, j
    items[fresh], items[-1] = 40, items[fresh]
exchange(0, 2)
print(items).unwrap()
"#,
        "5\n2\n[1, 1]\n[40, 2, 3]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn exchanging_managed_elements_allocates_nothing() {
    native(
        r#"
mut words = [("a" + "pple tree").unwrap(), ("b" + "anana tree").unwrap()].unwrap()
mut word = ("c" + "herry tree").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
words[0], words[1] = words[1], words[0]
word, words[0] = words[0], word
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(words).unwrap()
print(word).unwrap()
"#,
        "[\"cherry tree\", \"apple tree\"]\nbanana tree",
    );
}

#[test]
fn a_missing_index_traps_after_earlier_targets_are_assigned() {
    let output =
        support::run("mut items = [1].unwrap()\nmut a = 0\na, items[3] = 5, 6\nprint(a).unwrap()");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("index out of bounds"));
}

const SETUP: &str = "mut items = [1, 2, 3].unwrap()\nmut rows = [[1].unwrap(), [2].unwrap()].unwrap()\nmut a = 1\nfrozen = [1, 2].unwrap()\n";

#[rstest]
// Owned elements cannot be moved out of indexed storage; `swap` exchanges them.
#[case("rows[0], rows[1] = rows[1], rows[0]", "cannot move out of indexed")]
#[case("items[0], items[1] = 1, 2, 3", "unpacking arity does not match tuple")]
#[case(
    "items[0], items[1] = (1, 2, 3)",
    "unpacking arity does not match tuple"
)]
#[case("items[0], items[1] = 5", "unpacking requires a tuple")]
#[case(
    "items[0], (a, items[1]) = 1, (2, 3)",
    "nested assignment targets are not supported"
)]
#[case(
    "items[0], len(items) = 1, 2",
    "assignment requires a named binding, field, or collection element"
)]
#[case("frozen[0], a = 1, 2", "immutable")]
#[case("fresh, items[0] = 4, 5\nfresh = 3", "`fresh` is immutable")]
#[case("items[0], a = \"s\", 2", "expected i64, got str")]
#[case("a, a, items[0] = 1, 2, 3", "duplicate unpacking binding")]
#[case("a, b, a = 1, 2, 3", "duplicate unpacking binding")]
#[case("a, items[0]", "expected `=`")]
#[case("items[0], a = 1, 2,", "expected an expression")]
#[case("a = 1, 2", "expected the end of the statement")]
#[case("a, b = 1, 2, 3", "unpacking arity does not match tuple")]
#[case(
    "loan = &items\nitems[0], items[1] = 1, 2\nprint(loan).unwrap()",
    "borrow"
)]
#[case(
    "mut f = def [&mut a, &mut items]() -> ():\n    a, items[0] = 2, 3",
    "cannot shadow closure capture `a`"
)]
fn invalid_multiple_assignment(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(&format!("{SETUP}{source}"))
        .unwrap_err()
        .to_string();
    assert!(error.contains(expected), "{error}");
}

#[rstest]
#[case(
    "self.left, self.right = a, self.left",
    "field `left` is not initialized"
)]
#[case("self.left, other = a, a", "fields not initialized: right")]
#[case("self.left, self.missing = a, a", "unknown field `missing`")]
#[case(
    "self.left, self.right = a, a\n        self, other = Pair(1), 2",
    "cannot rebind self in a constructor"
)]
fn constructors_check_each_target_in_order(#[case] body: &str, #[case] expected: &str) {
    let source = format!(
        "class Pair:\n    left: i64\n    right: i64\n    def __init__(self, a: i64) -> ():\n        {body}\n"
    );
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
