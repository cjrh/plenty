//! Disjoint exclusive borrows of one owner (issue #593): slice views with
//! provably disjoint ranges, and distinct fields of a borrowed loop element.
//! See book/src/design/language/05-ownership/02-borrowing.md#disjoint-slice-views.
mod support;

use rstest::rstest;

const SWAP: &str = r#"
def swap(a: &mut i64, b: &mut i64) -> ():
    t = *a
    *a = *b
    *b = t
"#;

#[test]
fn split_views_of_one_list_are_disjoint() {
    support::assert_stdout(
        r#"
import std.bytes as bytes_mod

def total(v: &[i64]) -> i64:
    mut sum = 0
    for x in v:
        sum += *x
    sum

def halves(xs: &mut list[i64]) -> ():
    m = len(xs) // 2
    left = &mut xs[:m]
    right = &mut xs[m:]
    for x in left:
        *x += right[0]

def main() -> Result[(), AllocError | IoError | bytes_mod.BytesError]:
    mut xs = [1, 2, 3, 4, 5, 6]?
    m = 2
    left = &mut xs[:m]
    right = &mut xs[m:]
    left[0] = total(right)
    right[0] = left[1]
    print(xs)?
    low = &mut xs[0:2]
    high = &mut xs[4:6]
    low[1] = high[1]
    end = &mut xs[-2:]
    start = &mut xs[:-2]
    end[0] = start[0]
    print(xs)?
    halves(&mut xs)
    print(xs)?
    tail = &mut xs[m:]
    a = &mut tail[:1]
    b = &mut tail[1:]
    a[0] = b[0]
    first = &mut tail[0]
    second = &mut tail[1]
    *first += *second
    head = &mut xs[:m]
    head[0] = 0
    print(xs)?
    mut buffer = bytes_mod.from_text("abcd")?
    front = &mut buffer[:2]
    back = &mut buffer[2:]
    front[0] = back[1]
    print(buffer.to_text()?)?
    Ok(())
"#,
        "[18, 2, 2, 4, 5, 6]\n[18, 6, 2, 4, 18, 6]\n[22, 10, 6, 4, 18, 6]\n[0, 10, 8, 4, 18, 6]\ndbcd\n",
    );
}

#[test]
fn loop_element_fields_are_disjoint() {
    support::assert_stdout(
        &format!(
            r#"{SWAP}
class Point:
    x: i64
    y: i64

def flip(pairs: &mut list[(i64, i64)]) -> ():
    for n, s in pairs:
        swap(n, s)

def main() -> Result[(), AllocError | IoError]:
    mut pairs = [(1, 2), (3, 4)]?
    for n, s in &mut pairs:
        swap(n, s)
    print(pairs)?
    for p in &mut pairs:
        swap(&mut p[0], &mut p[1])
    print(pairs)?
    flip(&mut pairs)
    for n, s in &mut pairs[1:]:
        swap(n, s)
    print(pairs)?
    mut points = [Point(1, 2)]?
    for point in &mut points:
        swap(&mut point.x, &mut point.y)
    print(points[0].x, points[0].y)?
    mut rows = [[1, 2]?, [3, 4]?]?
    for row in &mut rows:
        swap(&mut row[0], &mut row[1])
    print(rows)?
    Ok(())
"#
        ),
        "[(2, 1), (4, 3)]\n[(1, 2), (3, 4)]\n[(2, 1), (3, 4)]\n2 1\n[[2, 1], [4, 3]]\n",
    );
}

#[rstest]
// Ranges that overlap, or whose order depends on the length.
#[case("a = &mut xs[:3]\nb = &mut xs[2:]\na[0] = b[0]")]
#[case("a = &mut xs[:-2]\nb = &mut xs[2:]\na[0] = b[0]")]
#[case("m = 2\nk = 2\na = &mut xs[:m]\nb = &mut xs[k:]\na[0] = b[0]")]
#[case("m = 2\na = &mut xs[:m + 0]\nb = &mut xs[m:]\na[0] = b[0]")]
#[case("mut m = 2\na = &mut xs[:m]\nb = &mut xs[m:]\na[0] = b[0]")]
#[case("m = 3\nright = &mut xs[m:]\na = &mut right[:2]\nb = &mut right[1:]\na[0] = b[0]")]
fn overlapping_views_are_rejected(#[case] body: &str) {
    let error = support::check_diagnostic(&format!("mut xs = [1, 2, 3, 4].unwrap()\n{body}"));
    assert!(error.contains("conflicting borrow"), "{body}\n{error}");
    assert!(
        error.contains("views of one owner are disjoint when one stops where the other starts"),
        "{body}\n{error}"
    );
}

#[rstest]
#[case(
    "mut xs = [1, 2, 3].unwrap()\nm = 1\na = &mut xs[:m]\nb = &mut xs[m:]\nprint(len(xs)).unwrap()\na[0] = b[0]",
    "cannot read or borrow `xs` while `xs[:m]` is exclusively borrowed"
)]
#[case(
    "mut xs = [1, 2, 3].unwrap()\na = &mut xs[1]\nv = &mut xs[1:]\n*a = v[0]",
    "cannot modify or exclusively borrow `xs[1:]` while `xs[1]` is exclusively borrowed"
)]
#[case(
    "mut xs = [1, 2, 3].unwrap()\nv = &mut xs[1:]\na = &mut v[0]\nb = &mut v[0]\n*a = *b",
    "conflicting borrow"
)]
#[case(
    "mut pairs = [(1, 2)].unwrap()\nfor p in &mut pairs:\n    a = &mut p[0]\n    b = &mut p[0]\n    *a = *b",
    "conflicting borrow"
)]
#[case(
    "mut pairs = [(1, 2)].unwrap()\nfor p in &mut pairs:\n    a = &mut p[0]\n    q = &mut *p\n    *a = 1",
    "conflicting borrow"
)]
#[case(
    "mut pairs = [(1, 2)].unwrap()\nfor n, s in &mut pairs:\n    first = &mut pairs[0]\n    *n += 1",
    "conflicting borrow"
)]
fn overlapping_element_borrows_are_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_diagnostic(source);
    assert!(error.contains(expected), "{source}\n{error}");
}
