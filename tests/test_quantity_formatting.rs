//! Coherent display, numeric specs and structural debug for quantities.
mod support;
use support::{assert_stdout, run_file_stdout as run};

#[test]
fn printing_and_structural_debug_preserve_existing_error_types() {
    assert_stdout(
        r#"
dimension Length("m")
dimension Time("s")
def main() -> Result[(), IoError | AllocError]:
    q = Quantity[f64, Length / Time](1.3)
    print(q, &q)?
    print(Quantity[i64, One](3), Quantity[i64, One / Time](2))?
    print(str.repr(&q)?)?
    print([q]?)?
    Ok(())
"#,
        "1.3 m/s 1.3 m/s\n3 2 1/s\nQuantity[f64, Length * Pow[Time, -1]](1.3)\n[Quantity[f64, Length * Pow[Time, -1]](1.3)]\n",
    );
}

#[test]
fn fstrings_apply_specs_to_numbers_and_conversions_to_text() {
    assert_eq!(
        run(r#"
dimension Length("m")
dimension Time("s")
def main() -> Result[(), IoError | AllocError]:
    q = Quantity[f64, Length / Time](1.3)
    n = Quantity[i64, Length](42)
    print(f"{q}|{q:.2f}|{q:08.2f}|{n:+06}|{n:#x}|{n!s:>8}|{q!r}"?)?
    print(f"{Quantity[f32, Length](0.1)}|{Quantity[i64, One](3)}"?)?
    print(f"{q!r:.8}|{q!a:.8}"?)?
    Ok(())
"#),
        "1.3 m/s|1.30 m/s|00001.30 m/s|+00042 m|0x2a m|    42 m|Quantity[f64, Length * Pow[Time, -1]](1.3)\n0.1 m|3\nQuantity|Quantity"
    );
}

#[test]
fn quantity_writer_methods_use_existing_formatters() {
    assert_eq!(
        run(r#"
from std.fmt import display, debug, string_formatter, finish, to_string, to_debug
dimension Length("m")
def main() -> Result[(), IoError | FormatError]:
    q = Quantity[f64, Length](1.25)
    mut out = string_formatter(200, 8)?
    display(&q, &mut out)?
    out.text("|")?
    debug(&q, &mut out)?
    out.text("|")?
    q.display(&mut out)?
    out.text("|")?
    q.debug(&mut out)?
    print(finish(&out)?)?
    print(to_string(&q, 40, 4)?, to_debug(&q, 40, 4)?)?
    Ok(())
"#),
        "1.25 m|Quantity[f64, Length](1.25)|1.25 m|Quantity[f64, Length](1.25)\n1.25 m Quantity[f64, Length](1.25)"
    );
}

#[test]
fn all_numeric_storage_and_domains_format_as_their_values() {
    for t in [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
    ] {
        let (value, spec) = if t.starts_with('f') {
            ("42.0", ".0f")
        } else {
            ("42", "d")
        };
        let source = format!(
            r#"
dimension Length("m")
def main() -> Result[(), IoError | AllocError]:
    q = Quantity[{t}, Length]({value})
    print(q)?
    print(f"{{q:{spec}}}"?)?
    Ok(())
"#
        );
        assert_eq!(run(&source), format!("{value} m\n42 m"));
    }
    for (domain, widths) in [
        ("Positive", &["i8", "i16", "i32", "i64"][..]),
        ("NonNegative", &["i8", "i16", "i32", "i64"][..]),
        (
            "NonZero",
            &["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"][..],
        ),
    ] {
        for t in widths {
            assert_eq!(
                run(&format!(
                    r#"
dimension Length("m")
def main() -> Result[(), IoError | AllocError]:
    q = Quantity[{domain}[{t}], Length](42)
    print(q, str.repr(&q)?)?
    print(f"{{q:#x}}|{{q!r}}"?)?
    Ok(())
"#
                )),
                format!(
                    "42 m Quantity[{domain}[{t}], Length](42)\n0x2a m|Quantity[{domain}[{t}], Length](42)"
                )
            );
        }
    }
}

#[test]
fn symbols_powers_unicode_and_nominal_dimensions_are_static() {
    assert_eq!(
        run(r#"
dimension A("α")
dimension B("s")
dimension C("s")
def spelling[D: Dimension](value: Quantity[i64, D]) -> Result[str, AllocError]:
    f"{value}"
def main() -> Result[(), IoError | AllocError]:
    print(spelling(Quantity[i64, A](2))?, spelling(Quantity[i64, B](3))?)?
    print(Quantity[i64, A / (B * C)](4), Quantity[i64, Pow[B, -128]](5))?
    print(Quantity[i64, B / C](6), Quantity[i64, B / B](7))?
    print(f"{Quantity[i64, A / (B * C)](4)}"?)?
    Ok(())
"#),
        "2 α 3 s\n4 α/(s*s) 5 1/s^128\n6 s/s 7\n4 α/(s*s)"
    );
}

#[test]
fn nested_specs_fail_normally_and_fields_evaluate_once() {
    assert_eq!(
        run(r#"
dimension Length("m")
def measured() -> Result[Quantity[f64, Length], IoError]:
    print("value")?
    Ok(Quantity[f64, Length](1.25))
def precision() -> Result[i64, IoError]:
    print("precision")?
    Ok(1)
def main() -> Result[(), IoError | AllocError | FormatError]:
    print(f"{measured()?:.{precision()?}f}"?)?
    q = Quantity[i64, Length](65)
    width = 4
    print(f"{q:0{width}d}|{q:c}"?)?
    bad = -1
    match f"{q:{bad}}":
        case Err(_):
            print("invalid width")?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#),
        "value\nprecision\n1.2 m\n0065 m|A m\ninvalid width"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn writer_display_debug_and_numeric_specs_need_no_allocations() {
    assert_eq!(
        run(r#"
import std.fmt
import std.bytes
import std.io
dimension Length("m")
def main() -> Result[(), IoError | AllocError | std.bytes.BytesError | FormatError]:
    mut scratch = std.bytes.with_capacity(32)?
    scratch.resize(32, 0u8)?
    data = std.bytes.with_capacity(256)?
    mut out = std.fmt.formatter(std.io.cursor(data), scratch, 256, 8)?
    q = Quantity[f64, Length](1.25)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    std.fmt.display(&q, &mut out)?
    std.fmt.debug(&q, &mut out)?
    out.format(f"{q:.2f}|{q!r}")?
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(out.completed())?
    Ok(())
"#),
        "67"
    );
}

#[test]
fn writer_limits_and_failures_preserve_partial_progress() {
    assert_eq!(
        run(r#"
import std.fmt
import std.bytes
from std.io import StreamError
dimension Length("m")
pub class Failing:
    pub def write(self: &mut Failing, source: &[u8]) -> Result[i64, StreamError]:
        Err(StreamError.OutOfRange)
    pub def flush(self: &mut Failing) -> Result[(), StreamError]:
        Ok(())
def main() -> Result[(), IoError | AllocError | std.bytes.BytesError | FormatError]:
    q = Quantity[i64, Length](12)
    mut limited = std.fmt.string_formatter(3, 8)?
    match std.fmt.display(&q, &mut limited):
        case Err(error):
            print(error, limited.completed(), std.fmt.finish(&limited)?)?
        case Ok(_):
            print("unexpected")?
    limited.text("!")?
    print(std.fmt.finish(&limited)?)?
    mut depth = std.fmt.string_formatter(80, 0)?
    match std.fmt.debug(&q, &mut depth):
        case Err(error):
            print(error, depth.completed())?
        case Ok(_):
            print("unexpected")?
    mut scratch = std.bytes.with_capacity(32)?
    scratch.resize(32, 0u8)?
    mut closed = std.fmt.formatter(Failing(), scratch, 80, 8)?
    match std.fmt.display(&q, &mut closed):
        case Err(error):
            print(error, closed.completed())?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#),
        "std.fmt.FormatError.LimitExceeded 2 12\n12!\nstd.fmt.FormatError.RecursionLimit 0\nstd.fmt.FormatError.Output(std.io.StreamError.OutOfRange) 0"
    );
}

#[test]
fn invalid_numeric_and_converted_specs_are_rejected() {
    for (field, expected) in [
        ("{q:x}", "type `x` formats integers"),
        ("{n:.2f}", "integers take no precision"),
        ("{q!s:.2f}", "type `f` requires a number"),
        ("{q!r:+}", "a sign requires an integer or float"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.plenty");
        std::fs::write(
            &path,
            format!(
                r#"
dimension Length("m")
def main() -> Result[(), AllocError]:
    q = Quantity[f64, Length](1.0)
    n = Quantity[i64, Length](1)
    unused = f"{field}"?
    Ok(())
"#
            ),
        )
        .unwrap();
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}: expected {expected}");
    }
}

#[test]
fn writer_method_evaluates_receiver_before_argument_and_handles_references() {
    assert_eq!(
        run(r#"
from std.fmt import string_formatter, finish, display
dimension Length("m")
def measured() -> Quantity[i64, Length]:
    print("value").unwrap()
    Quantity[i64, Length](12)
def index() -> i64:
    print("writer").unwrap()
    0
def main() -> Result[(), IoError | AllocError | FormatError]:
    mut outputs = [string_formatter(80, 8)?]?
    measured().display(&mut outputs[index()])?
    q = Quantity[i64, Length](3)
    r = &q
    display(&r, &mut outputs[0])?
    print(finish(&outputs[0])?)?
    Ok(())
"#),
        "value\nwriter\n12 m3 m"
    );
}
