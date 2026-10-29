//! Class patterns take a class apart like its generated constructor: one
//! pattern per field, positional then named (issue #558).
mod support;
use support::{assert_stdout as runs, check_diagnostic as diagnostic};

const DECLARATIONS: &str = "class Point:
    x: i64
    y: i64
class Guard:
    n: i64
    m: i64
    def __del__(self) -> ():
        pass
@newtype
class Meters:
    value: i64
class Wrap[T]:
    item: T
enum Shape:
    Dot
    Circle(i64)
";

/// The diagnostic for `main` with `body`, after the shared declarations.
fn rejects(body: &str) -> String {
    let mut source = format!("{DECLARATIONS}def main() -> Result[(), AllocError | IoError]:\n");
    for line in body.lines() {
        source.push_str("    ");
        source.push_str(line);
        source.push('\n');
    }
    source.push_str("    Ok(())\n");
    diagnostic(&source)
}

#[test]
fn positional_and_named_fields_match_in_order() {
    runs(
        r#"
class Point:
    x: i64
    y: i64

def axis(p: &Point) -> i64:
    match p:
        case Point(0, y):
            *y
        case Point(x=x, y=0):
            *x
        case Point(y=y, x=x):
            *x + *y

def main() -> Result[(), IoError]:
    print(axis(&Point(0, 9)), axis(&Point(7, 0)), axis(&Point(3, 4)))?
    match Point(3, 4):
        case Point(0, _) | Point(_, 0):
            print("on an axis")?
        case Point(a, b):
            print(a * b)?
    Ok(())
"#,
        "9 7 7\n12\n",
    );
}

#[test]
fn checked_block_errors_bind_their_fields() {
    runs(
        r#"
def report(a: i64, b: i64) -> Result[(), IoError]:
    result = checked:
        (a + 1) // b
    match result:
        case Ok(value):
            print(value)?
        case Err(OverflowError(wrapped)):
            print("overflow", wrapped)?
        case Err(ZeroDivisionError()):
            print("zero")?
    Ok(())

def main() -> Result[(), IoError]:
    report(9, 2)?
    report(i64.MAX, 1)?
    report(1, 0)?
    doubled = checked:
        i64.MAX * 2
    match doubled:
        case Ok(_):
            pass
        case Err(OverflowError(-2)):
            print("exact")?
        case Err(OverflowError(_)):
            print("other")?
    Ok(())
"#,
        "5\noverflow -9223372036854775808\nzero\nexact\n",
    );
}

#[test]
fn boxes_sums_options_and_generic_classes_match_through() {
    runs(
        r#"
class Point:
    x: i64
    y: i64

class Wrap[T]:
    item: T

@newtype
class Meters:
    value: i64

def main() -> Result[(), AllocError | IoError]:
    match Box(Point(5, 7))?:
        case Point(x, y):
            print(y - x)?
    o: Option[Point] = Some(Point(3, 4))
    match o:
        case Some(Point(x, 4)):
            print(x)?
        case Some(Point(_, _)) | Nothing:
            pass
    value: Point | str = Point(1, 2)
    match value:
        case Point(x, y):
            print(x + y)?
        case text: str:
            print(text)?
    match Wrap([1, 2]?):
        case Wrap(item):
            print(item)?
    match Meters(5):
        case Meters(0):
            print(0)?
        case Meters(v):
            print(v)?
    Ok(())
"#,
        "2\n3\n3\n[1, 2]\n5\n",
    );
}

#[test]
fn owned_matching_moves_bound_fields_and_destroys_the_rest() {
    runs(
        r#"
class Noisy:
    label: str
    def __del__(self) -> ():
        print("drop", self.label).unwrap()

class Pair:
    left: Noisy
    right: Noisy

def main() -> Result[(), IoError]:
    pair = Pair(Noisy("a"), Noisy("b"))
    match pair:
        case Pair(left, _):
            print("arm", left.label)?
    print("after")?
    Ok(())
"#,
        "drop b\narm a\ndrop a\nafter\n",
    );
    assert_eq!(
        diagnostic(
            "class Point:\n    x: list[i64]\n    y: i64\ndef main() -> Result[(), AllocError | IoError]:\n    p = Point([1]?, 2)\n    match p:\n        case Point(x, _):\n            print(x)?\n    print(p.y)?\n    Ok(())\n"
        ),
        "9:11: use of moved binding `p`\n  6:11: note: `p` is moved here"
    );
}

#[test]
fn borrowed_matching_binds_field_references() {
    runs(
        r#"
class Point:
    x: i64
    y: i64

class Guard:
    n: i64
    m: i64
    def __del__(self) -> ():
        print("guard").unwrap()

def main() -> Result[(), IoError]:
    mut p = Point(1, 2)
    match &mut p:
        case Point(x, _):
            *x = 10
    print(p.x)?
    g = Guard(3, 4)
    match &g:
        case Guard(n, m):
            print(*n + *m)?
    print(g.n)?
    Ok(())
"#,
        "10\n7\n3\nguard\n",
    );
    assert_eq!(
        diagnostic(
            "class Point:\n    x: i64\n    y: i64\ndef main() -> Result[(), IoError]:\n    mut p = Point(1, 2)\n    match &mut p:\n        case Point(x, _):\n            p.x = 3\n            *x = 4\n    Ok(())\n"
        )
        .split(':')
        .take(2)
        .collect::<Vec<_>>(),
        ["8", "13"]
    );
}

