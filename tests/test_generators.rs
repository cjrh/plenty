//! Laziness, suspension, affine ownership and destruction through native code.
mod support;
use rstest::rstest;
use std::process::Command;

const COUNT: &str = "def count(n: i64) -> Generator[i64]:\n    mut i = 0\n    while i < n:\n        yield i\n        i = i + 1\n";
fn run(source: &str) -> std::process::Output {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    Command::new(executable).output().unwrap()
}

#[rstest]
#[case("def g(n: i64) -> Generator[i64]:\n    print('start').unwrap()\n    yield n\n    print('after').unwrap()\ndef arg() -> i64:\n    print('arg').unwrap()\n    42\nmut it = g(arg()).unwrap()\nprint('created').unwrap()\nprint(next(it)).unwrap()\nprint('stop').unwrap()", "arg\ncreated\nstart\nOption[i64].Some(42)\nstop\n")]
#[case(
    "def g() -> Generator[str]:\n    print('never').unwrap()\n    yield 'x'\ng().unwrap()\nprint('done').unwrap()",
    "done\n"
)]
#[case(
    "mut g = count(2).unwrap()\nprint(next(g)).unwrap()\nprint(next(g)).unwrap()\nprint(next(g)).unwrap()\nprint(next(g)).unwrap()",
    "Option[i64].Some(0)\nOption[i64].Some(1)\nOption[i64].Nothing\nOption[i64].Nothing\n"
)]
#[case("def empty() -> Generator[i64]:\n    if False:\n        yield 1\nmut g = empty().unwrap()\nprint(next(g)).unwrap()\nprint(next(g)).unwrap()\nprint(list(empty().unwrap()).unwrap()).unwrap()", "Option[i64].Nothing\nOption[i64].Nothing\n[]\n")]
#[case(
    "def g() -> Generator[i64]:\n    yield 1\n    return\nfor x in g().unwrap():\n    print(x).unwrap()",
    "1\n"
)]
#[case("def g(flag: bool) -> Generator[i64]:\n    if flag:\n        yield 1\n        yield 2\n    else:\n        yield 3\n    yield 4\nprint(list(g(True).unwrap()).unwrap()).unwrap()\nprint(list(g(False).unwrap()).unwrap()).unwrap()", "[1, 2, 4]\n[3, 4]\n")]
#[case("print([x * x for x in count(6).unwrap() if x % 2 == 1].unwrap()).unwrap()\nprint({x % 2 for x in count(6).unwrap()}.unwrap()).unwrap()\nprint({x: x + 1 for x in count(3).unwrap()}.unwrap()).unwrap()", "[1, 9, 25]\n{0, 1}\n{0: 1, 1: 2, 2: 3}\n")]
#[case(
    "print([x * 10 + y for x in count(3).unwrap() for y in count(x).unwrap()].unwrap()).unwrap()",
    "[10, 20, 21]\n"
)]
#[case("for n in count(100).unwrap():\n    if n == 1:\n        continue\n    if n == 3:\n        break\n    print(n).unwrap()", "0\n2\n")]
#[case("def g() -> Generator[i64]:\n    for n in range(5):\n        if n == 1:\n            continue\n        if n == 3:\n            break\n        yield n\n    yield 99\nprint(list(g().unwrap()).unwrap()).unwrap()", "[0, 2, 99]\n")]
#[case("def g() -> Generator[i64]:\n    for a in count(3).unwrap():\n        for b in count(2).unwrap():\n            yield a * 10 + b\nprint(list(g().unwrap()).unwrap()).unwrap()", "[0, 1, 10, 11, 20, 21]\n")]
#[case("enum E:\n    A(i64)\n    B\ndef g(e: E) -> Generator[i64]:\n    match e:\n        case E.A(n):\n            yield n\n            yield n + 1\n        case E.B:\n            return\n    yield 9\nprint(list(g(E.A(2).unwrap()).unwrap()).unwrap()).unwrap()\nprint(list(g((E.B).unwrap()).unwrap()).unwrap()).unwrap()", "[2, 3, 9]\n[]\n")]
#[case("def g() -> Generator[list[str]]:\n    mut xs = [('a' + '\\0b').unwrap()].unwrap()\n    yield copy(xs).unwrap()\n    xs.append('c').unwrap()\n    yield xs\nprint(list(g().unwrap()).unwrap()).unwrap()", "[[\"a\\0b\"], [\"a\\0b\", \"c\"]]\n")]
#[case("def g() -> Generator[Option[str]]:\n    yield Option[str].Some(('hello' + ' world').unwrap())\n    yield Option[str].Nothing\nprint(list(g().unwrap()).unwrap()).unwrap()", "[Option[str].Some(\"hello world\"), Option[str].Nothing]\n")]
#[case(
    "def forward(n: i64) -> Generator[i64]:\n    count(n).unwrap()\nprint(list(forward(3)).unwrap()).unwrap()",
    "[0, 1, 2]\n"
)]
#[case("def forward(g: Generator[i64]) -> Generator[i64]:\n    return g\na = count(2).unwrap()\nb = a\nprint(list(forward(b)).unwrap()).unwrap()", "[0, 1]\n")]
#[case("def take(g: Generator[i64]) -> i64:\n    for x in g:\n        return x\n    -1\nprint(take(count(100).unwrap())).unwrap()\nprint(take(count(0).unwrap())).unwrap()", "0\n-1\n")]
#[case("mut g = count(1).unwrap()\nfor i in range(3):\n    print(list(g).unwrap()).unwrap()\n    g = count(i + 2).unwrap()\nprint(list(g).unwrap()).unwrap()", "[0]\n[0, 1]\n[0, 1, 2]\n[0, 1, 2, 3]\n")]
#[case("def choose(flag: bool, g: Generator[i64]) -> i64:\n    if flag:\n        print(list(g).unwrap()).unwrap()\n        return 1\n    print(list(g).unwrap()).unwrap()\n    2\nprint(choose(False, count(2).unwrap())).unwrap()", "[0, 1]\n2\n")]
#[case(
    "mut sum = 0\nfor x in count(100_000).unwrap():\n    sum = sum + x\nprint(sum).unwrap()",
    "4999950000\n"
)]
#[case(
    "def chars() -> Generator[str]:\n    for c in 'é\\0😀':\n        yield c.unwrap()\nprint(list(chars().unwrap()).unwrap()).unwrap()",
    "[\"é\", \"\\0\", \"😀\"]\n"
)]
#[case("def g(x: i8, y: u64, flag: bool) -> Generator[i8]:\n    if flag and y > 0u64:\n        yield x\n        yield x + 1i8\nprint(list(g(-128i8, 18446744073709551615u64, True).unwrap()).unwrap()).unwrap()", "[-128, -127]\n")]
#[case("def wrapped(g: Generator[i64]) -> Generator[i64]:\n    for x in g:\n        yield x\nprint(list(wrapped(count(2).unwrap()).unwrap()).unwrap()).unwrap()", "[0, 1]\n")]
fn native_generators(#[case] source: &str, #[case] expected: &str) {
    let output = run(&format!("{COUNT}{source}"));
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected.as_bytes(), "{source}");
}

