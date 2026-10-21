mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected);
}

#[test]
fn key_sort_handles_closures_fields_references_and_total_tuple_order() {
    native(
        r#"
def sort(items: &mut list[(i64, str)]) -> ():
    offset = 5
    key = def [offset](row: &(i64, str)) -> (str, i64):
        (row[1], row[0] + offset)
    items.sort_by_key(key)
class Rows:
    items: list[(i64, str)]
mut rows = Rows([(3, "b"), (2, "a"), (1, "a")].unwrap())
sort(&mut rows.items)
print(rows.items).unwrap()
def length(row: &list[i64]) -> i64:
    len(row)
mut lists = [[1, 2, 3].unwrap(), [].unwrap(), [4].unwrap()].unwrap()
lists.sort_by_key(length)
print(lists).unwrap()
def identity(value: &f64) -> f64:
    *value
mut floats = [0.0 / 0.0, 0.0, -0.0, -3.0].unwrap()
floats.sort_by_key(identity)
print(floats).unwrap()
mut empty: list[f64] = [].unwrap()
empty.sort_by_key(identity)
empty.append(1.0).unwrap()
empty.sort_by_key(identity, True)
print(empty).unwrap()
"#,
        "[(1, \"a\"), (2, \"a\"), (3, \"b\")]\n[[], [4], [1, 2, 3]]\n[-3.0, -0.0, 0.0, NaN]\n[1.0]",
    );
}

#[test]
fn key_sort_preserves_owned_elements_and_scales_to_many_rows() {
    native(
        r#"
class Row:
    key: i64
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def key(row: &Row) -> i64:
    row.key
mut rows = [Row(2, 1), Row(1, 2), Row(0, 3)].unwrap()
rows.sort_by_key(key)
print(rows[0].id).unwrap()
print("sorted").unwrap()
drop(rows)
def first(row: &(i64, i64)) -> i64:
    row[0]
mut values: list[(i64, i64)] = [].unwrap()
for i in range(1000):
    values.append(((i * 73) % 991, i)).unwrap()
values.sort_by_key(first)
mut ordered = True
mut total = 0
for i in range(len(values)):
    total += values[i][1]
    if i > 0 and values[i - 1][0] > values[i][0]:
        ordered = False
print(ordered).unwrap()
print(total).unwrap()
"#,
        "3\nsorted\n3\n2\n1\nTrue\n499500",
    );
}

