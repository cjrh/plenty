//! Laziness, suspension, affine ownership and destruction through native code.
mod support;
use rstest::rstest;
use support::run;

const COUNT: &str = "def count(n: i64) -> Generator[i64]:\n    mut i = 0\n    while i < n:\n        yield i\n        # 0 <= i < n <= i64.MAX, so the resumed increment fits.\n        unchecked:\n            i = i + 1\n";

#[test]
fn explicit_next_reborrows_a_mutable_generator_without_consuming_it() {
    let source = format!(
        "{COUNT}\ndef advance(it: &mut Generator[i64]) -> ():\n    print(next(&mut it)).unwrap()\nmut it = count(3)\nprint(next(&mut it)).unwrap()\nadvance(&mut it)\nprint(next(it)).unwrap()\nprint(next(&mut it)).unwrap()\n"
    );
    let output = run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"Option[i64].Some(0)\nOption[i64].Some(1)\nOption[i64].Some(2)\nOption[i64].Nothing\n"
    );
    for (body, expected) in [
        ("it = count(1)\nnext(&mut it)\n", "immutable binding"),
        (
            "mut it = count(1)\nr = &it\nnext(&mut r)\n",
            "shared reference as mutable",
        ),
        (
            "mut it = count(1)\nr = &mut it\nnext(&mut it)\nnext(&mut r)\n",
            "conflicting borrow",
        ),
        ("next(&mut count(1))\n", "named binding"),
    ] {
        let error = support::check_source(&format!("{COUNT}{body}"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{body}\n{error}");
    }
}
#[rstest]
#[case(
    "def g(n: i64) -> Generator[i64]:\n    print('start').unwrap()\n    yield n\n    print('after').unwrap()\ndef arg() -> i64:\n    print('arg').unwrap()\n    42\nmut it = g(arg())\nprint('created').unwrap()\nprint(next(it)).unwrap()\nprint('stop').unwrap()",
    "arg\ncreated\nstart\nOption[i64].Some(42)\nstop\n"
)]
#[case(
    "def g() -> Generator[str]:\n    print('never').unwrap()\n    yield 'x'\ng()\nprint('done').unwrap()",
    "done\n"
)]
#[case(
    "mut g = count(2)\nprint(next(g)).unwrap()\nprint(next(g)).unwrap()\nprint(next(g)).unwrap()\nprint(next(g)).unwrap()",
    "Option[i64].Some(0)\nOption[i64].Some(1)\nOption[i64].Nothing\nOption[i64].Nothing\n"
)]
#[case(
    "def empty() -> Generator[i64]:\n    if False:\n        yield 1\nmut g = empty()\nprint(next(g)).unwrap()\nprint(next(g)).unwrap()\nprint(list(empty()).unwrap()).unwrap()",
    "Option[i64].Nothing\nOption[i64].Nothing\n[]\n"
)]
#[case(
    "def g() -> Generator[i64]:\n    yield 1\n    return\nfor x in g():\n    print(x).unwrap()",
    "1\n"
)]
#[case(
    "def g(flag: bool) -> Generator[i64]:\n    if flag:\n        yield 1\n        yield 2\n    else:\n        yield 3\n    yield 4\nprint(list(g(True)).unwrap()).unwrap()\nprint(list(g(False)).unwrap()).unwrap()",
    "[1, 2, 4]\n[3, 4]\n"
)]
#[case(
    "print([x * x for x in count(6) if x % 2 == 1].unwrap()).unwrap()\nprint({x % 2 for x in count(6)}.unwrap()).unwrap()\nprint({x: x + 1 for x in count(3)}.unwrap()).unwrap()",
    "[1, 9, 25]\n{0, 1}\n{0: 1, 1: 2, 2: 3}\n"
)]
#[case(
    "print([x * 10 + y for x in count(3) for y in count(x)].unwrap()).unwrap()",
    "[10, 20, 21]\n"
)]
#[case(
    "for n in count(100):\n    if n == 1:\n        continue\n    if n == 3:\n        break\n    print(n).unwrap()",
    "0\n2\n"
)]
#[case(
    "def g() -> Generator[i64]:\n    for n in range(5):\n        if n == 1:\n            continue\n        if n == 3:\n            break\n        yield n\n    yield 99\nprint(list(g()).unwrap()).unwrap()",
    "[0, 2, 99]\n"
)]
// count(3) x count(2) bounds each encoded pair by 21.
#[case(
    "def g() -> Generator[i64]:\n    for a in count(3):\n        for b in count(2):\n            value = checked:\n                a * 10 + b\n            yield value.unwrap()\nprint(list(g()).unwrap()).unwrap()",
    "[0, 1, 10, 11, 20, 21]\n"
)]
// Only E.A(2) reaches the asserted increment.
#[case(
    "enum E:\n    A(i64)\n    B\ndef g(e: E) -> Generator[i64]:\n    match e:\n        case E.A(n):\n            yield n\n            value = checked:\n                n + 1\n            yield value.unwrap()\n        case E.B:\n            return\n    yield 9\nprint(list(g(E.A(2))).unwrap()).unwrap()\nprint(list(g(E.B)).unwrap()).unwrap()",
    "[2, 3, 9]\n[]\n"
)]
#[case(
    "def g() -> Generator[list[str]]:\n    mut xs = [('a' + '\\0b').unwrap()].unwrap()\n    yield copy(xs).unwrap()\n    xs.append('c').unwrap()\n    yield xs\nprint(list(g()).unwrap()).unwrap()",
    "[[\"a\\0b\"], [\"a\\0b\", \"c\"]]\n"
)]
#[case(
    "def g() -> Generator[Option[str]]:\n    yield Option[str].Some(('hello' + ' world').unwrap())\n    yield Option[str].Nothing\nprint(list(g()).unwrap()).unwrap()",
    "[Option[str].Some(\"hello world\"), Option[str].Nothing]\n"
)]
#[case(
    "def forward(n: i64) -> Generator[i64]:\n    count(n)\nprint(list(forward(3)).unwrap()).unwrap()",
    "[0, 1, 2]\n"
)]
#[case(
    "def forward(g: Generator[i64]) -> Generator[i64]:\n    return g\na = count(2)\nb = a\nprint(list(forward(b)).unwrap()).unwrap()",
    "[0, 1]\n"
)]
#[case(
    "def take(g: Generator[i64]) -> i64:\n    for x in g:\n        return x\n    -1\nprint(take(count(100))).unwrap()\nprint(take(count(0))).unwrap()",
    "0\n-1\n"
)]
#[case(
    "mut g = count(1)\nfor i in range(3):\n    print(list(g).unwrap()).unwrap()\n    g = count(i + 2)\nprint(list(g).unwrap()).unwrap()",
    "[0]\n[0, 1]\n[0, 1, 2]\n[0, 1, 2, 3]\n"
)]
#[case(
    "def choose(flag: bool, g: Generator[i64]) -> i64:\n    if flag:\n        print(list(g).unwrap()).unwrap()\n        return 1\n    print(list(g).unwrap()).unwrap()\n    2\nprint(choose(False, count(2))).unwrap()",
    "[0, 1]\n2\n"
)]
#[case(
    "mut sum = 0\nfor x in count(100_000):\n    sum = sum + x\nprint(sum).unwrap()",
    "4999950000\n"
)]
#[case(
    "def chars() -> Generator[str]:\n    for c in 'é\\0😀':\n        yield c\nprint(list(chars()).unwrap()).unwrap()",
    "[\"é\", \"\\0\", \"😀\"]\n"
)]
// The narrow-integer frame is resumed with x=-128, so x+1 fits i8.
#[case(
    "def g(x: i8, y: u64, flag: bool) -> Generator[i8]:\n    if flag and y > 0u64:\n        yield x\n        value = checked:\n            x + 1i8\n        yield value.unwrap()\nprint(list(g(-128i8, 18446744073709551615u64, True)).unwrap()).unwrap()",
    "[-128, -127]\n"
)]
#[case(
    "def wrapped(g: Generator[i64]) -> Generator[i64]:\n    for x in g:\n        yield x\nprint(list(wrapped(count(2))).unwrap()).unwrap()",
    "[0, 1]\n"
)]
fn native_generators(#[case] source: &str, #[case] expected: &str) {
    let mut program = support::program(&format!("{COUNT}{source}")).replacen(
        "def main() -> ():",
        "def main() -> Result[(), OverflowError[i64] | ZeroDivisionError]:",
        1,
    );
    program.push_str("    Ok(())\n");
    let output = run(&program);
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
#[case(
    "def g() -> Generator[Generator[i64]]:\n    yield count(1)",
    "cannot yield generators"
)]
#[case(
    "def g() -> Generator[Closure[[], i64]]:\n    yield (def() -> i64: 1)",
    "generators cannot yield closures"
)]
#[case(
    "def g(x: &mut i64) -> Generator[&mut i64]:\n    yield x",
    "generators cannot yield mutable references"
)]
#[case("a = count(2)\nb = a\nlist(a).unwrap()", "use of moved binding `a`")]
#[case(
    "def eat(g: Generator[i64]) -> ():\n    pass\na = count(2)\neat(a)\nlist(a).unwrap()",
    "use of moved binding `a`"
)]
#[case(
    "a = count(2)\nfor n in a:\n    break\nlist(a).unwrap()",
    "use of moved binding `a`"
)]
#[case(
    "a = count(2)\nif True:\n    list(a).unwrap()\nlist(a).unwrap()",
    "use of possibly moved binding `a`"
)]
#[case(
    "a = count(2)\nfor n in range(2):\n    list(a).unwrap()",
    "`a` is moved in a loop"
)]
#[case(
    "a = count(2)\nwhile True:\n    list(a).unwrap()\n    continue",
    "`a` is moved in a loop"
)]
#[case(
    "a = count(2)\nwhile True:\n    list(a).unwrap()\n    break\nlist(a).unwrap()",
    "moved binding `a`"
)]
#[case("a = count(2)\nnext(a)", "mutable generator binding")]
#[case("next(count(2))", "named mutable generator")]
#[case("mut a = 1\nnext(a)", "requires a generator")]
#[case("print(count(2)).unwrap()", "cannot be printed")]
#[case("len(count(2))", "len() is not defined for `Generator[i64]")]
#[case("1 in count(2)", "membership does not consume")]
#[case("count(2) == count(2)", "binary operators")]
#[case("[count(2)].unwrap()", "cannot be stored in collections")]
#[case("{'a': count(2)}.unwrap()", "cannot be stored in collections")]
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
    "a = count(2)\n[x for n in range(2) for x in a].unwrap()",
    "`a` is moved in a loop"
)]
#[case(
    "def condition(g: Generator[i64]) -> bool:\n    True\na = count(2)\nwhile condition(a):\n    pass",
    "`a` is moved in a loop"
)]
#[case(
    "a = count(2)\nlist((x for x in zip(a))).unwrap()",
    "`zip` expects 2 arguments, got 1"
)]
#[case(
    "list((x for x in enumerate())).unwrap()",
    "`enumerate` expects 1 or 2 arguments, got 0"
)]
#[case(
    "a = count(2)\nlist((x for x in reversed(a, a))).unwrap()",
    "`reversed` expects 1 arguments, got 2"
)]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(&format!("{COUNT}{source}"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected:?}, got {error:?}"
    );
}

