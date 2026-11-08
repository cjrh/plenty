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

fn unused_import(binding: &str) -> String {
    format!(
        "import binding `{binding}` is unused; remove it unless the import intentionally includes the module in checking [unused-import]"
    )
}

#[test]
fn unused_imports_report_each_binding_at_its_name() {
    let warnings = check_warnings(
        "import std.math as m\nfrom std.bytes import Bytes as B, BytesError\ndef main() -> ():\n    pass\n",
    );
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    for (warning, prefix, binding) in [
        (&warnings[0], "1:20: warning: ", "m"),
        (&warnings[1], "2:32: warning: ", "B"),
        (&warnings[2], "2:35: warning: ", "BytesError"),
    ] {
        assert_eq!(warning, &format!("{prefix}{}", unused_import(binding)));
    }
}

#[test]
fn imported_types_and_uninstantiated_generic_bodies_count_as_uses() {
    assert!(
        messages(
            r#"
from std.bytes import Bytes as B, BytesError
from std.sync import Mutex
import std.math as m
class Buffer:
    value: B

def unused_generic[T](value: T, bytes: &B, mutex: &Mutex[i64]) -> Result[i64, BytesError]:
    Ok(m.min(1, 2))

def main() -> ():
    pass
"#
        )
        .is_empty()
    );
}

#[test]
fn local_shadowing_does_not_count_as_using_an_import() {
    assert_eq!(
        messages("import std.math as m\ndef main() -> ():\n    m = 2\n    drop(m)\n"),
        [unused_import("m")]
    );
}

#[test]
fn only_selected_target_imports_and_uses_are_linted() {
    assert!(messages("if const target.pointer_width == 64:\n    import std.math as m\nelse:\n    import nonexistent\ndef main() -> ():\n    if const target.pointer_width == 64:\n        drop(m.min(1, 2))\n").is_empty());
}

#[test]
fn aliases_of_the_same_symbol_and_namespaces_are_tracked_independently() {
    assert_eq!(
        messages(
            "from std.math import min as first, min as second\nimport std.math as m\ndef main() -> ():\n    drop(first(1, 2))\n"
        ),
        [unused_import("second"), unused_import("m")]
    );
}

#[test]
fn builtin_type_reimports_count_type_uses() {
    assert!(messages("from std.channel import Sender\ndef unused(sender: Sender[i64]) -> ():\n    drop(sender)\ndef main() -> ():\n    pass\n").is_empty());
}

fn text_view_parameter(name: &str, ty: &str) -> String {
    format!(
        "parameter `{name}` only supplies text reads; consider `&strview` instead of `{ty}` to also accept substring views [text-view-parameter]"
    )
}

#[test]
fn text_readers_accept_a_view_without_changing_the_body() {
    for ty in ["str", "&str"] {
        for (output, body) in [
            ("i64", "len(text)"),
            ("i64", "text.byte_len()"),
            ("bool", "text.startswith(\"a\")"),
            ("bool", "text == \"a\""),
            ("bool", "\"a\" in text"),
            ("Result[str, AllocError]", "text.upper()"),
            ("Result[str, AllocError]", "text[1:]"),
        ] {
            let source = format!("def reader(text: {ty}) -> {output}:\n    {body}\n");
            assert_eq!(
                messages(&source),
                [text_view_parameter("text", ty)],
                "{source}"
            );
            let changed = source.replace(&format!("text: {ty}"), "text: &strview");
            assert!(messages(&changed).is_empty(), "{changed}");
        }
    }
}

#[test]
fn text_reader_iteration_branches_and_byte_borrows_are_supported() {
    let source = r#"
def inspect(text: str) -> Result[i64, OverflowError[i64]]:
    mut count = 0
    for scalar in text:
        if scalar.isalpha():
            count += 1
    raw = text.as_bytes()
    if raw.isascii():
        return Ok(count)
    Ok(text.byte_len())
"#;
    assert_eq!(messages(source), [text_view_parameter("text", "str")]);
    assert!(messages(&source.replace("text: str", "text: &strview")).is_empty());
}

