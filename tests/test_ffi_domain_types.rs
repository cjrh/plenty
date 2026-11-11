//! Integer domain types at the C boundary (issue #956). Code generation omits
//! zero, sign and range checks for `NonZero`, `Positive`, `NonNegative` and
//! `Bounded` values, so unchecked foreign code must never produce one. These
//! tests pin every position a C signature could name such a type.

mod support;
use std::process::Command;
use support::assert_success as success;

const PRELUDE: &str = r#"dimension Length("m")
type Divisor = NonZero[i64]
type Percent = Bounded[u8, 0, 100]
type Span = Quantity[Positive[i64], Length]
type Plain = Quantity[i64, Length]
@copy
class Holder:
    d: NonZero[i64]
@newtype
@copy
class Wrap:
    v: NonZero[i64]
"#;

fn import_error(declaration: &str) -> String {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("native.plentyi"),
        format!("{PRELUDE}pub opaque Pointer\n{declaration}\n"),
    )
    .unwrap();
    let main = temp.path().join("main.plenty");
    std::fs::write(&main, "import native\ndef main() -> ():\n    pass\n").unwrap();
    match plenty::check_file(&main, None) {
        Ok(()) => panic!("accepted: {declaration}"),
        Err(error) => error.to_string(),
    }
}

fn export_error(declaration: &str) -> String {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.plenty");
    std::fs::write(&path, format!("{PRELUDE}{declaration}\n")).unwrap();
    match plenty::check_module_file(&path, None) {
        Ok(_) => panic!("accepted: {declaration}"),
        Err(error) => error.to_string(),
    }
}

fn domain_message(domain: &str, element: &str) -> String {
    format!(
        "`{domain}` has no C ABI: foreign code is unchecked and could supply a value outside the domain; declare `{element}` and convert a received value with `try_from`, or pass `.get()` to foreign code"
    )
}

