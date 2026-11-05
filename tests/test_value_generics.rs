//! Value parameters specialize functions and nominal data without runtime fields.
mod support;
use support::assert_stdout as runs;
fn check_error(source: &str, message: &str) {
    let error = support::check_error(source);
    assert!(error.contains(message), "{source}\n{error}");
}

#[test]
fn values_in_bodies_forwarding_and_identity() {
    runs(
        r#"
const width: i64 = 7
class Tag[const N: i64]:
    def get(self) -> i64:
        N

def read[const N: i64](tag: &Tag[N]) -> i64:
    N

def outer[const N: i64]() -> i64:
    read(&Tag[N]())

def flag[const B: bool]() -> bool:
    B

def main() -> Result[(), IoError]:
    a: Tag[7] = Tag[width]()
    print(a.get(), read(&a), outer[7](), flag[True]())?
    Ok(())
"#,
        "7 7 7 True\n",
    );
}

#[test]
fn bounded_literals_and_runtime_checks() {
    runs(
        r#"
type Percent = Bounded[u8, 0, 100]
def take(n: Percent = 50) -> Percent:
    n
def bounds[T: IntType, const lo: T, const hi: T](n: &Bounded[T, lo, hi]) -> (T, T):
    (lo, hi)
def main() -> Result[(), IoError]:
    a: Percent = 42
    low: Bounded[i8, -128, 127] = -128
    high: Bounded[u64, 0, 18446744073709551615] = 18446744073709551615
    print(a.get(), take().get(), take(100).get(), low.get(), high.get())?
    print(Percent.try_from(0).unwrap().get(), Percent.try_from(100).unwrap().get())?
    print(Percent.try_from(101).is_err(), Bounded[i8, -4, 4].try_from(-5).is_err())?
    print(bounds(&a), bounds(&low))?
    Ok(())
"#,
        "42 50 100 -128 18446744073709551615\n0 100\nTrue True\n(0, 100) (-128, 127)\n",
    );
}

#[test]
fn rejected_bounds_and_literals() {
    check_error("x: Bounded[i64, 2, 8] = 1", "outside");
    check_error("x: Bounded[i64, 2, 8] = 9", "outside");
    check_error("x: Bounded[i64, 8, 2] = 5", "lower bound");
    check_error("x: Bounded[u8, -1, 100] = 5", "u8");
    check_error("x: Bounded[u8, 0, 256] = 5", "u8");
    check_error("x: Bounded[f64, 0, 1] = 0", "integer");
    check_error(
        "x: Bounded[i64, 0, 8] = 1\ny: Bounded[i64, 0, 9] = x",
        "expected",
    );
    check_error("x: Bounded[i64, 0, 8] = 1\nx.value", "private");
    check_error("x = Bounded[i64, 0, 8](1)", "literal");
}

#[test]
fn enum_and_method_value_parameters() {
    runs(
        r#"
enum Flag[const On: bool]:
    Value(i64)
    def enabled(self) -> bool:
        On
class Owner[const Base: i64]:
    def choose[const Other: i64](self) -> (i64, i64):
        (Base, Other)
def main() -> Result[(), IoError | AllocError]:
    a = Flag[True].Value(1)
    b = Flag[False].Value(1)
    print(a.enabled(), b.enabled(), Owner[7]().choose[9]())?
    f = Owner[5]()
    print(f.choose[-2]())?
    values = [8, 9]?
    print(values[0])?
    Ok(())
"#,
        "True False (7, 9)\n(5, -2)\n8\n",
    );
}

#[test]
fn widths_constants_aliases_and_function_values() {
    runs(
        r#"
const upper: u64 = u64.MAX
const enabled: bool = True
const one: i8 = -128
class Tag[const N: u64]:
    def get(self) -> u64:
        N
type Last = Bounded[u64, upper, upper]
def narrow[const N: i8]() -> i8:
    N
def choose[const B: bool](a: i64) -> i64:
    if B:
        return a
    0
def main() -> Result[(), IoError]:
    f = choose[enabled]
    print(f(3), narrow[one](), Tag[0xff]().get())?
    x: Last = 18446744073709551615
    print(x.get(), Last.try_from(upper).unwrap().get())?
    Ok(())
"#,
        "3 -128 255\n18446744073709551615 18446744073709551615\n",
    );
}

