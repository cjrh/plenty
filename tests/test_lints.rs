//! Checker warnings (issue #819): see
//! book/src/design/toolchain/01-building/warnings.md.
mod support;
use std::process::Command;
use support::{check_warnings, program};

const MAIN: &str = "def main() -> Result[(), AllocError]:\n";

fn redundant(text: &str, ty: &str) -> String {
    format!(
        "`copy({text})?` is redundant: `{ty}` copies on assignment and when passed or returned; write `{text}` [redundant-copy]"
    )
}

fn redundant_argument(ty: &str) -> String {
    format!(
        "this `copy(...)?` is redundant: `{ty}` copies on assignment and when passed or returned; write the argument without `copy(...)?` [redundant-copy]"
    )
}

/// Each warning's message, without its position.
fn messages(source: &str) -> Vec<String> {
    check_warnings(source)
        .into_iter()
        .map(|w| w.split_once(": warning: ").unwrap().1.to_owned())
        .collect()
}

#[test]
fn copyable_values_are_reported_with_their_replacement() {
    let source = r#"
@copy
class Point[T]:
    x: T
    y: T

class Named:
    name: str

enum Shape:
    Circle(f64)
    Square(f64)

def main() -> Result[(), AllocError | IoError]:
    original = "hello"
    a = copy(original)?
    b = copy(5)?
    point = Point[i64](1, 2)
    c = copy(point)?
    shape = Shape.Circle(1.0)
    d = copy(shape)?
    named = Named("x")
    e = copy(named.name)?
    words = ["a", "b"]?
    f = copy(words[1])?
    pair = (1, "a")
    g = copy((pair))?
    print(a, b, c.x, e, f, g[1])?
    match d:
        case Shape.Circle(r):
            print(r)?
        case Shape.Square(s):
            print(s)?
    Ok(())
"#;
    assert_eq!(
        messages(source),
        [
            redundant("original", "str"),
            redundant_argument("i64"),
            redundant("point", "Point[i64]"),
            redundant("shape", "Shape"),
            redundant("named.name", "str"),
            redundant_argument("str"),
            redundant("pair", "tuple[i64, str]"),
        ]
    );
}

fn redundant_replaced(text: &str, ty: &str, replacement: &str) -> String {
    format!(
        "`copy({text})?` is redundant: `{ty}` copies on assignment and when passed or returned; write `{replacement}` [redundant-copy]"
    )
}

fn redundant_borrowed_place(ty: &str) -> String {
    format!(
        "this `copy(&...)?` is redundant: `{ty}` copies on assignment and when passed or returned; write the borrowed place without `copy(&...)?` [redundant-copy]"
    )
}

/// Borrows and shared references of copyable values, with every warning's
/// suggestion; `BORROWS_REPLACED` applies them.
const BORROWS: &str = r#"
@copy
class Point:
    x: i64
    y: i64

class Named:
    name: str

def through(word: &str) -> Result[str, AllocError]:
    Ok(copy(word)?)

def field(named: &Named) -> Result[str, AllocError]:
    Ok(copy(&named.name)?)

def element(words: &list[str]) -> Result[str, AllocError]:
    Ok(copy(&words[0])?)

def point(p: &Point) -> Result[Point, AllocError]:
    Ok(copy(p)?)

def x_of(q: &Point) -> Result[i64, AllocError]:
    Ok(copy(q)?.x)

def grouped(g: &Point) -> Result[i64, AllocError]:
    Ok((copy(g)?).y)

def last(t: &tuple[i64, str]) -> Result[str, AllocError]:
    Ok(copy(t)?[1])

def shout(u: &str) -> Result[str, AllocError]:
    Ok(copy(u)?.upper()?)

def size(r: &str) -> Result[i64, AllocError]:
    Ok(copy(r)?.byte_len())

def main() -> Result[(), AllocError | IoError]:
    s = "hi"
    a = copy(&s)?
    named = Named("x")
    b = copy(&named.name)?
    words = ["a", "b"]?
    c = copy(&words[1])?
    n = 7
    d = copy((&n))?
    mut total = ""
    for w in &words:
        total = (total + copy(w)?)?
    print(a, b, c, d, total, through(s)?, field(&named)?, element(&words)?, point(&Point(1, 2))?.y)?
    print(x_of(&Point(3, 4))?, grouped(&Point(3, 4))?, last(&(5, "z"))?, shout("ab")?, size("abc")?)?
    Ok(())
"#;

