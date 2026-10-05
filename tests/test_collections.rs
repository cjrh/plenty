//! Observable native behavior and compile-time contracts for collections.
mod support;
use rstest::rstest;

#[rstest]
#[case("print([1, 2, 3])\nprint(len([1, 2]))", "[1, 2, 3]\n2\n")]
#[case(
    "print([1i8, -2i8, 127i8])\nprint([255u8, 0u8])",
    "[1, -2, 127]\n[255, 0]\n"
)]
#[case("print([[1, 2], [3]])\nprint([[1], [2]][-1][0])", "[[1, 2], [3]]\n2\n")]
#[case("print({'a': 1, 'b': 2, 'a': 3})", "{\"a\": 3, \"b\": 2}\n")]
#[case(
    "print(len({1, 1, 2}))\nprint(2 in {1, 2})\nprint(3 not in {1, 2})",
    "2\nTrue\nTrue\n"
)]
#[case("xs: list[i64] = []\nd: dict[str, i64] = {}\ns: set[i64] = set()\nprint(xs)\nprint(d)\nprint(s)", "[]\n{}\nset()\n")]
#[case(
    "print(list[i32]())\nprint(dict[str, list[i64]]())\nprint(set[str]())",
    "[]\n{}\nset()\n"
)]
#[case(
    "type Values = list[i64]\ndef empty() -> Values:\n    []\nprint(empty())",
    "[]\n"
)]
#[case("type Values = list[Count]\ntype Count = i64\ndef twice(xs: Values) -> Values:\n    [x * 2 for x in xs]\nprint(twice([1, 2]))", "[2, 4]\n")]
#[case("def empty() -> list[i64]:\n    return []\nprint(empty())", "[]\n")]
#[case("def size(xs: list[i64]) -> i64:\n    len(xs)\nprint(size([]))", "0\n")]
#[case("xs: list[list[i64]] = [[], [1]]\nprint(xs)", "[[], [1]]\n")]
#[case("print([x * x for x in range(6) if x > 2])", "[9, 16, 25]\n")]
#[case(
    "print([x * 10 + y for x in range(3) for y in range(x) if y < 2])",
    "[10, 20, 21]\n"
)]
#[case("print([x for x in range(8) if x > 1 if x < 4])", "[2, 3]\n")]
#[case(
    "x = 10\nprint([x for x in range(x) if x < 2])\nprint(x)",
    "[0, 1]\n10\n"
)]
#[case("print({x: x * x for x in [3, 2, 3]})", "{3: 9, 2: 4}\n")]
#[case("print(len({x // 2 for x in range(10)}))", "5\n")]
#[case(
    "print([[x + y for y in range(2)] for x in range(3)])",
    "[[0, 1], [1, 2], [2, 3]]\n"
)]
#[case("print(100 + len([x for x in range(4)]))", "104\n")]
#[case(
    "mut total = 0\nfor n in range(5):\n    total = total + n\nprint(total)",
    "10\n"
)]
#[case(
    "for x in [1, 2]:\n    for y in [3, 4]:\n        print(x + y)",
    "4\n5\n5\n6\n"
)]
#[case("d = {'a': 1, 'b': 2}\nfor key in d:\n    print(key)\n    print(d[key])\nprint(d.keys())\nprint(d.values())", "a\n1\nb\n2\n[\"a\", \"b\"]\n[1, 2]\n")]
#[case(
    "for c in 'hé🙂':\n    print(c)\nprint(len('hé🙂'))\nprint('hé🙂'[-1])",
    "h\né\n🙂\n3\n🙂\n"
)]
#[case("print(list(range(5, -2, -2)))\nprint(len(range(5, 0)))\nprint(3 in range(1, 8, 2))\nprint(4 not in range(1, 8, 2))", "[5, 3, 1, -1]\n0\nTrue\nTrue\n")]
#[case("print(list(range(9223372036854775806, 9223372036854775807)))\nprint(list(range(0, -9223372036854775808, -9223372036854775808)))", "[9223372036854775806]\n[0]\n")]
#[case(
    "print(range(2, 8, 2)[-1])\nprint(range(3, 0) == range(0))",
    "6\nTrue\n"
)]
#[case(
    "print(list('hé'))\nprint(len(set([1, 1, 2])))\nprint(list({'a': 1, 'b': 2}))",
    "[\"h\", \"é\"]\n2\n[\"a\", \"b\"]\n"
)]
#[case(
    "mut a = [1, 2]\nmut b = a\nb.append(3)\nb[0] = 9\nprint(a)\nprint(b)",
    "[1, 2]\n[9, 2, 3]\n"
)]
#[case(
    "mut a = {'a': [1]}\nmut b = a\nb['a'] = [2]\nb['b'] = [3]\nprint(a)\nprint(b)",
    "{\"a\": [1]}\n{\"a\": [2], \"b\": [3]}\n"
)]
#[case(
    "mut a = {1}\nmut b = a\nb.add(2)\nprint(len(a))\nprint(len(b))",
    "1\n2\n"
)]
#[case(
    "mut xs = [1, 2]\nfor x in xs:\n    xs.append(x + 10)\nprint(xs)",
    "[1, 2, 11, 12]\n"
)]
#[case(
    "mut xs = [[1]]\nmut child = xs[0]\nchild.append(2)\nprint(xs)\nxs[0] = child\nprint(xs)",
    "[[1]]\n[[1, 2]]\n"
)]
#[case("def changed(xs: list[i64]) -> list[i64]:\n    mut result = xs\n    result.append(2)\n    result\na = [1]\nprint(changed(a))\nprint(a)", "[1, 2]\n[1]\n")]
#[case("print([1, 2] == [1, 2])\nprint([[1]] != [[2]])\nprint({'a': 1, 'b': 2} == {'b': 2, 'a': 1})\nprint({1, 2} == {2, 1})", "True\nTrue\nTrue\nTrue\n")]
#[case("print([1] in [[1], [2]])\nprint('é' in 'héllo')", "True\nTrue\n")]
#[case("def find(xs: list[i64]) -> i64:\n    for x in xs:\n        if x > 2:\n            return x\n    -1\nprint(find([1, 3, 4]))\nprint(find([]))", "3\n-1\n")]
#[case("def f(n: i64) -> i64:\n    for x in range(1):\n        if n > 0:\n            return f(n - 1)\n    42\nprint(f(100_000))", "42\n")]
#[case("def source() -> list[i64]:\n    print('source')\n    [1, 2]\ndef item(n: i64) -> i64:\n    print(n)\n    n\nprint([item(x) for x in source() if x > 1])", "source\n2\n[2]\n")]
#[case(
    "def key(n: i64) -> i64:\n    print(n)\n    n\nprint({key(1): key(2), key(1): key(3)})",
    "1\n2\n1\n3\n{1: 3}\n"
)]
#[case("xs = [\n    1,\n    # comment\n    2,\n]\nprint(xs)", "[1, 2]\n")]
#[case("print([1 if x > 0 else 0 for x in [-1, 1]])", "[0, 1]\n")]
fn native_collections(#[case] source: &str, #[case] expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}

#[test]
fn collections_preserve_typed_function_results() {
    support::assert_value("xs = [1i8, 2i8]\nxs[-1]", "i8", "2\n");
}

#[rstest]
#[case("xs = []", "empty collection needs")]
#[case("d = {}", "empty collection needs")]
#[case("s = set()", "empty collection needs")]
#[case("xs = [1, True]", "expected list[i64]")]
#[case("xs: list[i32] = [1]", "expected list[i32]")]
#[case("xs: list[()] = []", "cannot be unit")]
#[case("d: dict[list[i64], i64] = {}", "dictionary keys")]
#[case("s = {[1]}", "set elements")]
#[case("type X = list[X]", "cyclic type alias")]
#[case("type X = list[Missing]", "unknown type")]
#[case("xs: list = []", "requires 1 type")]
#[case("xs = [1]\nxs.append(2)", "immutable")]
#[case("xs = [1]\nxs[0] = 2", "immutable")]
#[case("mut xs = [1]\nxs.append(True)", "expected i64")]
#[case("mut xs = [1]\nxs[0] = True", "expected i64")]
#[case("print([1][False])", "expected i64")]
#[case("print({1, 2}[0])", "indexing requires")]
#[case("print(True in [1])", "expected i64")]
#[case("for x in 42:\n    pass", "requires an iterable")]
#[case("for x in [1]:\n    pass\nprint(x)", "unknown binding")]
#[case("xs = [x for x in [1]]\nprint(x)", "unknown binding")]
#[case("xs = [x for x in [1] if x]", "expected bool")]
#[case("def f(xs: list[i64]) -> ():\n    xs.append(1)", "immutable")]
#[case("print(range(True))", "expected i64")]
#[case("xs = [1}", "mismatched")]
#[case("print(len(1))", "requires an iterable")]
fn invalid_collections(#[case] source: &str, #[case] diagnostic: &str) {
    let error = plenty::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(diagnostic),
        "{source}\nexpected {diagnostic:?}, got {error:?}"
    );
}

