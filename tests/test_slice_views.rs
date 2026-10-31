//! Borrowed slice views `&xs[a:b]` and `&mut xs[a:b]` (issue #581). See
//! book/src/design/language/04-expressions/slices.md#slice-views.
mod support;

use rstest::rstest;

#[test]
fn list_views_clamp_index_and_iterate_in_place() {
    support::assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3, 4, 5]?
    print(len(&xs[1:4]), len(&xs[-2:]), len(&xs[:-1]), len(&xs[:]))?
    print(len(&xs[3:1]), len(&xs[-100:100]), len(&xs[9:]))?
    v = &xs[1:4]
    print(v[0], v[2], v[-1])?
    for x in v:
        print(*x)?
    for x in reversed(&xs[3:]):
        print(*x)?
    for i, x in enumerate(&xs[2:]):
        print(i, *x)?
    w = &v[1:]
    print(len(w), w[0])?
    Ok(())
"#,
        "3 2 4 5\n0 5 0\n2 4 4\n2\n3\n4\n5\n4\n0 3\n1 4\n2 5\n2 3\n",
    );
}

/// `iter(v)` fuses into the view's loop, so items are `&T`, or `&mut T` for
/// an exclusive view, as in `for x in v`.
#[test]
fn views_are_pipeline_sources() {
    support::assert_stdout(
        r#"
import std.bytes as bytes_mod

def even(x: &i64) -> bool:
    return *x % 2 == 0

def ten(x: &i64) -> i64:
    return *x * 10

def bump(x: &mut i64) -> ():
    *x += 100

def main() -> Result[(), AllocError | IoError | bytes_mod.BytesError]:
    mut xs = [1, 2, 3, 4, 5, 6]?
    view = &xs[1:5]
    print(iter(view).filter(even).count(), iter(&xs[1:5]).map(ten).sum())?
    tens: list[i64] = iter(&xs[:3]).map(ten).collect()?
    print(tens, iter(reversed(&xs[:])).take(2).map(ten).sum())?
    iter(&mut xs[4:]).for_each(bump)
    print(xs)?
    buffer = bytes_mod.from_text("abc")?
    print(iter(&buffer[1:]).count())?
    Ok(())
"#,
        "2 140\n[10, 20, 30] 110\n[1, 2, 3, 4, 105, 106]\n2\n",
    );
}

/// A view prints as its target would: a `&[u8]` as a `bytes` literal, any
/// other view as a list, in `print`, `str.repr`, and f-strings. A `Bytes`
/// buffer itself keeps its own spelling.
#[test]
fn views_print_as_their_rows() {
    support::assert_stdout(
        r#"
import std.bytes
import std.fmt as fmt
from std.bytes import BytesError
from std.fmt import FormatError

class Point:
    x: i64
    y: i64

def show[T](v: &[T]) -> Result[(), AllocError | IoError]:
    print(v)?
    Ok(())

def main() -> Result[(), AllocError | IoError | FormatError | BytesError]:
    mut xs = [1, 2, 3, 4]?
    v = &xs[1:3]
    print(v, &xs[:0], len(v))?
    words = ["a", "b\n", "é"]?
    points = [Point(1, 2), Point(3, 4)]?
    pairs = [(1, "one"), (2, "two")]?
    print(&words[1:], &points[1:], &pairs[:])?
    nested = [[1, 2]?, [3]?]?
    floats = [1.5, -0.0]?
    print(&nested[:], &floats[:])?
    buffer = std.bytes.from_bytes(b"abc\x00\xff")?
    print(&buffer[1:3], &buffer[3:], &buffer[:0], buffer)?
    raw = [104u8, 105u8]?
    signed = [1i8, -2i8]?
    print(&raw[:], &signed[:])?
    show(&points[:1])?
    m = &mut xs[:2]
    m[0] = 9
    print(m)?
    print(str.repr(&xs[2:])?)?
    print(f"{&xs[1:3]} {&xs[:1]!r} {&words[2:]!a} [{&xs[:2]:>8}] {&buffer[3:]!a}"?)?
    mut out = fmt.string_formatter(100, 4)?
    w = &mut xs[2:]
    out.format(f"{w}|{w!s:<8}|")?
    print(fmt.finish(&out)?)?
    Ok(())
"#,
        concat!(
            "[2, 3] [] 2\n",
            "[\"b\\n\", \"é\"] [Point(x=3, y=4)] [(1, \"one\"), (2, \"two\")]\n",
            "[[1, 2], [3]] [1.5, -0.0]\n",
            "b\"bc\" b\"\\x00\\xff\" b\"\" b\"abc\\x00\\xff\"\n",
            "b\"hi\" [1, -2]\n",
            "[Point(x=1, y=2)]\n",
            "[9, 2]\n",
            "[3, 4]\n",
            "[2, 3] [9] [\"\\u{e9}\"] [  [9, 2]] b\"\\x00\\xff\"\n",
            "[3, 4]|[3, 4]  |\n",
        ),
    );
}

