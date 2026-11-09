//! `sort`, `sort_by_key` and `sort_stable_by_key` on exclusive slice views
//! (issue #886): the list sorts' contracts over a view's rows, leaving the
//! rows outside the view in place.
mod support;

fn native(source: &str) -> String {
    let output = support::run(source);
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
fn natural_sort_orders_only_the_view_in_both_directions() {
    let output = native(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [9, 3, 2, 1, 0, -1]?
    v = &mut xs[1:5]
    v.sort()
    print(xs)?
    (&mut xs[:]).sort(True)
    print(xs)?
    mut small = [3i8, -2i8, 1i8, -128i8]?
    (&mut small[1:]).sort()
    print(small)?
    mut words = ["pear", "fig", "apple", "kiwi"]?
    (&mut words[1:]).sort()
    print(words)?
    mut pairs = [(2, "b"), (1, "z"), (1, "a")]?
    (&mut pairs[:]).sort()
    print(pairs)?
    mut floats = [0.0 / 0.0, 1.5, -0.0, 0.0, -1.0]?
    (&mut floats[:]).sort()
    print(floats)?
    mut empty: list[i64] = []?
    (&mut empty[:]).sort()
    (&mut xs[2:2]).sort(True)
    print(empty, xs)?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "[9, 0, 1, 2, 3, -1]\n\
[9, 3, 2, 1, 0, -1]\n\
[3, -128, -2, 1]\n\
[\"pear\", \"apple\", \"fig\", \"kiwi\"]\n\
[(1, \"a\"), (1, \"z\"), (2, \"b\")]\n\
[-1.0, -0.0, 0.0, 1.5, NaN]\n\
[] [9, 3, 2, 1, 0, -1]"
    );
}

#[test]
fn natural_sort_matches_list_sort_on_larger_views() {
    // Crosses the introsort's insertion cutoff and ninther pivot sampling.
    let output = native(
        r#"
def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    mut xs: list[i64] = []?
    mut s = 7
    for _ in range(1000):
        s = (s * 1103515245 + 12345) % 2147483648
        xs.append(s % 100)?
    mut expected = xs[100:900]?
    expected.sort()
    mut words: list[str] = []?
    for x in &xs:
        words.append(str.from(*x)?)?
    mut expected_words = words[100:900]?
    expected_words.sort(True)
    (&mut xs[100:900]).sort()
    (&mut words[100:900]).sort(True)
    print(xs[100:900]? == expected, words[100:900]? == expected_words)?
    print(len(xs), xs[0] == expected[0])?
    Ok(())
"#,
    );
    assert_eq!(output.lines().next(), Some("True True"));
}

#[test]
fn key_sorts_take_the_list_forms() {
    let output = native(
        r#"
def negate(x: &i64) -> i64:
    # Fixture values are small.
    unchecked:
        0 - *x
def parity(x: &i64) -> i64:
    unchecked:
        *x % 2
def main() -> Result[(), AllocError | IoError]:
    mut xs = [5, 1, 4, 2, 3, 0]?
    v = &mut xs[1:5]
    v.sort_by_key(negate)
    print(xs)?
    (&mut xs[1:5]).sort_by_key(negate, True)
    print(xs)?
    (&mut xs[:]).sort_stable_by_key(parity)?
    print(xs)?
    (&mut xs[:]).sort_stable_by_key(parity, True)?
    print(xs)?
    mut names = ["pear", "fig", "apple", "kiwi", "date"]?
    (&mut names[1:]).sort_stable_by_key(def(s: &str) -> i64: len(s))?
    print(names)?
    offset = 10
    by_length = def [offset](s: &str) -> Result[(i64, str), AllocError]:
        unchecked:
            Ok((offset - len(s), copy(s)?))
    (&mut names[:]).sort_by_key(by_length)?
    print(names)?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "[5, 4, 3, 2, 1, 0]\n\
[5, 1, 2, 3, 4, 0]\n\
[2, 4, 0, 5, 1, 3]\n\
[5, 1, 3, 2, 4, 0]\n\
[\"pear\", \"fig\", \"kiwi\", \"date\", \"apple\"]\n\
[\"apple\", \"date\", \"kiwi\", \"pear\", \"fig\"]"
    );
}

#[test]
fn fallible_keys_return_their_error_and_keep_every_row() {
    let output = native(
        r#"
class Item:
    name: str
    rank: i64
def rank(item: &Item) -> Result[i64, str]:
    if item.rank < 0:
        return Err("negative rank".repeat(2).unwrap())
    Ok(item.rank)
def main() -> Result[(), AllocError | IoError | str]:
    mut items = [Item("a", 3), Item("b", 2), Item("c", -1), Item("d", 1)]?
    match (&mut items[:3]).sort_by_key(rank):
        case Ok(_):
            print("sorted")?
        case Err(error):
            print(error)?
    match (&mut items[:]).sort_stable_by_key(rank):
        case Ok(_):
            print("sorted")?
        case Err(error):
            match error:
                case message: str:
                    print(message)?
                case _:
                    print("allocation")?
    mut names: list[str] = []?
    for item in &items:
        names.append(copy(&item.name)?)?
    names.sort()
    print(names)?
    mut valid = [Item("x", 9), Item("y", 2), Item("z", 5)]?
    (&mut valid[1:]).sort_by_key(rank)?
    print(valid[0].name, valid[1].name, valid[2].name)?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "negative ranknegative rank\nnegative ranknegative rank\n[\"a\", \"b\", \"c\", \"d\"]\nx y z"
    );
}

#[test]
fn affine_and_inline_rows_move_without_copies() {
    let output = native(
        r#"
class Tracked:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def id(t: &Tracked) -> i64:
    t.id
def total(row: &list[i64]) -> i64:
    mut sum = 0
    for x in row:
        unchecked:
            sum += *x
    sum
def main() -> Result[(), AllocError | IoError]:
    mut rows = [[9]?, [5, 5]?, [1]?, [2, 2]?]?
    (&mut rows[1:]).sort_by_key(total, True)
    print(rows)?
    mut texts = [("b", 2), ("a", 1), ("c", 0)]?
    (&mut texts[:]).sort()
    print(texts)?
    mut tracked = [Tracked(3), Tracked(1), Tracked(2)]?
    (&mut tracked[:]).sort_stable_by_key(id)?
    print(tracked[0].id, tracked[1].id, tracked[2].id)?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "[[9], [5, 5], [2, 2], [1]]\n\
[(\"a\", 1), (\"b\", 2), (\"c\", 0)]\n\
1 2 3\n\
drop 1\ndrop 2\ndrop 3"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn stable_sort_reports_allocation_failure_and_unstable_sorts_allocate_nothing() {
    let output = native(
        r#"
def key(row: &(str, i64)) -> str:
    row[0]
def main() -> Result[(), AllocError | IoError]:
    mut rows = [("z", 0), ("pear", 1), ("apple", 2), ("fig", 3), ("apple", 4)]?
    print("__test_fail_allocations_after_0__")?
    result = (&mut rows[1:]).sort_stable_by_key(key)
    print("__test_restore_allocations__")?
    print(str.repr(result)?)?
    print(rows)?
    (&mut rows[1:]).sort_stable_by_key(key)?
    print(rows)?
    print("__test_begin_no_allocations__")?
    (&mut rows[1:]).sort_stable_by_key(key)?
    (&mut rows[:]).sort_by_key(key, True)
    (&mut rows[:]).sort()
    print("__test_end_no_allocations__")?
    print(rows)?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "Result[(), AllocError].Err(AllocError.OutOfMemory)\n\
[(\"z\", 0), (\"pear\", 1), (\"apple\", 2), (\"fig\", 3), (\"apple\", 4)]\n\
[(\"z\", 0), (\"apple\", 2), (\"apple\", 4), (\"fig\", 3), (\"pear\", 1)]\n\
[(\"apple\", 2), (\"apple\", 4), (\"fig\", 3), (\"pear\", 1), (\"z\", 0)]"
    );
}

#[rstest::rstest]
#[case(
    "xs = [2, 1].unwrap()\nys = sorted(&xs[:]).unwrap()",
    "sorted takes a list, not a slice view: copy the view with `list(v)?` and sort the copy"
)]
#[case(
    "xs = [2, 1].unwrap()\nv = &xs[1:]\nys = sorted(v).unwrap()",
    "sorted takes a list, not a slice view"
)]
#[case(
    "mut xs = [2, 1].unwrap()\nys = sorted(&mut xs[1:]).unwrap()",
    "or sort the view in place with `v.sort()`"
)]
#[case(
    "mut xs = [1, 2].unwrap()\nv = &xs[:]\nv.sort()",
    "a shared slice view is read-only, so it has no `sort`"
)]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nmut xs = [1, 2].unwrap()\nv = &xs[:]\nv.sort_by_key(key)",
    "a shared slice view is read-only, so it has no `sort_by_key`"
)]
#[case(
    "mut xs = [True, False].unwrap()\n(&mut xs[:]).sort()",
    "sort does not order bool elements"
)]
#[case("mut xs = [1, 2].unwrap()\n(&mut xs[:]).sort(1)", "expected bool")]
#[case(
    "mut xs = [1, 2].unwrap()\n(&mut xs[:]).sort(True, False)",
    "sort takes no arguments or one descending bool"
)]
#[case(
    "mut xs = [1, 2].unwrap()\n(&mut xs[:]).sort(reverse=True)",
    "`sort` takes the descending flag positionally"
)]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nmut xs = [1].unwrap()\n(&mut xs[:]).sort_stable_by_key(key, reverse=True).unwrap()",
    "`sort_stable_by_key` takes the descending flag positionally"
)]
#[case(
    "def key(x: i64) -> i64:\n    x\nmut xs = [1].unwrap()\n(&mut xs[:]).sort_by_key(key)",
    "sort_by_key callback must take &i64"
)]
#[case(
    "def key(x: &i64) -> i64:\n    *x\nmut xs = [1].unwrap()\n(&mut xs[:]).sort_stable_by_key(key)",
    "Result"
)]
#[case(
    "mut xs = [1, 2].unwrap()\nkey = def [&xs](x: &i64) -> i64:\n    unchecked:\n        len(xs) + *x\n(&mut xs[:]).sort_by_key(key)",
    "borrow"
)]
#[case(
    "mut xs = [1, 2].unwrap()\nv = &mut xs[:]\nw = &xs[:]\nv.sort()\nprint(w).unwrap()",
    "conflicting borrow"
)]
fn invalid_view_sorting(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