#[test]
fn one_consumer_accepts_more_than_256_generator_functions() {
    // Issue #430's reproduction: the former limit of 256 frames counted every
    // consumer together and blamed recursion.
    use std::fmt::Write;
    let mut source = String::new();
    for i in 0..257 {
        writeln!(source, "def g{i}() -> Generator[i64]:\n    yield {i}").unwrap();
    }
    source.push_str(
        "def total(values: Generator[i64]) -> Result[i64, OverflowError[i64]]:\n    mut result = 0\n    for value in values:\n        result = result + value\n    Ok(result)\ndef main() -> ():\n",
    );
    for i in 0..257 {
        writeln!(source, "    drop(total(g{i}()))").unwrap();
    }
    support::check_source(&source).unwrap();
}

#[test]
fn a_chain_of_300_distinct_generator_consumers_runs() {
    // Each consumer passes its generator on, so each needs its own frame one
    // dependency level deeper; no consumer repeats (issue #435).
    use std::fmt::Write;
    let mut source = format!(
        "{COUNT}def c0(values: Generator[i64]) -> Result[i64, OverflowError[i64]]:\n    mut result = 0\n    for value in values:\n        result = result + value\n    Ok(result)\n"
    );
    for i in 1..300 {
        writeln!(
            source,
            "def c{i}(values: Generator[i64]) -> Result[i64, OverflowError[i64]]:\n    c{}(values)",
            i - 1
        )
        .unwrap();
    }
    source.push_str("def main() -> i32:\n    i32(c299(count(5)).unwrap())\n");
    let output = run(&source);
    assert_eq!(output.status.code(), Some(10), "{output:?}");
}