/// Codegen addresses view rows with a compile-time row size, which must match
/// the runtime's list layout for every element representation.
#[test]
fn views_address_rows_of_every_element_layout() {
    support::assert_stdout(
        r#"
enum Shape:
    Dot
    Square(i64)

class Point:
    x: i32
    y: i32

def main() -> Result[(), AllocError | IoError]:
    small = [1i8, -2i8, 3i8]?
    halves = [1i16, 2i16, 3i16]?
    floats = [1.5f32, 2.5f32, 3.5f32]?
    flags = [True, False, True]?
    wide = [1.0, 2.0, 3.0]?
    pairs = [(1, "a"), (2, "b"), (3, "c")]?
    options = [Some(1), Nothing, Some(3)]?
    ranges = [range(1), range(2), range(3)]?
    points = [Point(1, 2), Point(3, 4), Point(5, 6)]?
    shapes = [Shape.Dot, Shape.Square(4), Shape.Dot]?
    print((&small[1:])[1], (&halves[1:])[1], (&floats[1:])[1], (&flags[1:])[0])?
    print((&wide[1:])[1], (&pairs[1:])[1][1], (&options[1:])[1], len((&ranges[1:])[1]))?
    point = &points[1:]
    print(point[1].x, point[1].y)?
    match (&shapes[1:])[0]:
        case Shape.Square(side):
            print(side)?
        case Shape.Dot:
            print("dot")?
    Ok(())
"#,
        "3 3 3.5 False\n3.0 c Option[i64].Some(3) 3\n5 6\n4\n",
    );
}

#[test]
fn exclusive_views_write_elements_in_place() {
    support::assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    m = &mut xs[1:]
    m[0] = 20
    m[1], m[2] = m[2], m[1]
    for x in &mut xs[:2]:
        *x *= 10
    print(xs)?
    mut names = ["a", "b", "c"]?
    for name in &mut names[1:]:
        *name = "z"
    print(names)?
    mut rows = [[1]?, [2]?]?
    r = &mut rows[1:]
    r[0].append(3)?
    print(rows)?
    Ok(())
"#,
        "[10, 200, 4, 3]\n[\"a\", \"z\", \"z\"]\n[[1], [2, 3]]\n",
    );
}

#[test]
fn byte_views_share_one_element_type() {
    support::assert_stdout(
        r#"
import std.bytes as bytes_mod

def checksum(v: &[u8]) -> i64:
    mut sum = 0
    for b in v:
        sum += i64(*b)
    return sum

def main() -> Result[(), AllocError | IoError | bytes_mod.BytesError]:
    raw = [104u8, 105u8]?
    mut buffer = bytes_mod.from_text("hi there")?
    literal = b"hi"
    print(checksum(&raw[:]), checksum(&buffer[:2]), checksum(&literal[:]))?
    print(&raw[:] == &buffer[:2], &buffer[3:] != &literal[:])?
    upper = &mut buffer[:2]
    upper[0] = 72
    print(buffer.to_text()?)?
    tail = &literal[1:]
    print(len(tail), tail[0])?
    Ok(())
"#,
        "209 209 209\nTrue True\nHi there\n1 105\n",
    );
}

