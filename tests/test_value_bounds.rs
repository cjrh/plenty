//! Floor division and modulo by a positive divisor avoid the general sign
//! fixup: a power of two is an arithmetic shift or a mask, and any other
//! positive divisor adjusts by the remainder's sign word. Signed sums and
//! products of values with known upper bounds (a slice count, a counter below
//! it) lower without an overflow check when the bounds prove the result fits;
//! a check the bounds do not exclude stays.
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

mod support;

fn plenty(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run(source: &str, flags: &[&str]) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let mut args = flags.to_vec();
    args.push("main.plenty");
    plenty(dir.path(), &args)
}

/// The `--dump` file `kind` of `source`, compiled with `--release`.
fn dump(source: &str, kind: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    plenty(
        dir.path(),
        &["--release", "--dump", kind, "main.plenty", "-o", "out"],
    );
    std::fs::read_to_string(dir.path().join(format!("out/{kind}.txt"))).unwrap()
}

fn floor_div(a: i128, b: i128) -> i128 {
    let (q, r) = (a / b, a % b);
    if r != 0 && (r < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

fn floor_mod(a: i128, b: i128) -> i128 {
    a - floor_div(a, b) * b
}

/// Every width, with constant power-of-two, other constant and variable
/// divisors, in trapping (`unchecked:`) and Result-returning functions.
#[test]
fn floor_division_and_modulo_match_the_definition() {
    let mut source = String::new();
    let mut main = String::new();
    let mut expected = String::new();
    for (ty, min, max) in [
        ("i8", i128::from(i8::MIN), i128::from(i8::MAX)),
        ("i16", i128::from(i16::MIN), i128::from(i16::MAX)),
        ("i32", i128::from(i32::MIN), i128::from(i32::MAX)),
        ("i64", i128::from(i64::MIN), i128::from(i64::MAX)),
    ] {
        let errors = format!("OverflowError[{ty}] | ZeroDivisionError");
        for divisor in [1, 2, 8, 64, 7, 100] {
            writeln!(
                source,
                "def tq{divisor}_{ty}(x: {ty}) -> {ty}:\n    unchecked:\n        x // {divisor}{ty}\n\n\
                 def tr{divisor}_{ty}(x: {ty}) -> {ty}:\n    unchecked:\n        x % {divisor}{ty}\n\n\
                 def cq{divisor}_{ty}(x: {ty}) -> Result[{ty}, {errors}]:\n    Ok(x // {divisor}{ty})\n\n\
                 def cr{divisor}_{ty}(x: {ty}) -> Result[{ty}, {errors}]:\n    Ok(x % {divisor}{ty})\n"
            )
            .unwrap();
        }
        writeln!(
            source,
            "def vq_{ty}(x: {ty}, d: {ty}) -> Result[{ty}, {errors}]:\n    Ok(x // d)\n\n\
             def vr_{ty}(x: {ty}, d: {ty}) -> Result[{ty}, {errors}]:\n    Ok(x % d)\n"
        )
        .unwrap();
        for x in [
            min,
            min + 1,
            -65,
            -64,
            -9,
            -8,
            -7,
            -1,
            0,
            1,
            7,
            8,
            9,
            63,
            max,
        ] {
            let literal = |v: i128| {
                if v == min {
                    format!("{ty}.MIN")
                } else if v < 0 {
                    format!("(-{}{ty})", -v)
                } else {
                    format!("{v}{ty}")
                }
            };
            let x_text = literal(x);
            for divisor in [1, 2, 8, 64, 7, 100] {
                let (q, r) = (floor_div(x, divisor), floor_mod(x, divisor));
                writeln!(
                    main,
                    "    print(tq{divisor}_{ty}({x_text}), tr{divisor}_{ty}({x_text}), \
                     cq{divisor}_{ty}({x_text})?, cr{divisor}_{ty}({x_text})?)?"
                )
                .unwrap();
                writeln!(expected, "{q} {r} {q} {r}").unwrap();
            }
            for divisor in [3, -3, 16, -16, max] {
                let d_text = literal(divisor);
                writeln!(
                    main,
                    "    print(vq_{ty}({x_text}, {d_text})?, vr_{ty}({x_text}, {d_text})?)?"
                )
                .unwrap();
                writeln!(
                    expected,
                    "{} {}",
                    floor_div(x, divisor),
                    floor_mod(x, divisor)
                )
                .unwrap();
            }
        }
    }
    let errors = ["i8", "i16", "i32", "i64"].map(|ty| format!("OverflowError[{ty}]"));
    write!(
        source,
        "def main() -> Result[(), AllocError | IoError | ZeroDivisionError | {}]:\n{main}    Ok(())\n",
        errors.join(" | ")
    )
    .unwrap();
    for flags in [&["--fast-compile"][..], &["--release"]] {
        assert_eq!(run(&source, flags), expected, "{flags:?}");
    }
}

const SHAPES: &str = r#"
def half(x: i64) -> i64:
    unchecked:
        x // 2

def low_bits(x: i64) -> i64:
    unchecked:
        x % 16

def sevenths(x: i64) -> i64:
    unchecked:
        x // 7

def main() -> Result[(), AllocError | IoError]:
    print(half(-3), low_bits(-3), sevenths(-8))?
    Ok(())
"#;

#[test]
fn positive_constant_divisors_need_no_general_fixup() {
    assert_eq!(run(SHAPES, &["--release"]), "-2 13 -2\n");
    let clif = dump(SHAPES, "clif-opt");
    let half = support::asm_body(&clif, "half");
    assert!(half.contains("sshr"), "{half}");
    assert!(!half.contains("sdiv") && !half.contains("select"), "{half}");
    let low_bits = support::asm_body(&clif, "low_bits");
    assert!(low_bits.contains("band"), "{low_bits}");
    assert!(!low_bits.contains("srem"), "{low_bits}");
    assert!(!low_bits.contains("select"), "{low_bits}");
    let sevenths = support::asm_body(&clif, "sevenths");
    assert!(!sevenths.contains("select"), "{sevenths}");
}

const BOUNDED: &str = r#"
def pair_sum(u: &[f64]) -> f64:
    n = len(u)
    mut total = 0.0
    for i in range(n):
        for j in range(n):
            unchecked:
                total += f64((i + j) // 2 + i + 1) * u[j]
    total

def pair_product(u: &[f64]) -> i64:
    n = len(u)
    mut total = 0
    for i in range(n):
        for j in range(n):
            unchecked:
                total = (i * j).wrapping_add(total)
    total

def byte_pair_sum(u: &[u8]) -> i64:
    n = len(u)
    mut total = 0
    for i in range(n):
        for j in range(n):
            unchecked:
                total = (i + j).wrapping_add(total)
    total

def distance(u: &[f64]) -> i64:
    n = len(u)
    mut total = 0
    for i in range(n):
        unchecked:
            total = (n - i).wrapping_add(total).wrapping_add(i + -1)
    total

def checked_sum(u: &[f64]) -> Result[i64, OverflowError[i64]]:
    n = len(u)
    mut total = 0
    for i in range(n):
        total = (i + n).wrapping_add(total)
    Ok(total)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    values = [1.0, 2.0, 3.0]?
    bytes = [1u8, 2u8, 3u8]?
    print(pair_sum(&values), pair_product(&values), byte_pair_sum(&bytes))?
    print(distance(&values), checked_sum(&values)?)?
    Ok(())
"#;

/// A count of 8-byte rows is at most `i64.MAX / 8`, so sums of counters below
/// it fit; their product may not, and counters below a byte count may sum
/// past `i64.MAX`.
#[test]
fn bounded_sums_drop_only_impossible_overflow_checks() {
    let expected = "53.0 9 18\n6 12\n";
    for flags in [&["--fast-compile"][..], &["--release"]] {
        assert_eq!(run(BOUNDED, flags), expected, "{flags:?}");
    }
    let clif = dump(BOUNDED, "clif-opt");
    let checks = |name: &str| {
        let body = support::asm_body(&clif, name);
        ["sadd_overflow", "ssub_overflow", "smul_overflow", "trap "]
            .map(|needle| body.matches(needle).count())
    };
    // Only the row check of `u[j]` can trap.
    let [adds, subs, muls, _] = checks("pair_sum");
    assert_eq!((adds, subs, muls), (0, 0, 0), "pair_sum");
    assert_eq!(checks("pair_product")[2], 1, "pair_product");
    assert_eq!(checks("byte_pair_sum")[0], 1, "byte_pair_sum");
    let [adds, subs, muls, _] = checks("distance");
    assert_eq!((adds, subs, muls), (0, 0, 0), "distance");
    assert_eq!(checks("checked_sum")[0], 0, "checked_sum");
}

const SEARCH: &str = r#"
import std.bytes

class Rng:
    seed: i64

    def next(self: &mut Rng) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
        self.seed = (self.seed * 3877 + 29573) % 139968
        Ok(self.seed)

def fill(block: &mut std.bytes.Bytes, rng: &mut Rng, codes: &[u8], limits: &list[i64]) -> Result[(), OverflowError[i64] | ZeroDivisionError]:
    for i in range(block.len()):
        r = rng.next()?
        mut k = 0
        while limits[k] < r:
            k += 1
        block.set(i, codes[k])
    Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    mut block = std.bytes.with_capacity(8)?
    block.resize(8, 0u8)?
    mut rng = Rng(42)
    limits = [40000, 90000, 139968]?
    fill(&mut block, &mut rng, b"abc", &limits)?
    print(block)?
    Ok(())
"#;

/// A call through `&mut Rng`, a class of scalar fields, cannot resize the
/// buffer, so the loop writes its bytes directly; and the search loop's step
/// follows a successful row check of the same index, so it cannot overflow.
#[test]
fn scalar_class_calls_keep_bytes_cached_and_checked_indexes_step_freely() {
    for flags in [&["--fast-compile"][..], &["--release"]] {
        assert_eq!(run(SEARCH, flags), "b\"bcbcbabb\"\n", "{flags:?}");
    }
    let ops = dump(SEARCH, "ops");
    let body = support::asm_body(&ops, "fill");
    assert!(body.contains("BytesSetView"), "{body}");
    let clif = dump(SEARCH, "clif-opt");
    let body = support::asm_body(&clif, "fill");
    // The remaining check is the sum in `Rng.next`; `k += 1` has none.
    assert_eq!(body.matches("sadd_overflow").count(), 1, "{body}");
    let asm = dump(SEARCH, "asm");
    let body = support::asm_body(&asm, "fill");
    let calls = |symbol: &str| {
        body.lines()
            .filter(|line| support::native_call(line) && line.contains(symbol))
            .count()
    };
    assert_eq!(calls("plenty_bytes_set"), 0, "{body}");
}

const CONJUNCTION: &str = r#"
const LIMIT: i64 = 50

def escape(c: f64) -> i64:
    mut z = 0.0
    mut i = 0
    while i < LIMIT and z <= 4.0:
        z = z * z + c
        unchecked:
            i += 1
    i

def main() -> Result[(), AllocError | IoError]:
    print(escape(0.1), escape(1.0))?
    Ok(())
"#;

/// The body of `while i < LIMIT and ...` runs only after `i < LIMIT` held,
/// so the step cannot overflow; and a condition that names a constant is
/// still plain enough to test at the bottom of the loop.
#[test]
fn a_counter_compared_before_and_steps_without_a_check() {
    for flags in [&["--fast-compile"][..], &["--release"]] {
        assert_eq!(run(CONJUNCTION, flags), "50 3\n", "{flags:?}");
    }
    let clif = dump(CONJUNCTION, "clif-opt");
    let body = support::asm_body(&clif, "escape");
    assert!(!body.contains("sadd_overflow"), "{body}");
    // The loop is rotated: its entry test folds (`0 < 50`), which leaves
    // the bottom test of the stepped counter.
    assert_eq!(body.matches("icmp slt").count(), 1, "{body}");
}

const REMAINDERS: &str = r#"
class Counter:
    n: i64

    def bump(self: &mut Counter) -> i64:
        unchecked:
            self.n = self.n + 1
        self.n


def scaled(x: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    Ok(x % 1000 * 3877 + 29573)


def scaled_by(x: i64, d: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    Ok(x % d * 3877 + 29573)


def wrapped(x: i8) -> Result[i8, OverflowError[i8] | ZeroDivisionError]:
    Ok(x % 100i8 + 27i8)


def over(x: i8) -> Result[i8, OverflowError[i8] | ZeroDivisionError]:
    Ok(x % 100i8 + 29i8)


def bump_all(counter: &mut Counter, out: &mut [i64]) -> ():
    for i in range(len(out)):
        out[i] = counter.bump()


def show(r: Result[i8, OverflowError[i8] | ZeroDivisionError]) -> Result[(), IoError]:
    match r:
        case Ok(v):
            print("ok", v)?
        case Err(_):
            print("error")?
    Ok(())


def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    mut counter = Counter(40)
    mut out = [0, 0, 0]?
    bump_all(&mut counter, &mut out)
    print(out, counter.n)?
    print(scaled(-1)?, scaled(999)?, scaled(i64.MIN)?, scaled_by(-1, 1000)?, scaled_by(7, -1000)?)?
    show(wrapped(-1i8))?
    show(wrapped(i8.MIN))?
    show(over(-1i8))?
    show(over(98i8))?
    Ok(())
"#;

/// A signed remainder by a positive constant `M` lies in `0..M`, so sums and
/// products that fit lose their checks; a variable divisor or a sum that can
/// leave the type keeps them (issue #937).
#[test]
fn a_remainder_by_a_positive_constant_is_bounded() {
    let expected = "[41, 42, 43] 43\n3902696 3902696 773957 3902696 -3820288\n\
                    ok 126\nok 99\nerror\nok 127\n";
    for flags in [&["--fast-compile"][..], &["--release"]] {
        assert_eq!(run(REMAINDERS, flags), expected, "{flags:?}");
    }
    let clif = dump(REMAINDERS, "clif-opt");
    let checks = |name: &str| {
        let body = support::asm_body(&clif, name);
        ["sadd_overflow", "smul_overflow"].map(|needle| body.matches(needle).count())
    };
    assert_eq!(checks("scaled"), [0, 0]);
    assert_eq!(checks("scaled_by"), [1, 1]);
    assert_eq!(checks("wrapped"), [0, 0]);
    assert_eq!(checks("over"), [1, 0]);
}

/// A reference parameter has one address value in the function, so the field
/// stored by the inlined `bump` is forwarded to the read after it: one load
/// per pass, where two splits of `counter` gave two (issue #937).
#[test]
fn a_field_stored_through_a_reference_parameter_is_not_read_again() {
    let clif = dump(REMAINDERS, "clif-opt");
    let body = support::asm_body(&clif, "bump_all");
    assert_eq!(body.matches("load.i64").count(), 1, "{body}");
}
