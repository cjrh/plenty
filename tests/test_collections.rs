//! Observable native behavior and compile-time contracts for collections.
mod support;
use rstest::rstest;

#[cfg(feature = "runtime-checks")]
#[test]
fn first_hash_and_keyed_collection_operations_allocate_nothing_after_reserve() {
    let output = support::run(
        r#"
mut d = dict[str, i64]().unwrap()
d.reserve(16).unwrap()
keys = ["a", "q", "A", "Q", "1", "!"].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
for key in keys:
    d.insert(key, len(d)).unwrap()
removed = d.pop("q")
d.insert("q", 99).unwrap()
print("__test_restore_allocations__").unwrap()
print(removed).unwrap()
print(d).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        visible,
        "Option[i64].Some(1)\n{\"a\": 0, \"A\": 2, \"Q\": 3, \"1\": 4, \"!\": 5, \"q\": 99}"
    );
}

#[rstest]
#[case(
    "print([1, 2, 3].unwrap()).unwrap()\nprint(len([1, 2].unwrap())).unwrap()",
    "[1, 2, 3]\n2\n"
)]
#[case(
    "print([1i8, -2i8, 127i8].unwrap()).unwrap()\nprint([255u8, 0u8].unwrap()).unwrap()",
    "[1, -2, 127]\n[255, 0]\n"
)]
#[case(
    "print([[1, 2].unwrap(), [3].unwrap()].unwrap()).unwrap()\nprint([[1].unwrap(), [2].unwrap()].unwrap()[-1][0]).unwrap()",
    "[[1, 2], [3]]\n2\n"
)]
#[case(
    "print({'a': 1, 'b': 2, 'a': 3}.unwrap()).unwrap()",
    "{\"a\": 3, \"b\": 2}\n"
)]
#[case(
    "print(len({1, 1, 2}.unwrap())).unwrap()\nprint(2 in {1, 2}.unwrap()).unwrap()\nprint(3 not in {1, 2}.unwrap()).unwrap()",
    "2\nTrue\nTrue\n"
)]
#[case(
    "xs: list[i64] = [].unwrap()\nd: dict[str, i64] = {}.unwrap()\ns: set[i64] = set().unwrap()\nprint(xs).unwrap()\nprint(d).unwrap()\nprint(s).unwrap()",
    "[]\n{}\nset()\n"
)]
#[case(
    "print(list[i32]().unwrap()).unwrap()\nprint(dict[str, list[i64]]().unwrap()).unwrap()\nprint(set[str]().unwrap()).unwrap()",
    "[]\n{}\nset()\n"
)]
#[case(
    "type Values = list[i64]\ndef empty() -> Values:\n    [].unwrap()\nprint(empty()).unwrap()",
    "[]\n"
)]
#[case(
    "type Values = list[Count]\ntype Count = i64\ndef twice(xs: Values) -> Result[Values, OverflowError[i64]]:\n    Ok([x * 2 for x in xs].unwrap())\ndef main() -> Result[(), OverflowError[i64]]:\n    print(twice([1, 2].unwrap())?).unwrap()\n    Ok(())\n",
    "[2, 4]\n"
)]
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
    "\ndef main() -> Result[(), OverflowError[i64]]:\n    print([x * x for x in range(6) if x > 2].unwrap()).unwrap()\n    Ok(())\n",
    "[9, 16, 25]\n"
)]
#[case(
    "\ndef main() -> Result[(), OverflowError[i64]]:\n    print([x * 10 + y for x in range(3) for y in range(x) if y < 2].unwrap()).unwrap()\n    Ok(())\n",
    "[10, 20, 21]\n"
)]
#[case(
    "print([x for x in range(8) if x > 1 if x < 4].unwrap()).unwrap()",
    "[2, 3]\n"
)]
#[case(
    "x = 10\nprint([x for x in range(x) if x < 2].unwrap()).unwrap()\nprint(x).unwrap()",
    "[0, 1]\n10\n"
)]
#[case(
    "\ndef main() -> Result[(), OverflowError[i64]]:\n    print({x: x * x for x in [3, 2, 3].unwrap()}.unwrap()).unwrap()\n    Ok(())\n",
    "{3: 9, 2: 4}\n"
)]
#[case(
    "\ndef main() -> Result[(), OverflowError[i64] | ZeroDivisionError]:\n    print(len({x // 2 for x in range(10)}.unwrap())).unwrap()\n    Ok(())\n",
    "5\n"
)]
#[case(
    "\ndef main() -> Result[(), OverflowError[i64]]:\n    print([[x + y for y in range(2)].unwrap() for x in range(3)].unwrap()).unwrap()\n    Ok(())\n",
    "[[0, 1], [1, 2], [2, 3]]\n"
)]
#[case(
    "\ndef main() -> Result[(), OverflowError[i64]]:\n    print(100 + len([x for x in range(4)].unwrap())).unwrap()\n    Ok(())\n",
    "104\n"
)]
#[case(
    "\ndef main() -> Result[(), OverflowError[i64]]:\n    mut total = 0\n    for n in range(5):\n        total = total + n\n    print(total).unwrap()\n    Ok(())\n",
    "10\n"
)]
#[case(
    "\ndef main() -> Result[(), OverflowError[i64]]:\n    for x in [1, 2].unwrap():\n        for y in [3, 4].unwrap():\n            print(x + y).unwrap()\n    Ok(())\n",
    "4\n5\n5\n6\n"
)]
#[case(
    "d = {'a': 1, 'b': 2}.unwrap()\nfor key in &d:\n    print(key).unwrap()\n    print(d[key]).unwrap()\nprint(d.keys_snapshot().unwrap()).unwrap()\nprint(d.values_snapshot().unwrap()).unwrap()",
    "a\n1\nb\n2\n[\"a\", \"b\"]\n[1, 2]\n"
)]
#[case(
    "for c in 'hé🙂':\n    print(c).unwrap()\nprint(len('hé🙂')).unwrap()\nprint('hé🙂'[-1]).unwrap()",
    "h\né\n🙂\n3\n🙂\n"
)]
#[case(
    "print(list(range(5, -2, -2)).unwrap()).unwrap()\nprint(len(range(5, 0))).unwrap()\nprint(3 in range(1, 8, 2)).unwrap()\nprint(4 not in range(1, 8, 2)).unwrap()",
    "[5, 3, 1, -1]\n0\nTrue\nTrue\n"
)]
#[case(
    "print(list(range(9223372036854775806, 9223372036854775807)).unwrap()).unwrap()\nprint(list(range(0, -9223372036854775808, -9223372036854775808)).unwrap()).unwrap()",
    "[9223372036854775806]\n[0]\n"
)]
#[case(
    "print(range(2, 8, 2)[-1]).unwrap()\nprint(range(3, 0) == range(0)).unwrap()",
    "6\nTrue\n"
)]
#[case(
    "print([c for c in 'hé'].unwrap()).unwrap()\nprint(len(set([1, 1, 2].unwrap()).unwrap())).unwrap()\nprint(list({'a': 1, 'b': 2}.unwrap()).unwrap()).unwrap()",
    "[\"h\", \"é\"]\n2\n[\"a\", \"b\"]\n"
)]
#[case(
    "mut a = [1, 2].unwrap()\nmut b = copy(a).unwrap()\nb.append(3).unwrap()\nb[0] = 9\nprint(a).unwrap()\nprint(b).unwrap()",
    "[1, 2]\n[9, 2, 3]\n"
)]
#[case(
    "mut a = {'a': [1].unwrap()}.unwrap()\nmut b = copy(a).unwrap()\nb.insert('a', [2].unwrap()).unwrap()\nb.insert('b', [3].unwrap()).unwrap()\nprint(a).unwrap()\nprint(b).unwrap()",
    "{\"a\": [1]}\n{\"a\": [2], \"b\": [3]}\n"
)]
#[case(
    "mut a = {1}.unwrap()\nmut b = copy(a).unwrap()\nb.add(2).unwrap()\nprint(len(a)).unwrap()\nprint(len(b)).unwrap()",
    "1\n2\n"
)]
#[case(
    "\ndef main() -> Result[(), OverflowError[i64]]:\n    mut xs = [1, 2].unwrap()\n    for x in copy(xs).unwrap():\n        xs.append(x + 10).unwrap()\n    print(xs).unwrap()\n    Ok(())\n",
    "[1, 2, 11, 12]\n"
)]
#[case(
    "mut xs = [[1].unwrap()].unwrap()\nmut child = copy(xs[0]).unwrap()\nchild.append(2).unwrap()\nprint(xs).unwrap()\nxs[0] = child\nprint(xs).unwrap()",
    "[[1]]\n[[1, 2]]\n"
)]
#[case(
    "def changed(xs: list[i64]) -> list[i64]:\n    mut result = xs\n    result.append(2).unwrap()\n    result\na = [1].unwrap()\nprint(changed(copy(a).unwrap())).unwrap()\nprint(a).unwrap()",
    "[1, 2]\n[1]\n"
)]
#[case(
    "print([1, 2].unwrap() == [1, 2].unwrap()).unwrap()\nprint([[1].unwrap()].unwrap() != [[2].unwrap()].unwrap()).unwrap()\nprint({'a': 1, 'b': 2}.unwrap() == {'b': 2, 'a': 1}.unwrap()).unwrap()\nprint({1, 2}.unwrap() == {2, 1}.unwrap()).unwrap()",
    "True\nTrue\nTrue\nTrue\n"
)]
#[case(
    "print([1].unwrap() in [[1].unwrap(), [2].unwrap()].unwrap()).unwrap()\nprint('é' in 'héllo').unwrap()",
    "True\nTrue\n"
)]
#[case(
    "def find(xs: list[i64]) -> i64:\n    for x in xs:\n        if x > 2:\n            return x\n    -1\nprint(find([1, 3, 4].unwrap())).unwrap()\nprint(find([].unwrap())).unwrap()",
    "3\n-1\n"
)]
#[case(
    "def f(n: i64) -> Result[i64, OverflowError[i64]]:\n    for x in range(1):\n        if n > 0:\n            return f(n - 1)\n    Ok(42)\ndef main() -> Result[(), OverflowError[i64]]:\n    print(f(100_000)?).unwrap()\n    Ok(())\n",
    "42\n"
)]
#[case(
    "def source() -> list[i64]:\n    print('source').unwrap()\n    [1, 2].unwrap()\ndef item(n: i64) -> i64:\n    print(n).unwrap()\n    n\nprint([item(x) for x in source() if x > 1].unwrap()).unwrap()",
    "source\n2\n[2]\n"
)]
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
#[case("print(range(True)).unwrap()", "expected i64")]
#[case("xs = [1}.unwrap()", "mismatched")]
#[case("print(len(1)).unwrap()", "len() is not defined for `i64`")]
#[case("print(range(0, 4, 0)).unwrap()", "a NonZero value cannot be zero")]
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
#[case(
    "print(len(range(-9223372036854775808, 9223372036854775807))).unwrap()",
    "range length exceeds i64"
)]
#[case(
    "print(len(range(-1, 9223372036854775807))).unwrap()",
    "range length exceeds i64"
)]
#[case(
    "print(len(range(9223372036854775807, -2, -1))).unwrap()",
    "range length exceeds i64"
)]
fn runtime_errors(#[case] source: &str, #[case] diagnostic: &str) {
    let output = support::run(source);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
}

