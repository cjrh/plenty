//! `Bounded[T, lo, hi].from(value)` converts a value whose range the checker
//! proves, with no run-time test, and arithmetic on a nonnegative `Bounded`'s
//! integer uses the type's bounds (issue #937).
use std::path::Path;
use std::process::Command;

mod support;
use support::assert_stdout as runs;

fn check_error(source: &str, message: &str) {
    let error = support::check_error(source);
    assert!(error.contains(message), "{source}\n{error}");
}

/// The `--release` CLIF of function `name` in `source`.
fn clif(source: &str, name: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = Command::new(support::plenty_bin())
        .current_dir::<&Path>(dir.path())
        .env("PLENTY_CACHE", "off")
        .args([
            "--release",
            "--dump",
            "clif-opt",
            "main.plenty",
            "-o",
            "out",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = std::fs::read_to_string(dir.path().join("out/clif-opt.txt")).unwrap();
    let start = dump
        .find(&format!("==== {name} "))
        .unwrap_or_else(|| panic!("no section {name} in:\n{dump}"));
    let rest = &dump[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

fn checks(body: &str) -> usize {
    body.matches("smul_overflow").count() + body.matches("sadd_overflow").count()
}

const SOURCE: &str = "\
const IM: i64 = 139968
const IA: i64 = 3877
const IC: i64 = 29573

type Seed = Bounded[i64, 0, 139967]
type Checked = Result[i64, OverflowError[i64] | ZeroDivisionError]


class Rng:
    seed: Seed

    def next(self: &mut Rng) -> Checked:
        self.seed = Seed.from((self.seed.get() * IA + IC) % IM)
        Ok(self.seed.get())


def fill(rng: &mut Rng, out: &mut [i64]) -> Result[(), OverflowError[i64] | ZeroDivisionError]:
    for i in range(len(out)):
        out[i] = rng.next()?
    Ok(())


def step(seed: Seed) -> Checked:
    Ok(seed.get() * IA + IC)


# The bounds hold for the Bounded, not for the integer it was tested from.
def tested(x: i64) -> Checked:
    match Seed.try_from(x):
        case Ok(seed):
            Ok(seed.get() * IA + IC)
        case Err(_):
            Ok(x * IA + IC)


def wide(seed: Bounded[i64, 0, 9223372036854775807]) -> Checked:
    Ok(seed.get() * IA + IC)


def signed(seed: Bounded[i64, -1, 139967]) -> Checked:
    Ok(seed.get() * IA + IC)


def show(result: Checked) -> Result[(), IoError]:
    match result:
        case Ok(value):
            print(value)?
        case Err(_):
            print(\"error\")?
    Ok(())


def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    mut rng = Rng(42)
    mut out = [0, 0, 0, 0]?
    fill(&mut rng, &mut out)?
    print(out, rng.seed.get())?
    show(step(7))?
    show(tested(7))?
    show(tested(-1))?
    show(tested(9223372036854775807))?
    show(wide(9223372036854775807))?
    show(signed(-1))?
    Ok(())
";

#[test]
fn bounds_remove_the_overflow_checks() {
    assert_eq!(checks(&clif(SOURCE, "fill")), 0);
    assert_eq!(checks(&clif(SOURCE, "step")), 0);
}

#[test]
fn only_the_bounded_value_carries_the_bounds() {
    // The `Err` arm multiplies and adds the untested integer.
    assert_eq!(checks(&clif(SOURCE, "tested")), 2);
    // No bound that rules out overflow.
    assert_eq!(checks(&clif(SOURCE, "wide")), 2);
    // The checker's interval -1..139967 proves both operations (issue #952).
    assert_eq!(checks(&clif(SOURCE, "signed")), 0);
}

#[test]
fn results_and_overflow_reports_are_unchanged() {
    runs(
        SOURCE,
        "[52439, 102040, 89085, 111062] 111062\n56712\n56712\n25696\nerror\nerror\n25696\n",
    );
}

#[test]
fn proved_ranges_convert_without_a_test() {
    runs(
        r#"
const MASK: i64 = 0xff
type Byte = Bounded[i64, 0, 255]
type Digit = Bounded[i64, 0, 9]
def main() -> Result[(), IoError]:
    x = -7
    a = Digit.from(x % 10)
    b = Byte.from(x & MASK)
    c = Byte.from(0x0f & x)
    d = Byte.from((x % 0x100))
    e = Byte.from(a)
    f = Bounded[u8, 0, 15].from(200u8 % 16)
    g = Bounded[u8, 0, 15].from(0xabu8 & 0x0f)
    print(a.get(), b.get(), c.get(), d.get(), e.get(), f.get(), g.get())?
    Ok(())
"#,
        "3 249 9 249 3 8 11\n",
    );
}

#[test]
fn unproved_values_are_rejected() {
    let full = "this value is in -9223372036854775808..9223372036854775807, which is not inside `Bounded[i64, 0, 9]`'s inclusive bounds 0..9";
    let program = |body: &str| {
        format!(
            "type Digit = Bounded[i64, 0, 9]\ndef f(x: i64, n: i64, wide: Bounded[i64, 0, 10], small: Bounded[i32, 0, 5]) -> Result[Digit, OverflowError[i64] | ZeroDivisionError]:\n    Ok({body})\ndef main() -> ():\n    f(1, 2, 3, 4).is_ok()\n"
        )
    };
    check_error(&program("Digit.from(x)"), full);
    check_error(&program("Digit.from(x)"), "Bounded.try_from(value)?");
    check_error(&program("Digit.from(x + 1)"), full);
    check_error(&program("Digit.from(x % n)"), full);
    check_error(&program("Digit.from(x & -1)"), full);
    check_error(&program("Digit.from(x % 10 + 1)"), "this value is in 1..10");
    check_error(&program("Digit.from(x % -10)"), "this value is in -9..0");
    check_error(
        &program("Digit.from(x % 11)"),
        "this value is in 0..10, which is not inside `Bounded[i64, 0, 9]`'s inclusive bounds 0..9",
    );
    check_error(&program("Digit.from(x & 15)"), "this value is in 0..15");
    check_error(&program("Digit.from(wide)"), "this value is in 0..10");
    check_error(&program("Digit.from(small)"), "expected");
    check_error(&program("Digit.from(x % 10, 1)"), "takes one value");
    check_error(
        &program("Bounded[i64, 1, 9].from(x % 10)"),
        "this value is in 0..9",
    );
}

#[test]
fn an_operator_hook_proves_nothing() {
    check_error(
        r#"
type Digit = Bounded[i64, 0, 9]
@copy
class Wild:
    n: i64
    def __mod__(self, other: i64) -> i64:
        self.n
    def __and__(self, other: i64) -> i64:
        self.n
def main() -> ():
    w = Wild(50)
    a = Digit.from(w % 10)
"#,
        "this value is in -9223372036854775808..9223372036854775807",
    );
}

#[test]
fn the_constructor_stays_private() {
    check_error(
        "x = Bounded[i64, 0, 8](1)",
        "from `Bounded.from(value)` when the compiler proves the value's range",
    );
}