#[test]
fn forwarding_uses_the_resolved_text_view_signature_and_argument_labels() {
    let source = r#"
def target(prefix: i64, data: &strview) -> i64:
    data.byte_len()
def forward(text: &str) -> i64:
    target(data=text, prefix=1)
"#;
    assert_eq!(messages(source), [text_view_parameter("text", "&str")]);
    assert!(messages(&source.replace("text: &str", "text: &strview")).is_empty());
}

#[test]
fn owning_and_ordinary_reference_uses_keep_their_string_contracts() {
    let source = r#"
class Stored:
    text: str

def owned(text: str) -> str:
    text

def borrowed(text: &str) -> &str:
    text

def stored(text: str) -> Stored:
    Stored(text)

def alias(text: str) -> i64:
    other = text
    len(other)

def read_and_retain(text: str) -> str:
    drop(text.byte_len())
    text

def generic[T](value: &T) -> ():
    pass

def generic_reference(text: &str) -> ():
    generic(text)

def consume(text: str) -> ():
    drop(text)

def owner_call(text: str) -> ():
    consume(text)

def ref_call(text: &str) -> i64:
    len(*text)

def unused(text: str) -> ():
    pass
"#;
    assert!(messages(source).is_empty());
}

#[test]
fn callable_values_generic_bodies_methods_and_defaults_are_exempt() {
    let source = r#"
def reader(text: &str) -> i64:
    len(text)

def caller() -> i64:
    callback = reader
    callback("hello")

def generic_reader[T](text: str, value: T) -> i64:
    text.byte_len()

class Reader:
    def read(self, text: str) -> i64:
        len(text)

def default_reader(text: str = "hi") -> i64:
    len(text)

def main() -> ():
    drop(generic_reader("hi", 1))
    drop(caller())
"#;
    assert!(messages(source).is_empty());
}

#[test]
fn parameter_shadowing_does_not_prove_that_the_original_parameter_is_read() {
    let source = r#"
def loop_shadow(text: str) -> ():
    for text in "abc":
        drop(text.byte_len())

def pattern_shadow(text: str) -> i64:
    match Some("abc"):
        case Some(text):
            text.byte_len()
        case Nothing:
            0
"#;
    assert!(messages(source).is_empty());
}

#[test]
fn already_borrowed_text_and_mutable_string_slots_are_not_reported() {
    assert!(messages("def view(text: &strview) -> i64:\n    len(text)\ndef old_view(text: &substr) -> i64:\n    len(text)\ndef slot(text: &mut str) -> i64:\n    len(text)\n").is_empty());
}

#[test]
fn text_view_parameter_cli_warning_is_advisory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reader.plenty");
    std::fs::write(
        &path,
        "def size(text: str) -> i64:\n    text.byte_len()\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--check")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(&text_view_parameter("text", "str")),
        "{stderr}"
    );
}

#[test]
fn callbacks_named_inside_uninstantiated_generic_bodies_keep_their_signature() {
    let source = r#"
def reader(text: str) -> i64:
    text.byte_len()

def unused[T](value: T) -> ():
    callback = reader
    drop(callback("hello"))
"#;
    assert!(messages(source).is_empty());
}

#[test]
fn changing_a_text_parameter_cannot_change_a_returned_loan_origin() {
    let source =
        "def read(text: str, value: &i64) -> &i64:\n    drop(text.byte_len())\n    value\n";
    assert!(messages(source).is_empty());
}

#[test]
fn captured_string_handles_remain_owning() {
    let source = r#"
def captured(text: str) -> i64:
    reader = def[&text]() -> i64:
        text.byte_len()
    reader()
"#;
    assert!(messages(source).is_empty());
}

#[test]
fn printing_and_resolved_alias_parameters_can_use_text_views() {
    let source = "type Text = str\ndef show(text: Text) -> Result[(), IoError]:\n    print(text)\n";
    assert_eq!(messages(source), [text_view_parameter("text", "str")]);
    assert!(messages(&source.replace("text: Text", "text: &strview")).is_empty());
}

#[test]
fn formatted_text_can_borrow_a_view() {
    let source = "import std.fmt\ndef render(text: str) -> Result[str, AllocError | std.fmt.FormatError]:\n    f\"{text!r:>8}\"\ndef main() -> ():\n    pass\n";
    assert_eq!(messages(source), [text_view_parameter("text", "str")]);
    assert!(messages(&source.replace("text: str", "text: &strview")).is_empty());
}
