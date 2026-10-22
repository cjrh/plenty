//! Native float ABI, IEEE semantics, and locally inferred prelude constructors.
mod support;
use rstest::rstest;
use std::process::Command;

#[test]
fn decimal_forms_unit_payloads_and_prelude_shadowing() {
    native(
        "print(1.).unwrap()\nprint(.5).unwrap()\nprint(1.e2).unwrap()\nprint(1.f32).unwrap()\ndef unit(x: Option[()]) -> ():\n    match x:\n        case Some(u):\n            u\n        case Nothing:\n            pass\nunit(Some(()))\nx: Result[(), ()] = Err(())\nprint(str.repr(x).unwrap()).unwrap()\nSome = 42\nprint(Some).unwrap()",
        "1.0\n0.5\n100.0\n1.0\nResult[(), ()].Err(())\n42\n",
    );
}

#[rstest]
#[case(
    "print(1.5 + 2.25).unwrap()\nprint(5.0 / 2.0).unwrap()\nprint(.5f32 * 4f32).unwrap()\nprint(1e-2 + 2E-2).unwrap()\nprint(-1.5f32).unwrap()",
    "3.75\n2.5\n2.0\n0.03\n-1.5\n"
)]
#[case(
    "print(16777216f32 + 1f32).unwrap()\nprint(16777216f64 + 1f64).unwrap()\nprint(f64(5) / 2.0).unwrap()\nprint(i32(-2.9)).unwrap()\nprint(f32(2.5)).unwrap()",
    "16777216.0\n16777217.0\n2.5\n-2\n2.5\n"
)]
#[case(
    "z = 0.0\nn = z / z\nprint(n == n).unwrap()\nprint(n != n).unwrap()\nprint(n < z).unwrap()\nprint(n <= z).unwrap()\nprint(n > z).unwrap()\nprint(n >= z).unwrap()\nprint(1.0 / z).unwrap()\nprint(1.0 / -z).unwrap()\nprint(-0.0 == z).unwrap()",
    "False\nTrue\nFalse\nFalse\nFalse\nFalse\ninf\n-inf\nTrue\n"
)]
#[case(
    "type Real = f32\ndef half(x: Real) -> Real:\n    x / 2f32\ndef change(x: &mut Real) -> ():\n    *x = half(*x)\nmut x = 6f32\nchange(&mut x)\nprint(x).unwrap()\nprint(half(x)).unwrap()",
    "3.0\n1.5\n"
)]
#[case(
    "def count(n: i64, x: f64) -> f64:\n    if n == 0:\n        x\n    else:\n        count(n - 1, x + 0.5)\nprint(count(10000, 0.0)).unwrap()",
    "5000.0\n"
)]
#[case(
    "class Point:\n    x: f32\n    y: f64\n    def shift(self: &mut Point) -> ():\n        self.x = self.x + 1f32\nmut p = Point(1.5f32, 2.5)\np.shift()\nprint(p).unwrap()\nprint(p.x).unwrap()\nprint(copy(p).unwrap() == p).unwrap()",
    "Point(x=2.5, y=2.5)\n2.5\nTrue\n"
)]
#[case(
    "mut xs = [1.5f32, -0f32].unwrap()\nxs.append(2.5f32).unwrap()\nxs[0] = 3.5f32\nfor x in &xs:\n    print(x).unwrap()\nprint(2.5f32 in xs).unwrap()\nprint({'x': 2.5}.unwrap().values_snapshot().unwrap()).unwrap()\nprint([1.0].unwrap() == [1.0].unwrap()).unwrap()\nprint([0.0].unwrap() == [-0.0].unwrap()).unwrap()",
    "3.5\n-0.0\n2.5\nTrue\n[2.5]\nTrue\nTrue\n"
)]
#[case(
    "def values(x: f32) -> Generator[f32]:\n    mut y = x\n    yield y\n    y = y + 0.5f32\n    yield y\nfor x in values(1.5f32):\n    print(x).unwrap()",
    "1.5\n2.0\n"
)]
#[case(
    "n = 0.0 / 0.0\nx = Some(n)\nprint(x == x).unwrap()\na = [n].unwrap()\nprint(a == a).unwrap()\nprint(n in a).unwrap()\nprint({'x': n}.unwrap() == {'x': n}.unwrap()).unwrap()",
    "False\nFalse\nFalse\nFalse\n"
)]
#[case(
    "def divide(a: f64, b: f64) -> Result[f64, str]:\n    if b == 0.0:\n        return Err('zero')\n    Ok(a / b)\nfor r in [divide(5.0, 2.0), divide(5.0, 0.0)].unwrap():\n    match r:\n        case Ok(value):\n            print(value).unwrap()\n        case Err(message):\n            print(message).unwrap()",
    "2.5\nzero\n"
)]
#[case(
    "type R = Result[(), str]\ndef done() -> ():\n    print('effect').unwrap()\ndef check(ok: bool) -> R:\n    Ok(done()) if ok else Err('bad')\nmatch check(True):\n    case Ok(u):\n        u\n    case Err(_):\n        pass\nprint(str.repr(check(False)).unwrap()).unwrap()\nprint(Option[()].Some(())).unwrap()\nprint(Some(())).unwrap()",
    "effect\nResult[(), str].Err(\"bad\")\nOption[()].Some(())\nOption[()].Some(())\n"
)]
#[case(
    "def show(x: Option[Result[f32, str]]) -> ():\n    match x:\n        case Nothing:\n            print('empty').unwrap()\n        case Some(r):\n            print(str.repr(r).unwrap()).unwrap()\nshow(Some(Ok(2f32)))\nshow(Some(Err('bad')))\nshow(Nothing)\nx: list[Option[i64]] = [Nothing, Some(42)].unwrap()\nprint(x).unwrap()",
    "Result[f32, str].Ok(2.0)\nResult[f32, str].Err(\"bad\")\nempty\n[Option[i64].Nothing, Option[i64].Some(42)]\n"
)]
#[case(
    "def make() -> Result[list[f64], str]:\n    Ok([1.5, 2.5].unwrap())\nmatch make():\n    case Ok(items):\n        print(items).unwrap()\n    case Err(_):\n        pass\nenum Done:\n    Value(())\nprint(Done.Value(())).unwrap()",
    "[1.5, 2.5]\nDone.Value(())\n"
)]
fn native(#[case] source: &str, #[case] expected: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected.as_bytes(), "{source}");
}

