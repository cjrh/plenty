mod support;

#[test]
fn generator_bridge_constructs_once_and_resumes_mutable_captures_lazily() {
    output(
        r#"
def source() -> list[i64]:
    print("source").unwrap()
    [2, 4, 6].unwrap()
def callback() -> Closure[[i64], i64]:
    print("callback").unwrap()
    count = 0
    def [mut count](n: i64) -> i64:
        count += 1
        print("visit", n).unwrap()
        n + count
def build() -> Generator[i64]:
    p = iter(source()).map(callback())
    print("prepared").unwrap()
    p.generator()
def main() -> ():
    mut stream = build()
    print("converted").unwrap()
    print(next(stream)).unwrap()
    print(next(stream)).unwrap()
    print(next(stream)).unwrap()
    print(next(stream), next(stream)).unwrap()
"#,
        "source\ncallback\nprepared\nconverted\nvisit 2\nOption[i64].Some(3)\nvisit 4\nOption[i64].Some(6)\nvisit 6\nOption[i64].Some(9)\nOption[i64].Nothing Option[i64].Nothing\n",
    );
}

#[test]
fn generator_bridge_rejects_borrowed_sources_and_captures_even_after_owned_mapping() {
    for source in [
        "values = [1, 2].unwrap()\np = iter(&values).map(def(n: &i64) -> i64: *n)\ng = p.generator()\n",
        "values = {1: 2}.unwrap()\np = iter(values.items()).map(def(k: &i64, v: &i64) -> i64: *k + *v)\ng = p.generator()\n",
        "n = 2\np = iter(range(3)).map(def [&n](x: i64) -> i64: x + n)\ng = p.generator()\n",
        "values = [1, 2].unwrap()\np = iter(range(2)).zip(&values).map(def(k: i64, v: &i64) -> i64: k + *v)\ng = p.generator()\n",
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(
            error.contains("owned sources and owned callback captures"),
            "{error}"
        );
    }
}

#[test]
fn generator_bridge_cleans_never_started_partial_exhausted_and_zip_pending_owners() {
    let source = r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def build(start: i64) -> Generator[Guard]:
    values = [Guard(start), Guard(start + 1), Guard(start + 2)].unwrap()
    capture = Guard(start + 9)
    iter(values).map(def [capture](g: Guard) -> Guard: g).generator()
def main() -> ():
    unused = build(10)
    drop(unused)
    print("unstarted").unwrap()
    mut partial = build(20)
    drop(next(partial))
    drop(partial)
    print("partial").unwrap()
    mut full = build(30)
    drop(next(full))
    drop(next(full))
    drop(next(full))
    drop(next(full))
    drop(full)
    print("full").unwrap()
    left = [Guard(40), Guard(41)].unwrap()
    mut zipped = iter(left).zip(range(0)).generator()
    drop(next(zipped))
    drop(zipped)
    print("zip").unwrap()
"#;
    let result = support::run(source);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8_lossy(&result.stdout);
    let mut lines = text.lines();
    for (marker, expected) in [
        (
            "unstarted",
            vec!["drop 10", "drop 11", "drop 12", "drop 19"],
        ),
        ("partial", vec!["drop 20", "drop 21", "drop 22", "drop 29"]),
        ("full", vec!["drop 30", "drop 31", "drop 32", "drop 39"]),
        ("zip", vec!["drop 40", "drop 41"]),
    ] {
        let mut actual = lines
            .by_ref()
            .take_while(|line| *line != marker)
            .collect::<Vec<_>>();
        actual.sort_unstable();
        assert_eq!(actual, expected, "{text}");
    }
    assert!(lines.next().is_none());
}

