//! `@copy` classes (issue #535): small records whose every field copies opt in
//! to copy semantics, so ordinary use copies the instance instead of moving it.
//! See book/src/design/language/03-types/classes.md#copy-classes.
mod support;
use support::{assert_stdout as runs, check_error, check_source};

const POINT: &str = r#"
@copy
class Point:
    x: i64
    y: i64
"#;

fn rejects(source: &str, expected: &str) {
    let error = check_error(source);
    assert!(error.contains(expected), "{source}\n{error}");
}

#[test]
fn accepted_field_types() {
    check_source(
        r#"
enum Kind:
    Small
    Large(i64)
@copy
class Empty:
    pass
@copy
class Label:
    text: str
    kind: Kind
    size: Option[f64]
    pair: (i64, str, bool)
    inner: Empty
"#,
    )
    .unwrap();
}

#[test]
fn a_field_may_name_a_copy_class_declared_later() {
    check_source(
        r#"
@copy
class Segment:
    start: Point
    end: Point
@copy
class Point:
    x: i64
"#,
    )
    .unwrap();
}

#[test]
fn assignment_arguments_and_returns_copy() {
    runs(
        &format!(
            r#"{POINT}
def shifted(p: Point) -> Point:
    mut q = p
    q.x = q.x + 10
    q
def main() -> Result[(), AllocError | IoError]:
    p = Point(1, 2)
    mut q = p
    q.y = 9
    r = shifted(p)
    print(p, q, r)?
    print(p == Point(1, 2), copy(p)? == p)?
    Ok(())
"#
        ),
        "Point(x=1, y=2) Point(x=1, y=9) Point(x=11, y=2)\nTrue True\n",
    );
}

#[test]
fn nested_copy_classes_copy_as_a_whole() {
    runs(
        &format!(
            r#"{POINT}
@copy
class Tagged:
    name: str
    at: Point
    previous: Option[Point]
def main() -> Result[(), AllocError | IoError]:
    a = Tagged("a", Point(1, 2), Nothing)
    mut b = a
    b.at.x = 7
    b.previous = Some(a.at)
    print(a)?
    print(b)?
    Ok(())
"#
        ),
        "Tagged(name=\"a\", at=Point(x=1, y=2), previous=Option[Point].Nothing)\n\
         Tagged(name=\"a\", at=Point(x=7, y=2), previous=Option[Point].Some(Point(x=1, y=2)))\n",
    );
}

#[test]
fn collections_closures_and_enums_copy_their_elements() {
    runs(
        &format!(
            r#"{POINT}
enum Shape:
    Dot(Point)
    Line(Point, Point)
def main() -> Result[(), AllocError | IoError]:
    p = Point(1, 2)
    mut points = [p, p]?
    points[0].x = 5
    first = points[0]
    for point in &points:
        print(point)?
    by_name = {{"p": p}}?
    get = def [p]() -> i64:
        p.x + p.y
    line = Shape.Line(p, first)
    match &line:
        case Shape.Line(a, b):
            print(a, b)?
        case Shape.Dot(a):
            print(a)?
    print(p, first, get(), len(by_name))?
    Ok(())
"#
        ),
        "Point(x=5, y=2)\nPoint(x=1, y=2)\nPoint(x=1, y=2) Point(x=5, y=2)\n\
         Point(x=1, y=2) Point(x=5, y=2) 3 1\n",
    );
}

#[test]
fn owned_receivers_and_generic_parameters_copy() {
    runs(
        &format!(
            r#"{POINT}
    def sum(self: Point) -> i64:
        self.x + self.y
def both[T](value: T) -> (T, T):
    (value, value)
def main() -> Result[(), AllocError | IoError]:
    p = Point(3, 4)
    print(p.sum(), both(p), p)?
    Ok(())
"#
        ),
        "7 (Point(x=3, y=4), Point(x=3, y=4)) Point(x=3, y=4)\n",
    );
}

#[test]
fn ordinary_classes_still_move() {
    rejects(
        r#"
class Point:
    x: i64
p = Point(1)
q = p
print(p).unwrap()
"#,
        "moved",
    );
}

#[test]
fn rejects_fields_that_move() {
    for (field, ty) in [
        ("items: list[i64]", "list[i64]"),
        ("boxed: Box[i64]", "Box[i64]"),
        ("handle: Handle", "Handle"),
        ("maybe: Option[list[i64]]", "Option[list[i64]]"),
    ] {
        rejects(
            &format!("class Handle:\n    n: i64\n@copy\nclass Bad:\n    {field}\n"),
            &format!(
                "`@copy` class `Bad` cannot have field `{}` of type `{ty}`, which moves",
                field.split(':').next().unwrap()
            ),
        );
    }
}

#[test]
fn rejects_a_destructor() {
    rejects(
        r#"
@copy
class Bad:
    n: i64
    def __del__(self: &mut Bad) -> ():
        pass
"#,
        "a `@copy` class cannot define `__del__`",
    );
}

#[test]
fn rejects_type_parameters() {
    rejects(
        "@copy\nclass Pair[T]:\n    a: T\n",
        "`@copy` classes cannot have type parameters",
    );
}

#[test]
fn rejects_unknown_and_malformed_annotations() {
    for (source, expected) in [
        (
            "@final\nclass A:\n    n: i64\n",
            "unknown annotation `@final`",
        ),
        (
            "@protocol.Show\nclass A:\n    n: i64\n",
            "unknown annotation `@protocol`",
        ),
        (
            "@copy()\nclass A:\n    n: i64\n",
            "`@copy` takes no arguments",
        ),
        ("@copy class A:\n    n: i64\n", "`@copy` takes no arguments"),
        (
            "@\nclass A:\n    n: i64\n",
            "expected an annotation name after `@`",
        ),
        (
            "@copy\n@copy\nclass A:\n    n: i64\n",
            "duplicate annotation `@copy`",
        ),
    ] {
        rejects(source, expected);
    }
}

#[test]
fn rejects_annotations_on_other_declarations() {
    for (source, place) in [
        ("@copy\ndef f() -> ():\n    pass\n", "class declarations"),
        ("@copy\nenum E:\n    A\n", "class declarations"),
        ("@copy\ntype T = i64\n", "class declarations"),
        (
            "class A:\n    @copy\n    n: i64\n",
            "class declarations, not class members",
        ),
        (
            "enum E:\n    @copy\n    A\n",
            "class declarations, not enum members",
        ),
        (
            "def main() -> ():\n    @copy\n    pass\n",
            "class declarations, not statements",
        ),
    ] {
        rejects(source, &format!("`@copy` applies only to {place}"));
    }
}

#[test]
fn the_copy_builtin_keeps_its_name() {
    runs(&format!("{POINT}\ncopy = 3\nprint(copy).unwrap()\n"), "3\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn copying_needs_no_allocation() {
    runs(
        &format!(
            r#"{POINT}
@copy
class Labelled:
    name: str
    at: Point
def pass_through(value: Labelled) -> Labelled:
    value
def main() -> Result[(), AllocError | IoError]:
    a = Labelled("origin", Point(0, 0))
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    mut b = a
    b.at.x = 4
    c = pass_through(b)
    d = (a, c)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(a, d)?
    Ok(())
"#
        ),
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n\
         __test_restore_allocations__\n__test_end_no_allocations__\n\
         Labelled(name=\"origin\", at=Point(x=0, y=0)) \
         (Labelled(name=\"origin\", at=Point(x=0, y=0)), Labelled(name=\"origin\", at=Point(x=4, y=0)))\n",
    );
}