const BORROWS_REPLACED: [(&str, &str); 14] = [
    ("copy(word)?", "*word"),
    ("copy(&named.name)?", "named.name"),
    ("copy(&words[0])?", "words[0]"),
    ("copy(p)?", "*p"),
    ("copy(q)?", "(*q)"),
    ("copy(g)?", "*g"),
    ("copy(t)?", "(*t)"),
    ("copy(u)?", "(*u)"),
    ("copy(r)?", "(*r)"),
    ("copy(&s)?", "s"),
    ("copy(&named.name)?", "named.name"),
    ("copy(&words[1])?", "words[1]"),
    ("copy((&n))?", "n"),
    ("copy(w)?", "*w"),
];

#[test]
fn borrows_and_references_of_copyable_values_are_reported() {
    assert_eq!(
        messages(BORROWS),
        [
            redundant_replaced("word", "str", "*word"),
            redundant_replaced("&named.name", "str", "named.name"),
            redundant_borrowed_place("str"),
            redundant_replaced("p", "Point", "*p"),
            redundant_replaced("q", "Point", "(*q)"),
            redundant_replaced("g", "Point", "*g"),
            redundant_replaced("t", "tuple[i64, str]", "(*t)"),
            redundant_replaced("u", "str", "(*u)"),
            redundant_replaced("r", "str", "(*r)"),
            redundant_replaced("&s", "str", "s"),
            redundant_replaced("&named.name", "str", "named.name"),
            redundant_borrowed_place("str"),
            redundant_replaced("&n", "i64", "n"),
            redundant_replaced("w", "str", "*w"),
        ]
    );
}

#[test]
fn applying_borrow_suggestions_checks_and_keeps_behavior() {
    let mut replaced = BORROWS.to_owned();
    for (from, to) in BORROWS_REPLACED {
        assert!(replaced.contains(from), "{from}");
        replaced = replaced.replacen(from, to, 1);
    }
    assert!(!replaced.contains("copy("), "{replaced}");
    assert_eq!(messages(&replaced), Vec::<String>::new());
    let expected = "hi x b 7 ab hi x a 2\n3 4 z AB 3\n";
    assert_eq!(support::run_stdout(BORROWS), expected);
    assert_eq!(support::run_stdout(&replaced), expected);
}

#[test]
fn borrows_of_moving_values_and_exclusive_references_are_left_alone() {
    let source = r#"
class Record:
    values: list[i64]

def peek(text: &mut str) -> Result[str, AllocError]:
    Ok(copy(text)?)

def main() -> Result[(), AllocError | IoError]:
    items = [1, 2]?
    kept = copy(&items)?
    record = Record(copy(&items)?)
    again = copy(&record)?
    values = copy(&record.values)?
    nested = [[1]?]?
    inner = copy(&nested[0])?
    r = &items
    through = copy(r)?
    bytes = copy(&b"z")?
    mut text = "a"
    peeked = peek(&mut text)?
    borrowed = copy(&text)
    print(len(kept), len(again.values), len(values), len(inner), len(through), len(bytes), peeked)?
    match borrowed:
        case Ok(t):
            print(t)?
        case Err(error):
            return Err(error)
    Ok(())
"#;
    assert_eq!(messages(source), Vec::<String>::new());
}

#[test]
fn warnings_point_at_the_copy_call() {
    let warnings = check_warnings(&format!("{MAIN}    x = 1\n    y = copy(x)?\n    Ok(())\n"));
    let [warning] = warnings.as_slice() else {
        panic!("{warnings:?}");
    };
    assert!(warning.starts_with("3:9: warning: "), "{warning}");
}

#[test]
fn moving_values_dereferences_results_and_computed_arguments_are_left_alone() {
    let source = r#"
class Record:
    values: list[i64]

def make() -> str:
    return "made"

def through(word: &str) -> Result[str, AllocError]:
    Ok(copy(*word)?)

def main() -> Result[(), AllocError | IoError]:
    items = [1, 2]?
    kept = copy(items)?
    record = Record(copy(items)?)
    again = copy(record)?
    original = "hello"
    r = &original
    rc = copy(*r)?
    direct = copy(original)
    made = copy(make())?
    print(len(kept), len(again.values), rc, made, through(original)?)?
    match direct:
        case Ok(text):
            print(text)?
        case Err(error):
            return Err(error)
    Ok(())
"#;
    assert_eq!(messages(source), Vec::<String>::new());
}

#[test]
fn generic_bodies_are_left_alone() {
    let source = r#"
def duplicate[T](value: T) -> Result[T, AllocError]:
    Ok(copy(value)?)

def main() -> Result[(), AllocError | IoError]:
    s = duplicate("x")?
    items = duplicate([1, 2]?)?
    print(s, len(items))?
    Ok(())
"#;
    assert_eq!(messages(source), Vec::<String>::new());
}

/// Method, anonymous function, and closure bodies (issue #846), with every
/// warning's suggestion; `BODIES_REPLACED` applies them.
const BODIES: &str = r#"
@copy
class Point:
    x: i64
    y: i64

    def again(self) -> Result[Point, AllocError]:
        Ok(copy(self)?)

    def first(self) -> Result[i64, AllocError]:
        Ok(copy(self)?.x)

    def label(self, name: str) -> Result[str, AllocError]:
        Ok(copy(name)?)