#[test]
fn generators_share_fluent_surface_and_owned_pipelines_cross_function_boundaries() {
    output(
        r#"
def numbers(stop: i64) -> Generator[i64]:
    for n in range(stop):
        yield n
def squares(stop: i64) -> Generator[i64]:
    values = iter(range(stop)).filter(def(n: &i64) -> bool: *n % 2 == 0).map(def(n: i64) -> i64: n * n)
    values.generator()
def consume(values: Generator[i64]) -> i64:
    values.fold(0, def(a: i64, n: i64) -> i64: a + n)
def main() -> ():
    values: list[i64] = numbers(5).filter(def(n: &i64) -> bool: *n > 1).map(def(n: i64) -> i64: n * 2).collect().unwrap()
    print(values).unwrap()
    print(consume(squares(6))).unwrap()
    mut stream = iter(range(10)).skip(2).take(3).enumerate(5).generator()
    print(next(stream), next(stream), next(stream), next(stream), next(stream)).unwrap()
    print(iter(range(4)).generator().map(def(n: i64) -> i64: n + 1).count()).unwrap()
"#,
        "[4, 6, 8]\n20\nOption[tuple[i64, i64]].Some((5, 2)) Option[tuple[i64, i64]].Some((6, 3)) Option[tuple[i64, i64]].Some((7, 4)) Option[tuple[i64, i64]].Nothing Option[tuple[i64, i64]].Nothing\n4\n",
    );
}

#[test]
fn generator_fluent_receivers_include_expressions_unwrapped_results_and_methods() {
    output(
        r#"
def numbers() -> Generator[i64]:
    yield 1
    yield 2
def fallible() -> Result[Generator[i64], str]:
    Ok(numbers())
def once[T](value: T) -> Generator[T]:
    yield value
class Source:
    def numbers(self) -> Generator[i64]:
        numbers()
def main() -> Result[(), str | IoError]:
    print((n * 2 for n in range(3)).count())?
    print(fallible()?.count(), fallible().unwrap().count())?
    source = Source()
    print(source.numbers().count())?
    print((numbers() if True else numbers()).take(1).count())?
    print(once(3).count(), once[str]("x").count(), numbers.new().count())?
    Ok(())
"#,
        "3\n2 2\n2\n1\n1 1 2\n",
    );
}

#[test]
fn generator_bridge_preserves_nested_conversion_and_zip_filter_progress() {
    output(
        r#"
def main() -> ():
    mut nested = iter(range(4)).generator().map(def(n: i64) -> str: str.from(n).unwrap()).generator()
    print(next(nested), next(nested)).unwrap()
    left = iter(range(8)).filter(def(n: &i64) -> bool: *n % 2 == 0)
    right = iter(range(10, 20)).skip(1).filter(def(n: &i64) -> bool: *n % 2 == 1)
    mut pairs = left.zip(right).take_while(def(a: &i64, b: &i64) -> bool: *a < 5).generator()
    print(next(pairs), next(pairs), next(pairs), next(pairs), next(pairs)).unwrap()
"#,
        "Option[str].Some(\"0\") Option[str].Some(\"1\")\nOption[tuple[i64, i64]].Some((0, 11)) Option[tuple[i64, i64]].Some((2, 13)) Option[tuple[i64, i64]].Some((4, 15)) Option[tuple[i64, i64]].Nothing Option[tuple[i64, i64]].Nothing\n",
    );
}

#[test]
fn generator_bridge_empty_and_error_consumers_release_state_without_extra_calls() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def source(g: Guard) -> Generator[i64]:
    print("source resumed").unwrap()
    yield g.n
def checked(g: Guard) -> Result[Guard, str]:
    print("visit", g.n).unwrap()
    if g.n == 2:
        return Err("two")
    Ok(g)
def main() -> ():
    mut empty = source(Guard(9)).take(0).generator()
    drop(next(empty))
    drop(next(empty))
    drop(empty)
    values = [Guard(1), Guard(2), Guard(3)].unwrap()
    stream = iter(values).map(checked).generator()
    result: Result[list[Guard], AllocError | str] = stream.try_collect()
    drop(result)
    print("done").unwrap()
"#,
        "drop 9\nvisit 1\nvisit 2\ndrop 2\ndrop 1\ndrop 3\ndone\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn generator_bridge_frames_and_fluent_consumers_do_not_allocate() {
    support::assert_visible_stdout(
        r#"
def make(stop: i64) -> Generator[i64]:
    offset = 3
    iter(range(stop)).filter(def(n: &i64) -> bool: *n % 2 == 0).map(def [offset](n: i64) -> i64: n + offset).generator()
def consume(values: Generator[i64]) -> i64:
    values.fold(0, def(a: i64, n: i64) -> i64: a + n)
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
mut values = make(6)
drop(next(values))
total = consume(values)
unused = make(10)
drop(unused)
print("__test_end_no_allocations__").unwrap()
print("__test_restore_allocations__").unwrap()
print(total).unwrap()
"#,
        "12",
    );
}