#[test]
fn argument_kinds_and_conflicting_inference() {
    check_error("def f[T]() -> ():\n    pass\nf[4]()", "type argument");
    check_error(
        "def f[const N: i64]() -> ():\n    pass\nf[i64]()",
        "constant",
    );
    check_error("def f[const N: i64]() -> ():\n    pass\nf[True]()", "i64");
    check_error("def f[const N: bool]() -> ():\n    pass\nf[1]()", "bool");
    check_error(
        "const n: u8 = 2\ndef f[const N: i64]() -> ():\n    pass\ndef main() -> ():\n    f[n]()",
        "got u8",
    );
    check_error("def f[const N: f64]() -> ():\n    pass", "const parameter");
    check_error(
        "def f[const N: T, T: IntType]() -> ():\n    pass",
        "preceding",
    );
    check_error("def f[const N: i64]() -> i64:\n    N\nf()", "cannot infer");
    check_error(
        "class Tag[const N: i64]:\n    pass\ndef pair[const N: i64](a: Tag[N], b: Tag[N]) -> ():\n    pass\npair(Tag[1](), Tag[2]())",
        "conflicting values",
    );
    check_error(
        "def f[const N: i64]() -> ():\n    pass\nf[1 + 1]()",
        "expressions in brackets",
    );
}

#[test]
fn bounded_expected_contexts_copy_and_failure() {
    runs(
        r#"
type Small = Bounded[i64, -2, 2]
class Config:
    count: Small
def result() -> Small:
    2
def main() -> Result[(), AllocError | IoError]:
    mut a: Small = -2
    b = a
    a = 1
    mut config = Config(0)
    config.count = 2
    xs: list[Small] = [-2, 0, 2]?
    print(a.get(), b.get(), result().get(), config.count.get(), xs[1].get(), a == b)?
    print(Small.try_from(-3).is_err(), Small.try_from(3).is_err())?
    Ok(())
"#,
        "1 -2 2 2 0 False\nTrue True\n",
    );
    check_error("def f() -> Bounded[i64, 0, 2]:\n    3", "outside");
    check_error("class C:\n    n: Bounded[i64, 0, 2]\nx = C(3)", "outside");
    check_error("mut x: Bounded[i64, 0, 2] = 1\nx = 3", "outside");
    check_error("n = 1\nx: Bounded[i64, 0, 2] = n", "expected");
    check_error("x: Bounded[i64, 0, 2] = 1\nx + x", "operator");
}

#[test]
fn imported_constants_and_public_generic_signatures() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("limits.plenty"),
        r#"
pub const lower: i64 = 2
pub const upper: i64 = 4 * 4
const hidden: i64 = 8
pub type Count = Bounded[i64, lower, upper]
pub def read[const lo: i64, const hi: i64](x: Bounded[i64, lo, hi]) -> i64:
    x.get()
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
import limits as l
from limits import upper as max_count
from limits import Count

def main() -> Result[(), IoError]:
    x: Bounded[i64, l.lower, max_count] = 3
    y: Count = x
    print(l.read(y))?
    Ok(())
"#,
    )
    .unwrap();
    let out = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &out, None).unwrap();
    let result = std::process::Command::new(&out).output().unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"3\n");
    std::fs::write(
        &main,
        "import limits as l\ntype Bad = Bounded[i64, 0, l.hidden]\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
    let error = plenty::compile_file_to_executable(&main, &out, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("private"), "{error}");
}

#[test]
fn value_arguments_do_not_change_copy_or_cleanup() {
    runs(
        r#"
class Owner[const N: i64]:
    item: list[i64]
@copy
class Copy[const N: i64]:
    value: i64

def main() -> Result[(), AllocError | IoError]:
    a = Owner[3]([1, 2]?)
    moved = a
    b = Copy[3](7)
    copied = b
    print(len(moved.item), copied.value, b.value)?
    Ok(())
"#,
        "2 7 7\n",
    );
    check_error(
        "class Owner[const N: i64]:\n    item: list[i64]\na = Owner[3]([1].unwrap())\nb = a\na.item",
        "moved",
    );
    check_error(
        "def f[const N: i64]() -> i64:\n    N = 1\n    N\nf[2]()",
        "binding",
    );
    check_error(
        "def f[const N: i64](x: N) -> ():\n    pass\nf[2](1)",
        "type argument",
    );
}

#[test]
fn dependent_method_parameters_and_borrowed_construction() {
    runs(
        r#"
class Choice[T: IntType]:
    def value[const N: T](self) -> T:
        N

def main() -> Result[(), IoError]:
    c = Choice[u8]()
    n = 3u8
    print(c.value[255](), Bounded[u8, 1, 4].try_from(&n).unwrap().get())?
    Ok(())
"#,
        "255 3\n",
    );
}

