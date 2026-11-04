//! The copy rule (issue #636, rule 1 of
//! book/src/proposals/consistency-rules.md): one statement, worded the same in
//! the reference, the cheat sheet and the `plenty` skill, and checked against
//! the compiler here.
mod support;
use support::{check_error, check_source};

/// The rule as every page states it, with links reduced to their text.
const RULE: &str = "Numbers, `bool`, `str`, `bytes`, ranges, and named function values copy. \
A tuple, `Option`, `Result`, enum, or anonymous sum (`A | B`) copies when all of its parts copy. \
A class copies only when it is declared `@copy`, and a generic one only when its type \
arguments also copy. \
Every other value moves: collections, boxes, closures that capture, generators, and other \
class instances.";

const PAGES: &[&str] = &[
    "book/src/design/language/05-ownership/01-bindings-moves-and-copies.md",
    "book/src/design/examples/cheatsheet.md",
    ".agents/skills/plenty/SKILL.md",
    ".agents/skills/plenty/references/ownership.md",
];

/// Markdown reduced to words: `[text](target)` becomes `text`, and line breaks
/// and list indentation become single spaces.
fn plain(markdown: &str) -> String {
    let mut text = String::new();
    let mut rest = markdown;
    while let Some(open) = rest.find('[') {
        let (before, after) = rest.split_at(open);
        text.push_str(before);
        match after.find("](").zip(after.find(')')) {
            Some((middle, close)) if middle < close && !after[1..middle].contains('[') => {
                text.push_str(&after[1..middle]);
                rest = &after[close + 1..];
            }
            _ => {
                text.push('[');
                rest = &after[1..];
            }
        }
    }
    text.push_str(rest);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn pages_state_the_same_rule() {
    let root = env!("CARGO_MANIFEST_DIR");
    for page in PAGES {
        let markdown = std::fs::read_to_string(format!("{root}/{page}")).unwrap();
        assert!(
            plain(&markdown).contains(RULE),
            "{page} must state the copy rule exactly as tests/test_copy_rule.rs does"
        );
    }
}

/// Each category the rule says copies is passed by value on every loop
/// iteration, which a moved value cannot be.
#[test]
fn listed_types_copy() {
    check_source(
        r#"
enum Shape:
    Dot(i64)
    Empty

@copy
class Point:
    x: i64

@copy
class Cell[T: IntType]:
    value: T

def increment(x: i64) -> Result[i64, OverflowError[i64]]:
    Ok(x + 1)

def take[T](value: T) -> ():
    pass

def main() -> ():
    number = 1
    flag = True
    text = "a"
    raw = b"a"
    span = range(0, 3)
    function = increment
    pair = (1, "a")
    maybe = Some(1)
    outcome: Result[i64, str] = Ok(1)
    shape = Shape.Dot(1)
    either: i64 | str = 1
    point = Point(1)
    cell = Cell(i32(1))
    for _ in range(0, 2):
        take(number)
        take(flag)
        take(text)
        take(raw)
        take(span)
        take(function)
        take(pair)
        take(maybe)
        take(outcome)
        take(shape)
        take(either)
        take(point)
        take(cell)
"#,
    )
    .unwrap();
}

/// Each value the rule says moves is rejected on a second use.
#[test]
fn other_values_move() {
    let cases = [
        ("", "[1]?"),
        ("", "(1, [1]?)"),
        ("", "Some([1]?)"),
        ("enum Holder:\n    Items(list[i64])\n", "Holder.Items([1]?)"),
        ("class Point:\n    x: i64\n", "Point(1)"),
        ("", "Box(1)?"),
    ];
    for (declarations, value) in cases {
        let source = format!(
            "{declarations}\ndef take[T](value: T) -> ():\n    pass\n\n\
             def main() -> Result[(), AllocError]:\n    value = {value}\n    \
             take(value)\n    take(value)\n    Ok(())\n"
        );
        let error = check_error(&source);
        assert!(
            error.contains("use of moved binding `value`"),
            "{source}\n{error}"
        );
    }
    let closure = "def take[F: Callable[[i64], Result[i64, OverflowError[i64]]]](f: F) -> ():\n    pass\n\n\
        def main() -> ():\n    n = 1\n    add = def [n](y: i64) -> Result[i64, OverflowError[i64]]:\n        Ok(y + n)\n    \
        take(add)\n    take(add)\n";
    let error = check_error(closure);
    assert!(error.contains("use of moved binding `add`"), "{error}");
}

/// `@copy` is a class annotation today; accepting it on enums is phase 2 of
/// rule 1 (B88). Delete this phase guard when that lands.
#[test]
fn copy_annotation_is_class_only() {
    let error = check_error("@copy\nenum Shape:\n    Dot(i64)\n");
    assert!(
        error.contains("`@copy` applies only to class declarations"),
        "{error}"
    );
}

/// `CopyType` (B84) admits exactly the values the rule says copy. Type
/// arguments are never references, so `&T` cannot satisfy it.
#[test]
fn copy_type_admits_the_listed_types() {
    check_source(
        r#"
enum Shape:
    Dot(i64)
    Empty

@copy
class Pair[T]:
    left: T
    right: T

def increment(x: i64) -> Result[i64, OverflowError[i64]]:
    Ok(x + 1)

def need[T: CopyType](value: T) -> (T, T):
    (value, value)

def need_ok[T: CopyType](result: Result[T, str]) -> (Result[T, str], Result[T, str]):
    (result, result)

def main() -> Result[(), RangeError]:
    nonzero = NonZero.try_from(3)?
    unit: Result[(), str] = Ok(())
    pending: Result[i64, str] = Ok(1)
    either: i64 | str = 1
    need(1)
    need(2.5)
    need(True)
    need("a")
    need(b"a")
    need(range(0, 3))
    need(nonzero)
    need(increment)
    need((1, "a"))
    need(Some(1))
    need(pending)
    need(Shape.Dot(1))
    need(either)
    need(Pair(1, 2))
    need(Pair(Pair("a", "b"), Pair("c", "d")))
    need_ok(unit)
    Ok(())
"#,
    )
    .unwrap();
}

#[test]
fn copy_type_rejects_values_that_move() {
    let cases = [
        ("", "[1]?", "list[i64]"),
        ("", "(1, [1]?)", "tuple[i64, list[i64]]"),
        ("", "Some([1]?)", "Option[list[i64]]"),
        (
            "enum Holder:\n    Items(list[i64])\n",
            "Holder.Items([1]?)",
            "Holder",
        ),
        ("", "Box(1)?", "Box[i64]"),
        (
            "@copy\nclass Pair[T]:\n    left: T\n    right: T\n",
            "Pair([1]?, [2]?)",
            "Pair[list[i64]]",
        ),
    ];
    for (declarations, value, ty) in cases {
        let source = format!(
            "{declarations}\ndef need[T: CopyType](value: T) -> ():\n    pass\n\n\
             def main() -> Result[(), AllocError]:\n    need({value})\n    Ok(())\n"
        );
        let error = check_error(&source);
        assert!(
            error.contains(&format!(
                "{ty} does not satisfy CopyType: it moves on ordinary use"
            )),
            "{source}\n{error}"
        );
    }
    let class = "class Point:\n    x: i64\n\ndef need[T: CopyType](value: T) -> ():\n    pass\n\n\
        def main() -> ():\n    need(Point(1))\n";
    let error = check_error(class);
    assert!(
        error.contains("Point does not satisfy CopyType: it moves on ordinary use; a class copies only when it is declared `@copy`"),
        "{error}"
    );
    let closure = "def need[T: CopyType](value: T) -> ():\n    pass\n\n\
        def main() -> ():\n    n = 1\n    add = def [n](y: i64) -> Result[i64, OverflowError[i64]]:\n        Ok(y + n)\n    \
        need(add)\n";
    let error = check_error(closure);
    assert!(error.contains("does not satisfy CopyType"), "{error}");
}

/// The bound applies to methods and to class and enum parameters alike.
#[test]
fn copy_type_bounds_methods_and_data() {
    check_source(
        r#"
class Stack[T: CopyType]:
    top: T

    def peek(self: &Stack[T]) -> T:
        self.top

    def paired[U: CopyType](self: &Stack[T], other: U) -> (T, U):
        (self.top, other)

enum Slot[T: CopyType]:
    Full(T)
    Empty

def main() -> ():
    stack = Stack("a")
    top = stack.peek()
    both = stack.paired(1)
    slot = Slot[str].Full(top)
"#,
    )
    .unwrap();
    for source in [
        "class Stack[T: CopyType]:\n    top: T\n\ndef main() -> Result[(), AllocError]:\n    \
         stack = Stack([1]?)\n    Ok(())\n",
        "enum Slot[T: CopyType]:\n    Full(T)\n    Empty\n\n\
         def main() -> Result[(), AllocError]:\n    slot = Slot[list[i64]].Full([1]?)\n    Ok(())\n",
    ] {
        let error = check_error(source);
        assert!(
            error.contains("list[i64] does not satisfy CopyType"),
            "{source}\n{error}"
        );
    }
}

/// Generic `@copy` enums are phase 3 of rule 1 (B88); `@copy` on any enum is
/// rejected until phase 2. Delete this phase guard when generic enums accept it.
#[test]
fn generic_copy_enum_waits_for_b88() {
    let error = check_error("@copy\nenum Slot[T]:\n    Full(T)\n    Empty\n");
    assert!(
        error.contains("`@copy` applies only to class declarations"),
        "{error}"
    );
}
