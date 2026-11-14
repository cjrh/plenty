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

#[test]
fn bounded_arguments_and_accessors_stay_in_registers() {
    let source = include_str!("../experiments/kernels/bounded-arguments/kernels.plenty");
    let helper = clif(source, "eval_a");
    assert!(helper.contains("(i64, i64) -> f64"), "{helper}");
    for unwanted in ["explicit_slot", "load.", "store", "overflow", "call "] {
        assert!(!helper.contains(unwanted), "{unwanted}: {helper}");
    }
    let loop_body = clif(source, "a_times_u");
    assert_eq!(checks(&loop_body), 0, "{loop_body}");
    // The output bounds trap still parks its arguments in one 16-byte slot.
    // No additional frame is needed merely to borrow eval_a's Bounded values.
    assert!(!loop_body.contains("explicit_slot 32"), "{loop_body}");
}

#[test]
fn bounded_native_values_preserve_widths_calls_and_containers() {
    let mut source = String::from(
        "def identity[T](value: T) -> T:\n    value\n\n\
         def wrapped[T](value: T) -> Result[Option[T], AllocError]:\n    Ok(Some(value))\n\n",
    );
    let mut body = String::new();
    let mut expected = String::new();
    for (index, (ty, low, high)) in [
        ("i8", "-128", "127"),
        ("u8", "0", "255"),
        ("i16", "-32768", "32767"),
        ("u16", "0", "65535"),
        ("i32", "-2147483648", "2147483647"),
        ("u32", "0", "4294967295"),
        ("i64", "-9223372036854775808", "9223372036854775807"),
        ("u64", "0", "18446744073709551615"),
    ]
    .into_iter()
    .enumerate()
    {
        source.push_str(&format!(
            "type B{index} = Bounded[{ty}, {low}, {high}]\n\
             class Holder{index}:\n    value: B{index}\n\n\
             def read{index}(value: &B{index}) -> {ty}:\n    value.get()\n\n"
        ));
        body.push_str(&format!(
            "    mut low{index}: B{index} = {low}
    high{index}: B{index} = {high}
    held{index} = Holder{index}(identity(high{index}))
    mut values{index} = [low{index}, held{index}.value]?
    print(read{index}(&values{index}[0]), wrapped(values{index}[1])?.unwrap().get())?
    low{index} = high{index}
    values{index}[0] = low{index}
    print(values{index}[0].get())?
"
        ));
        expected.push_str(&format!("{low} {high}\n{high}\n"));
    }
    source.push_str("def main() -> Result[(), AllocError | IoError]:\n");
    source.push_str(&body);
    source.push_str("    Ok(())\n");
    for fast in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.plenty"), &source).unwrap();
        let mut command = Command::new(support::plenty_bin());
        command.current_dir(dir.path()).env("PLENTY_CACHE", "off");
        if fast {
            command.arg("--fast-compile");
        }
        let output = command.arg("main.plenty").output().unwrap();
        assert!(
            output.status.success(),
            "{source}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn bounded_direct_reads_retain_exclusive_alias_checks() {
    check_error(
        "def main() -> ():\n    mut value: Bounded[i64, 0, 9] = 1\n    loan = &mut value\n    print(value.get(), loan.get()).unwrap()\n",
        "exclusively borrowed",
    );
}

#[test]
fn bounded_indirect_calls_and_ordinary_class_abi() {
    let source = r#"
class Plain:
    value: i64
type Small = Bounded[i8, -128, 127]
def plain(value: Plain) -> Plain:
    value
def bounded(value: Small) -> Small:
    value
def invoke(operation: Callable[[Small], Small], value: Small) -> Small:
    operation(value)
def main() -> Result[(), IoError]:
    print(invoke(bounded, -128).get(), plain(Plain(127)).value)?
    Ok(())
"#;
    runs(source, "-128 127\n");
    let ordinary = clif(source, "plain");
    assert!(ordinary.contains("(i128) -> i128"), "{ordinary}");
    let bounded = clif(source, "bounded");
    assert!(bounded.contains("(i8) -> i8"), "{bounded}");
}