#[rstest]
#[case("yield 1", "cannot be a generator")]
#[case("def g() -> i64:\n    yield 1", "yield requires")]
#[case("def g() -> Generator[i64]:\n    yield True", "expected i64, got bool")]
#[case("def g() -> Generator[i64]:\n    yield 1\n    return 2", "bare return")]
#[case("def g() -> Generator[()]:\n    yield ()", "cannot be unit")]
#[case(
    "def g() -> Generator[Generator[i64]]:\n    yield count(1).unwrap()",
    "cannot yield generators"
)]
#[case(
    "a = count(2).unwrap()\nb = a\nlist(a).unwrap()",
    "moved or possibly moved"
)]
#[case(
    "def eat(g: Generator[i64]) -> ():\n    pass\na = count(2).unwrap()\neat(a)\nlist(a).unwrap()",
    "moved or possibly moved"
)]
#[case(
    "a = count(2).unwrap()\nfor n in a:\n    break\nlist(a).unwrap()",
    "moved or possibly moved"
)]
#[case(
    "a = count(2).unwrap()\nif True:\n    list(a).unwrap()\nlist(a).unwrap()",
    "moved or possibly moved"
)]
#[case(
    "a = count(2).unwrap()\nfor n in range(2):\n    list(a).unwrap()",
    "loop backedge"
)]
#[case(
    "a = count(2).unwrap()\nwhile True:\n    list(a).unwrap()\n    continue",
    "loop backedge"
)]
#[case(
    "a = count(2).unwrap()\nwhile True:\n    list(a).unwrap()\n    break\nlist(a).unwrap()",
    "moved or possibly moved"
)]
#[case("a = count(2).unwrap()\nnext(a)", "mutable generator binding")]
#[case("next(count(2).unwrap())", "named mutable generator")]
#[case("mut a = 1\nnext(a)", "requires a generator")]
#[case("print(count(2).unwrap()).unwrap()", "cannot be printed")]
#[case("len(count(2).unwrap())", "len requires")]
#[case("1 in count(2).unwrap()", "membership does not consume")]
#[case("count(2).unwrap() == count(2).unwrap()", "binary operators")]
#[case("[count(2).unwrap()].unwrap()", "cannot be stored in collections")]
#[case("{'a': count(2).unwrap()}.unwrap()", "cannot be stored in collections")]
#[case(
    "type G = Generator[i64]\nx: list[G] = [].unwrap()",
    "cannot be stored in collections"
)]
#[case("enum E:\n    A(Generator[i64])", "cannot be stored in enum")]
#[case(
    "x: list[Option[Generator[i64]]] = [].unwrap()",
    "cannot be stored in collections"
)]
#[case(
    "a = count(2).unwrap()\n[x for n in range(2) for x in a].unwrap()",
    "loop backedge"
)]
#[case("def condition(g: Generator[i64]) -> bool:\n    True\na = count(2).unwrap()\nwhile condition(a):\n    pass", "loop backedge")]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(&format!("{COUNT}{source}"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected:?}, got {error:?}"
    );
}