/// A range stepping up from a nonnegative start, or over a narrow type,
/// needs no length check (issue #721).
#[test]
fn ranges_that_cannot_exceed_i64_lengths() {
    support::assert_stdout(
        "print(len(range(0, 9223372036854775807))).unwrap()\nprint(len(range(3, 9223372036854775807, 2))).unwrap()\nprint(len(range(-2147483648i32, 2147483647i32))).unwrap()",
        "9223372036854775807\n4611686018427387902\n4294967295\n",
    );
}

#[test]
fn large_builders_and_hash_tables() {
    let output = support::run(
        "\ndef main() -> Result[(), OverflowError[i64]]:\n    xs = [x for x in range(10_000)].unwrap()\n    d = {*x: x + 1 for x in &xs}.unwrap()\n    s = set(copy(xs).unwrap()).unwrap()\n    print(len(xs)).unwrap()\n    print(d[9999]).unwrap()\n    print(9999 in s).unwrap()\n    Ok(())\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"10000\n10000\nTrue\n");
}

#[rstest]
#[case(
    "\ndef main() -> Result[(), ZeroDivisionError]:\n    print([x for x in range(8) if x % 2 == 0].unwrap()).unwrap()\n    Ok(())\n",
    "[0, 2, 4, 6]\n"
)]
#[case(
    "\ndef main() -> Result[(), ZeroDivisionError]:\n    print(-7 % 3).unwrap()\n    print(7 % -3).unwrap()\n    print(-7 % -3).unwrap()\n    print(-128i8 % -1i8).unwrap()\n    print(255u8 % 2u8).unwrap()\n    Ok(())\n",
    "2\n-2\n-1\n0\n1\n"
)]
#[case(
    "print(list[i64]([1, 2].unwrap()).unwrap()).unwrap()\nprint(set[i64]([1, 1].unwrap()).unwrap()).unwrap()",
    "[1, 2]\n{1}\n"
)]
#[case(
    "d = {True: 1, False: 2}.unwrap()\nprint(d[True]).unwrap()\nprint(d[False]).unwrap()\nprint({-128i8: 2}.unwrap()[-128i8]).unwrap()\nprint({18446744073709551615u64: 3}.unwrap()[18446744073709551615u64]).unwrap()",
    "1\n2\n2\n3\n"
)]
#[case(
    "def f(xs: list[i64]) -> Result[list[i64], OverflowError[i64]]:\n    if len(xs) == 0:\n        return Ok([].unwrap())\n    Ok([x * 2 for x in xs].unwrap())\ndef main() -> Result[(), OverflowError[i64]]:\n    print(f([].unwrap())?).unwrap()\n    print(f([2].unwrap())?).unwrap()\n    Ok(())\n",
    "[]\n[4]\n"
)]
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

