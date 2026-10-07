mod support;
use rstest::rstest;
use std::process::Command;

#[rstest]
#[case(
    "mut a = [1, 2]\nmut b = copy(a)\nb.append(3)\nprint(a)\nprint(b)",
    "[1, 2]\n[1, 2, 3]\n"
)]
#[case("mut a = [1]\nmut b = a\nb.append(2)\nprint(b)", "[1, 2]\n")]
#[case(
    "def add(xs: &mut list[i64]) -> ():\n    xs.append(3)\nmut a = [1, 2]\nadd(&mut a)\nprint(a)",
    "[1, 2, 3]\n"
)]
#[case(
    "mut a = [1]\nr = &mut a\nr.append(2)\nprint(r)\na.append(3)\nprint(a)",
    "[1, 2]\n[1, 2, 3]\n"
)]
#[case(
    "def size(xs: &list[i64]) -> i64:\n    len(xs)\na = [1, 2]\nprint(size(&a))\nprint(a)",
    "2\n[1, 2]\n"
)]
#[case("mut x = 3\nr = &mut x\n*r = 7\nprint(*r)\nprint(x)", "7\n7\n")]
#[case("mut a = [[1]]\nmut b = copy(a)\nmut child = copy(b[0])\nchild.append(2)\nb[0] = child\nprint(a)\nprint(b)", "[[1]]\n[[1, 2]]\n")]
#[case(
    "mut a = [1]\nr = &mut a\ns = &r\nprint(s)\nr.append(2)\nprint(r)",
    "[1]\n[1, 2]\n"
)]
#[case("a = [1, 2]\nfor x in &a:\n    print(x)\nprint(a)", "1\n2\n[1, 2]\n")]
#[case("mut a = [1]\ndrop(a)\na = [2]\nprint(a)", "[2]\n")]
#[case(
    "def write_byte(x: &mut i8) -> ():\n    *x = -12i8\nmut x = 1i8\nwrite_byte(&mut x)\nprint(x)",
    "-12\n"
)]
#[case(
    "mut a = [1]\nr = &mut a\n*r = [2, 3]\nprint(r)\nprint(a)",
    "[2, 3]\n[2, 3]\n"
)]
#[case("def add(xs: &mut list[i64]) -> ():\n    xs.append(len(xs))\ndef twice(xs: &mut list[i64]) -> ():\n    add(xs)\n    add(&mut xs)\nmut a = [7]\ntwice(&mut a)\nprint(a)", "[7, 1, 2]\n")]
#[case("def count(xs: &list[i64]) -> i64:\n    mut n = 0\n    for x in xs:\n        n = n + x\n    n\na = [1, 2, 3]\nprint(count(&a))\nprint(a)", "6\n[1, 2, 3]\n")]
#[case("def g() -> Generator[i64]:\n    yield 3\ndef advance(g: &mut Generator[i64]) -> Option[i64]:\n    next(g)\nmut it = g()\nprint(advance(&mut it))\nprint(next(it))", "Option[i64].Some(3)\nOption[i64].Nothing\n")]
#[case("def g() -> Generator[i64]:\n    mut a = [1]\n    r = &mut a\n    r.append(2)\n    yield len(a)\n    yield 3\nprint(list(g()))", "[2, 3]\n")]
#[case(
    "mut a = [1]\nr = &a\nif True:\n    print(r)\nelse:\n    print(r)\na.append(2)\nprint(a)",
    "[1]\n[1, 2]\n"
)]
#[case("mut a = [1]\nmut n = 0\nwhile n < 3:\n    r = &a\n    print(len(r))\n    a.append(n)\n    n = n + 1\nprint(a)", "1\n2\n3\n[1, 0, 1, 2]\n")]
#[case("mut a = [1, 2]\na[len(a) - 1] = len(a) + 1\nprint(a)", "[1, 3]\n")]
#[case("type R = Result[list[i64], str]\na = R.Ok([1])\nb = copy(a)\nmatch b:\n    case R.Ok(items):\n        mut x = items\n        x.append(2)\n        print(x)\n    case R.Err(_):\n        pass\nprint(a)", "[1, 2]\nResult[list[i64], str].Ok([1])\n")]
#[case(
    "mut a: list[i64] = []\nfor n in range(20_000):\n    a.append(n)\nprint(len(a))\nprint(a[-1])",
    "20000\n19999\n"
)]
#[case("type View = &list[i64]\ndef length(v: View) -> i64:\n    len(v)\na = [1, 2]\nprint(length(&a))", "2\n")]
#[case(
    "mut text = 'old'\nr = &mut text\n*r = 'new' + ' text'\nprint(text)",
    "new text\n"
)]
#[case("mut x = 1\nr = (&mut x)\n*r = *(&r)\nprint(*r)", "1\n")]
fn native(#[case] source: &str, #[case] expected: &str) {
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected.as_bytes());
}

