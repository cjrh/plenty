//! Counted strings, owned values, and exhaustive sum types through native code.
use rstest::rstest;
use std::process::Command;

fn run(source: &str) -> std::process::Output {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    plenty::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    Command::new(executable).output().unwrap()
}

#[rstest]
#[case(
    "print('a\\0b')\nprint(len('a\\0b'))\nprint('a\\0b'[1])",
    "a\0b\n3\n\0\n"
)]
#[case(
    "print('a\0b' + 'c')\nprint('a\\0b' == 'a\\0c')\nprint('\\0b' in 'a\\0b')\nprint('' in '')",
    "a\0bc\nFalse\nTrue\nTrue\n"
)]
#[case("d = {'a\\0b': 1, 'a\\0c': 2, 'a': 3}\nprint(len(d))\nprint(d['a\\0c'])\nprint(len({'a\\0b', 'a\\0c', 'a'}))", "3\n2\n3\n")]
#[case(
    "print(len('é中😀é'))\nprint('é中😀'[-1])\nfor c in 'é\\0😀':\n    print(c)",
    "5\n😀\né\n\0\n😀\n"
)]
#[case(
    "mut a = [['a' + 'b']]\nb = copy(a)\na[0] = ['new']\nprint(b)\nprint(a)\nprint([['x' + 'y']][0][0])",
    "[[\"ab\"]]\n[[\"new\"]]\nxy\n"
)]
#[case("def repeat(n: i64, s: str) -> str:\n    if n == 0:\n        s\n    else:\n        repeat(n - 1, s + '')\nprint(repeat(10_000, 'ok'))", "ok\n")]
#[case("enum Color:\n    Red\n    Blue\nprint(Color.Red)\nprint(Color.Red == Color.Red)\nprint(Color.Red == Color.Blue)", "Color.Red\nTrue\nFalse\n")]
#[case("enum Reading:\n    Missing\n    Value(i64)\n    Invalid(str)\ndef show(r: Reading) -> str:\n    match r:\n        case Reading.Missing:\n            'missing'\n        case Reading.Value(n):\n            'value' if n == 42 else 'other'\n        case Reading.Invalid(reason):\n            reason\nprint(show(Reading.Missing))\nprint(show(Reading.Value(42)))\nprint(show(Reading.Invalid('bad' + ' input')))", "missing\nvalue\nbad input\n")]
#[case("enum Pair:\n    Value(i8, u64, str)\nx = Pair.Value(-128i8, 18446744073709551615u64, 'a\\0b')\nmatch x:\n    case Pair.Value(a, b, c):\n        print(a)\n        print(b)\n        print(c)\nprint(x == Pair.Value(-128i8, 18446744073709551615u64, 'a\\0b'))", "-128\n18446744073709551615\na\0b\nTrue\n")]
#[case("type Input = Reading\nenum Reading:\n    Data(list[str])\nx = Input.Data(['a' + 'b'])\nmatch copy(x):\n    case Reading.Data(items):\n        mut copy = items\n        copy.append('c')\n        print(copy)\nprint(x)", "[\"ab\", \"c\"]\nReading.Data([\"ab\"])\n")]
#[case("enum Outer:\n    Value(Inner)\nenum Inner:\n    Text(str)\nprint([Outer.Value(Inner.Text('hi'))] == [Outer.Value(Inner.Text('h' + 'i'))])\nprint({'x': Outer.Value(Inner.Text('hi'))})", "True\n{\"x\": Outer.Value(Inner.Text(\"hi\"))}\n")]
#[case("def choose(n: i64) -> Option[i64]:\n    if n > 0:\n        Option[i64].Some(n)\n    else:\n        Option[i64].Nothing\nfor n in [-1, 2]:\n    match choose(n):\n        case Option[i64].Some(value):\n            print(value)\n        case Option[i64].Nothing:\n            print('nothing')", "nothing\n2\n")]
#[case("type R = Result[i64, str]\ndef answer(ok: bool) -> R:\n    if ok:\n        R.Ok(42)\n    else:\n        R.Err('no')\nprint(answer(True))\nprint(answer(False))", "Result[i64, str].Ok(42)\nResult[i64, str].Err(\"no\")\n")]
#[case("enum E:\n    A\n    B(i64)\ndef f(e: E) -> i64:\n    match e:\n        case E.A:\n            return 1\n        case E.B(n):\n            n\nprint(f(E.B(42)))\nfor e in [E.A, E.B(2), E.B(3)]:\n    match e:\n        case E.A:\n            continue\n        case E.B(n):\n            if n == 3:\n                break\n            print(n)", "42\n2\n")]
#[case("enum E:\n    A\n    B(str)\nx = 99\nmatch E.B('yes'):\n    case E.B(x):\n        print(x)\n    case _:\n        pass\nprint(x)", "yes\n99\n")]
#[case("type O = Option[str]\ndef f(n: i64, o: O) -> str:\n    match o:\n        case O.Nothing:\n            'empty'\n        case O.Some(s):\n            if n == 0:\n                s\n            else:\n                f(n - 1, O.Some(s + ''))\nprint(f(10_000, O.Some('ok')))", "ok\n")]
#[case(
    "enum Pair:\n    Values(str, str)\ndef item(n: i64) -> str:\n    print(n)\n    'value' + ''\nmatch Pair.Values(item(1), item(2)):\n    case Pair.Values(_, last):\n        print(last)",
    "1\n2\nvalue\n"
)]
#[case(
    "type Inner = Result[str, i64]\ntype Outer = Option[Inner]\ndef value(o: Outer) -> str:\n    match o:\n        case Outer.Nothing:\n            return 'absent'\n        case Outer.Some(r):\n            match r:\n                case Inner.Ok(s):\n                    return s\n                case Inner.Err(_):\n                    return 'error'\nprint(value(Outer.Some(Inner.Ok('yes' + ''))))\nprint(value(Outer.Some(Inner.Err(3))))\nprint(value(Outer.Nothing))",
    "yes\nerror\nabsent\n"
)]
fn native_values(#[case] source: &str, #[case] expected: &str) {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected.as_bytes(), "{source}");
}

