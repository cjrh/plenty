//! `f"..."` literals (issue #157): Python-parity spellings, conversions,
//! static spec checks, and failure as a value.
use std::path::Path;

fn compile(dir: &Path, source: &str) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    executable
}

fn run(source: &str) -> String {
    let directory = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(compile(directory.path(), source))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The diagnostic for `line` placed in a `main` with some typed locals.
fn error(line: &str) -> String {
    let source = format!(
        "def main() -> Result[(), Failure]:\n    x = 5\n    s = \"t\"\n    y = 1.5\n    {line}\n    Ok(())\n"
    );
    match plenty::check_source(&source) {
        Ok(()) => panic!("accepted: {line}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn python_tutorial_and_spec_examples_match_python() {
    // Expected text is CPython 3.12 output for the same templates, except
    // that a float field without a type uses `print`'s spelling (`NaN`).
    assert_eq!(
        run(r#"
def main() -> Result[(), Failure]:
    year = 2016
    event = "Referendum"
    name = "Sjoerd"
    phone = 4127
    pi = 3.141592653589793
    print(f"Results of the {year} {event}"?)?
    print(f"{name:10} ==> {phone:10d}"?)?
    print(f"{pi:.3f}"?)?
    print(f"{1234567.891:,.2f}|{1234567:_}|{3735928559:_x}|{255:#o}|{5:#b}|{-42:+06}|{42: }|{3.5:+.1e}"?)?
    print(f"{0.000012345:g}|{123456789.0:g}|{1.5:G}|{1e-7:.2}|{100.0:.3}|{0.5:%}|{2.0:#.0f}|{f64.INF:F}|{f64.NAN:>5}"?)?
    print(f'{"left":<8}|{"right":>8}|{"mid":^9}|{"trunc":.3}|{"x":_^5}|{-7:=+6}|{7:0>4}|{-1234:010,}'?)?
    Ok(())
"#),
        "Results of the 2016 Referendum
Sjoerd     ==>       4127
3.142
1,234,567.89|1_234_567|dead_beef|0o377|0b101|-00042| 42|+3.5e+00
1.2345e-05|1.23457e+08|1.5|1e-07|1e+02|50.000000%|2.|INF|  NaN
left    |   right|   mid   |tru|__x__|-    7|0007|-0,001,234"
    );
}

#[test]
fn fields_conversions_and_self_documenting_expressions() {
    assert_eq!(
        run(r#"
class Point:
    x: i64
    y: i64

def label() -> str:
    return "made by a call"

def main() -> Result[(), Failure]:
    p = Point(3, -4)
    xs = [10, 20]?
    d = {"k": 5}?
    name = "Al"
    r = &name
    flag = True
    small = 0.1f32
    print(f"{p.x=} {xs[1]} {d['k']} {len(xs)} {label():.4} {{braces}}"?)?
    print(f'{name!r} {name!s} {r} {name=} {name=!s:>4} {flag=} {flag:>5}|'?)?
    print(f"{xs} {p} {Some(3)} {small} {1e16} {-0.0} {u64.MAX:,} {i64.MIN:_}"?)?
    print(f"""two
{p.y
}lines"""?)?
    print(f""?, f'plain'?)?
    Ok(())
"#),
        "p.x=3 20 5 2 made {braces}
\"Al\" Al Al name=\"Al\" name=  Al flag=True  True|
[10, 20] Point(x=3, y=-4) Option[i64].Some(3) 0.1 1e16 -0.0 18,446,744,073,709,551,615 -9_223_372_036_854_775_808
two
-4lines
 plain"
    );
}

#[test]
fn user_display_and_debug_methods_generic_bodies_and_closures() {
    assert_eq!(
        run(r#"
import std.fmt
from std.fmt import Formatter, FormatError

pub class Label:
    name: str

    pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.text(&self.name)

    pub def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.text("Label(")?
        out.debug_text(&self.name)?
        out.text(")")

    def tagged(self) -> Result[str, FormatError]:
        f"@{self.name}"

def wrap[T](value: &T) -> Result[str, FormatError]:
    f"<{value}>"

def numbered(n: i64) -> Generator[str]:
    for i in range(n):
        match f"item {i}":
            case Ok(text):
                yield text
            case Err(_):
                return

def main() -> Result[(), Failure]:
    label = Label("ready")
    print(f"{label} {label!r} {label:>7}| {label!r:.6}"?)?
    print(wrap(&5)?, wrap(&"x")?, wrap(&label)?, label.tagged()?)?
    render = def(n: i64) -> Result[str, FormatError]:
        f"n={n:03}"
    print(render(7)?)?
    for text in numbered(2):
        print(text)?
    Ok(())
"#),
        "ready Label(\"ready\")   ready| Label(
<5> <x> <ready> @ready
n=007
item 0
item 1"
    );
}

#[test]
fn source_only_compilation_imports_std_fmt_implicitly() {
    // No `import`: the compiler adds `std.fmt` for the f-string alone.
    plenty::check_source(
        "def main() -> Result[(), Failure]:\n    n = 3\n    print(f\"{n:>4}\"?)?\n    Ok(())\n",
    )
    .unwrap();
    // A user import is still rejected without a source root.
    let error = plenty::check_source(
        "import std.fmt\ndef main() -> Result[(), Failure]:\n    print(f\"{1}\"?)?\n    Ok(())\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("imports require a file-based compilation API"),
        "{error}"
    );
}

#[test]
fn result_type_and_propagation() {
    let error = plenty::check_source(
        "def main() -> Result[(), Failure]:\n    print(f\"{1}\")?\n    Ok(())\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("print cannot print a Result directly"),
        "{error}"
    );
    assert_eq!(
        run(r#"
from std.fmt import FormatError

def title(n: i64) -> Result[str, FormatError]:
    text = f"n{n}"?
    Ok(text)

def main() -> Result[(), Failure]:
    print(title(4)?)?
    Ok(())
"#),
        "n4"
    );
}

#[test]
fn specs_are_checked_against_field_types_at_compile_time() {
    let cases = [
        (
            "print(f\"{x:.2f}\"?)?",
            "5:16: format spec `.2f` does not apply to i64: integers take no precision",
        ),
        (
            "print(f\"{s:+}\"?)?",
            "5:16: format spec `+` does not apply to str: a sign requires an integer or float",
        ),
        (
            "print(f\"{y:d}\"?)?",
            "5:16: format spec `d` does not apply to f64: type `d` formats integers",
        ),
        (
            "print(f\"{x:,x}\"?)?",
            "`,` grouping applies to decimal output; use `_` with `x`",
        ),
        (
            "print(f\"{s:=5}\"?)?",
            "`=` alignment requires an integer or float",
        ),
        (
            "print(f\"{s:05}\"?)?",
            "`0` padding requires an integer or float",
        ),
        (
            "print(f\"{x!r:+}\"?)?",
            "format spec `+` does not apply to i64: a sign requires an integer or float",
        ),
        ("print(f\"{x:s}\"?)?", "type `s` formats text; use `!s`"),
    ];
    for (line, expected) in cases {
        let message = error(line);
        assert!(message.contains(expected), "{line}: {message}");
        assert!(!message.contains("__plenty"), "{line}: {message}");
    }
}

#[test]
fn template_syntax_errors_are_located() {
    let cases = [
        (
            "print(f\"{x + 1}\"?)?",
            "5:14: an f-string field is a name or literal with field access, indexing, and calls",
        ),
        (
            "print(f\"{x?}\"?)?",
            "an f-string field is a name or literal",
        ),
        (
            "print(f\"{x:q}\"?)?",
            "5:16: invalid format spec `q`: unexpected `q`",
        ),
        (
            "print(f\"{x:{x}}\"?)?",
            "nested replacement fields in a format spec are not supported",
        ),
        ("print(f\"{}\"?)?", "5:14: empty f-string field"),
        (
            "print(f\"a } b\"?)?",
            "5:15: single `}` in an f-string; write `}}`",
        ),
        (
            "print(f\"{x!a}\"?)?",
            "the `!a` conversion is not supported",
        ),
        ("print(f\"{x\"?)?", "unterminated f-string field"),
        (
            "print(f\"{\"q\"}\"?)?",
            "a field cannot reuse the enclosing quote",
        ),
        ("print(f\"{x:.}\"?)?", "expected digits after `.`"),
        (
            "print(f\"{x:99999}\"?)?",
            "widths and precisions are limited to 4096",
        ),
        ("print(f\"{x:z}\"?)?", "the `z` option is not supported"),
        ("print(f\"{x:n}\"?)?", "the type `n` is not supported"),
        (
            "print(f\"{undefined}\"?)?",
            "5:14: unknown binding `undefined`",
        ),
    ];
    for (line, expected) in cases {
        let message = error(line);
        assert!(message.contains(expected), "{line}: {message}");
    }
    let pattern = plenty::check_source(
        "def main() -> ():\n    s = \"x\"\n    match s:\n        case f\"x\":\n            pass\n        case _:\n            pass\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        pattern.contains("a pattern cannot be an f-string"),
        "{pattern}"
    );
    // `f` stays an ordinary name when no quote follows it.
    plenty::check_source("def main() -> ():\n    f = 1\n    g = f\n    drop(g)\n").unwrap();
}

#[test]
fn spec_methods_reject_invalid_runtime_specs_and_grow_scratch() {
    assert_eq!(
        run(r#"
import std.fmt
from std.fmt import spec

def main() -> Result[(), Failure]:
    mut out = std.fmt.string_formatter(10000, 8)?
    match out.text_spec("x", &spec(sign="+")):
        case Err(std.fmt.FormatError.InvalidSpec):
            print("invalid")?
        case _:
            print("unexpected")?
    out.float_spec(1e300, &spec(kind="f", precision=2))?
    out.unsigned_spec(u64.MAX, &spec(kind="b"))?
    text = std.fmt.finish(&out)?
    print(text.byte_len(), out.completed())?
    Ok(())
"#),
        "invalid\n368 368"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_a_format_error_value() {
    assert_eq!(
        run(r#"
def main() -> Result[(), Failure]:
    n = 12345
    print("__test_fail_allocations_after_0__")?
    made = f"value {n:>20}"
    print("__test_restore_allocations__")?
    match made:
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(str.repr(error)?)?
    Ok(())
"#),
        "std.fmt.FormatError.Bytes(std.bytes.BytesError.Allocation(AllocError.OutOfMemory))"
    );
}