#[test]
fn optimized_scalar_captures_follow_copies_branches_and_loop_assignments() {
    let source = r#"
def offset(n: i64) -> Closure[[i64], i64]:
    def [n](value: i64) -> i64:
        value + n
def replace(f: &mut Closure[[i64], i64], n: i64) -> ():
    *f = offset(n)
def exercise(start: i64) -> ():
    mut f = offset(start)
    mut changing = offset(start)
    for i in range(4):
        before = f
        previous = changing
        if i % 2 == 0:
            f = offset(i)
            changing = offset(i * start)
        else:
            f = offset(i + 10)
            changing = offset(i * start + 1)
        print(before(100), f(100)).unwrap()
        print(previous(1), changing(1)).unwrap()
    saved = f
    f = offset(20)
    replace(&mut f, 50)
    print(saved(1), f(1)).unwrap()
    bias = 1.5
    scaled = def [bias](n: f64) -> f64:
        n * bias
    print(scaled(2.0)).unwrap()
    count = 4
    mixed = def [count, bias](n: f64) -> bool:
        (bias > n) == (count > 0)
    print(mixed(1.0)).unwrap()
    for i in range(3):
        n = i * start
        callback = def [n](value: i64) -> i64:
            value + n
        print(iter(range(2)).map(callback).fold(0, def(a: i64, b: i64) -> i64: a + b)).unwrap()
def main() -> ():
    exercise(7)
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("captures.plenty");
    std::fs::write(&path, source).unwrap();
    for optimize in [false, true] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"));
        if optimize {
            command.arg("--release");
        }
        let result = command.arg("--no-cache").arg(&path).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "107 100\n8 1\n100 111\n1 9\n111 102\n9 15\n102 113\n15 23\n14 51\n3.0\nTrue\n1\n15\n29\n"
        );
    }
}

#[test]
fn pair_callbacks_enumerate_zip_and_dictionary_collection() {
    output(
        r#"
def total(acc: i64, i: i64, n: &i64) -> i64:
    acc + i * *n
def main() -> ():
    values = [10, 20, 30].unwrap()
    print(enumerate(&values, 1).fold(0, total)).unwrap()
    pairs: dict[i64, i64] = iter(range(4)).map(def(n: i64) -> (i64, i64): (n % 2, n)).collect().unwrap()
    print(pairs[0], pairs[1]).unwrap()
    selected: list[i64] = iter(pairs.items()).filter(def(k: &i64, v: &i64) -> bool: *v > 1).map(def(k: &i64, v: &i64) -> i64: *k + *v).collect().unwrap()
    print(len(selected)).unwrap()
    print(iter(range(3)).enumerate(5).map(def(i: i64, n: i64) -> i64: i + n).fold(0, def(a: i64, n: i64) -> i64: a + n)).unwrap()
    for a, b in zip(&values, range(2)):
        print(*a, b).unwrap()
"#,
        "140\n2 3\n2\n21\n10 0\n20 1\n",
    );
}

