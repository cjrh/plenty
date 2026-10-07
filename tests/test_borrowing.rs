mod support;
use rstest::rstest;
use std::process::Command;

#[rstest]
#[case(
    "mut a = [1, 2].unwrap()\nmut b = copy(a).unwrap()\nb.append(3).unwrap()\nprint(a).unwrap()\nprint(b).unwrap()",
    "[1, 2]\n[1, 2, 3]\n"
)]
#[case(
    "mut a = [1].unwrap()\nmut b = a\nb.append(2).unwrap()\nprint(b).unwrap()",
    "[1, 2]\n"
)]
#[case(
    "def add(xs: &mut list[i64]) -> ():\n    xs.append(3).unwrap()\nmut a = [1, 2].unwrap()\nadd(&mut a)\nprint(a).unwrap()",
    "[1, 2, 3]\n"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\nr.append(2).unwrap()\nprint(r).unwrap()\na.append(3).unwrap()\nprint(a).unwrap()",
    "[1, 2]\n[1, 2, 3]\n"
)]
#[case(
    "def size(xs: &list[i64]) -> i64:\n    len(xs)\na = [1, 2].unwrap()\nprint(size(&a)).unwrap()\nprint(a).unwrap()",
    "2\n[1, 2]\n"
)]
#[case(
    "mut x = 3\nr = &mut x\n*r = 7\nprint(*r).unwrap()\nprint(x).unwrap()",
    "7\n7\n"
)]
#[case("mut a = [[1].unwrap()].unwrap()\nmut b = copy(a).unwrap()\nmut child = copy(b[0]).unwrap()\nchild.append(2).unwrap()\nb[0] = child\nprint(a).unwrap()\nprint(b).unwrap()", "[[1]]\n[[1, 2]]\n")]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\ns = &r\nprint(s).unwrap()\nr.append(2).unwrap()\nprint(r).unwrap()",
    "[1]\n[1, 2]\n"
)]
#[case(
    "a = [1, 2].unwrap()\nfor x in &a:\n    print(x).unwrap()\nprint(a).unwrap()",
    "1\n2\n[1, 2]\n"
)]
#[case(
    "mut a = [1].unwrap()\ndrop(a)\na = [2].unwrap()\nprint(a).unwrap()",
    "[2]\n"
)]
#[case(
    "def write_byte(x: &mut i8) -> ():\n    *x = -12i8\nmut x = 1i8\nwrite_byte(&mut x)\nprint(x).unwrap()",
    "-12\n"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\n*r = [2, 3].unwrap()\nprint(r).unwrap()\nprint(a).unwrap()",
    "[2, 3]\n[2, 3]\n"
)]
#[case("def add(xs: &mut list[i64]) -> ():\n    xs.append(len(xs)).unwrap()\ndef twice(xs: &mut list[i64]) -> ():\n    add(xs)\n    add(&mut xs)\nmut a = [7].unwrap()\ntwice(&mut a)\nprint(a).unwrap()", "[7, 1, 2]\n")]
#[case("def count(xs: &list[i64]) -> i64:\n    mut n = 0\n    for x in xs:\n        n = n + x\n    n\na = [1, 2, 3].unwrap()\nprint(count(&a)).unwrap()\nprint(a).unwrap()", "6\n[1, 2, 3]\n")]
#[case("def g() -> Generator[i64]:\n    yield 3\ndef advance(g: &mut Generator[i64]) -> Option[i64]:\n    next(g)\nmut it = g().unwrap()\nprint(advance(&mut it)).unwrap()\nprint(next(it)).unwrap()", "Option[i64].Some(3)\nOption[i64].Nothing\n")]
#[case("def g() -> Generator[i64]:\n    mut a = [1].unwrap()\n    r = &mut a\n    r.append(2).unwrap()\n    yield len(a)\n    yield 3\nprint(list(g().unwrap()).unwrap()).unwrap()", "[2, 3]\n")]
#[case(
    "mut a = [1].unwrap()\nr = &a\nif True:\n    print(r).unwrap()\nelse:\n    print(r).unwrap()\na.append(2).unwrap()\nprint(a).unwrap()",
    "[1]\n[1, 2]\n"
)]
#[case("mut a = [1].unwrap()\nmut n = 0\nwhile n < 3:\n    r = &a\n    print(len(r)).unwrap()\n    a.append(n).unwrap()\n    n = n + 1\nprint(a).unwrap()", "1\n2\n3\n[1, 0, 1, 2]\n")]
#[case(
    "mut a = [1, 2].unwrap()\na[len(a) - 1] = len(a) + 1\nprint(a).unwrap()",
    "[1, 3]\n"
)]
#[case("type R = Result[list[i64], str]\na = R.Ok([1].unwrap())\nb = copy(a).unwrap()\nmatch b:\n    case R.Ok(items):\n        mut x = items\n        x.append(2).unwrap()\n        print(x).unwrap()\n    case R.Err(_):\n        pass\nprint(a).unwrap()", "[1, 2]\nResult[list[i64], str].Ok([1])\n")]
#[case(
    "mut a: list[i64] = [].unwrap()\nfor n in range(20_000):\n    a.append(n).unwrap()\nprint(len(a)).unwrap()\nprint(a[-1]).unwrap()",
    "20000\n19999\n"
)]
#[case("type View = &list[i64]\ndef length(v: View) -> i64:\n    len(v)\na = [1, 2].unwrap()\nprint(length(&a)).unwrap()", "2\n")]
#[case(
    "mut text = 'old'\nr = &mut text\n*r = ('new' + ' text').unwrap()\nprint(text).unwrap()",
    "new text\n"
)]
#[case("mut x = 1\nr = (&mut x)\n*r = *(&r)\nprint(*r).unwrap()", "1\n")]
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
#[case("a = [1].unwrap()\nb = a\nprint(a).unwrap()", "moved")]
#[case(
    "mut a = [1].unwrap()\nr = &a\na.append(2).unwrap()\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\nprint(a).unwrap()\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &a\ndrop(a)\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &a\nb = a\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case("a = [1].unwrap()\nr = &mut a", "mut binding")]
