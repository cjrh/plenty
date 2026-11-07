//! `Box[T]` owns one heap value and behaves like an owning reference.
mod support;
use support::assert_stdout as runs;

#[test]
fn boxes_move_borrow_and_reach_through_to_their_content() {
    runs(
        r#"
class Point:
    x: i64
    y: i64
    def sum(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.x + self.y)
def read(point: &Point) -> i64:
    point.x
def bump(point: &mut Point) -> Result[(), OverflowError[i64]]:
    point.x = point.x + 1
    Ok(())
def main() -> Result[(), OverflowError[i64]]:
    mut b = Box(Point(1, 2)).unwrap()
    print(b.x).unwrap()
    print(b.sum()?).unwrap()
    b.y = 5
    bump(&mut b)?
    print(read(&b)).unwrap()
    borrowed = &*b
    print(borrowed.y).unwrap()
    point = *b
    print(point).unwrap()
    Ok(())
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
def sum(point: Point) -> Result[i64, OverflowError[i64]]:
    Ok(point.x + point.y)
def tail(b: Box[Point]) -> Point:
    b
def early(b: Box[Point]) -> Point:
    return b
def wrapped(b: Box[Point]) -> Option[Point]:
    Some(b)
def boxed(x: i64) -> Box[Point]:
    Box(Point(x, x)).unwrap()
def main() -> Result[(), OverflowError[i64]]:
    print(sum(boxed(1))?).unwrap()
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
    print(sum(Box(boxed(13)).unwrap())?).unwrap()
    kept: Box[Point] = boxed(14)
    moved = kept
    print(moved).unwrap()
    Ok(())
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
def area(shape: &Shape) -> Result[i64, OverflowError[i64]]:
    match shape:
        case Shape.Circle(r):
            Ok(3 * *r * *r)
        case Shape.Rect(w, h):
            Ok(*w * *h)
def main() -> Result[(), OverflowError[i64]]:
    boxed = Box(Shape.Rect(2, 3)).unwrap()
    print(area(&boxed)?).unwrap()
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
    Ok(())
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
print(entries.keys_snapshot().unwrap().count(3)).unwrap()
print(entries.values_snapshot().unwrap().count(30)).unwrap()
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
        (
            "x = 1\ny = *x",
            "requires a reference, a Box, or a Shared handle",
        ),
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

/// Boxes are not zeroed: the store writes the value word and the payload it
/// names, and readers read only those (issue #814). Each box here can reuse
/// the freed storage of a larger value, so a reader that used bytes past a
/// smaller sum variant's payload could print, copy or compare stale content.
/// The output cannot show such a read when the stale bytes happen to match;
/// running the program under Valgrind checks that no branch uses them.
#[test]
fn reused_box_storage_holds_only_the_new_content() {
    runs(
        r#"
class Small:
    a: i64
class Big:
    a: i64
    b: i64
    c: i64
    e: str
enum Shape:
    Empty
    Num(i64)
    S(Small)
    B(Big)
    Pair(i64, i64)
def unbox(b: Box[Shape]) -> Shape:
    b
def main() -> Result[(), AllocError | IoError]:
    drop(Box(Big(1, 2, 3, "old"))?)
    record = Box(Big(4, 5, 6, "new"))?
    print(record)?
    drop(Box(Shape.B(Big(7, 8, 9, "gone")))?)
    small = Box(Shape.S(Small(10)))?
    twin = copy(small)?
    print(twin == small, twin)?
    drop(twin)
    drop(Box(Shape.B(Big(11, 12, 13, "gone")))?)
    number = Box(Shape.Num(14))?
    print(number == copy(number)?, number)?
    drop(Box(Shape.B(Big(15, 16, 17, "gone")))?)
    pair = unbox(Box(Shape.Pair(18, 19))?)
    print(pair)?
    drop(Box(Shape.B(Big(20, 21, 22, "gone")))?)
    empty = Box(Shape.Empty)?
    print(empty == copy(empty)?, empty)?
    drop(Box(Some(Big(23, 24, 25, "gone")))?)
    nothing: Option[Big] = Nothing
    none = Box(nothing)?
    print(none == copy(none)?, none)?
    Ok(())
"#,
        "Box(Big(a=4, b=5, c=6, e=\"new\"))\n\
         True Box(Shape.S(Small(a=10)))\n\
         True Box(Shape.Num(14))\n\
         Shape.Pair(18, 19)\n\
         True Box(Shape.Empty)\n\
         True Box(Option[Big].Nothing)\n",
    );
}

/// A box of a class with a record stores the record with no value word
/// (issue #814): construction copies the record, a borrow names the bytes
/// before it, and moving out clears it. A protocol box of the same class
/// keeps the word, since its adapters receive the slot without the type;
/// `Sum.tag` makes releasing it with the flat layout fail.
#[test]
fn flat_class_boxes_hold_the_record_itself() {
    runs(
        r#"
class Named:
    name: str
    count: i64
class Pair:
    first: Named
    second: Named
class Guard:
    n: i64
    def __del__(self) -> ():
        print(f"drop {self.n}".unwrap()).unwrap()
protocol Total:
    def total(self) -> i64:
        pass
class Sum:
    a: i64
    b: i64
    tag: str
    def total(self) -> i64:
        unchecked: # Small values.
            self.a + self.b
def rename(named: &mut Named) -> ():
    named.name = "renamed"
def main() -> Result[(), AllocError | IoError]:
    mut b = Box(Named("x", 1))?
    rename(&mut *b)
    b.count = 42
    c = copy(b)?
    print(c == b, c)?
    taken = *b
    print(taken.name)?
    pair = Box(Pair(Named("a", 1), Named("b", 2)))?
    print(pair.second.name, copy(pair)? == pair)?
    drop(Box(Guard(7))?)
    held = *Box(Guard(8))?
    print(held.n)?
    drop(held)
    erased = Box[Total](Sum(2, 3, "erased"))?
    plain = Box(Sum(4, 5, "plain"))?
    print(erased.total(), plain.total())?
    maybe: Option[Box[Named]] = Some(Box(Named("opt", 1))?)
    print(maybe)?
    Ok(())
"#,
        "True Box(Named(name=\"renamed\", count=42))\n\
         renamed\n\
         b True\n\
         drop 7\n\
         8\n\
         drop 8\n\
         5 9\n\
         Option[Box[Named]].Some(Box(Named(name=\"opt\", count=1)))\n",
    );
}