#[test]
fn views_compare_elementwise() {
    support::assert_stdout(
        r#"
class Point:
    x: i64

def main() -> Result[(), AllocError | IoError]:
    a = [1, 2, 3, 1, 2, 3]?
    print(&a[:3] == &a[3:], &a[:2] == &a[1:3], &a[:2] != &a[:3])?
    words = ["a", "b"]?
    other = ["x", "a", "b"]?
    print(&words[:] == &other[1:])?
    points = [Point(1), Point(2), Point(1)]?
    print(&points[:1] == &points[2:])?
    mut m = [1, 2]?
    view = &mut m[:]
    print(view == &a[:2])?
    Ok(())
"#,
        "True False True\nTrue\nTrue\nTrue\n",
    );
}

#[test]
fn view_parameters_take_part_of_an_owner() {
    support::assert_stdout(
        r#"
class Bag:
    items: list[i64]

    def head_total(self, n: i64) -> i64:
        return total(&self.items[:n])

def total(v: &[i64]) -> i64:
    mut sum = 0
    for x in v:
        sum += *x
    return sum

def bump(v: &mut [i64]) -> ():
    for x in v:
        *x += 1

def count[T](v: &[T]) -> i64:
    return len(v)

def first(v: &[i64]) -> &i64:
    return &v[0]

def last_mut(v: &mut [i64]) -> &mut i64:
    return &mut v[len(v) - 1]

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    print(total(&xs[1:3]), total(&xs[:]), count(&xs[2:]))?
    bump(&mut xs[2:])
    print(xs)?
    view = &xs[1:]
    print(total(view), *first(view))?
    slot = last_mut(&mut xs[:2])
    *slot = 9
    print(xs)?
    bag = Bag([5, 6, 7]?)
    print(bag.head_total(2))?
    Ok(())
"#,
        "5 10 2\n[1, 2, 4, 5]\n11 2\n[1, 9, 4, 5]\n11\n",
    );
}

#[test]
fn scoped_threads_borrow_views() {
    support::assert_stdout(
        r#"
def total(v: &[i64]) -> i64:
    mut sum = 0
    for x in v:
        sum += *x
    return sum

def bump(v: &mut [i64]) -> ():
    for x in v:
        *x += 1

def main() -> Result[(), ThreadError | AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    with spawn(total, &xs[:2])? as a, spawn(total, &xs[2:])? as b:
        print(a.join(), b.join())?
    with spawn(bump, &mut xs[1:])? as job:
        job.join()
    print(xs)?
    Ok(())
"#,
        "3 7\n[1, 3, 4, 5]\n",
    );
}

#[test]
fn view_index_is_checked_against_the_view() {
    let output = support::run(
        r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3, 4]?
    v = &xs[1:3]
    i = len(xs) - 2
    print(v[i])?
    Ok(())
"#,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("index out of bounds: index 2, length 2"),
        "{stderr}"
    );
}