#[case("mut a = [1].unwrap()\nr = &a\ns = &mut r", "shared reference")]
#[case(
    "def f(a: &mut list[i64], b: &list[i64]) -> ():\n    pass\nmut a = [1].unwrap()\nf(&mut a, &a)",
    "conflicting borrow"
)]
#[case("def f() -> &i64:\n    x = 1\n    &x", "returned references")]
#[case("r = &[1].unwrap()[0]", "named collection")]
#[case(
    "mut a = [1].unwrap()\nr = &a\nwhile len(a) > 0:\n    a.append(2).unwrap()\n    print(r).unwrap()",
    "conflicting borrow"
)]
#[case("a = [1].unwrap()\ndrop(a)\nprint(a).unwrap()", "moved")]
#[case(
    "def g() -> Generator[i64]:\n    a = [1].unwrap()\n    r = &a\n    yield 1\n    print(r).unwrap()",
    "across yield"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\ns = &r\nr.append(2).unwrap()\nprint(s).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nfor x in &a:\n    a.append(x).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &a\nif True:\n    a = [2].unwrap()\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &a\nwhile True:\n    print(r).unwrap()\n    a.append(2).unwrap()",
    "conflicting borrow"
)]
#[case(
    "def f(x: &list[i64], y: list[i64]) -> ():\n    pass\na = [1].unwrap()\nf(&a, a)",
    "conflicting borrow"
)]
#[case("def main() -> ():\n    x = 1\n    &x", "expected (), got &i64")]
#[case("x = 1\nprint(*x).unwrap()", "dereference requires")]
#[case("a = [1].unwrap()\nx = [&a].unwrap()", "cannot be stored")]
#[case(
    "def g() -> Generator[i64]:\n    yield 1\nmut it = g().unwrap()\nfor x in &it:\n    pass",
    "borrowed generator iteration"
)]
#[case(
    "def g(x: &i64) -> Generator[i64]:\n    yield *x",
    "capture references"
)]
#[case("x = 1\ndrop(&x)", "owned value")]
#[case(
    "s = ('a' + 'b').unwrap()\nfor n in range(2):\n    print(s).unwrap()\n    drop(s)",
    "loop backedge"
)]
#[case("n = 1\nwhile n > 0:\n    drop(n)", "loop backedge")]
#[case("s = ('a' + 'b').unwrap()\ndrop((s))\nprint(s).unwrap()", "moved")]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected}, got {error}"
    );
}