#[test]
fn pipeline_sources_keep_user_items_methods_and_owned_nested_pairs() {
    output(
        r#"
class Source:
    def items(self: &Source) -> list[i64]:
        [4, 5].unwrap()
def numbers() -> dict[i64, i64]:
    {1: 10, 2: 20}.unwrap()
def main() -> ():
    source = Source()
    print(iter(source.items()).fold(0, def(a: i64, b: i64) -> i64: a + b)).unwrap()
    print(iter(numbers().items()).map(def(k: &i64, v: &i64) -> i64: *k + *v).fold(0, def(a: i64, b: i64) -> i64: a + b)).unwrap()
    for pair, n in iter(range(2)).enumerate().zip(range(10, 12)):
        print(pair, n).unwrap()
"#,
        "9\n33\n(0, 0) 10\n(1, 1) 11\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn pair_collection_reclaims_items_at_each_failed_allocation() {
    for budget in 0..=8 {
        let source = format!(
            r#"
class Guard:
    n: i64
def build() -> Result[dict[i64, Guard], AllocError]:
    iter(range(20)).map(def(n: i64) -> (i64, Guard): (n, Guard(n))).collect()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = build()
print("__test_restore_allocations__").unwrap()
drop(result)
print("clean").unwrap()
"#
        );
        support::assert_visible_stdout(&source, "clean");
    }
}

#[test]
fn zipped_pipelines_pull_each_accepted_item_left_first() {
    output(
        r#"
def left(n: i64) -> i64:
    print("left", n).unwrap()
    n
def right(n: i64) -> i64:
    print("right", n).unwrap()
    n
def main() -> ():
    a = iter(range(5)).map(left).filter(def(n: &i64) -> bool: *n % 2 == 0)
    b = iter(range(2)).map(right)
    for x, y in a.zip(b):
        print("pair", x, y).unwrap()
"#,
        "left 0\nright 0\npair 0 0\nleft 1\nleft 2\nright 1\npair 2 1\nleft 3\nleft 4\n",
    );
}

#[test]
fn free_adapters_in_loop_sources_pull_filtered_arms_independently() {
    output(
        r#"
def even(n: &i64) -> bool:
    *n % 2 == 0
def visit(n: i64) -> i64:
    print("left", n).unwrap()
    n
def right() -> Generator[i64]:
    print("right", 0).unwrap()
    yield 10
    print("right", 1).unwrap()
    yield 11
def main() -> ():
    for i, n in enumerate(iter(range(5)).filter(even)):
        print(i, n).unwrap()
    for a, b in zip(iter(range(5)).map(visit).filter(even), right()):
        print(a, b).unwrap()
    for a, b in zip(iter(range(3)).map(visit), iter(range(5)).filter(even)):
        print(a, b).unwrap()
"#,
        "0 0\n1 2\n2 4\nleft 0\nright 0\n0 10\nleft 1\nleft 2\nright 1\n2 11\nleft 3\nleft 4\nleft 0\n0 0\nleft 1\n1 2\nleft 2\n2 4\n",
    );
}

#[test]
fn fallible_pair_collection_and_tuple_accumulator() {
    output(
        r#"
def pair(n: i64) -> Result[(i64, i64), str]:
    if n == 3:
        return Err("stop")
    Ok((n, n * n))
def sum(acc: (i64, i64), a: i64, b: i64) -> Result[(i64, i64), str]:
    Ok((acc[0] + a, acc[1] + b))
def main() -> ():
    values: Result[dict[i64, i64], str | AllocError] = iter(range(2)).map(pair).try_collect()
    print(values.unwrap()[1]).unwrap()
    print(zip(range(3), range(10, 13)).try_fold((0, 0), sum).unwrap()).unwrap()
    result: Result[dict[i64, i64], str | AllocError] = iter(range(5)).map(pair).try_collect()
    print(result.is_err()).unwrap()
"#,
        "1\n(3, 33)\nTrue\n",
    );
}

#[test]
fn pair_ownership_survives_filter_short_zip_and_duplicate_dictionary_keys() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def guards() -> Generator[Guard]:
    yield Guard(1)
    yield Guard(2)
    yield Guard(3)
def main() -> ():
    for a, b in iter(guards()).zip(range(1)):
        print(a.n, b).unwrap()
    print("zip done").unwrap()
    values = [(Guard(4), 1), (Guard(5), 2)].unwrap()
    print(iter(values).filter(def(g: &Guard, n: &i64) -> bool: *n > 1).count()).unwrap()
    pairs = [(0, Guard(6)), (0, Guard(7))].unwrap()
    collected: dict[i64, Guard] = iter(pairs).collect().unwrap()
    print(collected[0].n).unwrap()
    drop(collected)
"#,
        "1 0\ndrop 1\ndrop 2\nzip done\ndrop 4\ndrop 5\n1\ndrop 6\n7\ndrop 7\n",
    );
}

#[test]
fn pair_predicates_and_reducers_keep_accumulator_whole() {
    output(
        r#"
def combine(acc: (i64, i64), a: i64, b: i64) -> (i64, i64):
    (acc[0] + a, acc[1] + b)
def visit(a: i64, b: i64) -> Result[(), str]:
    if a == 1:
        return Err("stop")
    print(a, b).unwrap()
    Ok(())
def main() -> ():
    print(iter([(1, 10), (2, 20)].unwrap()).reduce(combine).unwrap()).unwrap()
    print(iter([(1, 10), (2, 20)].unwrap()).find(def(a: &i64, b: &i64) -> bool: *b == 20).unwrap()).unwrap()
    print(iter(range(3)).enumerate().any(def(i: &i64, n: &i64) -> bool: *i == 1)).unwrap()
    print(iter(range(3)).enumerate().all(def(i: &i64, n: &i64) -> bool: *i == *n)).unwrap()
    print(iter(range(3)).enumerate().try_for_each(visit).is_err()).unwrap()
"#,
        "(3, 30)\n(2, 20)\nTrue\nTrue\n0 0\nTrue\n",
    );
}