#[test]
fn a_class_with_del_is_not_destructured_by_move() {
    assert_eq!(
        rejects("match Guard(1, 2):\n    case Guard(n, _):\n        print(n)?"),
        "19:14: cannot move fields out of `Guard`: its `__del__` expects every field initialized; match a reference such as `&value` to borrow them"
    );
    // Binding nothing, or the whole value, moves no field.
    rejects_nothing("match Guard(1, 2):\n    case Guard(_, _):\n        pass");
}

fn rejects_nothing(body: &str) {
    let source = format!(
        "{DECLARATIONS}def main() -> Result[(), AllocError | IoError]:\n{}    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>()
    );
    support::check_source(&source).unwrap();
}

#[test]
fn coverage_follows_the_subpatterns() {
    assert_eq!(
        rejects("match Point(1, 2):\n    case Point(0, _):\n        pass"),
        "18:11: non-exhaustive match; missing Point(_, _)"
    );
    assert_eq!(
        rejects(
            "match Point(1, 2):\n    case Point(a, b):\n        pass\n    case Point(0, 0):\n        pass"
        ),
        "21:9: unreachable case"
    );
    rejects_nothing("match Point(1, 2):\n    case Point(x=_, y=_):\n        pass");
    assert_eq!(
        rejects(
            "o: Option[Point] = Nothing\nmatch o:\n    case Some(Point(0, _)):\n        pass\n    case Nothing:\n        pass"
        ),
        "19:11: non-exhaustive match; missing Some(Point(_, _))"
    );
}

#[test]
fn malformed_class_patterns_are_rejected() {
    for (body, expected) in [
        (
            "case Point(x):\n    pass",
            "19:14: missing field `y` in `Point` pattern; match it with `y=_` to ignore it",
        ),
        (
            "case Point(x, x=y):\n    pass",
            "19:23: field `x` is matched more than once",
        ),
        (
            "case Point(y=b, a):\n    pass",
            "19:25: positional field patterns must precede named ones",
        ),
        (
            "case Point(x=a, z=b):\n    pass",
            "19:25: unknown field `z` on Point",
        ),
        (
            "case Point(a, b, c):\n    pass",
            "19:26: `Point` has 2 fields, but this pattern names 3",
        ),
        (
            "case Meters(v):\n    pass",
            "19:14: a `Meters` pattern cannot match a value of type Point",
        ),
        (
            "case Shape(v):\n    pass",
            "19:14: a `Shape` pattern cannot match a value of type Point",
        ),
        ("case Nope(v):\n    pass", "19:14: unknown type `Nope`"),
    ] {
        let body = format!("match Point(1, 2):\n{}", indent(body));
        assert_eq!(rejects(&body), expected, "{body}");
    }
    assert_eq!(
        rejects("match Wrap(1):\n    case Wrap[i64](item):\n        pass"),
        "19:18: a pattern takes no type arguments, because the matched value supplies them; write `Wrap(...)`"
    );
    assert_eq!(
        rejects(
            "match Shape.Circle(1):\n    case Circle(r):\n        pass\n    case _:\n        pass"
        ),
        "19:14: qualify user-defined variants with their enum type; write `Shape.Circle`"
    );
    assert_eq!(
        rejects(
            "r = checked:\n    1 + 2\nmatch r:\n    case Ok(v):\n        pass\n    case Err(OverflowError):\n        pass"
        ),
        "23:18: `OverflowError` names a class, so it cannot name a binding; write `OverflowError(...)` to match it"
    );
    assert_eq!(
        rejects(
            "r = checked:\n    1 + 2\nmatch r:\n    case Ok(v):\n        pass\n    case Err(ZeroDivisionError()):\n        pass"
        ),
        "23:18: a `ZeroDivisionError` pattern cannot match a payload of type OverflowError[i64]; bind it with a name or skip it with `_`"
    );
}

fn indent(body: &str) -> String {
    body.lines().map(|line| format!("    {line}\n")).collect()
}

#[test]
fn every_field_must_be_visible() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("geo.plenty"),
        "pub class Point:\n    pub x: i64\n    y: i64\npub class Open:\n    pub x: i64\n    pub y: i64\npub def origin() -> Point:\n    Point(0, 0)\nclass Hidden:\n    pub x: i64\n",
    )
    .unwrap();
    let check = |body: &str| {
        let main = dir.path().join("main.plenty");
        std::fs::write(
            &main,
            format!("import geo\nfrom geo import Open\ndef main() -> Result[(), IoError]:\n{body}    Ok(())\n"),
        )
        .unwrap();
        plenty::check_file(&main, None).map_err(|error| error.to_string())
    };
    check("    match geo.Open(1, 2):\n        case geo.Open(a, y=b):\n            print(a + b)?\n    match geo.Open(1, 2):\n        case Open(a, b):\n            print(a * b)?\n").unwrap();
    let error =
        check("    match geo.origin():\n        case geo.Point(x, y=_):\n            print(x)?\n")
            .unwrap_err();
    assert!(
        error.ends_with(
            "main.plenty:5:27: `geo.Point.y` is private to its defining module; declare the member public to allow access from another module"
        ),
        "{error}"
    );
    let error =
        check("    match geo.origin():\n        case geo.Hidden(x):\n            print(x)?\n")
            .unwrap_err();
    assert!(
        error.ends_with("main.plenty:5:14: `geo.Hidden` is private or is not a declared export"),
        "{error}"
    );
}
