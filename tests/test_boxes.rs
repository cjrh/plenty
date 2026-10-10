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
fn boxed_collections_reach_shared_and_mutating_methods() {
    runs(
        r#"
def append(items: &mut Box[Box[list[i64]]]) -> ():
    items.append(3).unwrap()
def read(items: &Box[Box[list[i64]]]) -> i64:
    items.get(0).unwrap()
mut items = Box(Box([1, 2].unwrap()).unwrap()).unwrap()
append(&mut items)
print(read(&items)).unwrap()
items.reserve(8).unwrap()
items.extend([4, 5].unwrap()).unwrap()
items.reverse()
print(items.pop().unwrap()).unwrap()
print(items.count(3)).unwrap()
print(items.slice(0, 2).unwrap()).unwrap()
items.clear()
print(items).unwrap()
mut entries = Box({1: 10}.unwrap()).unwrap()
entries.insert(2, 20).unwrap()
entries.update({3: 30}.unwrap()).unwrap()
print(entries.get(2).unwrap()).unwrap()
print(entries.pop(1).unwrap()).unwrap()
print(entries.keys().unwrap().count(3)).unwrap()
print(entries.values().unwrap().count(30)).unwrap()
entries.clear()
print(entries).unwrap()
mut values = Box({1, 2}.unwrap()).unwrap()
values.add(3).unwrap()
values.update({4}.unwrap()).unwrap()
print(values.issuperset({1, 2}.unwrap())).unwrap()
values.intersection_update({2, 3, 4}.unwrap())
values.difference_update({4}.unwrap())
print(values.discard(2)).unwrap()
values.clear()
print(values).unwrap()
text = Box(Box("  hello  ").unwrap()).unwrap()
print(text.strip().unwrap()).unwrap()
print(text.startswith("  ")).unwrap()
print(text.get(2).unwrap()).unwrap()
print(text).unwrap()
"#,
        "1\n1\n1\n[5, 4]\nBox(Box([]))\n20\n10\n1\n1\nBox({})\nTrue\nTrue\nBox(set())\nhello\nTrue\nh\nBox(Box(\"  hello  \"))\n",
    );
}

#[test]
fn temporary_boxed_receivers_hold_contents_until_the_call_finishes() {
    runs(
        r#"
class Resource:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
print(Box(Box("hello").unwrap()).unwrap().startswith("he")).unwrap()
Box("hello").unwrap().strip().unwrap()
print(Box([1, 2].unwrap()).unwrap().get(1).unwrap()).unwrap()
items = Box([Resource("held")].unwrap()).unwrap().slice(0, 1).unwrap()
print("alive").unwrap()
drop(items)
"#,
        "True\n2\nalive\nheld\n",
    );
}

#[test]
fn boxed_method_receivers_preserve_places_and_owned_payloads() {
    runs(
        r#"
class Resource:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
class Holder:
    items: Box[list[Resource]]
mut holder = Holder(Box([Resource("first")].unwrap()).unwrap())
holder.items.append(Resource("second")).unwrap()
item = holder.items.pop().unwrap()
print(item.name).unwrap()
holder.items.clear()
drop(item)
mut boxes = [Box([1].unwrap()).unwrap()].unwrap()
boxes[0].append(2).unwrap()
print(boxes[0].get(1).unwrap()).unwrap()
print(boxes).unwrap()
"#,
        "second\nfirst\nsecond\n2\n[Box([1, 2])]\n",
    );
}

#[test]
fn boxed_method_borrows_reject_immutable_shared_and_aliasing_mutation() {
    for (source, expected) in [
        (
            "items = Box([1].unwrap()).unwrap()\nitems.append(2).unwrap()",
            "mutable borrowing requires a mut binding",
        ),
        (
            "mut items = Box(Box([1].unwrap()).unwrap()).unwrap()\nshared = &items\nshared.clear()",
            "cannot borrow shared reference as mutable",
        ),
        (
            "mut items = Box([1].unwrap()).unwrap()\nshared = &items\nitems.append(2).unwrap()\nprint(shared.get(0)).unwrap()",
            "borrow",
        ),
        (
            "mut values = Box({1, 2}.unwrap()).unwrap()\nshared = &*values\nvalues.intersection_update(shared)",
            "borrow",
        ),
        (
            "items = Box([1].unwrap()).unwrap()\nmoved = items\nprint(items.get(0)).unwrap()",
            "moved",
        ),
        (
            "class Resource:\n    value: i64\nitems = Box([Resource(1)].unwrap()).unwrap()\nitems.slice(0, 1).unwrap()",
            "slice with owned elements requires an owned temporary",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
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