#[rstest]
#[case("print(1.0f64 + 1i64).unwrap()", "cannot combine f64 with i64")]
#[case("x: f32 = 1.0f64", "expected f32, got f64")]
#[case("print(1f32 + 1f64).unwrap()", "cannot combine f32 with f64")]
#[case("print(1 / 2).unwrap()", "use `//`")]
#[case("print(1.0 // 2.0).unwrap()", "require integers")]
#[case("print(1.0 % 2.0).unwrap()", "require integers")]
#[case("x = 1e400", "out of range")]
#[case("x = 1e40f32", "out of range")]
#[case("x = 1e+", "invalid f64 literal")]
#[case("x = 1.2u32", "invalid f64 literal")]
#[case("x = {1.0}.unwrap()", "set elements must")]
#[case("x = {1.0: 2}.unwrap()", "dictionary keys must")]
#[case("x = Ok(1)", "cannot infer `Ok`; provide a Result annotation")]
#[case("x = Err('bad')", "cannot infer `Err`; provide a Result annotation")]
#[case("x = Nothing", "cannot infer `Nothing`; provide an Option annotation")]
#[case(
    "enum E:\n    A\n    B\ndef g() -> Result[(), E]:\n    Err(E.A)\nmut result = Ok(())\nresult = g()",
    "cannot infer `Ok`; provide a Result annotation, parameter, or return type"
)]
#[case("x: Option[i64] = Nothing()", "nullary variants")]
#[case("x: Option[i64] = Ok(1)", "requires a Result context")]
#[case("x: Result[i64, str] = Some(1)", "`Some` requires an Option context")]
#[case("x: Result[(), str] = Ok(42)", "expected (), got i64")]
#[case("x: Result[i64, str] = Err(42)", "expected str, got i64")]
#[case(
    "enum E:\n    Ok(i64)\nmatch E.Ok(1):\n    case Ok(x):\n        pass",
    "matches only Result"
)]
#[case("match Some(1):\n    case Some(x):\n        pass", "non-exhaustive")]
#[case("def Ok() -> i64:\n    1", "builtin")]
#[case(
    "match Some(()):\n    case Some(u):\n        print(u).unwrap()\n    case Nothing:\n        pass",
    "expected a value, got ()"
)]
#[case(
    "match Some(()):\n    case Some(u):\n        x = copy(u).unwrap()\n    case Nothing:\n        pass",
    "expected a value, got ()"
)]
#[case(
    "match Some(()):\n    case Some(u):\n        r = &u\n    case Nothing:\n        pass",
    "expected a value, got ()"
)]
#[case("x = 42\ny = Some(&x)", "references cannot be stored")]
#[case(
    "mut xs = [1.0].unwrap()\nr = &xs\nxs.append(2.0).unwrap()\nprint(r).unwrap()",
    "borrow"
)]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected:?}, got {error:?}"
    );
}

