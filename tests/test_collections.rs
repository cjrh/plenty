//! Observable native behavior and compile-time contracts for collections.
mod support;
use rstest::rstest;

#[rstest]
#[case(
    "print([1, 2, 3].unwrap()).unwrap()\nprint(len([1, 2].unwrap())).unwrap()",
    "[1, 2, 3]\n2\n"
)]
#[case(
    "print([1i8, -2i8, 127i8].unwrap()).unwrap()\nprint([255u8, 0u8].unwrap()).unwrap()",
    "[1, -2, 127]\n[255, 0]\n"
)]
#[case("print([[1, 2].unwrap(), [3].unwrap()].unwrap()).unwrap()\nprint([[1].unwrap(), [2].unwrap()].unwrap()[-1][0]).unwrap()", "[[1, 2], [3]]\n2\n")]
#[case(
    "print({'a': 1, 'b': 2, 'a': 3}.unwrap()).unwrap()",
    "{\"a\": 3, \"b\": 2}\n"
)]
#[case(
    "print(len({1, 1, 2}.unwrap())).unwrap()\nprint(2 in {1, 2}.unwrap()).unwrap()\nprint(3 not in {1, 2}.unwrap()).unwrap()",
    "2\nTrue\nTrue\n"
)]
#[case("xs: list[i64] = [].unwrap()\nd: dict[str, i64] = {}.unwrap()\ns: set[i64] = set().unwrap()\nprint(xs).unwrap()\nprint(d).unwrap()\nprint(s).unwrap()", "[]\n{}\nset()\n")]
#[case(
    "print(list[i32]().unwrap()).unwrap()\nprint(dict[str, list[i64]]().unwrap()).unwrap()\nprint(set[str]().unwrap()).unwrap()",
    "[]\n{}\nset()\n"
)]
#[case(
    "type Values = list[i64]\ndef empty() -> Values:\n    [].unwrap()\nprint(empty()).unwrap()",
    "[]\n"
)]
#[case("type Values = list[Count]\ntype Count = i64\ndef twice(xs: Values) -> Values:\n    [x * 2 for x in xs].unwrap()\nprint(twice([1, 2].unwrap())).unwrap()", "[2, 4]\n")]
#[case(
    "def empty() -> list[i64]:\n    return [].unwrap()\nprint(empty()).unwrap()",
    "[]\n"
)]
#[case(
    "def size(xs: list[i64]) -> i64:\n    len(xs)\nprint(size([].unwrap())).unwrap()",
    "0\n"
)]
#[case(
    "xs: list[list[i64]] = [[].unwrap(), [1].unwrap()].unwrap()\nprint(xs).unwrap()",
    "[[], [1]]\n"
)]
#[case(
    "print([x * x for x in range(6).unwrap() if x > 2].unwrap()).unwrap()",
    "[9, 16, 25]\n"
)]
#[case(
    "print([x * 10 + y for x in range(3).unwrap() for y in range(x).unwrap() if y < 2].unwrap()).unwrap()",
    "[10, 20, 21]\n"
)]
#[case(
    "print([x for x in range(8).unwrap() if x > 1 if x < 4].unwrap()).unwrap()",
    "[2, 3]\n"
)]
#[case(
    "x = 10\nprint([x for x in range(x).unwrap() if x < 2].unwrap()).unwrap()\nprint(x).unwrap()",
    "[0, 1]\n10\n"
)]
#[case(
    "print({x: x * x for x in [3, 2, 3].unwrap()}.unwrap()).unwrap()",
    "{3: 9, 2: 4}\n"
)]
#[case(
    "print(len({x // 2 for x in range(10).unwrap()}.unwrap())).unwrap()",
    "5\n"
)]
#[case(
    "print([[x + y for y in range(2).unwrap()].unwrap() for x in range(3).unwrap()].unwrap()).unwrap()",
    "[[0, 1], [1, 2], [2, 3]]\n"
)]
#[case(
    "print(100 + len([x for x in range(4).unwrap()].unwrap())).unwrap()",
    "104\n"
)]
#[case(
    "mut total = 0\nfor n in range(5).unwrap():\n    total = total + n\nprint(total).unwrap()",
    "10\n"
)]
#[case(
    "for x in [1, 2].unwrap():\n    for y in [3, 4].unwrap():\n        print(x + y).unwrap()",
    "4\n5\n5\n6\n"
)]
#[case("d = {'a': 1, 'b': 2}.unwrap()\nfor key in &d:\n    print(key).unwrap()\n    print(d[key]).unwrap()\nprint(d.keys().unwrap()).unwrap()\nprint(d.values().unwrap()).unwrap()", "a\n1\nb\n2\n[\"a\", \"b\"]\n[1, 2]\n")]
#[case(
    "for c in 'hé🙂':\n    print(c.unwrap()).unwrap()\nprint(len('hé🙂')).unwrap()\nprint('hé🙂'[-1].unwrap()).unwrap()",
    "h\né\n🙂\n3\n🙂\n"
)]
#[case("print(list(range(5, -2, -2).unwrap()).unwrap()).unwrap()\nprint(len(range(5, 0).unwrap())).unwrap()\nprint(3 in range(1, 8, 2).unwrap()).unwrap()\nprint(4 not in range(1, 8, 2).unwrap()).unwrap()", "[5, 3, 1, -1]\n0\nTrue\nTrue\n")]
#[case("print(list(range(9223372036854775806, 9223372036854775807).unwrap()).unwrap()).unwrap()\nprint(list(range(0, -9223372036854775808, -9223372036854775808).unwrap()).unwrap()).unwrap()", "[9223372036854775806]\n[0]\n")]
#[case(
    "print(range(2, 8, 2).unwrap()[-1]).unwrap()\nprint(range(3, 0).unwrap() == range(0).unwrap()).unwrap()",
    "6\nTrue\n"
)]
#[case(
    "print([c.unwrap() for c in 'hé'].unwrap()).unwrap()\nprint(len(set([1, 1, 2].unwrap()).unwrap())).unwrap()\nprint(list({'a': 1, 'b': 2}.unwrap()).unwrap()).unwrap()",
    "[\"h\", \"é\"]\n2\n[\"a\", \"b\"]\n"
)]
#[case(
    "mut a = [1, 2].unwrap()\nmut b = copy(a).unwrap()\nb.append(3).unwrap()\nb[0] = 9\nprint(a).unwrap()\nprint(b).unwrap()",
    "[1, 2]\n[9, 2, 3]\n"
)]
#[case(
    "mut a = {'a': [1].unwrap()}.unwrap()\nmut b = copy(a).unwrap()\nb['a'] = [2].unwrap()\nb.insert('b', [3].unwrap()).unwrap()\nprint(a).unwrap()\nprint(b).unwrap()",
    "{\"a\": [1]}\n{\"a\": [2], \"b\": [3]}\n"
)]
#[case(
    "mut a = {1}.unwrap()\nmut b = copy(a).unwrap()\nb.add(2).unwrap()\nprint(len(a)).unwrap()\nprint(len(b)).unwrap()",
    "1\n2\n"
)]
#[case(
    "mut xs = [1, 2].unwrap()\nfor x in copy(xs).unwrap():\n    xs.append(x + 10).unwrap()\nprint(xs).unwrap()",
    "[1, 2, 11, 12]\n"
)]
#[case(
    "mut xs = [[1].unwrap()].unwrap()\nmut child = copy(xs[0]).unwrap()\nchild.append(2).unwrap()\nprint(xs).unwrap()\nxs[0] = child\nprint(xs).unwrap()",
    "[[1]]\n[[1, 2]]\n"
)]
#[case("def changed(xs: list[i64]) -> list[i64]:\n    mut result = xs\n    result.append(2).unwrap()\n    result\na = [1].unwrap()\nprint(changed(copy(a).unwrap())).unwrap()\nprint(a).unwrap()", "[1, 2]\n[1]\n")]
#[case("print([1, 2].unwrap() == [1, 2].unwrap()).unwrap()\nprint([[1].unwrap()].unwrap() != [[2].unwrap()].unwrap()).unwrap()\nprint({'a': 1, 'b': 2}.unwrap() == {'b': 2, 'a': 1}.unwrap()).unwrap()\nprint({1, 2}.unwrap() == {2, 1}.unwrap()).unwrap()", "True\nTrue\nTrue\nTrue\n")]
#[case("print([1].unwrap() in [[1].unwrap(), [2].unwrap()].unwrap()).unwrap()\nprint('é' in 'héllo').unwrap()", "True\nTrue\n")]
#[case("def find(xs: list[i64]) -> i64:\n    for x in xs:\n        if x > 2:\n            return x\n    -1\nprint(find([1, 3, 4].unwrap())).unwrap()\nprint(find([].unwrap())).unwrap()", "3\n-1\n")]
#[case("def f(n: i64) -> i64:\n    for x in range(1).unwrap():\n        if n > 0:\n            return f(n - 1)\n    42\nprint(f(100_000)).unwrap()", "42\n")]
#[case("def source() -> list[i64]:\n    print('source').unwrap()\n    [1, 2].unwrap()\ndef item(n: i64) -> i64:\n    print(n).unwrap()\n    n\nprint([item(x) for x in source() if x > 1].unwrap()).unwrap()", "source\n2\n[2]\n")]
#[case(
    "def key(n: i64) -> i64:\n    print(n).unwrap()\n    n\nprint({key(1): key(2), key(1): key(3)}.unwrap()).unwrap()",
    "1\n2\n1\n3\n{1: 3}\n"
)]
#[case(
    "xs = [\n    1,\n    # comment\n    2,\n].unwrap()\nprint(xs).unwrap()",
    "[1, 2]\n"
)]
#[case(
    "print([1 if x > 0 else 0 for x in [-1, 1].unwrap()].unwrap()).unwrap()",
    "[0, 1]\n"
)]
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
    support::assert_value("xs = [1i8, 2i8].unwrap()\nxs[-1]", "i8", "2\n");
}

