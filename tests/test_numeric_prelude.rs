//! Native float ABI, IEEE semantics, and locally inferred prelude constructors.
use rstest::rstest;
use std::process::Command;

#[test]
fn decimal_forms_unit_payloads_and_prelude_shadowing() {
    native("print(1.)\nprint(.5)\nprint(1.e2)\nprint(1.f32)\ndef unit(x: Option[()]) -> ():\n    match x:\n        case Some(u):\n            u\n        case Nothing:\n            pass\nunit(Some(()))\nx: Result[(), ()] = Err(())\nprint(x)\nSome = 42\nprint(Some)",
        "1.0\n0.5\n100.0\n1.0\nResult[(), ()].Err(())\n42\n");
}

#[rstest]
#[case(
    "print(1.5 + 2.25)\nprint(5.0 / 2.0)\nprint(.5f32 * 4f32)\nprint(1e-2 + 2E-2)\nprint(-1.5f32)",
    "3.75\n2.5\n2.0\n0.03\n-1.5\n"
)]
#[case("print(16777216f32 + 1f32)\nprint(16777216f64 + 1f64)\nprint(f64(5) / 2.0)\nprint(i32(-2.9))\nprint(f32(2.5))", "16777216.0\n16777217.0\n2.5\n-2\n2.5\n")]
#[case("z = 0.0\nn = z / z\nprint(n == n)\nprint(n != n)\nprint(n < z)\nprint(n <= z)\nprint(n > z)\nprint(n >= z)\nprint(1.0 / z)\nprint(1.0 / -z)\nprint(-0.0 == z)", "False\nTrue\nFalse\nFalse\nFalse\nFalse\ninf\n-inf\nTrue\n")]
#[case("print(i8(1000.0))\nprint(i8(-1000.0))\nprint(u8(-10.0))\nprint(u8(1000.0))\nprint(i16(1e9))\nprint(u16(1e9))\nprint(i64(1e100))\nprint(u64(1e100))\nprint(i32(0.0 / 0.0))", "127\n-128\n0\n255\n32767\n65535\n9223372036854775807\n18446744073709551615\n0\n")]
#[case("type Real = f32\ndef half(x: Real) -> Real:\n    x / 2f32\ndef change(x: &mut Real) -> ():\n    *x = half(*x)\nmut x = 6f32\nchange(&mut x)\nprint(x)\nprint(half(x))", "3.0\n1.5\n")]
#[case("def count(n: i64, x: f64) -> f64:\n    if n == 0:\n        x\n    else:\n        count(n - 1, x + 0.5)\nprint(count(10000, 0.0))", "5000.0\n")]
#[case("class Point:\n    x: f32\n    y: f64\n    def shift(self: &mut Point) -> ():\n        self.x = self.x + 1f32\nmut p = Point(1.5f32, 2.5)\np.shift()\nprint(p)\nprint(p.x)\nprint(copy(p) == p)", "Point(x=2.5, y=2.5)\n2.5\nTrue\n")]
#[case("mut xs = [1.5f32, -0f32]\nxs.append(2.5f32)\nxs[0] = 3.5f32\nfor x in &xs:\n    print(x)\nprint(2.5f32 in xs)\nprint({'x': 2.5}.values())\nprint([1.0] == [1.0])\nprint([0.0] == [-0.0])", "3.5\n-0.0\n2.5\nTrue\n[2.5]\nTrue\nTrue\n")]
#[case("def values(x: f32) -> Generator[f32]:\n    mut y = x\n    yield y\n    y = y + 0.5f32\n    yield y\nfor x in values(1.5f32):\n    print(x)", "1.5\n2.0\n")]
#[case("n = 0.0 / 0.0\nx = Some(n)\nprint(x == x)\na = [n]\nprint(a == a)\nprint(n in a)\nprint({'x': n} == {'x': n})", "False\nFalse\nFalse\nFalse\n")]
#[case("def divide(a: f64, b: f64) -> Result[f64, str]:\n    if b == 0.0:\n        return Err('zero')\n    Ok(a / b)\nfor r in [divide(5.0, 2.0), divide(5.0, 0.0)]:\n    match r:\n        case Ok(value):\n            print(value)\n        case Err(message):\n            print(message)", "2.5\nzero\n")]
#[case("type R = Result[(), str]\ndef done() -> ():\n    print('effect')\ndef check(ok: bool) -> R:\n    Ok(done()) if ok else Err('bad')\nmatch check(True):\n    case Ok(u):\n        u\n    case Err(_):\n        pass\nprint(check(False))\nprint(Option[()].Some(()))\nprint(Some(()))", "effect\nResult[(), str].Err(\"bad\")\nOption[()].Some(())\nOption[()].Some(())\n")]
#[case("def show(x: Option[Result[f32, str]]) -> ():\n    match x:\n        case Nothing:\n            print('empty')\n        case Some(r):\n            print(r)\nshow(Some(Ok(2f32)))\nshow(Some(Err('bad')))\nshow(Nothing)\nx: list[Option[i64]] = [Nothing, Some(42)]\nprint(x)", "Result[f32, str].Ok(2.0)\nResult[f32, str].Err(\"bad\")\nempty\n[Option[i64].Nothing, Option[i64].Some(42)]\n")]
#[case("def make() -> Result[list[f64], str]:\n    Ok([1.5, 2.5])\nmatch make():\n    case Ok(items):\n        print(items)\n    case Err(_):\n        pass\nenum Done:\n    Value(())\nprint(Done.Value(()))", "[1.5, 2.5]\nDone.Value(())\n")]
fn native(#[case] source: &str, #[case] expected: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    plenty::compile_source_to_executable(source, &executable)
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
#[case("print(1.0 + 1)", "expected f64, got i64")]
#[case("x: f32 = 1.0", "expected f32, got f64")]
#[case("print(1f32 + 1f64)", "expected f32, got f64")]
#[case("print(1 / 2)", "use `//`")]
#[case("print(1.0 // 2.0)", "require integers")]
#[case("print(1.0 % 2.0)", "require integers")]
#[case("x = 1e400", "out of range")]
#[case("x = 1e40f32", "out of range")]
#[case("x = 1e+", "invalid f64 literal")]
#[case("x = 1.2u32", "invalid f64 literal")]
#[case("x = {1.0}", "set elements must")]
#[case("x = {1.0: 2}", "dictionary keys must")]
#[case("x = Ok(1)", "cannot infer `Ok`")]
#[case("x = Err('bad')", "cannot infer `Err`")]
#[case("x = Nothing", "cannot infer `Nothing`")]
#[case("x: Option[i64] = Nothing()", "nullary variants")]
#[case("x: Option[i64] = Ok(1)", "requires a Result context")]
#[case("x: Result[(), str] = Ok(42)", "expected (), got i64")]
#[case("x: Result[i64, str] = Err(42)", "expected str, got i64")]
#[case(
    "enum E:\n    Ok(i64)\nmatch E.Ok(1):\n    case Ok(x):\n        pass",
    "matches only Result"
)]
#[case("match Some(1):\n    case Some(x):\n        pass", "non-exhaustive")]
#[case("def Ok() -> i64:\n    1", "builtin")]
#[case(
    "match Some(()):\n    case Some(u):\n        print(u)\n    case Nothing:\n        pass",
    "expected a value, got ()"
)]
#[case(
    "match Some(()):\n    case Some(u):\n        x = copy(u)\n    case Nothing:\n        pass",
    "expected a value, got ()"
)]
#[case(
    "match Some(()):\n    case Some(u):\n        r = &u\n    case Nothing:\n        pass",
    "expected a value, got ()"
)]
#[case("x = 42\ny = Some(&x)", "references and generators cannot be stored")]
#[case("mut xs = [1.0]\nr = &xs\nxs.append(2.0)\nprint(r)", "borrow")]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = plenty::check_source(source).unwrap_err().to_string();
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
                "print({float}({value}{integer}))\nprint({integer}({value}.75{float}))\n"
            ));
            expected.push_str(&format!("{value}.0\n{value}\n"));
        }
    }
    native(&source, &expected);
}

#[test]
fn float_enum_dags_compare_without_expanding_shared_payloads() {
    let mut source = String::from("enum E0:\n    Value(f64)\nx0 = E0.Value(2.5)\n");
    for n in 1..40 {
        source.push_str(&format!(
            "enum E{n}:\n    Pair(E{}, E{})\nx{n} = E{n}.Pair(x{}, x{})\n",
            n - 1,
            n - 1,
            n - 1,
            n - 1
        ));
    }
    source.push_str("print(x39 == x39)\n");
    native(&source, "True\n");
}