#[test]
fn copy_template_checks_types_nested_under_value_arguments() {
    check_error(
        "@copy\nclass Bad[const N: i64]:\n    values: list[Bounded[i64, 0, N]]",
        "cannot have field",
    );
}

#[test]
fn constant_bounds_can_read_quantities_and_radix_literals() {
    runs(
        r#"
dimension Length("m")
const q: Quantity[i64, Length] = Quantity[i64, Length](10)
const upper: i64 = q.get()
type Small = Bounded[i64, 0, upper]
def main() -> Result[(), IoError]:
    a: Small = 0xA
    b: Bounded[u8, 0xFE, 0xFF] = 0xFE
    print(a.get(), b.get())?
    Ok(())
"#,
        "10 254\n",
    );
}

#[test]
fn every_integer_width_has_inclusive_runtime_bounds() {
    for (ty, low, high) in [
        ("i8", "-128", "127"),
        ("i16", "-32768", "32767"),
        ("i32", "-2147483648", "2147483647"),
        ("i64", "-9223372036854775808", "9223372036854775807"),
        ("u8", "0", "255"),
        ("u16", "0", "65535"),
        ("u32", "0", "4294967295"),
        ("u64", "0", "18446744073709551615"),
    ] {
        runs(
            &format!(
                r#"
type Full = Bounded[{ty}, {low}, {high}]
def sample() -> {ty}:
    print("once").unwrap()
    {high}
def main() -> Result[(), IoError]:
    low: Full = {low}
    high: Full = {high}
    print(low.get(), high.get())?
    print(Full.try_from(sample()).unwrap().get())?
    print(Bounded[{ty}, 1, 1].try_from(0).is_err(), Bounded[{ty}, 1, 1].try_from(2).is_err())?
    Ok(())
"#
            ),
            &format!("{low} {high}\nonce\n{high}\nTrue True\n"),
        );
    }
}

#[test]
fn constant_type_dependencies_are_bounded_and_invalid_storage_is_rejected() {
    check_error(
        "dimension L(\"l\")\nconst q: Quantity[Bounded[i64, 0, 1], L] = 0\ndef main() -> ():\n    n: Bounded[i64, 0, 1] = 0\n",
        "Quantity storage",
    );
    check_error(
        "dimension L(\"l\")\nconst q: Quantity[Bounded[i64, 0, upper], L] = 0\nconst upper: i64 = q.get()\ndef main() -> ():\n    pass\n",
        "constant expression",
    );
}

#[test]
fn module_constants_cannot_take_arguments_in_generic_positions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("limits.plenty"),
        "pub const limit: i64 = 10\n",
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    for constant in ["limit", "cap", "l.limit"] {
        for argument in ["1", "i64", "limit[1]"] {
            for usage in [
                format!("x: Bounded[i64, 0, {constant}[{argument}]] = 9"),
                format!("drop(value[{constant}[{argument}]]())"),
                format!("f = value[{constant}[{argument}]]"),
            ] {
                let source = format!(
                    "import limits as l\nfrom limits import limit as cap\nconst limit: i64 = 10\ndef value[const N: i64]() -> i64:\n    N\ndef main() -> ():\n    {usage}\n"
                );
                std::fs::write(&main, &source).unwrap();
                let error = plenty::check_file(&main, None).unwrap_err().to_string();
                assert!(error.contains("cannot take arguments"), "{source}\n{error}");
            }
        }
    }
}

/// Run with `cargo test --test integration value_generic_checking_scale -- --ignored --nocapture`.
/// Report compiler checking cost without turning shared-machine timing into a
/// pass/fail contract. Literal bounds must not traverse unrelated constants.
#[test]
#[ignore = "compiler scaling probe; reports timings for 250 through 4000 constants and bounded functions"]
fn value_generic_checking_scale() {
    use std::fmt::Write;
    for count in [250, 500, 1000, 2000, 4000] {
        let mut source = String::new();
        for i in 0..count {
            writeln!(source, "const c{i}: i64 = {i}").unwrap();
        }
        for i in 0..count {
            writeln!(source, "def sample{i}() -> Bounded[i64, 1, 2]:\n    1").unwrap();
        }
        source.push_str("def main() -> ():\n    pass\n");
        let start = std::time::Instant::now();
        plenty::check_source(&source).unwrap();
        eprintln!(
            "{count} constants and bounded functions: {:?}",
            start.elapsed()
        );
    }
}