#[rstest]
#[case(
    "xs = [1, 2].unwrap()\nv = &xs[:]\nv[0] = 5",
    "cannot borrow a shared slice view as mutable; take an exclusive view with `&mut xs[a:b]`"
)]
#[case(
    "xs = [1, 2].unwrap()\nv = &mut xs[:]",
    "mutable borrowing requires a mut binding"
)]
#[case(
    "mut xs = [1, 2].unwrap()\nv = &xs[:]\nxs.append(3).unwrap()\nprint(len(v)).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut xs = [1, 2].unwrap()\na = &mut xs[:2]\nb = &xs[1:]\na[0] = b[0]",
    "conflicting borrow"
)]
#[case(
    "class Holder:\n    v: &[i64]",
    "slice views cannot be stored in class fields yet (backlog B11)"
)]
#[case(
    "xs = [1, 2].unwrap()\nt = (&xs[:], 1)",
    "slice views cannot be stored in tuples yet (backlog B11)"
)]
#[case(
    "xs = [1, 2].unwrap()\no = Some(&xs[:])",
    "slice views cannot be stored in enum payloads yet (backlog B11)"
)]
#[case(
    "def f(o: Option[&[i64]]) -> ():\n    pass",
    "slice views cannot be stored in enum payloads yet (backlog B11)"
)]
#[case(
    "xs = [1, 2].unwrap()\nys = [&xs[:]].unwrap()",
    "slice views cannot be stored in collections yet (backlog B11)"
)]
#[case(
    "def tail(xs: &list[i64]) -> &[i64]:\n    &xs[1:]",
    "a function cannot return a slice view yet (backlog B11)"
)]
#[case(
    "def total(v: &[i64]) -> i64:\n    0\nxs = [1, 2].unwrap()\nn = total(&xs)",
    "this parameter takes a slice view, `&xs[a:b]`; pass a whole list or buffer as `&xs[:]`"
)]
#[case(
    "def bump(v: &mut [i64]) -> ():\n    pass\nmut xs = [1, 2].unwrap()\nbump(&xs[:])",
    "this parameter takes an exclusive slice view, `&mut xs[a:b]`"
)]
#[case(
    "xs = [1, 2].unwrap()\nb = &xs[:] == &xs",
    "a slice view equals another view, so view both sides, as `&xs[:]`"
)]
#[case(
    "xs = [1, 2].unwrap()\nb = &xs[:1] < &xs[1:]",
    "slice views support `==` and `!=`, not `<`"
)]
#[case(
    "xs = [1, 2].unwrap()\nb = 2 in &xs[:]",
    "slice views do not support `in` yet"
)]
#[case(
    "xs = [1, 2].unwrap()\nt = f\"{&xs[:]:d}\".unwrap()",
    "format spec `d` does not apply to &[i64]"
)]
#[case(
    "xs = [1, 2].unwrap()\nv = &xs[:]\nw = v[1:].unwrap()",
    "a copying slice does not take a slice view"
)]
#[case(
    "xs = [1, 2].unwrap()\nw = list(&xs[:]).unwrap()",
    "list() does not take a slice view"
)]
#[case(
    "xs = [1, 2].unwrap()\nv = &xs[:]\nn = v.count(1)",
    "a slice view has no method `count`"
)]
#[case(
    "xs = [1, 2, 3].unwrap()\nv = &xs[::2]",
    "a slice view selects adjacent elements, so it takes no step"
)]
#[case("mut b = b\"abc\"\nv = &mut b[1:]", "a `bytes` literal is read-only")]
#[case(
    "n = len(&[1, 2].unwrap()[:1])",
    "a slice view borrows a named list, Bytes, bytes, or str value"
)]
#[case(
    "xs = [1, 2].unwrap()\nv = &xs[:]?",
    "a slice view `&xs[a:b]` does not allocate, so it takes no `?`"
)]
#[case(
    "def f(v: &[&i64]) -> ():\n    pass",
    "a slice view's elements are list elements"
)]
#[case(
    "rows = [[1].unwrap()].unwrap()\nv = &rows[:]\nrow = v[0]",
    "cannot move an element out of a slice view"
)]
#[case(
    "def f(v: &&[i64]) -> ():\n    pass",
    "a slice view is already a borrow"
)]
#[case(
    "xs = [1, 2].unwrap()\nv = &xs[:]\nf = def() -> i64:\n    return len(v)",
    "anonymous functions cannot capture `v`"
)]
fn invalid_views_are_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_diagnostic(source);
    assert!(error.contains(expected), "{source}\n{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn views_add_no_allocations() {
    let output = support::run(
        r#"
import std.bytes as bytes_mod

def total(v: &[i64]) -> i64:
    mut sum = 0
    for x in v:
        sum += *x
    return sum

def even(x: &i64) -> bool:
    return *x % 2 == 0

def ten(x: &i64) -> i64:
    return *x * 10

def main() -> Result[(), AllocError | IoError | bytes_mod.BytesError]:
    mut xs = [1, 2, 3, 4, 5]?
    mut buffer = bytes_mod.from_text("abc")?
    mut sum = 0
    print("__test_begin_no_allocations__")?
    sum += total(&xs[1:4])
    for x in &mut xs[:2]:
        *x += 1
    view = &xs[-2:]
    sum += view[0] + len(&view[1:])
    if &xs[:1] == &xs[1:2]:
        sum += 100
    tail = &mut buffer[1:]
    tail[0] = 66
    sum += i64(tail[1])
    sum += iter(&xs[1:4]).filter(even).count() + iter(&xs[:]).map(ten).sum()
    for b in &buffer:
        sum += i64(*b)
    print("__test_end_no_allocations__")?
    print(sum, xs, buffer.to_text()?)?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).ends_with("546 [2, 3, 3, 4, 5] aBc\n"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
