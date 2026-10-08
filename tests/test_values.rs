//! Counted strings, owned values, and exhaustive sum types through native code.
mod support;
use rstest::rstest;
use std::process::Command;

fn run(source: &str) -> std::process::Output {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    Command::new(executable).output().unwrap()
}

#[rstest]
#[case(
    "print('a\\0b').unwrap()\nprint(len('a\\0b')).unwrap()\nprint('a\\0b'[1].unwrap()).unwrap()",
    "a\0b\n3\n\0\n"
)]
#[case(
    "print(('a\0b' + 'c').unwrap()).unwrap()\nprint('a\\0b' == 'a\\0c').unwrap()\nprint('\\0b' in 'a\\0b').unwrap()\nprint('' in '').unwrap()",
    "a\0bc\nFalse\nTrue\nTrue\n"
)]
#[case("d = {'a\\0b': 1, 'a\\0c': 2, 'a': 3}.unwrap()\nprint(len(d)).unwrap()\nprint(d['a\\0c']).unwrap()\nprint(len({'a\\0b', 'a\\0c', 'a'}.unwrap())).unwrap()", "3\n2\n3\n")]
#[case(
    "print(len('é中😀é')).unwrap()\nprint('é中😀'[-1].unwrap()).unwrap()\nfor c in 'é\\0😀':\n    print(c.unwrap()).unwrap()",
    "5\n😀\né\n\0\n😀\n"
)]
#[case(
    "mut a = [[('a' + 'b').unwrap()].unwrap()].unwrap()\nb = copy(a).unwrap()\na[0] = ['new'].unwrap()\nprint(b).unwrap()\nprint(a).unwrap()\nprint([[('x' + 'y').unwrap()].unwrap()].unwrap()[0][0]).unwrap()",
    "[[\"ab\"]]\n[[\"new\"]]\nxy\n"
)]
#[case("def repeat(n: i64, s: str) -> str:\n    if n == 0:\n        s\n    else:\n        repeat(n - 1, (s + '').unwrap())\nprint(repeat(10_000, 'ok')).unwrap()", "ok\n")]
#[case("enum Color:\n    Red\n    Blue\nprint((Color.Red).unwrap()).unwrap()\nprint((Color.Red).unwrap() == (Color.Red).unwrap()).unwrap()\nprint((Color.Red).unwrap() == (Color.Blue).unwrap()).unwrap()", "Color.Red\nTrue\nFalse\n")]
#[case("enum Reading:\n    Missing\n    Value(i64)\n    Invalid(str)\ndef show(r: Reading) -> str:\n    match r:\n        case Reading.Missing:\n            'missing'\n        case Reading.Value(n):\n            'value' if n == 42 else 'other'\n        case Reading.Invalid(reason):\n            reason\nprint(show((Reading.Missing).unwrap())).unwrap()\nprint(show(Reading.Value(42).unwrap())).unwrap()\nprint(show(Reading.Invalid(('bad' + ' input').unwrap()).unwrap())).unwrap()", "missing\nvalue\nbad input\n")]
#[case("enum Pair:\n    Value(i8, u64, str)\nx = Pair.Value(-128i8, 18446744073709551615u64, 'a\\0b').unwrap()\nmatch x:\n    case Pair.Value(a, b, c):\n        print(a).unwrap()\n        print(b).unwrap()\n        print(c).unwrap()\nprint(x == Pair.Value(-128i8, 18446744073709551615u64, 'a\\0b').unwrap()).unwrap()", "-128\n18446744073709551615\na\0b\nTrue\n")]
#[case("type Input = Reading\nenum Reading:\n    Data(list[str])\nx = Input.Data([('a' + 'b').unwrap()].unwrap()).unwrap()\nmatch copy(x).unwrap():\n    case Reading.Data(items):\n        mut copy = items\n        copy.append('c').unwrap()\n        print(copy).unwrap()\nprint(x).unwrap()", "[\"ab\", \"c\"]\nReading.Data([\"ab\"])\n")]
#[case("enum Outer:\n    Value(Inner)\nenum Inner:\n    Text(str)\nprint([Outer.Value(Inner.Text('hi').unwrap()).unwrap()].unwrap() == [Outer.Value(Inner.Text(('h' + 'i').unwrap()).unwrap()).unwrap()].unwrap()).unwrap()\nprint({'x': Outer.Value(Inner.Text('hi').unwrap()).unwrap()}.unwrap()).unwrap()", "True\n{\"x\": Outer.Value(Inner.Text(\"hi\"))}\n")]
#[case("def choose(n: i64) -> Option[i64]:\n    if n > 0:\n        Option[i64].Some(n)\n    else:\n        Option[i64].Nothing\nfor n in [-1, 2].unwrap():\n    match choose(n):\n        case Option[i64].Some(value):\n            print(value).unwrap()\n        case Option[i64].Nothing:\n            print('nothing').unwrap()", "nothing\n2\n")]
#[case("type R = Result[i64, str]\ndef answer(ok: bool) -> R:\n    if ok:\n        R.Ok(42)\n    else:\n        R.Err('no')\nprint(answer(True)).unwrap()\nprint(answer(False)).unwrap()", "Result[i64, str].Ok(42)\nResult[i64, str].Err(\"no\")\n")]
#[case("enum E:\n    A\n    B(i64)\ndef f(e: E) -> i64:\n    match e:\n        case E.A:\n            return 1\n        case E.B(n):\n            n\nprint(f(E.B(42).unwrap())).unwrap()\nfor e in [(E.A).unwrap(), E.B(2).unwrap(), E.B(3).unwrap()].unwrap():\n    match e:\n        case E.A:\n            continue\n        case E.B(n):\n            if n == 3:\n                break\n            print(n).unwrap()", "42\n2\n")]
#[case("enum E:\n    A\n    B(str)\nx = 99\nmatch E.B('yes').unwrap():\n    case E.B(x):\n        print(x).unwrap()\n    case _:\n        pass\nprint(x).unwrap()", "yes\n99\n")]
#[case("type O = Option[str]\ndef f(n: i64, o: O) -> str:\n    match o:\n        case O.Nothing:\n            'empty'\n        case O.Some(s):\n            if n == 0:\n                s\n            else:\n                f(n - 1, O.Some((s + '').unwrap()))\nprint(f(10_000, O.Some('ok'))).unwrap()", "ok\n")]
#[case(
    "enum Pair:\n    Values(str, str)\ndef item(n: i64) -> str:\n    print(n).unwrap()\n    ('value' + '').unwrap()\nmatch Pair.Values(item(1), item(2)).unwrap():\n    case Pair.Values(_, last):\n        print(last).unwrap()",
    "1\n2\nvalue\n"
)]
#[case(
    "type Inner = Result[str, i64]\ntype Outer = Option[Inner]\ndef value(o: Outer) -> str:\n    match o:\n        case Outer.Nothing:\n            return 'absent'\n        case Outer.Some(r):\n            match r:\n                case Inner.Ok(s):\n                    return s\n                case Inner.Err(_):\n                    return 'error'\nprint(value(Outer.Some(Inner.Ok(('yes' + '').unwrap())))).unwrap()\nprint(value(Outer.Some(Inner.Err(3)))).unwrap()\nprint(value(Outer.Nothing)).unwrap()",
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
#[case(
    "enum E:\n    A(i64)\nprint(E.A(True).unwrap()).unwrap()",
    "expected i64, got bool"
)]
#[case("enum E:\n    A\nprint(E.A().unwrap()).unwrap()", "nullary variants")]
#[case(
    "enum E:\n    A(i64)\nprint((E.A).unwrap()).unwrap()",
    "payload arguments"
)]
#[case(
    "enum E:\n    A\n    B\nmatch (E.A).unwrap():\n    case E.A:\n        pass",
    "non-exhaustive match; missing E.B"
)]
#[case(
    "enum E:\n    A\n    B\nmatch (E.A).unwrap():\n    case E.A:\n        pass\n    case E.A:\n        pass",
    "duplicate variant case"
)]
#[case(
    "enum E:\n    A\nmatch (E.A).unwrap():\n    case _:\n        pass\n    case E.A:\n        pass",
    "unreachable case"
)]
#[case(
    "enum E:\n    A\nenum F:\n    A\nmatch (E.A).unwrap():\n    case F.A:\n        pass",
    "expected E, got F"
)]
#[case(
    "enum E:\n    A(i64, i64)\nmatch E.A(1, 2).unwrap():\n    case E.A(x, x):\n        pass",
    "duplicate payload binding"
)]
#[case(
    "enum E:\n    A(i64)\nmatch E.A(1).unwrap():\n    case E.A(x):\n        pass\nprint(x).unwrap()",
    "unknown binding"
)]
#[case(
    "enum E:\n    A(E)",
    "recursive data declarations are not supported yet: E -> E"
)]
#[case(
    "type A = list[E]\nenum E:\n    A(A)",
    "recursive data declarations are not supported yet: A -> E -> A"
)]
#[case("enum E:\n    A\nx: set[E] = set().unwrap()", "set elements must")]
#[case("x = Option[i64, str].Nothing", "Option requires 1")]
#[case("x = Result[i64].Ok(1)", "Result requires 2")]
#[case("x = Result[(), str].Ok(1)", "expected (), got i64")]
#[case(
    "enum E:\n    A\nE = 1\nprint((E.A).unwrap()).unwrap()",
    "shadows a type qualifier"
)]
#[case("enum E:\n    A\n    B\ndef f(e: E) -> i64:\n    match e:\n        case E.A:\n            1\n        case E.B:\n            True", "expected i64, got bool")]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
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
    source.push_str("print((E39.End).unwrap()).unwrap()\n");
    source.push_str("x0 = (E0.End).unwrap()\n");
    for n in 1..40 {
        source.push_str(&format!(
            "x{n} = E{n}.Pair(x{}, x{}).unwrap()\n",
            n - 1,
            n - 1
        ));
    }
    source.push_str("print(x39 == x39).unwrap()\n");
    source.push_str(
        "items = [x39].unwrap()\nother = copy(items).unwrap()\nprint(items == other).unwrap()\n",
    );
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
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("type nesting exceeds"), "{error}");
    }
}

#[test]
fn excessive_builtin_type_names_are_diagnosed() {
    let mut source = String::from("type T0 = i64\n");
    for n in 1..20 {
        source.push_str(&format!("type T{n} = Result[T{}, T{}]\n", n - 1, n - 1));
    }
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(error.contains("concrete type name exceeds"), "{error}");
}
