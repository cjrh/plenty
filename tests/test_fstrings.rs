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
fn byte_values_use_their_print_spelling() {
    assert_eq!(
        run(r#"
def main() -> Result[(), Failure]:
    data = b"ab\x00\xff"
    print(f"{data} {data!r} {data:>14}| {b'hi'}"?)?
    Ok(())
"#),
        "b\"ab\\x00\\xff\" b\"ab\\x00\\xff\"  b\"ab\\x00\\xff\"| b\"hi\""
    );
    for (line, expected) in [
        ("x = bf\"a\"", "byte f-strings are not supported"),
        ("x = F\"a\"", "f-strings use a lowercase `f` prefix"),
    ] {
        let error = plenty::check_source(&format!("def main() -> ():\n    {line}\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{line}: {error}");
    }
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
        (
            "print(f\"{x:z}\"?)?",
            "5:16: format spec `z` does not apply to i64: the `z` option applies to floats",
        ),
        ("print(f\"{s:z}\"?)?", "the `z` option applies to floats"),
        ("print(f\"{x:,n}\"?)?", "type `n` takes no grouping"),
        ("print(f\"{y:_n}\"?)?", "type `n` takes no grouping"),
        ("print(f\"{x:+c}\"?)?", "type `c` takes no sign"),
        ("print(f\"{x:#c}\"?)?", "type `c` takes no `#`"),
        ("print(f\"{x:.1c}\"?)?", "integers take no precision"),
        ("print(f\"{y:c}\"?)?", "type `c` formats integers"),
        ("print(f\"{s:n}\"?)?", "type `n` requires a number"),
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
        ("print(f\"{x + s}\"?)?", "5:18: expected i64, got str"),
        (
            "print(f\"{x?}\"?)?",
            "5:15: `?` requires a Result or Option operand",
        ),
        (
            "print(f\"{x:q}\"?)?",
            "5:16: invalid format spec `q`: unexpected `q`",
        ),
        ("print(f\"{}\"?)?", "5:14: empty f-string field"),
        (
            "print(f\"a } b\"?)?",
            "5:15: single `}` in an f-string; write `}}`",
        ),
        (
            "print(f\"{x!q}\"?)?",
            "5:16: expected the conversion `r`, `s`, or `a` after `!`",
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

#[test]
fn fields_are_arbitrary_expressions_evaluated_in_order() {
    // Expected text is CPython 3.14 output, except `!r` quoting.
    assert_eq!(
        run(r#"
def tick(n: i64) -> Result[i64, Failure]:
    print(f"tick {n}"?)?
    Ok(n)

def main() -> Result[(), Failure]:
    a = 3
    b = 4.5
    xs = [1, 2, 3]?
    name = "Ada"
    print(f"{a + 1} {f64(a) * b:.2f} {a > 2} {a != 3} {'yes' if a > 2 else 'no'} {xs[a - 2] * 10:>5}"?)?
    print(f"{1, 2} {a,} {-a} {not True} {[x * 2 for x in xs]?} {len(name) == 3=}"?)?
    print(f"{tick(1)?}{tick(2)?:>{tick(3)?}}|{name.upper()?!r}"?)?
    Ok(())
"#),
        "4 13.50 True False yes    20
(1, 2) (3,) -3 False [2, 4, 6] len(name) == 3=True
tick 1
tick 2
tick 3
1  2|\"ADA\""
    );
}

#[test]
fn a_question_mark_in_a_field_returns_from_the_enclosing_function() {
    assert_eq!(
        run(r#"
enum Problem:
    Odd
    Format

def half(n: i64) -> Result[i64, Problem]:
    if n % 2 != 0:
        return Err(Problem.Odd)
    Ok(n // 2)

def show(n: i64) -> Result[str, Problem]:
    match f"half of {n} is {half(n)?}":
        case Ok(text):
            Ok(text)
        case Err(_):
            Err(Problem.Format)

def main() -> Result[(), Failure]:
    print(show(8).unwrap())?
    match show(3):
        case Err(Problem.Odd):
            print("odd")?
        case _:
            print("unexpected")?
    Ok(())
"#),
        "half of 8 is 4\nodd"
    );
}

#[test]
fn nested_spec_fields_supply_fill_alignment_width_and_precision() {
    // CPython 3.14 gives the same text for these templates and values.
    assert_eq!(
        run(r#"
import std.fmt

def check(made: Result[str, std.fmt.FormatError]) -> Result[(), Failure]:
    match made:
        case Ok(text):
            print(text)?
        case Err(std.fmt.FormatError.InvalidSpec):
            print("invalid")?
        case Err(_):
            print("other")?
    Ok(())

def main() -> Result[(), Failure]:
    w = 8
    p = 2u8
    f = "*"
    al = "^"
    x = 65
    print(f"{3.14159:{f}{al}{w}.{p}f}|{'ab':{w}}|{12:{al}{w}}|{12:<{w}}|{x:{w}}|{4.5:{w}.{p}}|{'abc':.{p}}"?)?
    print(f"{x:{f}>{w}}|{x:+0{w}_}|{'é':é{al}5}"?)?
    empty = ""
    print(f"[{x:{empty}{empty}{w}}]"?)?
    check(f"{x:{-1}}")?
    check(f"{x:{4097}}")?
    check(f"{x:{18446744073709551615u64}}")?
    check(f"{3.5:.{-1}}")?
    check(f"{x:{'ab'}}")?
    check(f"{x:{'<'}{'<'}5}")?
    check(f"{x:{'*'}{empty}5}")?
    check(f"{x:*{empty}5}")?
    check(f"{'t':{'='}5}")?
    Ok(())
"#),
        "**3.14**|ab      |   12   |12      |      65|     4.5|ab
******65|+000_065|ééééé
[      65]
invalid
invalid
invalid
invalid
invalid
65<<<
invalid
invalid
invalid"
    );
}

#[test]
fn nested_spec_fields_are_checked_at_compile_time() {
    let cases = [
        (
            "print(f\"{x:{y}}\"?)?",
            "5:17: a nested field in a format spec is an integer width or precision, or a `str` fill or alignment, not f64",
        ),
        (
            "print(f\"{x:{x}{x}}\"?)?",
            "5:16: invalid format spec `{}{}`: a nested integer field is the width or, after `.`, the precision",
        ),
        (
            "print(f\"{x:<5{s}}\"?)?",
            "a nested `str` field is the fill or the alignment, at the start of the spec",
        ),
        (
            "print(f\"{x:{s}05}\"?)?",
            "`0` cannot follow a nested alignment field; write the fill instead",
        ),
        (
            "print(f\"{x:{x!r}}\"?)?",
            "5:16: a nested f-string field is an expression without `=`, a conversion, or a spec",
        ),
        (
            "print(f\"{x:{x:{x}}}\"?)?",
            "5:19: a nested f-string field cannot contain another field",
        ),
        (
            "print(f\"{x:{s}{s}{x}.{x}f}\"?)?",
            "format spec `{}{}{}.{}f` does not apply to i64: integers take no precision",
        ),
        (
            "print(f\"{s:{s}{x}d}\"?)?",
            "format spec `{}{}d` does not apply to str: type `d` requires a number",
        ),
        ("print(f\"{x:{undefined}}\"?)?", "5:17: unknown binding `undefined`"),
    ];
    for (line, expected) in cases {
        let message = error(line);
        assert!(message.contains(expected), "{line}: {message}");
        assert!(!message.contains("__plenty"), "{line}: {message}");
    }
}

#[test]
fn c_n_z_and_ascii_follow_python() {
    // Expected text is CPython 3.14 output, except that `!a` spells escapes as
    // `debug_text` does (`\u{e9}`, double quotes) rather than `'\xe9'`.
    assert_eq!(
        run(r#"
def main() -> Result[(), Failure]:
    x = 65
    print(f"{x:c}|{x:5c}|{x:<5c}|{x:05c}|{x:*^5c}|{x:=5c}|{233u8:c}|{128512:c}"?)?
    print(f"{1234567:n}|{1234.5678:n}|{1e20:n}|{0.0001:n}|{1.0:n}|{1.5:.3n}|{1.0:#n}|{-0.0:n}|{12:08n}|{-5:+n}|{255:#n}"?)?
    ninf = -f64.INF
    print(f"{-0.0:z}|{-0.0001:z.2f}|{-0.0001:z.1%}|{-1e-9:z.2e}|{-0.0:z.0f}|{ninf:z}|{-0.004:z.2g}|{-0.0:+z}|{-0.0:z5.1f}|{-0.04:z.1f}|{-0.0:z010.2f}|{-0.01f32:z.1f}"?)?
    word = "héllo\t"
    ys = ["é"]?
    print(f"{word!a}|{'é😀'!a}|{ys!a}|{'é'!a:>10}|{5!a}|{word=!a}"?)?
    Ok(())
"#),
        "A|    A|A    |0000A|**A**|    A|é|😀
1234567|1234.57|1e+20|0.0001|1|1.5|1.00000|-0|00000012|-5|255
0.0|0.00|0.0%|-1.00e-09|0|-inf|-0.004|+0.0|  0.0|0.0|0000000.00|0.0
\"h\\u{e9}llo\\t\"|\"\\u{e9}\\u{1f600}\"|[\"\\u{e9}\"]|  \"\\u{e9}\"|5|word=\"h\\u{e9}llo\\t\""
    );
    assert_eq!(
        run(r#"
import std.fmt

def main() -> Result[(), Failure]:
    for code in [-1, 55296, 1114112]?:
        match f"{code:c}":
            case Err(std.fmt.FormatError.InvalidSpec):
                print("invalid")?
            case _:
                print("unexpected")?
    match f"{u64.MAX:c}":
        case Err(std.fmt.FormatError.InvalidSpec):
            print("invalid")?
        case _:
            print("unexpected")?
    Ok(())
"#),
        "invalid\ninvalid\ninvalid\ninvalid"
    );
}

#[test]
fn format_writes_an_f_string_to_an_existing_formatter() {
    assert_eq!(
        run(r#"
import std.fmt
from std.fmt import Formatter, FormatError

pub class Point:
    x: i64
    y: i64

    pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"({self.x}, {self.y})")

    pub def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"Point(x={self.x!r}, y={self.y!r})")

class Report:
    lines: i64

    def format(self, text: str) -> str:
        text

def main() -> Result[(), Failure]:
    p = Point(1, -2)
    print(f"{p} {p!r} {p:>9}|"?)?
    mut out = std.fmt.string_formatter(1000, 8)?
    width = 4
    out.format(f"x={p.x:>{width}} p={p}")?
    out.format(f"; {[1, 2]?}")?
    print(std.fmt.finish(&out)?, out.completed())?
    mut small = std.fmt.string_formatter(4, 8)?
    match small.format(f"{12345}"):
        case Err(FormatError.LimitExceeded):
            print("limit", small.completed(), std.fmt.finish(&small)?)?
        case _:
            print("unexpected")?
    report = Report(1)
    print(report.format("user method"))?
    Ok(())
"#),
        "(1, -2) Point(x=1, y=-2)   (1, -2)|
x=   1 p=(1, -2); [1, 2] 24
limit 4 1234
user method"
    );
    let source = |line: &str| {
        format!(
            "import std.fmt\ndef main() -> Result[(), Failure]:\n    mut out = std.fmt.string_formatter(10, 1)?\n    s = \"x\"\n    {line}\n    Ok(())\n"
        )
    };
    for (line, expected) in [
        (
            "out.format(s)?",
            "5:5: `Formatter.format` takes one f-string literal, as in `out.format(f\"{x}\")`",
        ),
        (
            "out.format(f\"{s}\"?)?",
            "`Formatter.format` takes one f-string literal",
        ),
        ("s.format(f\"{s}\")?", "unsupported method `format`"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.plenty");
        std::fs::write(&path, source(line)).unwrap();
        let message = plenty::compile_file_to_executable(&path, &directory.path().join("p"), None)
            .unwrap_err()
            .to_string();
        assert!(message.contains(expected), "{line}: {message}");
    }
}