#[rstest]
#[case(
    "xs = [1, 2].unwrap()\nd = dict(xs)",
    "dict conversion requires a dictionary",
    "hashable"
)]
#[case(
    "xs = [1.0, 2.0].unwrap()\ns = set(xs).unwrap()",
    "set elements must be hashable",
    "dict"
)]
fn set_and_dict_conversion_errors_name_their_own_cause(
    #[case] source: &str,
    #[case] expected: &str,
    #[case] unrelated: &str,
) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
    assert!(!error.contains(unrelated), "{error}");
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

#[test]
fn len_calls_shared_methods_on_std_containers_and_user_types() {
    let output = support::run(
        r#"
from std.deque import Deque
import std.bytes

class Sized[T]:
    value: T
    def len(self) -> i64:
        7
    def __del__(self) -> ():
        print("drop sized").unwrap()

enum Size:
    Small
    def len(self) -> i64:
        2

def borrowed(value: &Sized[i64]) -> i64:
    len(value)

def mutable(value: &mut Sized[i64]) -> i64:
    len(value)

def make() -> Sized[i64]:
    print("make").unwrap()
    Sized(42)

def main() -> ():
    mut deque = Deque[i64]().unwrap()
    deque.append(1).unwrap()
    print(len(deque), len(&deque), deque.pop()).unwrap()
    mut buffer = std.bytes.from_bytes(b"abc").unwrap()
    print(len(buffer), len(&mut buffer), buffer.get(1)).unwrap()
    mut value = Sized(1)
    print(len(value), borrowed(&value), mutable(&mut value), value.value).unwrap()
    print(len(&value), len(&mut value)).unwrap()
    print(len(Size.Small)).unwrap()
    print(len(make())).unwrap()
    print(len(std.bytes.from_bytes(b"once").unwrap())).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1 1 Option[i64].Some(1)\n3 3 98\n7 7 7 1\n7 7\n2\nmake\n7\ndrop sized\n4\ndrop sized\n"
    );
}

