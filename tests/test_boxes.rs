//! `Box[T]` owns one heap value and behaves like an owning reference.
mod support;

fn runs(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn boxes_move_borrow_and_reach_through_to_their_content() {
    runs(
        r#"
class Point:
    x: i64
    y: i64
    def sum(self) -> i64:
        self.x + self.y
def read(point: &Point) -> i64:
    point.x
def bump(point: &mut Point) -> ():
    point.x = point.x + 1
mut b = Box(Point(1, 2)).unwrap()
print(b.x).unwrap()
print(b.sum()).unwrap()
b.y = 5
bump(&mut b)
print(read(&b)).unwrap()
borrowed = &*b
print(borrowed.y).unwrap()
point = *b
print(point).unwrap()
"#,
        "1\n3\n2\n5\nPoint(x=2, y=5)\n",
    );
}

#[test]
fn a_box_converts_to_its_content_where_the_content_is_required() {
    runs(
        r#"
class Point:
    x: i64
    y: i64
class Holder:
    point: Point
    def set(self: &mut Holder, point: Point) -> ():
        self.point = point
def sum(point: Point) -> i64:
    point.x + point.y
def tail(b: Box[Point]) -> Point:
    b
def early(b: Box[Point]) -> Point:
    return b
def wrapped(b: Box[Point]) -> Option[Point]:
    Some(b)
def boxed(x: i64) -> Box[Point]:
    Box(Point(x, x)).unwrap()
print(sum(boxed(1))).unwrap()
print(tail(boxed(2))).unwrap()
print(early(boxed(3))).unwrap()
print(wrapped(boxed(4))).unwrap()
annotated: Point = boxed(5)
print(annotated).unwrap()
mut holder = Holder(boxed(6))
print(holder.point).unwrap()
holder.point = boxed(7)
print(holder.point).unwrap()
holder.set(boxed(8))
print(holder.point).unwrap()
mut points: list[Point] = [boxed(9)].unwrap()
points.append(boxed(10)).unwrap()
points[0] = boxed(11)
print(points).unwrap()
pair: (Point, i64) = (boxed(12), 1)
print(pair).unwrap()
print(sum(Box(boxed(13)).unwrap())).unwrap()
kept: Box[Point] = boxed(14)
moved = kept
print(moved).unwrap()
"#,
        "2
Point(x=2, y=2)
Point(x=3, y=3)
Option[Point].Some(Point(x=4, y=4))
\
Point(x=5, y=5)
Point(x=6, y=6)
Point(x=7, y=7)
Point(x=8, y=8)
\
[Point(x=11, y=11), Point(x=10, y=10)]
(Point(x=12, y=12), 1)
26
Box(Point(x=14, y=14))
",
    );
}

#[test]
fn converting_a_box_drops_the_box_and_keeps_its_content_alive() {
    runs(
        r#"
class Resource:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
def keep(resource: Resource) -> Resource:
    resource
first = keep(Box(Resource("first")).unwrap())
print("held").unwrap()
drop(first)
mut slot = Resource("old")
slot = Box(Resource("new")).unwrap()
print("end").unwrap()
"#,
        "held
first
old
end
new
",
    );
}

#[test]
fn matching_a_box_matches_its_content() {
    runs(
        r#"
enum Shape:
    Circle(i64)
    Rect(i64, i64)
def area(shape: &Shape) -> i64:
    match shape:
        case Shape.Circle(r):
            3 * *r * *r
        case Shape.Rect(w, h):
            *w * *h
boxed = Box(Shape.Rect(2, 3)).unwrap()
print(area(&boxed)).unwrap()
match &boxed:
    case Shape.Rect(w, _):
        print(*w).unwrap()
    case Shape.Circle(_):
        pass
match boxed:
    case Shape.Rect(w, h):
        print(w + h).unwrap()
    case Shape.Circle(_):
        pass
"#,
        "6\n2\n5\n",
    );
}

#[test]
fn boxes_copy_compare_print_and_live_in_collections() {
    runs(
        r#"
class Resource:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
class Bag:
    values: list[i64]
a = Box(Bag([1, 2].unwrap())).unwrap()
mut b = copy(a).unwrap()
b.values.append(3).unwrap()
print(a).unwrap()
print(b).unwrap()
print(a == b).unwrap()
items = [Box(Resource("first")).unwrap(), Box(Resource("second")).unwrap()].unwrap()
print(items[1].name).unwrap()
print("end").unwrap()
"#,
        "Box(Bag(values=[1, 2]))\nBox(Bag(values=[1, 2, 3]))\nFalse\nsecond\nend\nfirst\nsecond\n",
    );
}

#[test]
fn box_contents_and_dereferences_are_checked() {
    for (source, expected) in [
        ("x = 1\nb = Box(&x)", "cannot be stored in a box"),
        ("x = 1\ny = *x", "requires a reference or a Box"),
        ("b = Box(1).unwrap()\nc = b\nprint(b).unwrap()", "moved"),
        (
            "def f(x: i64) -> ():\n    pass\nb = Box(1).unwrap()\nf(b)\nprint(b).unwrap()",
            "moved",
        ),
        (
            "b = Box(1).unwrap()\nprint(b + 1).unwrap()",
            "expected Box[i64], got i64",
        ),
        (
            "b = Box(True).unwrap()\nif b:\n    pass",
            "expected bool, got Box[bool]",
        ),
        (
            "def f(x: str) -> ():\n    pass\nb = Box(1).unwrap()\nf(b)",
            "expected str, got Box[i64]",
        ),
        (
            "b = Box(1).unwrap()\nr = &mut *b",
            "mutable borrowing requires a mut binding",
        ),
        ("class Box:\n    x: i64", "builtin"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}