#[test]
fn casts_cover_every_integer_width_and_float_width() {
    let mut source = String::new();
    let mut expected = String::new();
    for integer in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        let value = if integer.starts_with('i') {
            "-12"
        } else {
            "12"
        };
        for float in ["f32", "f64"] {
            source.push_str(&format!(
                "print({float}({value}{integer})).unwrap()\nprint({integer}({value}.75{float})).unwrap()\n"
            ));
            expected.push_str(&format!("{value}.0\n{value}\n"));
        }
    }
    native(&source, &expected);
}

const INTEGERS: [(&str, i128, i128); 8] = [
    ("i8", i8::MIN as i128, i8::MAX as i128),
    ("i16", i16::MIN as i128, i16::MAX as i128),
    ("i32", i32::MIN as i128, i32::MAX as i128),
    ("i64", i64::MIN as i128, i64::MAX as i128),
    ("u8", 0, u8::MAX as i128),
    ("u16", 0, u16::MAX as i128),
    ("u32", 0, u32::MAX as i128),
    ("u64", 0, u64::MAX as i128),
];

/// Every source/target pair at both ends of both ranges and one step outside
/// the target's, so each bound is tried from the side that fits and the side
/// that does not.
#[test]
fn checked_conversions_cover_every_width_and_signedness() {
    let mut source = String::new();
    let mut expected = String::new();
    for (from, from_min, from_max) in INTEGERS {
        // Values arrive through a parameter so the check runs on a register
        // of the source width, not on a literal.
        for (to, _, _) in INTEGERS {
            source.push_str(&format!(
                "def {from}_to_{to}(value: {from}) -> Result[{to}, RangeError]:\n    {to}.try_from(value)\n"
            ));
        }
        for (to, to_min, to_max) in INTEGERS {
            let mut values = vec![
                from_min,
                from_max,
                0,
                to_min - 1,
                to_min,
                to_max,
                to_max + 1,
            ];
            values.retain(|value| (from_min..=from_max).contains(value));
            values.sort_unstable();
            values.dedup();
            for value in values {
                source.push_str(&format!(
                    "print(str.repr({from}_to_{to}({value}{from})).unwrap()).unwrap()\n"
                ));
                expected.push_str(&if (to_min..=to_max).contains(&value) {
                    format!("Result[{to}, RangeError].Ok({value})\n")
                } else {
                    format!("Result[{to}, RangeError].Err(RangeError.OutOfRange)\n")
                });
            }
        }
    }
    native(&source, &expected);
}