#[rstest]
#[case("enum E:\n    A\n    A", "duplicate variant")]
#[case("enum E:\n    A(i64)\nprint(E.A(True))", "expected i64, got bool")]
#[case("enum E:\n    A\nprint(E.A())", "nullary variants")]
#[case("enum E:\n    A(i64)\nprint(E.A)", "payload arguments")]
#[case(
    "enum E:\n    A\n    B\nmatch E.A:\n    case E.A:\n        pass",
    "non-exhaustive match; missing E.B"
)]
#[case(
    "enum E:\n    A\n    B\nmatch E.A:\n    case E.A:\n        pass\n    case E.A:\n        pass",
    "duplicate variant case"
)]
#[case(
    "enum E:\n    A\nmatch E.A:\n    case _:\n        pass\n    case E.A:\n        pass",
    "unreachable case"
)]
#[case(
    "enum E:\n    A\nenum F:\n    A\nmatch E.A:\n    case F.A:\n        pass",
    "expected E, got F"
)]
#[case(
    "enum E:\n    A(i64, i64)\nmatch E.A(1, 2):\n    case E.A(x, x):\n        pass",
    "duplicate payload binding"
)]
#[case(
    "enum E:\n    A(i64)\nmatch E.A(1):\n    case E.A(x):\n        pass\nprint(x)",
    "unknown binding"
)]
#[case("enum E:\n    A(E)", "recursive enum")]
#[case("type A = list[E]\nenum E:\n    A(A)", "cyclic type alias")]
#[case("enum E:\n    A\nx: set[E] = set()", "set elements must")]
#[case("x = Option[i64, str].Nothing", "Option requires 1")]
#[case("x = Result[i64].Ok(1)", "Result requires 2")]
#[case("x = Result[(), str].Ok(())", "payloads cannot be unit")]
#[case("enum E:\n    A\nE = 1\nprint(E.A)", "shadows a type qualifier")]
#[case("enum E:\n    A\n    B\ndef f(e: E) -> i64:\n    match e:\n        case E.A:\n            1\n        case E.B:\n            True", "expected i64, got bool")]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = plenty::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected:?}, got {error:?}"
    );
}

#[test]
fn input_preserves_nul_and_validates_utf8() {
    use std::io::Write;
    use std::process::Stdio;
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("input");
    plenty::compile_legacy_source_to_executable(":readline drop :println", &executable).unwrap();
    let run = |input: &[u8]| {
        let mut child = Command::new(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };
    let valid = run("é\0😀\r\n".as_bytes());
    assert!(valid.status.success());
    assert_eq!(valid.stdout, "é\0😀\n".as_bytes());
    assert_eq!(run(b"").stdout, b"\n");
    for invalid in [
        &b"\x80"[..],
        &b"\xc0\x80"[..],
        &b"\xe0\x80\x80"[..],
        &b"\xed\xa0\x80"[..],
        &b"\xf4\x90\x80\x80"[..],
        &b"\xf5\x80\x80\x80"[..],
        &b"\xf0\x9f"[..],
        &b"\xc2A"[..],
    ] {
        let output = run(invalid);
        assert_eq!(output.status.code(), Some(1), "{invalid:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("invalid UTF-8 input"));
    }
}

#[test]
fn shared_enum_dependencies_do_not_expand_exponentially() {
    let mut source = String::from("enum E0:\n    End\n");
    for n in 1..40 {
        source.push_str(&format!(
            "enum E{n}:\n    End\n    Pair(E{}, E{})\n",
            n - 1,
            n - 1
        ));
    }
    source.push_str("print(E39.End)\n");
    source.push_str("x0 = E0.End\n");
    for n in 1..40 {
        source.push_str(&format!("x{n} = E{n}.Pair(x{}, x{})\n", n - 1, n - 1));
    }
    source.push_str("print(x39 == x39)\n");
    source.push_str("items = [x39]\nother = copy(items)\nprint(items == other)\n");
    let output = run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"E39.End\nTrue\nTrue\n");
}

#[test]
fn excessive_type_depth_is_diagnosed() {
    let mut source = String::from("enum E0:\n    End\n");
    for n in 1..66 {
        source.push_str(&format!("enum E{n}:\n    Wrap(E{})\n", n - 1));
    }
    let nested = format!("type Deep = {}i64{}", "list[".repeat(65), "]".repeat(65));
    for source in [source, nested] {
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("type nesting exceeds"), "{error}");
    }
}

#[test]
fn excessive_builtin_type_names_are_diagnosed() {
    let mut source = String::from("type T0 = i64\n");
    for n in 1..20 {
        source.push_str(&format!("type T{n} = Result[T{}, T{}]\n", n - 1, n - 1));
    }
    let error = plenty::check_source(&source).unwrap_err().to_string();
    assert!(error.contains("concrete type name exceeds"), "{error}");
}