#[rstest]
#[case("a = [1]\nb = a\nprint(a)", "moved")]
#[case("mut a = [1]\nr = &a\na.append(2)\nprint(r)", "conflicting borrow")]
#[case("mut a = [1]\nr = &mut a\nprint(a)\nprint(r)", "conflicting borrow")]
#[case("mut a = [1]\nr = &a\ndrop(a)\nprint(r)", "conflicting borrow")]
#[case("mut a = [1]\nr = &a\nb = a\nprint(r)", "conflicting borrow")]
#[case("a = [1]\nr = &mut a", "mut binding")]
#[case("mut a = [1]\nr = &a\ns = &mut r", "shared reference")]
#[case(
    "def f(a: &mut list[i64], b: &list[i64]) -> ():\n    pass\nmut a = [1]\nf(&mut a, &a)",
    "conflicting borrow"
)]
#[case("def f() -> &i64:\n    x = 1\n    &x", "returned references")]
#[case("r = &[1][0]", "named collection")]
#[case(
    "mut a = [1]\nr = &a\nwhile len(a) > 0:\n    a.append(2)\n    print(r)",
    "conflicting borrow"
)]
#[case("a = [1]\ndrop(a)\nprint(a)", "moved")]
#[case(
    "def g() -> Generator[i64]:\n    a = [1]\n    r = &a\n    yield 1\n    print(r)",
    "across yield"
)]
#[case(
    "mut a = [1]\nr = &mut a\ns = &r\nr.append(2)\nprint(s)",
    "conflicting borrow"
)]
#[case("mut a = [1]\nfor x in &a:\n    a.append(x)", "conflicting borrow")]
#[case(
    "mut a = [1]\nr = &a\nif True:\n    a = [2]\nprint(r)",
    "conflicting borrow"
)]
#[case(
    "mut a = [1]\nr = &a\nwhile True:\n    print(r)\n    a.append(2)",
    "conflicting borrow"
)]
#[case(
    "def f(x: &list[i64], y: list[i64]) -> ():\n    pass\na = [1]\nf(&a, a)",
    "conflicting borrow"
)]
#[case("def main() -> ():\n    x = 1\n    &x", "expected (), got &i64")]
#[case("x = 1\nprint(*x)", "dereference requires")]
#[case("a = [1]\nx = [&a]", "cannot be stored")]
#[case(
    "def g() -> Generator[i64]:\n    yield 1\nmut it = g()\nfor x in &it:\n    pass",
    "borrowed generator iteration"
)]
#[case(
    "def g(x: &i64) -> Generator[i64]:\n    yield *x",
    "capture references"
)]
#[case("x = 1\ndrop(&x)", "owned value")]
#[case(
    "s = 'a' + 'b'\nfor n in range(2):\n    print(s)\n    drop(s)",
    "loop backedge"
)]
#[case("n = 1\nwhile n > 0:\n    drop(n)", "loop backedge")]
#[case("s = 'a' + 'b'\ndrop((s))\nprint(s)", "moved")]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected}, got {error}"
    );
}
