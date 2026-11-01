//! The copy rule (issue #636, rule 1 of
//! book/src/proposals/consistency-rules.md): one statement, worded the same in
//! the reference, the cheat sheet and the `plenty` skill, and checked against
//! the compiler here.
mod support;
use support::{check_error, check_source};

/// The rule as every page states it, with links reduced to their text.
const RULE: &str = "Numbers, `bool`, `str`, `bytes`, ranges, and named function values copy. \
A tuple, `Option`, `Result`, enum, or anonymous sum (`A | B`) copies when all of its parts copy. \
A class copies only when it is declared `@copy`. \
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

def increment(x: i64) -> i64:
    x + 1

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
    let closure = "def take[F: Callable[[i64], i64]](f: F) -> ():\n    pass\n\n\
        def main() -> ():\n    n = 1\n    add = def [n](y: i64) -> i64:\n        y + n\n    \
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