#[test]
fn borrowed_pairs_retain_loans_and_cannot_escape_into_collections() {
    for source in [
        "mut xs = [1, 2].unwrap()\np = enumerate(&xs)\nxs.append(3).unwrap()\nprint(p.count()).unwrap()",
        "xs = [1, 2].unwrap()\np = zip(&xs, &xs)\nvalues: list[(i64, i64)] = p.collect().unwrap()",
        "xs = [1, 2].unwrap()\np = enumerate(enumerate(&xs))\nprint(p.count()).unwrap()",
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(
            error.contains("borrow") || error.contains("reference"),
            "{error}"
        );
    }
}

#[test]
fn mutable_pair_items_are_shared_in_predicates_and_mutable_in_mapping() {
    output(
        r#"
def bump(i: i64, n: &mut i64) -> i64:
    *n += i
    *n
def main() -> ():
    mut values = [10, 20].unwrap()
    print(iter(&mut values).enumerate().filter(def(i: &i64, n: &i64) -> bool: *n > 0).map(bump).count()).unwrap()
    print(values).unwrap()
"#,
        "2\n[10, 21]\n",
    );
    let error = support::check_source("mut values = [1].unwrap()\np = iter(&mut values).filter(def(n: &mut i64) -> bool: True)\nprint(p.count()).unwrap()").unwrap_err().to_string();
    assert!(
        error.contains("shared") || error.contains("mutable"),
        "{error}"
    );
}

#[test]
fn enumerate_counts_accepted_items_and_preserves_maximum_final_index() {
    output(
        r#"
def main() -> ():
    for i, n in iter(range(5)).filter(def(n: &i64) -> bool: *n % 2 == 0).enumerate(start=7):
        print(i, n).unwrap()
    print(iter(range(1)).enumerate(9223372036854775807).count()).unwrap()
    print(iter(range(3)).zip(range(3)).take(0).count()).unwrap()
"#,
        "7 0\n8 2\n9 4\n1\n0\n",
    );
}

#[test]
fn optimized_callbacks_preserve_imported_and_caller_trap_sites() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("arithmetic.plenty"),
        "pub def square(n: i64) -> i64:\n    n * n\n",
    )
    .unwrap();
    let source = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    for (body, expected) in [
        (
            "    print(iter(range(4000000000, 4000000001)).map(square).count()).unwrap()\n",
            "arithmetic.plenty:2:5: integer overflow",
        ),
        (
            "    print(iter(range(2)).map(square).count()).unwrap()\n    zero = 0\n    print(1 // zero).unwrap()\n",
            "main.plenty:5:11: division by zero",
        ),
    ] {
        std::fs::write(
            &source,
            format!("from arithmetic import square\ndef main() -> ():\n{body}"),
        )
        .unwrap();
        for optimize in [false, true] {
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"));
            if optimize {
                command.arg("--release");
            }
            let compiled = command
                .args(["--no-cache", "--compile"])
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                compiled.status.success(),
                "{}",
                String::from_utf8_lossy(&compiled.stderr)
            );
            let result = std::process::Command::new(&executable).output().unwrap();
            assert_eq!(result.status.code(), Some(1));
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(expected),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}

#[test]
fn changing_inlined_callback_invalidates_cached_caller() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, "from arithmetic import bump\ndef main() -> ():\n    print(iter(range(3)).map(bump).fold(0, def(a: i64, b: i64) -> i64: a + b)).unwrap()\n").unwrap();
    for (delta, expected) in [(1, "6\n"), (2, "9\n")] {
        std::fs::write(
            dir.path().join("arithmetic.plenty"),
            format!("pub def bump(n: i64) -> i64:\n    n + {delta}\n"),
        )
        .unwrap();
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(["--release", "--no-program-cache"])
            .arg(&source)
            .env("PLENTY_CACHE_DIR", dir.path().join("cache"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&result.stdout), expected);
    }
}