#[test]
fn imports_reject_domain_types_in_every_position() {
    for (declaration, domain, element) in [
        // Return values.
        (
            r#"extern def f() -> NonZero[i64] = "f""#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f() -> Positive[i64] = "f""#,
            "Positive[i64]",
            "i64",
        ),
        (
            r#"extern def f() -> NonNegative[i32] = "f""#,
            "NonNegative[i32]",
            "i32",
        ),
        (
            r#"extern def f() -> Bounded[u8, 0, 100] = "f""#,
            "Bounded[u8, 0, 100]",
            "u8",
        ),
        (r#"extern def f() -> Divisor = "f""#, "NonZero[i64]", "i64"),
        (
            r#"extern def f() -> Percent = "f""#,
            "Bounded[u8, 0, 100]",
            "u8",
        ),
        (r#"extern def f() -> Span = "f""#, "Positive[i64]", "i64"),
        (
            r#"extern def f() -> Option[NonZero[i64]] = "f""#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f(text: &str as c_string) -> Result[NonZero[i64], CStrError] = "f""#,
            "NonZero[i64]",
            "i64",
        ),
        // By-value arguments: one rule in both directions.
        (
            r#"extern def f(d: NonZero[i64]) -> () = "f""#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: Percent) -> () = "f""#,
            "Bounded[u8, 0, 100]",
            "u8",
        ),
        (
            r#"extern def f(d: Span) -> () = "f""#,
            "Positive[i64]",
            "i64",
        ),
        // Scalar borrows, including out-parameters.
        (
            r#"extern def f(d: &NonZero[i64]) -> () = "f""#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: &mut NonZero[i64]) -> () = "f""#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: &mut Positive[i16]) -> () = "f""#,
            "Positive[i16]",
            "i16",
        ),
        (
            r#"extern def f(d: &mut NonNegative[i64]) -> () = "f""#,
            "NonNegative[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: &mut Percent) -> () = "f""#,
            "Bounded[u8, 0, 100]",
            "u8",
        ),
        (
            r#"extern def f(d: &mut Divisor) -> () = "f""#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: &mut Span) -> () = "f""#,
            "Positive[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: &mut NonZero[i32] as errno) -> i32 = "f""#,
            "NonZero[i32]",
            "i32",
        ),
        // Buffers of domain-typed elements.
        (
            r#"extern def f(d: &mut list[NonZero[i64]] as strided) -> () = "f""#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: &list[Positive[i64]] as strided) -> () = "f""#,
            "Positive[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: &mut list[Span] as strided) -> () = "f""#,
            "Positive[i64]",
            "i64",
        ),
        (
            r#"extern def f(d: &[Bounded[u8, 0, 100]] as strided) -> () = "f""#,
            "Bounded[u8, 0, 100]",
            "u8",
        ),
        // Raw memory intrinsics.
        (
            r#"extern def f(p: Pointer, offset: u64) -> NonZero[i64] = intrinsic "load""#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f(p: Pointer, offset: u64, value: Percent) -> () = intrinsic "store""#,
            "Bounded[u8, 0, 100]",
            "u8",
        ),
        // Calls through a native address, the callback route into C.
        (
            r#"extern def f(function: Pointer) -> NonZero[i64] = function"#,
            "NonZero[i64]",
            "i64",
        ),
        (
            r#"extern def f(function: Pointer, d: &mut Positive[i64]) -> () = function"#,
            "Positive[i64]",
            "i64",
        ),
    ] {
        let error = import_error(declaration);
        assert!(
            error.contains(&domain_message(domain, element)),
            "{declaration}\n{error}"
        );
    }
}

#[test]
fn imports_reject_every_carrier_of_a_domain_value() {
    // Classes and quantities have no raw ABI at all, whatever they store.
    for declaration in [
        r#"extern def f() -> Wrap = "f""#,
        r#"extern def f() -> Holder = "f""#,
        r#"extern def f() -> Plain = "f""#,
        r#"extern def f(d: &mut Wrap) -> () = "f""#,
        r#"extern def f(d: &mut Holder) -> () = "f""#,
        r#"extern def f(d: &mut Plain) -> () = "f""#,
        r#"extern def f(d: &mut list[Plain] as strided) -> () = "f""#,
        r#"extern def f(d: &mut list[Holder] as strided) -> () = "f""#,
    ] {
        let error = import_error(declaration);
        assert!(
            error.contains("has no supported C ABI"),
            "{declaration}\n{error}"
        );
    }
    // Variadic declarations do not parse.
    let error = import_error(r#"extern def f(first: i64, ...) -> NonZero[i64] = "f""#);
    assert!(error.contains("expected an identifier"), "{error}");
}

#[test]
fn exports_reject_domain_types_in_every_position() {
    for (declaration, domain, element) in [
        // Parameters, which C fills.
        (
            "export def f(d: NonZero[i64]) -> i64 = \"lib_f\":\n    7 % d",
            "NonZero[i64]",
            "i64",
        ),
        (
            "export def f(d: Positive[i64]) -> i64 = \"lib_f\":\n    7 % d",
            "Positive[i64]",
            "i64",
        ),
        (
            "export def f(d: NonNegative[i64]) -> i64 = \"lib_f\":\n    d.get()",
            "NonNegative[i64]",
            "i64",
        ),
        (
            "export def f(d: Percent) -> u8 = \"lib_f\":\n    d.get()",
            "Bounded[u8, 0, 100]",
            "u8",
        ),
        (
            "export def f(d: Divisor) -> i64 = \"lib_f\":\n    7 % d",
            "NonZero[i64]",
            "i64",
        ),
        (
            "export def f(d: Span) -> () = \"lib_f\":\n    pass",
            "Positive[i64]",
            "i64",
        ),
        (
            "export def f(d: &NonZero[i64]) -> i64 = \"lib_f\":\n    7 % *d",
            "NonZero[i64]",
            "i64",
        ),
        (
            "export def f(d: &mut Positive[i64]) -> () = \"lib_f\":\n    pass",
            "Positive[i64]",
            "i64",
        ),
        (
            "export def f(d: &mut Span) -> () = \"lib_f\":\n    pass",
            "Positive[i64]",
            "i64",
        ),
        (
            "export def f(d: Option[NonZero[i64]]) -> () = \"lib_f\":\n    pass",
            "NonZero[i64]",
            "i64",
        ),
        (
            "export def f(d: &list[NonZero[i64]]) -> () = \"lib_f\":\n    pass",
            "NonZero[i64]",
            "i64",
        ),
        // Results: the same rule in both directions.
        (
            "export def f() -> NonZero[i64] = \"lib_f\":\n    7",
            "NonZero[i64]",
            "i64",
        ),
        (
            "export def f() -> Percent = \"lib_f\":\n    7",
            "Bounded[u8, 0, 100]",
            "u8",
        ),
        (
            "export def f(n: i64) -> Result[NonZero[i64], RangeError] = \"lib_f\":\n    NonZero.try_from(n)",
            "NonZero[i64]",
            "i64",
        ),
    ] {
        let error = export_error(declaration);
        assert!(
            error.contains(&domain_message(domain, element)),
            "{declaration}\n{error}"
        );
    }
    // A plain quantity is a generic class instance without a domain.
    let error = export_error("export def f(d: Plain) -> () = \"lib_f\":\n    pass");
    assert!(error.contains("nongeneric facade"), "{error}");
}

/// A nongeneric class crosses C as an opaque handle that only a Plenty
/// factory creates, so C never builds the domain values stored in it.
#[test]
fn exported_handles_keep_domain_fields_behind_plenty_constructors() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("source.plenty");
    std::fs::write(
        &source,
        r#"
@newtype
@copy
class Wrap:
    v: NonZero[i64]

class Holder:
    d: Positive[i64]
    e: Bounded[i64, 1, 9]

export def wrap(n: i64) -> Result[Wrap, AllocError] = "domain_wrap":
    match NonZero.try_from(n):
        case Ok(v):
            Ok(Wrap(v))
        case Err(_):
            one: NonZero[i64] = 1
            Ok(Wrap(one))

export def remainder(n: i64, w: &Wrap) -> i64 = "domain_remainder":
    n % w.v

export def consume(n: i64, w: Wrap) -> i64 = "domain_consume":
    n % w.v

export def hold(n: i64) -> Result[Holder, AllocError] = "domain_hold":
    match Positive.try_from(n):
        case Ok(d):
            Ok(Holder(d, 3))
        case Err(_):
            Ok(Holder(1, 3))

export def held(n: i64, h: &mut Holder) -> i64 = "domain_held":
    n % h.d

export def digit(h: &Holder) -> i64 = "domain_digit":
    h.e.get()
"#,
    )
    .unwrap();
    let library = root.join("libdomain.a");
    let artifacts = plenty::compile_file_to_library(
        &source,
        &library,
        None,
        &plenty::LibraryOptions::new("domain", plenty::LibraryKind::Static),
    )
    .unwrap();
    let header = std::fs::read_to_string(&artifacts.header).unwrap();
    assert!(
        header.contains("typedef struct domain_Wrap domain_Wrap;"),
        "{header}"
    );
    assert!(
        header.contains("typedef struct domain_Holder domain_Holder;"),
        "{header}"
    );
    let host = root.join("host.c");
    std::fs::write(
        &host,
        r#"
#include "domain.h"
#include <assert.h>
int main(void) {
    uint32_t error = 0;
    domain_Wrap *wrap = 0;
    assert(domain_wrap(0, &wrap, &error) == 0);
    assert(domain_remainder(7, wrap) == 0);
    assert(domain_consume(7, wrap) == 0);
    assert(domain_wrap(4, &wrap, &error) == 0);
    assert(domain_remainder(7, wrap) == 3);
    domain_Wrap_destroy(wrap);
    domain_Holder *holder = 0;
    assert(domain_hold(-5, &holder, &error) == 0);
    assert(domain_held(7, holder) == 0);
    assert(domain_digit(holder) == 3);
    domain_Holder_destroy(holder);
    assert(domain_hold(5, &holder, &error) == 0);
    assert(domain_held(7, holder) == 2);
    domain_Holder_destroy(holder);
}
"#,
    )
    .unwrap();
    let link_args = std::fs::read_to_string(artifacts.link_args).unwrap();
    let executable = root.join("host");
    success(
        Command::new("cc")
            .args(["-Wall", "-Wextra", "-Werror", "-I"])
            .arg(root)
            .arg(&host)
            .arg(&library)
            .args(link_args.lines())
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    success(Command::new(executable).output().unwrap());
}

/// The documented pattern: declare the underlying integer and check it.
#[test]
fn foreign_integers_enter_a_domain_only_through_try_from() {
    for optimize in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let fixture = root.join("fixture.c");
        std::fs::write(
            &fixture,
            r#"
#include <stddef.h>
#include <stdint.h>
#include <string.h>
int64_t domain_zero(void) { return 0; }
void domain_fill(int64_t *out) { *out = -3; }
void domain_clear(uint8_t *data, size_t len, size_t stride) {
    for (size_t i = 0; i != len; ++i) memset(data + i * stride, 0, sizeof(int64_t));
}
int64_t domain_twice(int64_t n) { return 2 * n; }
"#,
        )
        .unwrap();
        let object = root.join("fixture.o");
        success(
            Command::new("cc")
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c"])
                .arg(&fixture)
                .arg("-o")
                .arg(&object)
                .output()
                .unwrap(),
        );
        std::fs::write(
            root.join("native.plentyi"),
            r#"
pub extern def zero() -> i64 = "domain_zero"
pub extern def fill(out: &mut i64) -> () = "domain_fill"
pub extern def clear(xs: &mut list[i64] as strided) -> () = "domain_clear"
pub extern def twice(n: i64) -> i64 = "domain_twice"
"#,
        )
        .unwrap();
        let main = root.join("main.plenty");
        std::fs::write(
            &main,
            r#"
import native

type Digit = Bounded[i64, 1, 9]

def main() -> Result[(), AllocError | IoError]:
    print(NonZero.try_from(native.zero()).is_err())?
    mut slot: i64 = 5
    native.fill(&mut slot)
    print(Positive.try_from(slot).is_err(), NonNegative.try_from(slot).is_err())?
    mut xs = [3, 4]?
    native.clear(&mut xs)
    print(Digit.try_from(xs[0]).is_err())?
    seven: NonZero[i64] = 7
    match NonZero.try_from(native.twice(seven.get())):
        case Ok(divisor):
            print(100 % divisor)?
        case Err(_):
            print("rejected")?
    Ok(())
"#,
        )
        .unwrap();
        let executable = root.join("program");
        let options = plenty::CompileOptions {
            optimize,
            link_args: vec![object.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&main, &executable, None, &options)
            .unwrap();
        let output = success(Command::new(&executable).output().unwrap());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "True\nTrue True\nTrue\n2\n"
        );
    }
}

/// Routes other than C signatures that could build a domain value without
/// its check. The issue #955 audit reasoned about the decoding ones.
#[test]
fn decoding_and_reinterpretation_cannot_build_a_domain_value() {
    for (body, expected) in [
        (
            "@serial\nclass Config:\n    d: NonZero[i64]\ndef main() -> Result[(), IoError]:\n    options = std.json.codec_options()\n    config = std.json.load[Config](b\"{\\\"d\\\":0}\", &options).unwrap()\n    print(7 % config.d)?\n    Ok(())\n",
            "NonZero[i64] has no __decode__ serialization hook",
        ),
        (
            "@serial\nclass Config:\n    d: Bounded[u8, 0, 100]\ndef main() -> Result[(), IoError]:\n    options = std.json.codec_options()\n    config = std.json.load[Config](b\"{\\\"d\\\":200}\", &options).unwrap()\n    print(config.d.get())?\n    Ok(())\n",
            "Bounded[u8, 0, 100] has no __decode__ serialization hook",
        ),
        (
            "def main() -> Result[(), IoError]:\n    options = std.json.codec_options()\n    d = std.json.load[NonZero[i64]](b\"0\", &options).unwrap()\n    print(d)?\n    Ok(())\n",
            "NonZero[i64] has no __decode__ serialization hook",
        ),
        (
            "def main() -> Result[(), IoError]:\n    options = std.json.codec_options()\n    xs = std.json.load[list[Positive[i64]]](b\"[0]\", &options).unwrap()\n    print(xs)?\n    Ok(())\n",
            "Positive[i64] has no __decode__ serialization hook",
        ),
        (
            "from std.atomic import Atomic\ndef main() -> ():\n    one: NonZero[i64] = 1\n    cell = Atomic[NonZero[i64]](one)\n",
            "Atomic[NonZero[i64]] is not supported",
        ),
        (
            "def main() -> ():\n    d = NonZero[i64].from_le_bytes(b\"\\0\\0\\0\\0\\0\\0\\0\\0\")\n",
            "class construction uses Class(...) or Class.new(...)",
        ),
        (
            "def main() -> Result[(), AllocError]:\n    one: NonZero[i64] = 1\n    mut xs = [one, one]?\n    view = &mut xs[0:2]\n    view[0] = 0\n    Ok(())\n",
            "`0` is not a `NonZero[i64]`",
        ),
    ] {
        let source = format!("import std.json\n{body}");
        let error = support::check_file_error(&source);
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

/// A plain `unchecked:` suite traps in the default mode before a wrapped
/// product can become a zero `NonZero`. Under `--unchecked assume` the
/// violation is documented undefined behavior, as for any other integer.
#[test]
fn unchecked_suites_trap_before_leaving_a_domain() {
    support::assert_file_traps(
        "def square(d: NonZero[u8]) -> NonZero[u8]:\n    unchecked:\n        d * d\ndef main() -> Result[(), IoError]:\n    d: NonZero[u8] = 16\n    print(7u8 % square(d))?\n    Ok(())\n",
        "integer overflow",
    );
    support::assert_file_traps(
        "def double(d: Positive[i8]) -> Positive[i8]:\n    unchecked:\n        d + d\ndef main() -> Result[(), IoError]:\n    d: Positive[i8] = 100\n    print(double(d))?\n    Ok(())\n",
        "integer overflow",
    );
}