#[rstest]
#[case("def len(self) -> bool:\n        True")]
#[case("def len(self, extra: i64) -> i64:\n        extra")]
#[case("def len(self: &mut Bad) -> i64:\n        1")]
#[case("def len(self: Bad) -> i64:\n        1")]
#[case("def size(self) -> i64:\n        1")]
#[case("def len[T](self) -> i64:\n        1")]
fn len_rejects_methods_that_cannot_be_observed(#[case] method: &str) {
    let source = format!("class Bad:\n    {method}\nprint(len(Bad())).unwrap()");
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(
        error.contains("requires an accessible `def len(self) -> i64` method"),
        "{error}"
    );
}

#[rstest]
#[case(
    "from std.deque import Deque\ndef main() -> ():\n    value = Deque[i64]().unwrap()",
    "std.deque.Deque[i64]"
)]
#[case(
    "class Container:\n    def contains(self, item: i64) -> bool:\n        True\ndef main() -> ():\n    value = Container()",
    "Container"
)]
fn membership_on_std_and_user_containers_names_the_actual_limit(
    #[case] setup: &str,
    #[case] ty: &str,
) {
    for op in ["in", "not in"] {
        let source = format!("{setup}\n    print(1 {op} value).unwrap()");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains(&format!("membership is not defined for `{ty}`")),
            "{error}"
        );
        assert!(error.contains("do not call user methods"), "{error}");
        assert!(!error.contains("requires an iterable"), "{error}");
    }
}