#[test]
fn collecting_uses_the_expected_collection_type() {
    output(
        r#"
def main() -> Result[(), AllocError | IoError | ParseError]:
    xs: list[i64] = iter(range(5)).map(def(n: i64) -> i64: n * n).collect()?
    seen: set[i64] = iter(range(5)).map(def(n: i64) -> i64: n % 2).collect()?
    parsed: list[i64] = iter(["1", "2", "3"]?).map(def(s: str) -> Result[i64, ParseError]: i64.parse(s)).try_collect()?
    print(xs, len(seen), parsed)?
    Ok(())
"#,
        "[0, 1, 4, 9, 16] 2 [1, 2, 3]\n",
    );
}

#[test]
fn fallible_callback_terminals_stop_at_first_error() {
    output(
        r#"
def add(a: i64, b: i64) -> Result[i64, str]:
    if b == 3:
        return Err("stop")
    print("add", b).unwrap()
    Ok(a + b)
def visit(n: i64) -> Result[(), str]:
    if n == 2:
        return Err("stop")
    print("visit", n).unwrap()
    Ok(())
def main() -> ():
    a = iter(range(5)).try_fold(0, add)
    print(str.repr(&a).unwrap()).unwrap()
    b = iter(range(0)).try_fold(10, add)
    print(str.repr(&b).unwrap()).unwrap()
    c = iter(range(5)).try_for_each(visit)
    print(str.repr(&c).unwrap()).unwrap()
    d = iter(range(0)).try_for_each(visit)
    print(str.repr(&d).unwrap()).unwrap()
"#,
        "add 0\nadd 1\nadd 2\nResult[i64, str].Err(\"stop\")\nResult[i64, str].Ok(10)\nvisit 0\nvisit 1\nResult[(), str].Err(\"stop\")\nResult[(), str].Ok(())\n",
    );
}

