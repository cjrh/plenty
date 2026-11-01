//! `print`, `str.repr` and f-strings share one formatting path (issue #615):
//! `display` at the top level, `debug` for `!r`, `str.repr` and aggregate
//! elements, and structure for a type with neither.

mod support;
use support::{check_file_error, run_file_stdout as run};

const P: &str = r#"from std.fmt import Formatter, FormatError

class P:
    n: i64

    def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"<{self.n}>")

    def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"P#{self.n}")
"#;

fn with_p(body: &str) -> String {
    format!("{P}\n{body}")
}

#[test]
fn print_repr_and_fields_use_display_and_debug() {
    let source = with_p(
        r#"
class Q:
    s: str
    p: P

class D:
    n: i64

    def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"D{self.n}")

def main() -> Result[(), AllocError | IoError | FormatError]:
    p = P(3)
    ps = [P(3)]?
    print(p)?
    print(&p)?
    print(str.repr(p)?)?
    print(ps)?
    print(f"{ps}"?)?
    print(f"{p} {p!r} [{p:>5}] {p=}"?)?
    print(p, ps, "raw")?
    print(Q("hi", P(4)))?
    print(["a"]?, (P(1), "b"))?
    print(D(2), [D(2)]?, str.repr(D(2))?)?
    mut d: dict[str, P] = {}?
    d.insert("k", P(5))?
    print(d)?
    print(Some(P(6)))?
    Ok(())
"#,
    );
    assert_eq!(
        run(&source),
        "<3>\n<3>\nP#3\n[P#3]\n[P#3]\n<3> P#3 [  <3>] p=P#3\n<3> [P#3] raw\n\
         Q(s=\"hi\", p=P#4)\n[\"a\"] (P#1, \"b\")\nD2 [D2] D2\n{\"k\": P#5}\n\
         Option[P].Some(P#6)"
    );
}

#[test]
fn generic_classes_and_enum_payloads_use_their_methods() {
    assert_eq!(
        run(r#"from std.fmt import Formatter, FormatError

class Pt:
    x: i64

    def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"({self.x})")

class Pair[T]:
    a: T
    b: T

    def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"{self.a!r}~{self.b!r}")

enum Shape:
    Dot(Pt)
    Blank

    def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        match self:
            case Shape.Dot(p):
                out.format(f"dot at {p!r}")
            case Shape.Blank:
                out.text("blank")

def main() -> Result[(), AllocError | IoError | FormatError]:
    print(Pair("a", "b"), [Pair(1, 2)]?)?
    print(Shape.Dot(Pt(1)), [Shape.Blank]?)?
    Ok(())
"#),
        "\"a\"~\"b\" [1~2]\ndot at (1) [Shape.Blank]"
    );
}

#[test]
fn method_errors_propagate_and_print_nothing() {
    assert_eq!(
        run(r#"from std.fmt import Formatter, FormatError

class Bad:
    n: i64

    def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        Err(FormatError.InvalidSpec)

def describe(result: Result[(), IoError | FormatError]) -> str:
    match result:
        case Err(error):
            match error:
                case cause: FormatError:
                    match cause:
                        case FormatError.InvalidSpec:
                            "invalid"
                        case _:
                            "other"
                case _:
                    "io"
        case _:
            "printed"

def main() -> Result[(), AllocError | IoError | FormatError]:
    print(describe(print("before", [Bad(1)]?)))?
    match str.repr(Bad(1)):
        case Err(error):
            match error:
                case cause: FormatError:
                    print("repr", cause == FormatError.InvalidSpec)?
                case _:
                    print("repr alloc")?
        case _:
            print("repr other")?
    match f"{[Bad(1)]?}":
        case Err(error):
            match error:
                case cause: FormatError:
                    print("field invalid", cause == FormatError.InvalidSpec)?
                case _:
                    print("field alloc")?
        case _:
            print("field other")?
    Ok(())
"#),
        "invalid\nrepr True\nfield invalid True"
    );
}

#[test]
fn only_values_with_methods_widen_the_result() {
    let error = check_file_error(&with_p(
        r#"
def main() -> Result[(), IoError]:
    print(P(1))?
    Ok(())
"#,
    ));
    assert!(
        error.contains("cannot propagate as IoError; missing alternatives: std.fmt.FormatError"),
        "{error}"
    );
    let error = check_file_error(&with_p(
        r#"
def main() -> Result[(), AllocError | IoError]:
    print(str.repr([P(1)]?)?)?
    Ok(())
"#,
    ));
    assert!(
        error.contains("missing alternatives: std.fmt.FormatError"),
        "{error}"
    );
    assert_eq!(
        run(r#"class Plain:
    n: i64

def main() -> Result[(), AllocError | IoError]:
    print(Plain(1), [Plain(2)]?, str.repr(Plain(3))?)?
    Ok(())
"#),
        "Plain(n=1) [Plain(n=2)] Plain(n=3)"
    );
}

#[test]
fn private_element_types_format_in_their_module() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("shapes.plenty"),
        r#"from std.fmt import Formatter, FormatError

class Hidden:
    n: i64

    def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"H{self.n}")

pub class Holder:
    items: list[Hidden]

pub def make() -> Result[Holder, AllocError]:
    Ok(Holder([Hidden(1)]?))
"#,
    )
    .unwrap();
    let main = directory.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"import shapes
from std.fmt import FormatError

def main() -> Result[(), AllocError | IoError | FormatError]:
    holder = shapes.make()?
    print(holder)?
    print(str.repr(holder)?)?
    Ok(())
"#,
    )
    .unwrap();
    let executable = directory.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = std::process::Command::new(&executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "shapes.Holder(items=[H1])\nshapes.Holder(items=[H1])\n"
    );
}

#[test]
fn methods_with_other_signatures_are_rejected_by_name() {
    let error = check_file_error(
        r#"class Wrong:
    n: i64

    def display(self) -> str:
        "w"

def main() -> Result[(), IoError]:
    print(Wrong(1))?
    Ok(())
"#,
    );
    assert!(
        error.ends_with(
            "main.plenty:8:5: cannot format `Wrong`: formatting calls its `display` method, which must be `def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]`; rename the method if it has another purpose"
        ),
        "{error}"
    );
    let error = check_file_error(
        r#"import std.log
from std.log import Level, Clock

def main() -> Result[(), AllocError | IoError | std.log.LogError]:
    log = std.log.logger(std.log.memory_sink(4)?, Level.Info, Clock.Off)
    print([log]?)?
    Ok(())
"#,
    );
    assert!(
        error.contains("cannot format `std.log.Logger[std.log.MemorySink]`: formatting calls its `debug` method"),
        "{error}"
    );
    assert!(!error.contains("__plenty_"), "{error}");
    let error = check_file_error(
        r#"import std.cli.progress

def main() -> Result[(), IoError]:
    print(std.cli.progress.bar(10u64))?
    Ok(())
"#,
    );
    assert!(
        error.contains(
            "cannot format `std.cli.progress.Progress`: formatting calls its `display` method"
        ),
        "{error}"
    );
}