#[rstest]
#[case("print([1][1])", "index out of bounds")]
#[case("print([1][-2])", "index out of bounds")]
#[case("print({'a': 1}['b'])", "dictionary key not found")]
#[case("print(range(0, 4, 0))", "range step cannot be zero")]
#[case(
    "print(len(range(-9223372036854775808, 9223372036854775807)))",
    "range length exceeds i64"
)]
fn runtime_errors(#[case] source: &str, #[case] diagnostic: &str) {
    let output = support::run(source);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
}

#[test]
fn large_builders_and_hash_tables() {
    let output = support::run("xs = [x for x in range(10_000)]\nd = {x: x + 1 for x in xs}\ns = set(xs)\nprint(len(xs))\nprint(d[9999])\nprint(9999 in s)");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"10000\n10000\nTrue\n");
}

#[rstest]
#[case("print([x for x in range(8) if x % 2 == 0])", "[0, 2, 4, 6]\n")]
#[case(
    "print(-7 % 3)\nprint(7 % -3)\nprint(-7 % -3)\nprint(-128i8 % -1i8)\nprint(255u8 % 2u8)",
    "2\n-2\n-1\n0\n1\n"
)]
#[case("print(list[i64]([1, 2]))\nprint(set[i64]([1, 1]))", "[1, 2]\n{1}\n")]
#[case("d = {True: 1, False: 2}\nprint(d[True])\nprint(d[False])\nprint({-128i8: 2}[-128i8])\nprint({18446744073709551615u64: 3}[18446744073709551615u64])", "1\n2\n2\n3\n")]
#[case("def f(xs: list[i64]) -> list[i64]:\n    if len(xs) == 0:\n        return []\n    [x * 2 for x in xs]\nprint(f([]))\nprint(f([2]))", "[]\n[4]\n")]
#[case(
    "type A = dict[str, B]\ntype B = list[i64]\nx: A = {'a': [1]}\nprint(x)",
    "{\"a\": [1]}\n"
)]
fn native_collection_edges(#[case] source: &str, #[case] expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn collection_context_does_not_bypass_name_shadowing() {
    let error = plenty::check_source("list = 1\nxs: list[i64] = list()").unwrap_err();
    assert!(error.to_string().contains("not callable"));
}

#[test]
fn collection_signatures_link_even_when_the_function_is_unused() {
    let output = support::run("def show(xs: list[i64]) -> ():\n    print(xs)\nprint(42)");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn collection_example_runs() {
    let output = support::run(include_str!("../examples/collections.plenty"));
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"[0, 4, 16, 36, 64]\n0\n16\n256\n1296\n4096\n5\n"
    );
}