#[rstest]
#[case(
    "x = 300\nprint(str.repr(u8.try_from(x)).unwrap()).unwrap()\nprint(u8.wrapping_from(x)).unwrap()\nprint(u8.try_from(44).unwrap()).unwrap()",
    "Result[u8, RangeError].Err(RangeError.OutOfRange)\n44\n44\n"
)]
#[case(
    "type Count = u8\ndef narrow(text: &str) -> Result[Count, ParseError]:\n    wide = i64.parse(text)?\n    match Count.try_from(wide):\n        case Ok(value):\n            Ok(value)\n        case Err(error):\n            match error:\n                case RangeError.OutOfRange:\n                    Err(ParseError.OutOfRange)\nfor text in ['7', '700', 'x'].unwrap():\n    print(str.repr(narrow(&text)).unwrap()).unwrap()",
    "Result[u8, ParseError].Ok(7)\nResult[u8, ParseError].Err(ParseError.OutOfRange)\nResult[u8, ParseError].Err(ParseError.Invalid)\n"
)]
#[case(
    "match i8.try_from(200u8):\n    case Ok(value):\n        print(value).unwrap()\n    case Err(error):\n        print(error == RangeError.OutOfRange).unwrap()\nxs = [1, 300, -1].unwrap()\nfor x in &xs:\n    print(str.repr(u8.try_from(*x)).unwrap()).unwrap()",
    "True\nResult[u8, RangeError].Ok(1)\nResult[u8, RangeError].Err(RangeError.OutOfRange)\nResult[u8, RangeError].Err(RangeError.OutOfRange)\n"
)]
#[case(
    "def negate(value: u64) -> Result[i64, RangeError]:\n    Ok(-i64.try_from(value)?)\nprint(str.repr(negate(5u64)).unwrap()).unwrap()\nprint(str.repr(negate(18446744073709551615u64)).unwrap()).unwrap()\na = 3u8\nb = 3u16\nc = 3u32\nprint(-i16(a)).unwrap()\nprint(-i32(b)).unwrap()\nprint(-i64(c)).unwrap()",
    "Result[i64, RangeError].Ok(-5)\nResult[i64, RangeError].Err(RangeError.OutOfRange)\n-3\n-3\n-3\n"
)]
#[case(
    "a = -128i8\nb = 5i64\nc = 1.5\nprint(-(a + 1i8)).unwrap()\nprint(-b).unwrap()\nprint(-c).unwrap()\nzero: u8 = 0\nprint(zero).unwrap()\nprint(0u8).unwrap()",
    "127\n-5\n-1.5\n0\n0\n"
)]
fn checked_conversions_and_signed_negation(#[case] source: &str, #[case] expected: &str) {
    native(source, expected);
}

#[rstest]
#[case("x = u8.try_from()", "try_from takes one integer argument")]
#[case("x = u8.try_from(1, 2)", "try_from takes one integer argument")]
#[case("x = u8.try_from(1.5)", "try_from requires an integer value")]
#[case("x = u8.try_from('1')", "try_from requires an integer value")]
#[case("x = u8.try_from(True)", "try_from requires an integer value")]
#[case("x = f64.try_from(1)", "try_from is defined for integer types")]
#[case("x: u8 = u8.try_from(300)", "expected u8, got Result[u8, RangeError]")]
#[case(
    "def f() -> Result[u8, IoError]:\n    Ok(u8.try_from(300)?)",
    "`?` requires identical Result error types"
)]
#[case(
    "x = 3u8\ny = -x",
    "unary minus is not defined for unsigned type u8; convert to a signed type first, for example -i16(value)"
)]
#[case(
    "x = 3u16\ny = -x",
    "for unsigned type u16; convert to a signed type first, for example -i32(value)"
)]
#[case(
    "x = 3u32\ny = -x",
    "for unsigned type u32; convert to a signed type first, for example -i64(value)"
)]
#[case(
    "x = 3u64\ny = -x",
    "for unsigned type u64; convert to a signed type first, for example -i64.try_from(value)?"
)]
#[case("y = -(3u8)", "unary minus is not defined for unsigned type u8")]
#[case("y = -u8(3)", "unary minus is not defined for unsigned type u8")]
#[case(
    "x = 0u8\ny: u8 = -x",
    "unary minus is not defined for unsigned type u8"
)]
#[case(
    "x = 3u8\ny = 1u8 + -x",
    "unary minus is not defined for unsigned type u8"
)]
#[case(
    "type Count = u8\nx: Count = 3\ny = -x",
    "unary minus is not defined for unsigned type u8"
)]
#[case("y = -3u8", "unary minus is not defined for unsigned type u8")]
#[case("y: u8 = -3", "unary minus is not defined for unsigned type u8")]
fn conversion_and_negation_diagnostics(#[case] source: &str, #[case] expected: &str) {
    diagnostics(source, expected);
}

#[test]
fn float_enum_trees_compare_structurally() {
    let mut source = String::from("enum E0:\n    Value(f64)\nx0 = E0.Value(2.5)\n");
    for n in 1..8 {
        source.push_str(&format!(
            "enum E{n}:\n    Pair(E{}, E{})\nx{n} = E{n}.Pair(x{}, copy(x{}).unwrap())\n",
            n - 1,
            n - 1,
            n - 1,
            n - 1
        ));
    }
    source.push_str("print(x7 == x7).unwrap()\n");
    native(&source, "True\n");
}