#[test]
fn collection_context_and_fallible_item_diagnostics_are_specific() {
    for (source, message) in [
        (
            "xs = iter(range(3)).collect().unwrap()",
            "expected list[T] or set[T]",
        ),
        (
            "xs: list[i64] = iter(range(3)).try_collect().unwrap()",
            "Result items",
        ),
        (
            "xs: list[i64] = iter([1, 2].unwrap()).map(def(n: i64) -> bool: True).collect().unwrap()",
            "expected i64",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn try_collect_drops_partial_output_and_unvisited_owned_items() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def checked(g: Guard) -> Result[Guard, str]:
    if g.n == 2:
        return Err("two")
    Ok(g)
def main() -> ():
    values = [Guard(1), Guard(2), Guard(3)].unwrap()
    result: Result[list[Guard], AllocError | str] = iter(values).map(checked).try_collect()
    match result:
        case Ok(_):
            print("unexpected").unwrap()
        case Err(_):
            print("failed").unwrap()
"#,
        "drop 2\ndrop 1\ndrop 3\nfailed\n",
    );
}

#[test]
fn generator_collection_transfers_affine_items_into_the_owner() {
    // Issue #398: a borrowed LoadLocal item leaked the destructor-bearing value.
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def guards() -> Generator[Guard]:
    yield Guard(1)
    yield Guard(2)
def main() -> ():
    ordinary = list(guards()).unwrap()
    drop(ordinary)
    pipeline: list[Guard] = iter(guards()).collect().unwrap()
    drop(pipeline)
"#,
        "drop 1\ndrop 2\ndrop 1\ndrop 2\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn known_length_collect_reserves_once_and_reports_each_allocation_failure() {
    for budget in 0..=2 {
        let source = format!(
            r#"
def build() -> Result[list[i64], AllocError]:
    iter(range(100)).map(def(n: i64) -> i64: n * 2).skip(5).take(12).collect()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = build()
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(values):
        print(len(values), values[0], values[11]).unwrap()
    case Err(_):
        print("allocation failed").unwrap()
"#
        );
        support::assert_visible_stdout(
            &source,
            if budget == 2 {
                "12 10 32"
            } else {
                "allocation failed"
            },
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn fallible_non_collecting_terminals_add_no_allocation() {
    support::assert_visible_stdout(
        r#"
def add(a: i64, b: i64) -> Result[i64, str]:
    Ok(a + b)
def visit(n: i64) -> Result[(), str]:
    Ok(())
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
total = iter(range(10)).try_fold(0, add).unwrap()
iter(range(10)).try_for_each(visit).unwrap()
print("__test_end_no_allocations__").unwrap()
print("__test_restore_allocations__").unwrap()
print(total).unwrap()
"#,
        "45",
    );
}

fn output(source: &str, expected: &str) {
    let result = support::run(source);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), expected);
}

#[test]
fn fused_range_and_borrowed_list_fold() {
    output(
        r#"
def even(n: &i64) -> bool:
    *n % 2 == 0
def square(n: i64) -> i64:
    n * n
def add(a: i64, b: i64) -> i64:
    a + b
def main() -> ():
    p = iter(range(10)).filter(even).map(square)
    print(p.fold(0, add)).unwrap()
    values = [1, 2, 3, 4].unwrap()
    total = iter(&values).map(def(n: &i64) -> i64: *n * 2).fold(0, add)
    print(total, len(values)).unwrap()
"#,
        "120\n20 4\n",
    );
}

#[test]
fn adapters_preserve_order_and_bounds() {
    output(
        r#"
def main() -> ():
    for n in iter(range(10)).skip(2).take(4).filter(def(n: &i64) -> bool: *n % 2 == 1):
        print(n).unwrap()
    print(iter(range(10)).take_while(def(n: &i64) -> bool: *n < 3).count()).unwrap()
    print(iter(range(10)).take(0).count(), iter(range(10)).skip(-1).count()).unwrap()
"#,
        "3\n5\n3\n0 10\n",
    );
}

#[test]
fn pipeline_is_affine() {
    let error = support::check_source(
        r#"
def main() -> ():
    p = iter(range(3))
    print(p.count()).unwrap()
    print(p.count()).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("moved"), "{error}");
}

#[test]
fn source_loan_protects_delayed_consumption() {
    let error = support::check_source(
        r#"
def main() -> ():
    mut values = [1, 2].unwrap()
    p = iter(&values)
    values.append(3).unwrap()
    print(p.count()).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn callbacks_and_owned_sources_are_released_on_break_and_drop() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def main() -> ():
    values = [Guard(1), Guard(2), Guard(3)].unwrap()
    capture = Guard(9)
    p = iter(values).map(def [capture](g: Guard) -> i64: g.n + capture.n)
    for n in p:
        print(n).unwrap()
        break
    print("after").unwrap()
    unused = iter([Guard(4)].unwrap()).map(def(g: Guard) -> i64: g.n)
    drop(unused)
    print("done").unwrap()
"#,
        "drop 1\n10\ndrop 9\ndrop 2\ndrop 3\nafter\ndrop 4\ndone\n",
    );
}

#[test]
fn terminal_short_circuit_and_empty_reduction() {
    output(
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def main() -> ():
    print(iter(range(5)).reduce(add), iter(range(0)).reduce(add)).unwrap()
    print(iter(range(5)).any(def(n: &i64) -> bool: *n == 2)).unwrap()
    print(iter(range(5)).all(def(n: &i64) -> bool: *n < 3)).unwrap()
    print(iter(range(5)).find(def(n: &i64) -> bool: *n > 2)).unwrap()
    print(iter(range(0)).any(def(n: &i64) -> bool: True), iter(range(0)).all(def(n: &i64) -> bool: False)).unwrap()
"#,
        "Option[i64].Some(10) Option[i64].Nothing\nTrue\nFalse\nOption[i64].Some(3)\nFalse True\n",
    );
}

#[test]
fn captured_mutable_borrow_is_retained_until_consumption() {
    output(
        r#"
def main() -> ():
    mut seen = 0
    visit = def [&mut seen](n: i64) -> i64:
        seen += 1
        n * 2
    p = iter(range(5)).map(visit)
    print(p.take(2).count(), seen).unwrap()
"#,
        "2 2\n",
    );
}

#[test]
fn local_type_annotation_and_name_shadowing() {
    output(
        r#"
def main() -> ():
    p: Iter[i64] = iter(range(3))
    print(p.count()).unwrap()
"#,
        "3\n",
    );
    output(
        r#"
def iter(n: i64) -> i64:
    n + 1
def main() -> ():
    print(iter(3)).unwrap()
"#,
        "4\n",
    );
}

#[test]
fn unsupported_placements_are_rejected() {
    for source in [
        "def bad() -> Iter[i64]:\n    iter(range(3))\n",
        "def bad(p: Iter[i64]) -> ():\n    pass\n",
        "def bad(p: &mut Iter[i64]) -> ():\n    pass\n",
        "def bad() -> Option[Iter[i64]]:\n    Nothing\n",
        "p = iter(range(3))\nprint(&p).unwrap()\n",
        "p = iter(range(3))\nprint(p).unwrap()\n",
        "p = iter(range(3))\nf = (def [p]() -> i64: p.count())\n",
        "p = iter(range(3))\nvalues = [p].unwrap()\n",
        "def bad() -> Generator[i64]:\n    p = iter(range(3))\n    yield 1\n",
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains("pipeline"), "{source}\n{error}");
    }
}

#[test]
fn empty_and_dropped_pipeline_does_not_call_callbacks_or_advance_generator() {
    output(
        r#"
def numbers() -> Generator[i64]:
    print("advanced").unwrap()
    yield 1
def visit(n: i64) -> i64:
    print("callback").unwrap()
    n
def main() -> ():
    p = iter(numbers()).map(visit)
    drop(p)
    print(iter(numbers()).map(visit).take(0).count()).unwrap()
"#,
        "0\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn non_collecting_terminals_do_not_allocate() {
    output(
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def visit(n: i64) -> ():
    pass
def main() -> ():
    values = [1, 2, 3].unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    print("__test_begin_no_allocations__").unwrap()
    a = iter(&values).map(def(n: &i64) -> i64: *n).fold(0, add)
    b = iter(range(3)).reduce(add)
    c = iter(range(3)).count()
    d = iter(range(3)).any(def(n: &i64) -> bool: *n == 1)
    e = iter(range(3)).all(def(n: &i64) -> bool: *n < 3)
    f = iter(range(3)).find(def(n: &i64) -> bool: *n == 2)
    iter(range(3)).for_each(visit)
    print("__test_end_no_allocations__").unwrap()
    print("__test_restore_allocations__").unwrap()
    print(a, b, c, d, e, f).unwrap()
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n6 Option[i64].Some(3) 3 True True Option[i64].Some(2)\n",
    );
}

#[test]
fn fused_range_and_borrowed_list_have_no_adapter_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("pipeline.plenty");
    std::fs::write(&source, r#"
def add(a: i64, b: i64) -> i64:
    a + b
def range_total(n: i64) -> i64:
    iter(range(n)).filter(def(n: &i64) -> bool: *n % 2 == 0).map(def(n: i64) -> i64: n * n).fold(0, add)
def borrowed_total(values: &list[i64]) -> i64:
    iter(values).map(def(n: &i64) -> i64: *n).fold(0, add)
def main() -> ():
    values = [1, 2, 3].unwrap()
    print(range_total(10), borrowed_total(&values)).unwrap()
"#).unwrap();
    for function in ["range_total", "borrowed_total"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(["--release", "--dump", "clif,asm", "--dump-filter", function])
            .arg(&source)
            .args(["-o", "-"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let dump = String::from_utf8(output.stdout).unwrap();
        for absent in [
            "call_indirect",
            "plenty_collection_call",
            "plenty_generator_resume",
            "__plenty_fn_add",
        ] {
            assert!(!dump.contains(absent), "{function}: {absent}\n{dump}");
        }
        assert!(dump.contains("call"), "{dump}");
    }
}

#[test]
fn converted_range_has_one_fused_frame_and_no_adapter_resume_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("bridge.plenty");
    std::fs::write(
        &source,
        r#"
def even(n: &i64) -> bool:
    *n % 2 == 0
def square(n: i64) -> i64:
    n * n
def build(stop: i64) -> Generator[i64]:
    iter(range(stop)).filter(even).map(square).generator()
def main() -> ():
    mut values = build(10)
    drop(next(values))
"#,
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args([
            "--release",
            "--no-cache",
            "--dump",
            "clif",
            "--dump-filter",
            "__plenty_pipeline_",
        ])
        .arg(&source)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = String::from_utf8(output.stdout).unwrap();
    assert_eq!(dump.matches("kind=resume").count(), 1, "{dump}");
    assert_eq!(dump.matches("kind=function").count(), 1, "{dump}");
    for absent in [
        "plenty_generator_resume",
        "call_indirect",
        "__plenty_fn_even",
        "__plenty_fn_square",
    ] {
        assert!(!dump.contains(absent), "{absent}\n{dump}");
    }
}
