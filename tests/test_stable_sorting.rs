//! `list.sort_stable_by_key` (issue #806): stable key order with merge
//! workspace reserved before any row moves.
mod support;

fn native(source: &str) -> String {
    let output = support::run(&support::program(source));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn equal_keys_keep_input_order_in_both_directions() {
    // Each size crosses the 16-row insertion runs and several merge levels;
    // the reference is an unstable sort with an explicit position tie breaker.
    let output = native(
        r#"
class Row:
    group: i64
    position: i64
    label: str
def group(row: &Row) -> i64:
    row.group
def negated_group(row: &Row) -> i64:
    # Groups are 0..12, so negation fits.
    unchecked:
        0 - row.group
def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    for size in [0, 1, 2, 16, 17, 100, 1000]?:
        for descending in [False, True]?:
            mut rows: list[Row] = []?
            mut reference: list[(i64, i64)] = []?
            mut s = size + 3
            for i in range(size):
                s = (s * 1103515245 + 12345) % 2147483648
                g = s % 13
                rows.append(Row(g, i, f"r{i}"?))?
                if descending:
                    reference.append((0 - g, i))?
                else:
                    reference.append((g, i))?
            reference.sort()
            if descending:
                rows.sort_stable_by_key(group, True)?
            else:
                rows.sort_stable_by_key(group)?
            mut same = True
            for i in range(size):
                if rows[i].position != reference[i][1] or rows[i].label != f"r{reference[i][1]}"?:
                    same = False
            # A descending sort equals an ascending sort by the negated key.
            mut negated: list[Row] = []?
            for row in &rows:
                negated.append(Row(row.group, row.position, copy(row.label)?))?
            negated.sort_stable_by_key(negated_group)?
            if descending:
                for i in range(size):
                    if negated[i].position != rows[i].position:
                        same = False
            print(size, descending, same)?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "0 False True\n0 True True\n1 False True\n1 True True\n2 False True\n2 True True\n\
16 False True\n16 True True\n17 False True\n17 True True\n100 False True\n100 True True\n\
1000 False True\n1000 True True"
    );
}

#[test]
fn timestamp_records_sort_by_a_tuple_key_without_a_position() {
    let output = native(
        r#"
from std.time import Timestamp
class Event:
    at: Timestamp
    name: str
def when(event: &Event) -> (i64, u32):
    (event.at.seconds, event.at.nanoseconds)
def main() -> Result[(), AllocError | IoError]:
    mut events = [Event(Timestamp(5, 0), "c"), Event(Timestamp(-1, 7), "a"), Event(Timestamp(5, 0), "d"), Event(Timestamp(-1, 7), "b"), Event(Timestamp(-1, 6), "first")]?
    events.sort_stable_by_key(when)?
    for event in &events:
        print(event.name)?
    Ok(())
"#,
    );
    assert_eq!(output, "first\na\nb\nc\nd");
}

#[test]
fn affine_rows_move_without_copies_and_drop_once() {
    let output = native(
        r#"
class Guard:
    key: i64
    id: i64
    name: str
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def key(guard: &Guard) -> (i64, str):
    (guard.key, guard.name)
def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    mut guards: list[Guard] = []?
    for i in range(40):
        guards.append(Guard((i * 7) % 3, i, "same"))?
    guards.sort_stable_by_key(key)?
    mut ordered = True
    for i in range(1, len(guards)):
        a = &guards[i - 1]
        b = &guards[i]
        if a.key > b.key or (a.key == b.key and a.id > b.id):
            ordered = False
    print("ordered", ordered)?
    drop(guards)
    Ok(())
"#,
    );
    let mut lines = output.lines();
    assert_eq!(lines.next(), Some("ordered True"));
    let mut dropped: Vec<_> = lines.collect();
    dropped.sort_unstable();
    let mut expected: Vec<_> = (0..40).map(|i| format!("drop {i}")).collect();
    expected.sort_unstable();
    assert_eq!(dropped, expected);
}

#[test]
fn key_errors_return_the_error_and_keep_every_owner() {
    let output = native(
        r#"
class Row:
    key: i64
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def checked(row: &Row) -> Result[i64, str]:
    if row.key == 99:
        return Err("a heap allocated key error".repeat(10).unwrap())
    Ok(row.key)
def allocating(row: &Row) -> Result[str, AllocError]:
    f"k{row.key}"
def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    mut rows: list[Row] = []?
    for i in range(50):
        rows.append(Row((i * 31) % 17, i))?
    rows.append(Row(99, 50))?
    match rows.sort_stable_by_key(checked):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            match error:
                case message: str:
                    print("key error", len(message))?
                case _:
                    print("unexpected allocation failure")?
    mut total = 0
    for row in &rows:
        # Fifty-one ids below 51 sum to 1275.
        unchecked:
            total = total + row.id
    print(len(rows), total)?
    # A key failing with AllocError gives the plain allocation result.
    result: Result[(), AllocError] = rows.sort_stable_by_key(allocating)
    print(result.is_ok())?
    drop(rows)
    Ok(())
"#,
    );
    let mut lines = output.lines();
    assert_eq!(lines.next(), Some("key error 260"));
    assert_eq!(lines.next(), Some("51 1275"));
    assert_eq!(lines.next(), Some("True"));
    let mut dropped: Vec<_> = lines.collect();
    dropped.sort_unstable();
    let mut expected: Vec<_> = (0..51).map(|i| format!("drop {i}")).collect();
    expected.sort_unstable();
    assert_eq!(dropped, expected);
}

/// Each comparison computes two keys; the key prints once per call.
fn key_calls(order: &str, size: usize) -> usize {
    native(&format!(
        r#"
def key(x: &i64) -> i64:
    print("k").unwrap()
    *x
def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    mut values: list[i64] = []?
    mut s = 11
    for i in range({size}):
        s = (s * 1103515245 + 12345) % 2147483648
        {order}
    values.sort_stable_by_key(key)?
    for i in range(1, len(values)):
        if values[i - 1] > values[i]:
            print("unordered")?
    Ok(())
"#
    ))
    .lines()
    .inspect(|line| assert_eq!(*line, "k"))
    .count()
}

#[test]
fn key_calls_are_linear_for_ordered_input_and_n_log_n_otherwise() {
    let n = 1000;
    assert_eq!(key_calls("values.append(i)?", n), 2 * (n - 1));
    assert_eq!(key_calls("values.append(7)?", n), 2 * (n - 1));
    // Probe, merges of ceil(log2 n) levels, and 16-row insertion runs.
    let log = (usize::BITS - n.leading_zeros()) as usize;
    let bound = 2 * (n + n * log + 120 * n.div_ceil(16));
    for order in ["values.append(s % 1000)?", "values.append(0 - i)?"] {
        let calls = key_calls(order, n);
        assert!(calls <= bound, "{order}: {calls} > {bound}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_leaves_the_list_unchanged_and_ordered_input_allocates_nothing() {
    let output = native(
        r#"
def key(row: &(str, i64)) -> str:
    row[0]
def main() -> Result[(), AllocError | IoError]:
    mut rows = [("pear", 1), ("apple", 2), ("fig", 3), ("apple", 4)]?
    print("__test_fail_allocations_after_0__")?
    result = rows.sort_stable_by_key(key)
    print("__test_restore_allocations__")?
    print(str.repr(result)?)?
    print(rows)?
    rows.sort_stable_by_key(key)?
    print(rows)?
    print("__test_begin_no_allocations__")?
    rows.sort_stable_by_key(key)?
    print("__test_end_no_allocations__")?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "Result[(), AllocError].Err(AllocError.OutOfMemory)\n\
[(\"pear\", 1), (\"apple\", 2), (\"fig\", 3), (\"apple\", 4)]\n\
[(\"apple\", 2), (\"apple\", 4), (\"fig\", 3), (\"pear\", 1)]"
    );
}

#[rstest::rstest]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nitems = [1].unwrap()\nitems.sort_stable_by_key(key).unwrap()",
    "immutable"
)]
#[case(
    "def key(x: i64) -> i64:\n    x\nmut items = [1].unwrap()\nitems.sort_stable_by_key(key).unwrap()",
    "sort_stable_by_key callback must take &i64"
)]
#[case(
    "def key(x: &i64) -> bool:\n    True\nmut items = [1].unwrap()\nitems.sort_stable_by_key(key).unwrap()",
    "sort_stable_by_key callback must return"
)]
#[case(
    "mut items = [1].unwrap()\nitems.sort_stable_by_key().unwrap()",
    "sort_stable_by_key takes a key callback"
)]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nmut items = [1].unwrap()\nitems.sort_stable_by_key(key, reverse=True).unwrap()",
    "`sort_stable_by_key` takes the descending flag positionally"
)]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nmut items = [1].unwrap()\nitems.sort_stable_by_key(key)",
    "Result"
)]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nmut items = {1}.unwrap()\nitems.sort_stable_by_key(key).unwrap()",
    "sort_stable_by_key"
)]
fn invalid_stable_key_sorting(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