#[rstest]
#[case("xs = [].unwrap()", "empty collection needs")]
#[case("d = {}.unwrap()", "empty collection needs")]
#[case("s = set().unwrap()", "empty collection needs")]
#[case("xs = [1, True].unwrap()", "expected list[i64]")]
#[case("xs: list[i32] = [1i64].unwrap()", "expected list[i32]")]
#[case("xs: list[()] = [].unwrap()", "cannot be unit")]
#[case("d: dict[list[i64], i64] = {}.unwrap()", "dictionary keys")]
#[case("s = {[1].unwrap()}.unwrap()", "set elements")]
#[case("type X = list[X]", "cyclic type alias")]
#[case("type X = list[Missing]", "unknown type")]
#[case("xs: list = [].unwrap()", "requires 1 type")]
#[case("xs = [1].unwrap()\nxs.append(2).unwrap()", "immutable")]
#[case("xs = [1].unwrap()\nxs[0] = 2", "immutable")]
#[case("mut xs = [1].unwrap()\nxs.append(True).unwrap()", "expected i64")]
#[case("mut xs = [1].unwrap()\nxs[0] = True", "expected i64")]
#[case("print([1].unwrap()[False]).unwrap()", "expected i64")]
#[case("print({1, 2}.unwrap()[0]).unwrap()", "indexing requires")]
#[case("print(True in [1].unwrap()).unwrap()", "expected i64")]
#[case("for x in 42:\n    pass", "requires an iterable")]
#[case(
    "for x in [1].unwrap():\n    pass\nprint(x).unwrap()",
    "unknown binding"
)]
#[case(
    "xs = [x for x in [1].unwrap()].unwrap()\nprint(x).unwrap()",
    "unknown binding"
)]
#[case("xs = [x for x in [1].unwrap() if x].unwrap()", "expected bool")]
#[case("def f(xs: list[i64]) -> ():\n    xs.append(1).unwrap()", "immutable")]
#[case("print(range(True).unwrap()).unwrap()", "expected i64")]
#[case("xs = [1}.unwrap()", "mismatched")]
#[case("print(len(1)).unwrap()", "requires an iterable")]
fn invalid_collections(#[case] source: &str, #[case] diagnostic: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(diagnostic),
        "{source}\nexpected {diagnostic:?}, got {error:?}"
    );
}