class Named:
    name: str

    def shown(self) -> Result[str, AllocError]:
        Ok(copy(self.name)?)

enum Shape:
    Circle(f64)
    Square(f64)

    def same(self) -> Result[Shape, AllocError]:
        Ok(copy(self)?)

def main() -> Result[(), AllocError | IoError]:
    p = Point(1, 2)
    inner = def(s: str) -> Result[str, AllocError]:
        Ok(copy(s)?)
    k = 3
    word = "w"
    held = def[k, p]() -> Result[tuple[i64, i64], AllocError]:
        Ok((copy(k)?, copy(p)?.y))
    borrowed = def[&word]() -> Result[str, AllocError]:
        Ok(copy(&word)?)
    match Shape.Square(2.0).same()?:
        case Shape.Circle(r):
            print(r)?
        case Shape.Square(s):
            print(s)?
    print(p.again()?.x, p.first()?, p.label("n")?, Named("m").shown()?, inner("b")?, held()?[0], held()?[1], borrowed()?)?
    Ok(())
"#;

const BODIES_REPLACED: [(&str, &str); 9] = [
    ("copy(self)?", "*self"),
    ("copy(self)?", "(*self)"),
    ("copy(name)?", "name"),
    ("copy(self.name)?", "self.name"),
    ("copy(self)?", "*self"),
    ("copy(s)?", "s"),
    ("copy(k)?", "k"),
    ("copy(p)?", "p"),
    ("copy(&word)?", "word"),
];

#[test]
fn method_anonymous_function_and_closure_bodies_are_reported() {
    assert_eq!(
        messages(BODIES),
        [
            redundant_replaced("self", "Point", "*self"),
            redundant_replaced("self", "Point", "(*self)"),
            redundant("name", "str"),
            redundant("self.name", "str"),
            redundant_replaced("self", "Shape", "*self"),
            redundant("s", "str"),
            redundant("k", "i64"),
            redundant("p", "Point"),
            redundant_replaced("&word", "str", "word"),
        ]
    );
}

#[test]
fn applying_body_suggestions_checks_and_keeps_behavior() {
    let mut replaced = BODIES.to_owned();
    for (from, to) in BODIES_REPLACED {
        assert!(replaced.contains(from), "{from}");
        replaced = replaced.replacen(from, to, 1);
    }
    assert!(!replaced.contains("copy("), "{replaced}");
    assert_eq!(messages(&replaced), Vec::<String>::new());
    let expected = "2.0\n1 1 n m b 3 2 w\n";
    assert_eq!(support::run_stdout(BODIES), expected);
    assert_eq!(support::run_stdout(&replaced), expected);
}

#[test]
fn generic_method_and_closure_bodies_are_left_alone() {
    let source = r#"
class Holder[T]:
    item: T

    def dup(self) -> Result[T, AllocError]:
        Ok(copy(self.item)?)

enum Maybe[T]:
    Some(T)
    Nothing

    def get(self, fallback: T) -> Result[T, AllocError]:
        match self:
            case Maybe.Some(v):
                return Ok(copy(v)?)
            case Maybe.Nothing:
                return Ok(copy(fallback)?)

def twice[T](value: T) -> Result[T, AllocError]:
    f = def(x: T) -> Result[T, AllocError]:
        Ok(copy(x)?)
    g = def[&value]() -> Result[T, AllocError]:
        Ok(copy(value)?)
    g()?
    f(copy(&value)?)

def main() -> Result[(), AllocError | IoError]:
    h = Holder(5)
    m = Maybe[str].Some("a")
    print(h.dup()?, m.get("b")?, twice("c")?, len(twice([1]?)?))?
    Ok(())
"#;
    assert_eq!(messages(source), Vec::<String>::new());
}

#[test]
fn checking_prints_warnings_and_succeeds_but_running_does_not_print_them() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.plenty");
    std::fs::write(
        &file,
        program(
            "def main() -> Result[(), AllocError | IoError]:\n    s = \"hi\"\n    t = copy(s)?\n    print(t)?\n    Ok(())\n",
        ),
    )
    .unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--check")
        .arg(&file)
        .output()
        .unwrap();
    assert!(checked.status.success());
    assert!(checked.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&checked.stderr),
        format!(
            "{}:3:9: warning: {}\n",
            file.display(),
            redundant("s", "str")
        )
    );
    let ran = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg(&file)
        .output()
        .unwrap();
    assert!(ran.status.success());
    assert_eq!(ran.stdout, b"hi\n");
    assert!(ran.stderr.is_empty());
}