#[test]
fn expanding_generator_consumers_are_bounded() {
    // Runtime seeds [1, 2, 3] and three expansions bound items by 24.
    // The frontend must still reject unbounded specialization, not arithmetic.
    for (call, expected) in [
        // Each frame requires a deeper one: the dependency depth stops it.
        // The limit counts `total` and the four builtin integer-domain and bounded `get` methods.
        (
            "total((increment(v).unwrap() for v in values), n - 1)?",
            "dependency depth limit of 261 (256 plus one per generic function or generator consumer) exceeded",
        ),
        // Two per frame reach the consumer's limit within that depth.
        (
            "total((increment(v).unwrap() for v in values), n - 1)? + total((double(v).unwrap() for v in values), n - 1)?",
            "generator specialization limit of 16384 frames of `total` exceeded",
        ),
    ] {
        let source = format!(
            "def increment(v: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(v + 1)\ndef double(v: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(v * 2)\ndef total(values: Generator[i64], n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(0)\n    Ok({call})\ndef main() -> ():\n    drop(total((v for v in [1, 2, 3].unwrap()), 3))\n"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn local_arithmetic_results_survive_suspension_in_both_modes() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("results.plenty");
    std::fs::write(
        &source,
        r#"
def increment(n: i8) -> Result[i8, OverflowError[i8]]:
    Ok(n + 1i8)
def values() -> Generator[Result[i8, OverflowError[i8]]]:
    yield increment(i8.MAX)
    yield increment(-1i8)
def main() -> Result[(), AllocError | IoError]:
    mut stream = values()
    print(str.repr(next(stream).unwrap())?)?
    print(str.repr(next(stream).unwrap())?)?
    print(next(stream).is_none(), next(stream).is_none())?
    Ok(())
"#,
    )
    .unwrap();
    for optimize in [false, true] {
        let executable = dir.path().join("program");
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"));
        if !optimize {
            command.arg("--fast-compile");
        }
        let compiled = command
            .args(["--no-cache", "--compile"])
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(compiled.status.success(), "{compiled:?}");
        let result = std::process::Command::new(&executable).output().unwrap();
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout,
            b"Result[i8, OverflowError[i8]].Err(OverflowError[i8](wrapped=-128))\nResult[i8, OverflowError[i8]].Ok(0)\nTrue True\n");
    }
}

// Issue #768: resume stores the yielded record after its result word. Four
// i64 fields already exceed the fixed scratch area's 32-byte payload space.
#[rstest]
#[case(false, 4)]
#[case(true, 4)]
#[case(false, 8)]
#[case(true, 8)]
fn resume_reserves_the_complete_inline_yield(#[case] optimize: bool, #[case] fields: usize) {
    let declarations = (0..fields)
        .map(|field| format!("    field_{field}: i64\n"))
        .collect::<String>();
    let arguments = vec!["index"; fields].join(", ");
    let source = format!(
        r#"@copy
class Span:
{declarations}
def values(text: str) -> Generator[Span]:
    unchecked trap:
        mut index = 0
        for scalar in text:
            index += 1
            yield Span({arguments})

def gather(text: &str, output: &mut list[i64]) -> Result[(), AllocError]:
    for value in values(*text):
        output.append(value.field_{last})?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    mut output: list[i64] = []?
    gather(&"abc", &mut output)?
    print(output)?
    Ok(())
"#,
        last = fields - 1,
    );
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("yield-record");
    plenty::compile_source_to_executable_with_options(
        &source,
        &executable,
        &plenty::CompileOptions {
            optimize,
            ..Default::default()
        },
    )
    .unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"[1, 2, 3]\n");
}