#[rstest]
#[case("print([1].unwrap()[1]).unwrap()", "index out of bounds")]
#[case("print([1].unwrap()[-2]).unwrap()", "index out of bounds")]
#[case("print({'a': 1}.unwrap()['b']).unwrap()", "dictionary key not found")]
#[case("print(range(0, 4, 0).unwrap()).unwrap()", "range step cannot be zero")]
#[case(
    "print(len(range(-9223372036854775808, 9223372036854775807).unwrap())).unwrap()",
    "range length exceeds i64"
)]
fn runtime_errors(#[case] source: &str, #[case] diagnostic: &str) {
    let output = support::run(source);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
}

#[test]
fn large_builders_and_hash_tables() {
    let output = support::run("xs = [x for x in range(10_000).unwrap()].unwrap()\nd = {x: x + 1 for x in &xs}.unwrap()\ns = set(copy(xs).unwrap()).unwrap()\nprint(len(xs)).unwrap()\nprint(d[9999]).unwrap()\nprint(9999 in s).unwrap()");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"10000\n10000\nTrue\n");
}

#[rstest]
#[case(
    "print([x for x in range(8).unwrap() if x % 2 == 0].unwrap()).unwrap()",
    "[0, 2, 4, 6]\n"
)]
#[case(
    "print(-7 % 3).unwrap()\nprint(7 % -3).unwrap()\nprint(-7 % -3).unwrap()\nprint(-128i8 % -1i8).unwrap()\nprint(255u8 % 2u8).unwrap()",
    "2\n-2\n-1\n0\n1\n"
)]
#[case("print(list[i64]([1, 2].unwrap()).unwrap()).unwrap()\nprint(set[i64]([1, 1].unwrap()).unwrap()).unwrap()", "[1, 2]\n{1}\n")]
#[case("d = {True: 1, False: 2}.unwrap()\nprint(d[True]).unwrap()\nprint(d[False]).unwrap()\nprint({-128i8: 2}.unwrap()[-128i8]).unwrap()\nprint({18446744073709551615u64: 3}.unwrap()[18446744073709551615u64]).unwrap()", "1\n2\n2\n3\n")]
#[case("def f(xs: list[i64]) -> list[i64]:\n    if len(xs) == 0:\n        return [].unwrap()\n    [x * 2 for x in xs].unwrap()\nprint(f([].unwrap())).unwrap()\nprint(f([2].unwrap())).unwrap()", "[]\n[4]\n")]
#[case(
    "type A = dict[str, B]\ntype B = list[i64]\nx: A = {'a': [1].unwrap()}.unwrap()\nprint(x).unwrap()",
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
    let error = support::check_source("list = 1\nxs: list[i64] = list().unwrap()").unwrap_err();
    assert!(error.to_string().contains("not callable"));
}

#[test]
fn collection_signatures_link_even_when_the_function_is_unused() {
    let output =
        support::run("def show(xs: list[i64]) -> ():\n    print(xs).unwrap()\nprint(42).unwrap()");
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