#[test]
fn descending_sort_evaluates_its_receiver_index_once() {
    native(
        r#"
def index() -> i64:
    print("index").unwrap()
    0
mut rows = [[3, 1, 2].unwrap()].unwrap()
rows[index()].sort(True)
print(rows).unwrap()
"#,
        "index\n[[3, 2, 1]]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn key_sort_allocates_no_buffer_and_sorted_reports_allocation_failure() {
    native(
        r#"
def key(row: &(str, i64)) -> str:
    row[0]
mut rows = [("pear", 1), ("apple", 2), ("fig", 3)].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
rows.sort_by_key(key)
print("__test_end_no_allocations__").unwrap()
match sorted(&rows):
    case Ok(_):
        print("unexpected success").unwrap()
    case Err(_):
        print("allocation failed").unwrap()
print("__test_restore_allocations__").unwrap()
print(rows).unwrap()
"#,
        "allocation failed\n[(\"apple\", 2), (\"fig\", 3), (\"pear\", 1)]",
    );
}

#[rstest::rstest]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nitems = [1].unwrap()\nitems.sort_by_key(key)",
    "immutable"
)]
#[case(
    "def key(x: i64) -> i64:\n    x\nmut items = [1].unwrap()\nitems.sort_by_key(key)",
    "callback must take &i64"
)]
#[case(
    "def key(x: &i64) -> bool:\n    True\nmut items = [1].unwrap()\nitems.sort_by_key(key)",
    "callback must return"
)]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nmut items = [1].unwrap()\nloan = &items\nitems.sort_by_key(key)\nprint(loan).unwrap()",
    "borrow"
)]
#[case(
    "mut items = [1].unwrap()\nitems.sort_by_key()",
    "takes a key callback"
)]
#[case("sorted(1)", "list")]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nitems = [1].unwrap()\nshared = &items\nshared.sort_by_key(key)",
    "shared"
)]
#[case(
    "mut items = [1].unwrap()\nkey = def [&items](x: &i64) -> i64:\n    len(items) + *x\nitems.sort_by_key(key)",
    "borrow"
)]
#[case(
    "mut items = [1].unwrap()\ncount = 0\nkey = def [mut count](x: &i64) -> i64:\n    count += 1\n    *x\nitems.sort_by_key(key)",
    "mutable"
)]
fn invalid_key_sorting(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn runtime_key_sort_orders_every_scalar_key_and_many_owned_rows() {
    native(
        r#"
class Item:
    name: str
    score: f64
    rank: i64
def by_score(item: &Item) -> f64:
    item.score
def by_pair(item: &Item) -> (i64, str):
    (item.rank % 7, item.name)
mut items: list[Item] = [].unwrap()
mut s = 7
mut total = 0
for i in range(3000):
    s = (s * 1103515245 + 12345) % 2147483648
    mut score = f64(s % 1000) / 7.0
    if s % 97 == 0:
        score = 0.0 / 0.0
    items.append(Item(f"n{s % 300}".unwrap(), score, i)).unwrap()
    total += i
items.sort_by_key(by_score, True)
mut ok = True
for i in range(1, len(items)):
    a = items[i - 1].score
    b = items[i].score
    if (a == a and b != b) or (a == a and b == b and a < b):
        ok = False
items.sort_by_key(by_pair)
for i in range(1, len(items)):
    if by_pair(&items[i - 1]) > by_pair(&items[i]):
        ok = False
mut sum = 0
for item in &items:
    sum += item.rank
print(ok, sum == total).unwrap()
def negated(x: &i64) -> i64:
    0 - *x
mut ints: list[i64] = [].unwrap()
for i in range(2000):
    s = (s * 1103515245 + 12345) % 2147483648
    ints.append(s % 100 - 50).unwrap()
ints.sort_by_key(negated)
ok = True
for i in range(1, len(ints)):
    if ints[i - 1] < ints[i]:
        ok = False
print(ok).unwrap()
def wide(x: &u64) -> u64:
    *x
mut bigs = [18446744073709551615u64, 0u64, 9223372036854775808u64, 1u64].unwrap()
bigs.sort_by_key(wide)
print(bigs).unwrap()
def narrow(x: &i8) -> i8:
    *x
mut bytes = [i8(-128), i8(127), i8(0), i8(-1)].unwrap()
bytes.sort_by_key(narrow, True)
print(bytes).unwrap()
def single(x: &f32) -> f32:
    *x
mut floats = [f32(1.5), f32(0.0) / f32(0.0), f32(-0.0), f32(0.0), f32(-2.0)].unwrap()
floats.sort_by_key(single)
print(floats).unwrap()
"#,
        "True True\nTrue\n[0, 1, 9223372036854775808, 18446744073709551615]\n[127, 0, -1, -128]\n[-2.0, -0.0, 0.0, 1.5, NaN]",
    );
}

#[test]
fn inconsistent_key_keeps_every_row() {
    assert_eq!(
        support::run_file_stdout(
            r#"
from std.time import monotonic_now, instant_at
from std.fmt import FormatError
def jitter(row: &(i64, str)) -> u64:
    monotonic_now().saturating_duration_since(&instant_at(0)).as_nanoseconds() % 5u64
def main() -> Result[(), AllocError | FormatError | IoError]:
    mut rows: list[(i64, str)] = []?
    for i in range(500):
        rows.append((i, f"r{i}"?))?
    rows.sort_by_key(jitter)
    mut total = 0
    for row in &rows:
        total += row[0]
    print(len(rows), total)?
    Ok(())
"#
        ),
        "500 124750"
    );
}