// Issue #927: a named closure's borrowed captures follow the yield rule for
// other local loans instead of being rejected throughout a generator body.
#[test]
fn named_closure_borrows_end_before_yield() {
    support::assert_stdout(
        r#"def apply[F: Callable[[&i64], bool]](f: &F, x: &i64) -> bool:
    f(x)

def flags(xs: &list[i64]) -> Generator[bool]:
    limit = 1
    keep = def [&limit](x: &i64) -> bool:
        *x > limit
    first = keep(&xs[0])
    lent = apply(&keep, &xs[1])
    yield first
    yield lent

def latest() -> Generator[i64]:
    mut total = 0
    mut set = def [&mut total](v: i64) -> ():
        total = v
    set(5)
    yield total
    yield total

def present(xs: &list[i64]) -> Generator[i64]:
    has = def [&xs](v: i64) -> bool:
        len(xs) > v
    for v in range(4):
        if has(v):
            yield v

def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    for v in flags(&xs):
        print(v)?
    for v in latest():
        print(v)?
    for v in present(&xs):
        print(v)?
    Ok(())
"#,
        "False\nTrue\n5\n5\n0\n1\n2\n",
    );
}

#[rstest]
#[case(
    "def g(xs: &list[i64]) -> Generator[i64]:\n    limit = 1\n    keep = def [&limit](x: &i64) -> bool:\n        *x > limit\n    for x in xs:\n        if keep(x):\n            yield *x"
)]
#[case(
    "def g(xs: &list[i64]) -> Generator[i64]:\n    limit = 1\n    keep = def [&limit](x: &i64) -> bool:\n        *x > limit\n    for x in iter(xs).filter(keep):\n        yield *x"
)]
fn named_closure_borrows_cannot_cross_yield(#[case] source: &str) {
    let error = support::check_error(source);
    assert!(
        error.contains("a borrow of `limit` cannot remain live across yield"),
        "{error}"
    );
}
