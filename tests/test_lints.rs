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

#[test]
fn warnings_point_at_the_copy_call() {
    let warnings = check_warnings(&format!("{MAIN}    x = 1\n    y = copy(x)?\n    Ok(())\n"));
    let [warning] = warnings.as_slice() else {
        panic!("{warnings:?}");
    };
    assert!(warning.starts_with("3:9: warning: "), "{warning}");
}

#[test]
fn moving_values_references_results_and_computed_arguments_are_left_alone() {
    let source = r#"
class Record:
    values: list[i64]

def make() -> str:
    return "made"

def through(word: &str) -> Result[str, AllocError]:
    Ok(copy(word)?)

def main() -> Result[(), AllocError | IoError]:
    items = [1, 2]?
    kept = copy(items)?
    record = Record(copy(items)?)
    again = copy(record)?
    original = "hello"
    r = &original
    rc = copy(r)?
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