#[test]
fn len_preserves_method_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("container.plenty");
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        "from container import Container\ndef main() -> ():\n    print(len(Container())).unwrap()\n",
    )
    .unwrap();
    for public in [false, true] {
        let visibility = if public { "pub " } else { "" };
        std::fs::write(
            &module,
            format!("pub class Container:\n    {visibility}def len(self) -> i64:\n        3\n"),
        )
        .unwrap();
        let result = plenty::check_file(&main, None);
        if public {
            result.unwrap();
        } else {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("Container.len` is private"), "{error}");
        }
    }
}

#[test]
fn len_shared_method_cannot_bypass_an_exclusive_loan() {
    let error = support::check_source(
        r#"
class Container:
    value: i64
    def len(self) -> i64:
        self.value
mut value = Container(3)
borrow = &mut value
print(len(value), borrow.value).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("conflicting borrow"), "{error}");
}

#[test]
fn len_observes_projected_builtin_values_without_moving_their_owners() {
    let output = support::run(
        r#"
class Holder:
    values: list[i64]
    def __del__(self) -> ():
        print("drop holder").unwrap()

def make() -> Holder:
    print("make").unwrap()
    Holder([1, 2, 3].unwrap())

def lend(value: &Holder) -> &Holder:
    print("lend").unwrap()
    value

def index() -> i64:
    print("index").unwrap()
    0

print(len([[1, 2, 3].unwrap()].unwrap()[index()])).unwrap()
print(len({1: [1, 2, 3].unwrap()}.unwrap()[1])).unwrap()
print(len(make().values)).unwrap()
holder = Holder([1, 2, 3, 4].unwrap())
print(len(lend(&holder).values), len(holder.values)).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "index\n3\n3\nmake\n3\ndrop holder\nlend\n4 4\ndrop holder\n"
    );
}

#[test]
fn len_borrows_projected_class_receivers_and_drops_temporary_parents_once() {
    let output = support::run(
        r#"
class Container:
    value: i64
    def len(self) -> i64:
        self.value
    def __del__(self) -> ():
        print("drop", self.value).unwrap()

class Holder:
    item: Container
    def __del__(self) -> ():
        print("drop holder", self.item.value).unwrap()

def make() -> Holder:
    print("make").unwrap()
    Holder(Container(3))

def lend(value: &Holder) -> &Holder:
    print("lend").unwrap()
    value

def index() -> i64:
    print("index").unwrap()
    0

print(len(make().item)).unwrap()
print(len([Container(4)].unwrap()[index()])).unwrap()
print(len({1: Container(5)}.unwrap()[1])).unwrap()
print(len((Container(7),)[0])).unwrap()
print(len([Holder(Container(8))].unwrap()[0].item)).unwrap()
holder = Holder(Container(6))
print(len(lend(&holder).item), holder.item.value).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "make\n3\ndrop holder 3\ndrop 3\nindex\n4\ndrop 4\n5\ndrop 5\n7\ndrop 7\n8\ndrop holder 8\ndrop 8\nlend\n6 6\ndrop holder 6\ndrop 6\n"
    );
}
