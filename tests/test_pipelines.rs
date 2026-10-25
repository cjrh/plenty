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

#[test]
fn filter_value_keeps_items_equal_to_a_value() {
    output(
        r#"
def numbers() -> Generator[i64]:
    for n in [2, 7, 2].unwrap():
        yield n
def main() -> Result[(), AllocError | IoError]:
    xs = [3, 1, 3, 2, 3]?
    print(iter(&xs).filter_value(3).count(), iter(xs).filter_value(1).count())?
    flags = [True, False, True]?
    print(iter(&flags).filter_value().count(), iter(&flags).filter_value(False).count())?
    names = ["ada", "bob", "ada"]?
    for i, name in iter(&names).filter_value("ada").enumerate():
        print(i, name)?
    small: list[u8] = iter(range(10)).map(def(n: i64) -> u8: u8(n % 3)).filter_value(2).collect()?
    print(small, iter(range(10)).filter_value(value=4).count())?
    print(numbers().filter_value(2).chain(range(5)).filter_value(2).count())?
    mut counts = [1, 2, 1]?
    for n in iter(&mut counts).filter_value(1):
        *n += 10
    print(counts)?
    mut stream = iter(range(10)).filter_value(7).generator()
    print(next(stream), next(stream))?
    Ok(())
"#,
        "3 1\n2 1\n0 ada\n1 ada\n[2, 2, 2] 1\n3\n[11, 2, 11]\nOption[i64].Some(7) Option[i64].Nothing\n",
    );
}

#[test]
fn filter_value_evaluates_once_and_drops_rejected_items_and_the_value() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def wanted() -> Guard:
    print("value").unwrap()
    Guard(1)
def main() -> Result[(), AllocError | IoError]:
    guards = [Guard(1), Guard(2), Guard(1)]?
    kept = iter(guards).filter_value(wanted())
    print("built")?
    for g in kept:
        print("keep", g.n)?
    print("done")?
    Ok(())
"#,
        "value\nbuilt\nkeep 1\ndrop 1\ndrop 2\nkeep 1\ndrop 1\ndrop 1\ndone\n",
    );
}

#[test]
fn filter_value_compares_borrowed_items_with_a_borrowed_value() {
    output(
        r#"
def main() -> Result[(), AllocError | IoError]:
    names = ["ada", "bob", "ada"]?
    wanted = copy(names[0])?
    print(iter(&names).filter_value(&wanted).count(), wanted)?
    found = &wanted
    print(iter(&names).filter_value(found).count())?
    print(iter(&names).filter_value(&names[1]).count())?
    mut counts = [1, 2, 1]?
    one = 1
    for n in iter(&mut counts).filter_value(&one):
        *n += 10
    print(counts)?
    Ok(())
"#,
        "2 ada\n2\n1\n[11, 2, 11]\n",
    );
}

#[test]
fn filter_value_keeps_a_borrowed_value_borrowed_until_the_pipeline_ends() {
    for (source, expected) in [
        (
            "names = [\"ada\", \"bob\"].unwrap()\nwanted = copy(names[0]).unwrap()\nkept = iter(&names).filter_value(&wanted)\ndrop(wanted)\nn = kept.count()\n",
            "use of moved binding `wanted`",
        ),
        (
            "names = [\"ada\", \"bob\"].unwrap()\nmut wanted = copy(names[0]).unwrap()\nkept = iter(&names).filter_value(&wanted)\nwanted = copy(names[1]).unwrap()\nn = kept.count()\n",
            "cannot assign to `wanted` while it is borrowed",
        ),
        (
            "names = [\"ada\", \"bob\"].unwrap()\nmut wanted = copy(names[0]).unwrap()\nfor name in iter(&names).filter_value(&wanted):\n    wanted = copy(names[1]).unwrap()\n",
            "cannot assign to `wanted` while it is borrowed",
        ),
        (
            "mut ys = [1, 2, 1].unwrap()\nfor y in iter(&mut ys).filter_value(&ys[0]):\n    *y += 10\n",
            "while `ys` is exclusively borrowed",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}: {error}");
    }
    // Releasing the pipeline ends the loan.
    output(
        r#"
def main() -> Result[(), AllocError | IoError]:
    names = ["ada", "bob"]?
    mut wanted = copy(names[0])?
    print(iter(&names).filter_value(&wanted).count())?
    wanted = copy(names[1])?
    print(wanted)?
    Ok(())
"#,
        "1\nbob\n",
    );
}

#[test]
fn filter_value_requires_a_value_of_the_item_type() {
    for (source, expected) in [
        (
            "xs = [3, 1].unwrap()\nn = iter(&xs).filter_value().count()\n",
            "filter_value() keeps items equal to True, but this pipeline yields &i64",
        ),
        (
            "xs = [3, 1].unwrap()\nn = iter(&xs).filter_value(\"3\").count()\n",
            "filter_value compares &i64 items with a str value; it requires i64 or &i64",
        ),
        (
            "xs = [3, 1].unwrap()\nmut v = 3\nn = iter(&xs).filter_value(&mut v).count()\n",
            "filter_value compares &i64 items with a &mut i64 value; it requires i64 or &i64",
        ),
        (
            "v = 3\nn = iter([3, 1].unwrap()).filter_value(&v).count()\n",
            "filter_value compares i64 items with a &i64 value; it requires i64",
        ),
        (
            "n = iter(range(3)).filter_value(1.0).count()\n",
            "filter_value compares i64 items with a f64 value; it requires i64",
        ),
        (
            "xs = [3, 1].unwrap()\nn = iter(&xs).enumerate().filter_value(3).count()\n",
            "filter_value does not accept pair items",
        ),
        (
            "n = iter(range(3)).filter_value(1, 2).count()\n",
            "filter_value takes at most one value",
        ),
        (
            "n = iter(range(3)).filter_value(item=1).count()\n",
            "filter_value has no parameter named `item`",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}: {error}");
    }
}

#[test]
fn filter_value_lowers_to_an_inline_comparison() {
    let source = r#"
def matches(xs: &list[i64], v: i64) -> i64:
    iter(xs).filter_value(v).count()
def main() -> ():
    xs = [1, 2, 1].unwrap()
    print(matches(&xs, 1)).unwrap()
"#;
    let ops = dump_ops(source, "matches");
    let names: Vec<_> = ops
        .lines()
        .map(|line| line.trim().split([' ', '(']).next().unwrap())
        .collect();
    assert!(names.contains(&"Eq"), "{ops}");
    assert!(
        !names
            .iter()
            .any(|name| name.starts_with("Call") && *name != "CallerSite"
                || name.starts_with("Closure")),
        "{ops}"
    );
    output(source, "2\n");
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

#[test]
fn chain_yields_both_sides_in_order() {
    output(
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def small(n: &i64) -> bool:
    *n < 3
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2]?
    ys = [3]?
    for x in iter(&xs).chain(&ys):
        print(*x)?
    print(iter(range(3)).chain(range(10, 12)).fold(0, add))?
    for n in iter(range(10)).take_while(small).chain(range(100, 102)).take(4):
        print(n)?
    for n in iter(range(6)).filter(def(n: &i64) -> bool: *n % 2 == 1).chain(range(0)):
        print(n)?
    print(iter(range(0)).chain(range(0)).count())?
    Ok(())
"#,
        "1\n2\n3\n24\n0\n1\n2\n100\n1\n3\n5\n0\n",
    );
}

#[test]
fn chain_pairs_borrows_and_mutable_references() {
    output(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2]?
    mut ys = [3]?
    for x in iter(&mut xs).chain(&mut ys):
        *x += 10
    print(xs, ys)?
    names = ["a", "b"]?
    more = ["c"]?
    for i, s in iter(&names).enumerate().chain(iter(&more).enumerate(start=5)):
        print(i, s)?
    for a, b in zip(iter(range(2)).chain(range(7, 9)), iter(&names).chain(&more)):
        print(a, b)?
    counted: list[i64] = iter(range(3)).chain(range(7, 9)).map(def(n: i64) -> i64: n + 1).collect()?
    print(counted)?
    bounded: list[i64] = iter(range(9223372036854775807)).chain(range(10)).take(3).collect()?
    print(bounded)?
    rows = [[1, 2]?, [3]?]?
    firsts: list[i64] = iter(&rows).flat_map(def(r: &list[i64]) -> list[i64]: [r[0]].unwrap()).collect()?
    print(firsts)?
    Ok(())
"#,
        "[11, 12] [13]\n0 a\n1 b\n5 c\n0 a\n1 b\n7 c\n[1, 2, 3, 8, 9]\n[0, 1, 2]\n[1, 3]\n",
    );
}

#[test]
fn chain_requires_the_same_item_type_and_keeps_both_loans() {
    let error = support::check_source(
        r#"
def main() -> ():
    print(iter(range(3)).chain(["a"].unwrap()).count()).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("chain requires both sides to yield the same item type"),
        "{error}"
    );
    let error = support::check_source(
        r#"
def main() -> ():
    mut xs = [1, 2].unwrap()
    ys = [3].unwrap()
    for x in iter(&ys).chain(&xs):
        xs.append(*x).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn flat_map_yields_each_inner_source_in_order() {
    output(
        r#"
def twice(n: i64) -> list[i64]:
    [n, n * 10].unwrap()
def numbers(n: i64) -> Generator[i64]:
    for i in range(n):
        yield i * 100
def words(n: i64) -> list[str]:
    mut out = list[str]().unwrap()
    for i in range(n):
        out.append(f"w{n}.{i}".unwrap()).unwrap()
    out
def main() -> Result[(), AllocError | IoError]:
    for n in iter(range(3)).flat_map(twice):
        print(n)?
    for n in iter(range(4)).flat_map(def(n: i64) -> range[i64]: range(n)).take(4):
        print(n)?
    print(iter(range(4)).flat_map(numbers).count())?
    print(iter(range(5)).filter(def(n: &i64) -> bool: *n % 2 == 0).flat_map(words).count())?
    for i, s in enumerate(iter(range(3)).flat_map(words).chain(["end"]?)):
        print(i, s)?
    for a, b in zip(iter(range(3)).flat_map(twice), iter(range(2)).flat_map(twice).skip(1)):
        print(a, b)?
    print(iter(range(10)).take(2).flat_map(twice).flat_map(twice).count())?
    print(iter(range(5)).flat_map(numbers).find(def(n: &i64) -> bool: *n == 200))?
    Ok(())
"#,
        "0\n0\n1\n10\n2\n20\n0\n0\n1\n0\n6\n6\n0 w1.0\n1 w2.0\n2 w2.1\n3 end\n0 0\n0 1\n1 10\n8\nOption[i64].Some(200)\n",
    );
}

#[test]
fn flat_map_releases_inner_sources_on_exhaustion_break_and_drop() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def guards(n: i64) -> list[Guard]:
    mut out = list[Guard]().unwrap()
    for i in range(2):
        out.append(Guard(n * 10 + i)).unwrap()
    out
def main() -> ():
    for g in iter(range(1, 4)).flat_map(guards):
        print("got", g.n).unwrap()
        if g.n == 20:
            break
    print("after").unwrap()
    p = iter(range(1, 3)).flat_map(guards)
    drop(p)
    print(iter(range(1, 3)).flat_map(guards).take(1).count()).unwrap()
    print("done").unwrap()
"#,
        "got 10\ndrop 10\ngot 11\ndrop 11\ngot 20\ndrop 20\ndrop 21\nafter\ndrop 10\ndrop 11\n1\ndone\n",
    );
}

#[test]
fn flat_map_callbacks_must_return_owned_sources() {
    for source in [
        "def main() -> ():\n    print(iter(range(3)).flat_map(def(n: i64) -> i64: n).count()).unwrap()\n",
        "def main() -> ():\n    print(iter(range(3)).flat_map(def(n: i64) -> Result[list[i64], AllocError]: [n]).count()).unwrap()\n",
        "def same(row: &list[i64]) -> &list[i64]:\n    row\ndef main() -> ():\n    xs = [[1].unwrap()].unwrap()\n    print(iter(&xs).flat_map(same).count()).unwrap()\n",
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(
            error.contains("flat_map callbacks must return an owned"),
            "{source}\n{error}"
        );
    }
}

#[test]
fn chain_and_flat_map_convert_to_generators() {
    output(
        r#"
def twice(n: i64) -> list[i64]:
    [n, n].unwrap()
def build(n: i64) -> Generator[i64]:
    iter(range(n)).flat_map(twice).chain(range(50, 52)).generator()
def main() -> Result[(), AllocError | IoError]:
    for n in build(2):
        print(n)?
    mut partial = build(3)
    print(next(partial))?
    drop(partial)
    Ok(())
"#,
        "0\n0\n1\n1\n50\n51\nOption[i64].Some(0)\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn chain_and_flat_map_terminals_do_not_allocate() {
    output(
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def main() -> ():
    values = [1, 2, 3].unwrap()
    more = [4].unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    print("__test_begin_no_allocations__").unwrap()
    a = iter(&values).chain(&more).map(def(n: &i64) -> i64: *n).fold(0, add)
    b = iter(range(4)).flat_map(def(n: i64) -> range[i64]: range(n)).fold(0, add)
    c = iter(range(3)).chain(range(5)).any(def(n: &i64) -> bool: *n == 4)
    print("__test_end_no_allocations__").unwrap()
    print("__test_restore_allocations__").unwrap()
    print(a, b, c).unwrap()
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n10 4 True\n",
    );
}

#[test]
fn chain_and_flat_map_have_no_adapter_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("compose.plenty");
    std::fs::write(
        &source,
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def spread(n: i64) -> range[i64]:
    range(n)
def chained(values: &list[i64], n: i64) -> i64:
    iter(values).map(def(v: &i64) -> i64: *v).chain(range(n)).fold(0, add)
def flattened(n: i64) -> i64:
    iter(range(n)).flat_map(spread).fold(0, add)
def main() -> ():
    values = [1, 2, 3].unwrap()
    print(chained(&values, 4), flattened(5)).unwrap()
"#,
    )
    .unwrap();
    for function in ["chained", "flattened"] {
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
    }
}

#[test]
fn chain_terminals_split_per_side_and_keep_early_exits() {
    // A terminal runs one loop per chain side; an early exit in the first
    // loop must skip the second.
    output(
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def small(n: &i64) -> bool:
    *n < 3
def check(n: i64) -> Result[(), str]:
    if n == 11:
        return Err("eleven")
    Ok(())
def main() -> Result[(), AllocError | IoError]:
    print(iter(range(5)).chain(range(10, 15)).any(def(n: &i64) -> bool: *n == 2))?
    print(iter(range(5)).chain(range(10, 15)).any(def(n: &i64) -> bool: *n == 12))?
    print(iter(range(5)).chain(range(10, 15)).all(def(n: &i64) -> bool: *n < 12))?
    print(iter(range(5)).chain(range(10, 15)).find(def(n: &i64) -> bool: *n > 3))?
    print(iter(range(5)).chain(range(10, 15)).find(def(n: &i64) -> bool: *n > 13))?
    print(iter(range(10)).take_while(small).chain(range(100, 103)).fold(0, add))?
    print(iter(range(3)).chain(range(10, 13)).take(4).fold(0, add))?
    print(iter(range(3)).chain(range(10, 13)).take(2).count())?
    print(iter(range(2)).chain(range(5, 7)).chain(range(20, 22)).fold(0, add))?
    print(iter(range(2)).chain(range(5, 7)).enumerate().map(def(i: i64, n: i64) -> i64: i * 100 + n).fold(0, add))?
    words = ["a", "bb"]?
    more = ["ccc"]?
    print(iter(words).chain(more).map(def(s: str) -> i64: s.byte_len()).fold(0, add))?
    print(iter(range(3)).chain(range(10, 13)).try_for_each(check).is_err())?
    print(iter(range(3)).chain(range(10, 13)).reduce(add))?
    print(iter(range(0)).chain(range(0)).reduce(add))?
    Ok(())
"#,
        "True\nTrue\nFalse\nOption[i64].Some(4)\nOption[i64].Some(14)\n306\n13\n2\n53\n612\n6\nTrue\nOption[i64].Some(36)\nOption[i64].Nothing\n",
    );
}

#[test]
fn spliced_closure_literals_keep_call_semantics() {
    output(
        r#"
def show(tag: str, n: i64) -> i64:
    print(tag, n).unwrap()
    n
def main() -> Result[(), AllocError | IoError]:
    mut k = 3
    scaled = iter(range(3)).map(def [k](n: i64) -> i64: n * k)
    k = 10
    print(scaled.fold(0, def(a: i64, b: i64) -> i64: a + b), k)?
    picked: list[i64] = iter(range(4)).filter(def(n: &i64) -> bool: show("test", *n) % 2 == 0).map(def(n: i64) -> i64: show("map", n) * 10).collect()?
    print(picked)?
    seen: list[i64] = []?
    kept: list[i64] = iter(range(5)).filter(def [mut seen](n: &i64) -> bool: seen.append(*n).is_ok() and len(seen) % 2 == 0).collect()?
    print(kept)?
    words = ["a", "bb"]?
    sizes: list[i64] = iter(range(2)).map(def [&words](i: i64) -> i64: len(words[i])).collect()?
    print(sizes, len(words))?
    print(iter(range(10)).find(def(n: &i64) -> bool: show("find", *n) > 1))?
    print(iter(range(10)).take_while(def(n: &i64) -> bool: show("while", *n) < 2).count())?
    nested: list[i64] = iter(range(3)).flat_map(def [k](n: i64) -> list[i64]: [n, n * k].unwrap()).collect()?
    print(nested)?
    Ok(())
"#,
        "9 10\ntest 0\nmap 0\ntest 1\ntest 2\nmap 2\ntest 3\n[0, 20]\n[1, 3]\n[1, 2] 2\nfind 0\nfind 1\nfind 2\nOption[i64].Some(2)\nwhile 0\nwhile 1\nwhile 2\n2\n[0, 0, 1, 10, 2, 20]\n",
    );
}

#[test]
fn spliced_closure_literals_keep_borrow_and_capture_diagnostics() {
    for (source, expected) in [
        (
            "mut xs = [1, 2].unwrap()\np = iter(range(2)).map(def [&xs](i: i64) -> i64: xs[i])\nxs[0] = 5\nprint(p.count()).unwrap()\n",
            "cannot modify or exclusively borrow `xs[0]` while `xs` is borrowed",
        ),
        (
            "n = 2\nprint(iter(range(3)).map(def(x: i64) -> i64: x + n).count()).unwrap()\n",
            "anonymous functions cannot capture `n`",
        ),
        (
            "xs = [1].unwrap()\nprint(iter(range(2)).map(def [xs](i: i64) -> list[i64]: xs).count()).unwrap()\n",
            "cannot move out of a closure capture",
        ),
        (
            "s = [1].unwrap()\np = iter(range(2)).map(def [s](i: i64) -> i64: len(s) + i)\nprint(len(s)).unwrap()\nprint(p.count()).unwrap()\n",
            "use of moved binding `s`",
        ),
    ] {
        let error = support::check_source(source).map_or_else(
            |error| error.to_string(),
            |()| format!("accepted:\n{source}"),
        );
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

/// Owned captures move into the plan and are dropped with it, after an early
/// exit and after each failed allocation; the runtime checks the balance.
#[cfg(feature = "runtime-checks")]
#[test]
fn spliced_owned_captures_are_released_on_early_exit_and_allocation_failure() {
    support::assert_visible_stdout(
        r#"
tags = [7, 8].unwrap()
print(iter(range(10)).any(def [tags](n: &i64) -> bool: *n == len(tags))).unwrap()
"#,
        "True",
    );
    for budget in 0..=8 {
        let source = format!(
            r#"
tags = [7, 8].unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result: Result[list[list[i64]], AllocError] = iter(range(3)).map(def [tags](n: i64) -> Result[list[i64], AllocError]: [n, tags[0]]).try_collect()
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(values):
        print(len(values), values[2][1]).unwrap()
    case Err(_):
        print("allocation failed").unwrap()
"#
        );
        support::assert_visible_stdout(
            &source,
            if budget == 8 {
                "3 7"
            } else {
                "allocation failed"
            },
        );
    }
}

fn dump_ops(source: &str, function: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pipeline.plenty");
    std::fs::write(&path, source).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(["--dump", "ops", "--dump-filter", function])
        .arg(&path)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn closure_literals_in_a_chain_lower_into_the_fused_loop() {
    let source = r#"
def scaled(values: &list[i64], k: i64) -> i64:
    iter(values).map(def [k](n: &i64) -> i64: *n * k).filter(def(n: &i64) -> bool: *n > 2).fold(0, def(a: i64, b: i64) -> i64: a + b)
def reductions(n: i64) -> bool:
    iter(range(n)).any(def(x: &i64) -> bool: *x == 7) and iter(range(n)).all(def(x: &i64) -> bool: *x < 9) and iter(range(n)).take_while(def(x: &i64) -> bool: *x < 3).find(def(x: &i64) -> bool: *x > 1).is_some()
def visit(n: i64) -> ():
    iter(range(n)).for_each(def(x: i64) -> (): print(x).unwrap())
def nested(n: i64) -> i64:
    iter(range(n)).flat_map(def(x: i64) -> list[i64]: [x, x].unwrap()).count()
def stored(n: i64, k: i64) -> i64:
    f = def [k](x: i64) -> i64:
        x * k
    iter(range(n)).map(f).count()
def fallible(n: i64) -> Result[i64, AllocError]:
    iter(range(n)).try_fold(0, def(a: i64, x: i64) -> Result[i64, AllocError]: Ok(a + len([x]?)))
def main() -> ():
    values = [1, 2, 3].unwrap()
    print(scaled(&values, 2), reductions(10), nested(2)).unwrap()
    visit(1)
    print(stored(2, 3), fallible(2).unwrap()).unwrap()
"#;
    for function in ["scaled", "reductions", "visit", "nested"] {
        let ops = dump_ops(source, function);
        for absent in ["ClosureNew", "ClosureCall", "__plenty_closure"] {
            assert!(!ops.contains(absent), "{function}: {absent}\n{ops}");
        }
    }
    // A closure held in a local and a body with `?` keep the call.
    for function in ["stored", "fallible"] {
        let ops = dump_ops(source, function);
        assert!(ops.contains("ClosureCall"), "{function}\n{ops}");
    }
}

#[test]
fn comprehensions_lower_to_the_same_ops_as_pipelines() {
    let source = r#"
def keep(x: &i64) -> bool:
    *x % 3 != 0
def twice(x: i64) -> i64:
    x * 2
def comprehension(n: i64) -> Result[list[i64], AllocError]:
    [twice(x) for x in range(n) if keep(&x)]
def pipeline(n: i64) -> Result[list[i64], AllocError]:
    iter(range(n)).filter(keep).map(twice).collect()
def main() -> ():
    print(comprehension(10).unwrap(), pipeline(10).unwrap()).unwrap()
"#;
    // Source positions and loan bookkeeping differ by construction.
    let normalized = |function| {
        dump_ops(source, function)
            .lines()
            .skip_while(|line| !line.starts_with("local "))
            .filter(|line| {
                let op = line.trim_start();
                let name = op.split([' ', '(']).next().unwrap();
                !["Site", "CallerSite", "Access", "Loan", "UseLoan", "EndLoan"].contains(&name)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(normalized("comprehension"), normalized("pipeline"));
    output(source, "[2, 4, 8, 10, 14, 16] [2, 4, 8, 10, 14, 16]\n");
}

#[test]
fn sum_min_and_max_reduce_numbers_and_text() {
    output(
        r#"
def numbers(n: i64) -> Generator[i64]:
    for i in range(n):
        yield i
def main() -> Result[(), AllocError | IoError]:
    mut xs = [3, -1, 7, -1]?
    print(iter(&xs).sum(), iter(&xs).min(), iter(&mut xs).max())?
    print(iter(range(0)).sum(), iter(range(0)).min(), iter(range(0)).max())?
    print(numbers(5).sum(), numbers(5).map(def(n: i64) -> i64: 10 - n).min())?
    print(iter(&xs).chain(&xs).sum(), iter(range(10)).filter(def(n: &i64) -> bool: *n % 3 == 0).sum())?
    small: u8 = iter(range(3)).map(def(n: i64) -> u8: u8(n)).sum()
    halves = iter(range(3)).map(def(n: i64) -> f32: f32(n) / 2.0).sum()
    print(small, halves, iter(range(0)).map(def(n: i64) -> f64: f64(n)).sum())?
    names = ["bob", "ada", "cy"]?
    print(iter(&names).min(), iter(names).max())?
    scores = {"a": 3, "b": 9}?
    print(scores.items().map(def(k: &str, v: &i64) -> i64: *v).max())?
    p = iter(range(4)).skip(1)
    print(p.sum())?
    Ok(())
"#,
        "8 Option[i64].Some(-1) Option[i64].Some(7)\n0 Option[i64].Nothing Option[i64].Nothing\n10 Option[i64].Some(6)\n16 18\n3 1.5 0.0\nOption[str].Some(\"ada\") Option[str].Some(\"cy\")\nOption[i64].Some(9)\n6\n",
    );
}

#[test]
fn float_min_and_max_follow_ieee_minimum_and_maximum() {
    // -0.0 orders below 0.0 in either order. The sign of a NaN result shows
    // that the first NaN item is kept, as `extremum_replaces` documents.
    output(
        r#"
import std.math as math
def sign(x: Option[f64]) -> f64:
    match x:
        case Some(v):
            math.copysign(1.0, v)
        case Nothing:
            0.0
def main() -> Result[(), AllocError | IoError]:
    neg = math.copysign(f64.NAN, -1.0)
    cases = [
        [0.0, -0.0]?, [-0.0, 0.0]?, [2.0, 1.0, 1.0]?,
        [neg, 2.0, f64.NAN]?, [2.0, f64.NAN, neg, 1.0]?, [2.0, 1.0, neg]?,
    ]?
    for values in &cases:
        low = iter(values).min()
        high = iter(values).max()
        print(low, sign(low), high, sign(high))?
    zeros = [0.0, -0.0]?
    print(iter(&zeros).map(def(z: &f64) -> f32: f32(*z)).min())?
    Ok(())
"#,
        concat!(
            "Option[f64].Some(-0.0) -1.0 Option[f64].Some(0.0) 1.0\n",
            "Option[f64].Some(-0.0) -1.0 Option[f64].Some(0.0) 1.0\n",
            "Option[f64].Some(1.0) 1.0 Option[f64].Some(2.0) 1.0\n",
            "Option[f64].Some(NaN) -1.0 Option[f64].Some(NaN) -1.0\n",
            "Option[f64].Some(NaN) 1.0 Option[f64].Some(NaN) 1.0\n",
            "Option[f64].Some(NaN) -1.0 Option[f64].Some(NaN) -1.0\n",
            "Option[f32].Some(-0.0)\n",
        ),
    );
}

#[test]
fn float_min_and_max_consume_every_item_after_nan() {
    output(
        r#"
def noisy(values: list[f64]) -> Generator[f64]:
    for v in values:
        print("item", v).unwrap()
        yield v
def main() -> Result[(), AllocError | IoError]:
    print(noisy([2.0, f64.NAN, 1.0]?).min())?
    Ok(())
"#,
        "item 2.0\nitem NaN\nitem 1.0\nOption[f64].Some(NaN)\n",
    );
}

#[test]
fn min_and_max_agree_with_std_math() {
    output(
        r#"
import std.math as math
# Same value, including the sign of a zero; any NaN matches any NaN.
def same(a: Option[f64], b: Option[f64]) -> bool:
    match a:
        case Some(x):
            match b:
                case Some(y):
                    if math.isnan(x) or math.isnan(y):
                        return math.isnan(x) and math.isnan(y)
                    x == y and math.copysign(1.0, x) == math.copysign(1.0, y)
                case Nothing:
                    False
        case Nothing:
            b.is_none()
def main() -> Result[(), AllocError | IoError]:
    neg = math.copysign(f64.NAN, -1.0)
    cases = [
        list[f64]()?, [1.5]?, [0.0, -0.0]?, [-0.0, 0.0]?, [3.0, 1.0, 1.0, 3.0]?,
        [neg, 2.0, f64.NAN]?, [2.0, f64.NAN, neg]?, [2.0, 1.0, neg]?,
        [f64.INF, -f64.INF, 0.0]?,
    ]?
    mut differences = 0
    for values in &cases:
        if not same(iter(values).min(), math.min_all(values)):
            differences += 1
        if not same(iter(values).max(), math.max_all(values)):
            differences += 1
        if len(values) == 2:
            if not same(iter(values).min(), Some(math.min(values[0], values[1]))):
                differences += 1
            if not same(iter(values).max(), Some(math.max(values[0], values[1]))):
                differences += 1
    ints = [4, -2, 9, -2]?
    names = ["bob", "ada", "cy", "ada"]?
    print(differences, iter(&ints).min() == math.min_all(&ints), iter(&ints).max() == math.max_all(&ints))?
    print(iter(&names).min() == math.min_all(&names), iter(&names).max() == math.max_all(&names))?
    Ok(())
"#,
        "0 True True\nTrue True\n",
    );
}

#[test]
fn sum_overflow_traps_like_addition() {
    let result = support::run(
        "def main() -> ():\n    values = [i64.MAX, 1].unwrap()\n    print(iter(&values).sum()).unwrap()\n",
    );
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("integer overflow"), "{stderr}");
    assert!(result.stdout.is_empty());
}

#[test]
fn sum_min_and_max_reject_unsupported_items() {
    for (source, message) in [
        (
            "print(iter([\"a\"].unwrap()).sum()).unwrap()",
            "sum requires numeric items, found str",
        ),
        (
            "print(iter([True].unwrap()).min()).unwrap()",
            "min requires numeric or str items, found bool",
        ),
        (
            "print(enumerate(range(3)).max()).unwrap()",
            "max requires numeric or str items, found tuple[i64, i64]",
        ),
        (
            "xs = [[1].unwrap()].unwrap()\nprint(iter(&xs).min()).unwrap()",
            "found &list[i64]",
        ),
        (
            "print(iter(range(3)).sum(1)).unwrap()",
            "sum takes no arguments",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}: {error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn sum_min_and_max_do_not_allocate() {
    support::assert_visible_stdout(
        r#"
values = [2.5, 1.0].unwrap()
names = ["bob", "ada"].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
a = iter(range(10)).sum()
b = iter(&values).min()
c = iter(&names).max()
d = iter(range(10)).map(def(n: i64) -> i64: n * 2).max()
print("__test_end_no_allocations__").unwrap()
print("__test_restore_allocations__").unwrap()
print(a, b, c, d).unwrap()
"#,
        "45 Option[f64].Some(1.0) Option[str].Some(\"bob\") Option[i64].Some(18)",
    );
}

#[test]
fn next_resumes_pipelines_in_their_creating_scope() {
    output(
        r#"
def main() -> Result[(), AllocError | IoError]:
    xs = ["a", "bb", "", "ccc", "dd"]?
    mut p = iter(xs).filter(def(s: &str) -> bool: s != "").map(def(s: &str) -> i64: len(s))
    print(next(p))?
    print(next(&mut p))?
    print(p.sum())?
    mut c = iter(range(2)).chain(range(10, 13))
    print(next(c))?
    print(next(c))?
    print(next(c))?
    print(c.count())?
    mut e = iter(range(3)).enumerate()
    print(next(e))?
    for i, x in e:
        print(i + x)?
    d: dict[i64, i64] = {1: 10, 2: 20, 3: 30}?
    mut k = iter(d.keys()).map(def(x: &i64) -> i64: *x * 2)
    print(next(k))?
    rest: list[i64] = k.collect()?
    print(rest)?
    mut f = iter(range(3)).flat_map(def(x: i64) -> list[i64]: [x, x].unwrap())
    print(next(f))?
    mut n = 0
    while True:
        match next(f):
            case Some(v):
                n += v
            case Nothing:
                break
    print(n, next(f))?
    names = ["ada", "bob", "ada"]?
    mut v = iter(&names).filter_value("ada").enumerate().map(def(i: i64, s: &str) -> i64: i + len(s))
    print(next(v))?
    print(v.count())?
    Ok(())
"#,
        "Option[i64].Some(1)\nOption[i64].Some(2)\n5\n\
Option[i64].Some(0)\nOption[i64].Some(1)\nOption[i64].Some(10)\n2\n\
Option[tuple[i64, i64]].Some((0, 0))\n2\n4\n\
Option[i64].Some(2)\n[4, 6]\n\
Option[i64].Some(0)\n6 Option[i64].Nothing\n\
Option[i64].Some(3)\n1\n",
    );
}

#[test]
fn take_while_and_zip_stay_empty_after_next_returns_nothing() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def small(g: &Guard) -> bool:
    g.n < 2
def main() -> ():
    values = [Guard(0), Guard(1), Guard(2), Guard(3)].unwrap()
    mut p = iter(values).take_while(small)
    print(next(p).is_some()).unwrap()
    print(next(p).is_some()).unwrap()
    print("third").unwrap()
    print(next(p).is_some()).unwrap()
    print(next(p).is_some()).unwrap()
    others = [Guard(10), Guard(11)].unwrap()
    mut z = iter(others).zip(range(1))
    print(next(z).is_some()).unwrap()
    print(next(z).is_some()).unwrap()
    print(next(z).is_some()).unwrap()
    mut q = iter(range(10)).take_while(def(x: &i64) -> bool: *x < 2)
    print(next(q), next(q), next(q), q.count()).unwrap()
    mut r = zip(range(5), range(1))
    print(next(r), next(r), r.count()).unwrap()
    print("end").unwrap()
"#,
        "True\ndrop 0\nTrue\ndrop 1\nthird\ndrop 2\nFalse\nFalse\n\
True\ndrop 10\nFalse\ndrop 11\nFalse\n\
Option[i64].Some(0) Option[i64].Some(1) Option[i64].Nothing 0\n\
Option[tuple[i64, i64]].Some((0, 0)) Option[tuple[i64, i64]].Nothing 0\n\
end\ndrop 3\n",
    );
}

#[test]
fn next_rejects_immutable_consumed_and_reference_pipelines() {
    let immutable = support::check_error(
        r#"
def main() -> ():
    p = iter(range(3))
    drop(next(p))
"#,
    );
    assert!(
        immutable.contains("next requires a mutable pipeline binding"),
        "{immutable}"
    );
    let consumed = support::check_error(
        r#"
def main() -> ():
    mut p = iter(range(3))
    drop(p.count())
    drop(next(p))
"#,
    );
    assert!(consumed.contains("use of moved binding `p`"), "{consumed}");
    let reference = support::check_error(
        r#"
def main() -> ():
    xs = [1, 2].unwrap()
    mut p = iter(&xs)
    drop(next(p))
"#,
    );
    assert!(
        reference.contains("references cannot be stored in a next result"),
        "{reference}"
    );
    let conflict = support::check_error(
        r#"
def main() -> ():
    mut xs = [1, 2].unwrap()
    mut p = iter(&xs).map(def(n: &i64) -> i64: *n)
    drop(next(p))
    xs.append(3).unwrap()
    drop(next(p))
"#,
    );
    assert!(conflict.contains("borrow"), "{conflict}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn next_partial_pipelines_release_owned_state() {
    support::assert_visible_stdout(
        r#"
def partial() -> Result[(), AllocError]:
    words = ["x", "y", "z"]?
    mut w = iter(words).map(def(s: &str) -> Result[str, AllocError]: s + "!")
    drop(next(w))
    mut f = iter(range(3)).flat_map(def(x: i64) -> list[i64]: [x, x].unwrap())
    drop(next(f))
    nested = [[1, 2]?, [3]?]?
    mut z = iter(nested).zip(range(1))
    drop(next(z))
    drop(next(z))
    Ok(())
def main() -> Result[(), AllocError | IoError]:
    for i in range(100):
        partial()?
    print("__test_small_live_heap__")?
    print("done")?
    Ok(())
"#,
        "done",
    );
}

#[test]
fn lent_pipelines_resume_in_the_lender() {
    output(
        r#"
class Reader:
    seen: i64
    def skip(self: &mut Reader, p: &mut Iter[i64]) -> ():
        _ = next(p)
        self.seen += 1

def first[T](p: &mut Iter[T]) -> Option[T]:
    next(p)

def drain(p: &mut Iter[i64]) -> i64:
    match next(p):
        case Some(x):
            return x + drain(p)
        case Nothing:
            return 0

def pairs(p: &mut Iter[i64]) -> Result[(), AllocError | IoError]:
    for a, b in zip(p, range(100, 102)):
        print(a, b)?
    Ok(())

def mapped(p: &mut Iter[i64]) -> Result[(), AllocError | IoError]:
    mut q = p.map(def(x: i64) -> i64: x * 10)
    print(next(q))?
    print(next(p))?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    lim = 3
    mut p = iter(range(20)).filter(def [lim](x: &i64) -> bool: *x != lim).take_while(def(x: &i64) -> bool: *x < 15)
    mut r = Reader(0)
    r.skip(&mut p)
    print(r.seen)?
    print(first(&mut p))?
    pairs(&mut p)?
    mapped(&mut p)?
    print(drain(&mut p))?
    print(next(p))?
    names = ["ada", "bob", "ada", "cy", "ada"]?
    mut v = iter(&names).filter_value("ada").map(def(s: &str) -> i64: len(s))
    print(first(&mut v))?
    print(v.count())?
    mut c = iter(range(2)).chain(range(5, 8))
    print(drain(&mut c))?
    mut f = iter(range(3)).flat_map(def(x: i64) -> list[i64]: [x, x + 10].unwrap())
    print(first(&mut f), first(&mut f), first(&mut f))?
    print(drain(&mut f))?
    Ok(())
"#,
        "1\nOption[i64].Some(1)\n2 100\n4 101\nOption[i64].Some(60)\nOption[i64].Some(7)\n77\n\
Option[i64].Nothing\nOption[i64].Some(3)\n2\n19\n\
Option[i64].Some(0) Option[i64].Some(10) Option[i64].Some(1)\n25\n",
    );
}

#[test]
fn lent_pipelines_write_back_on_every_exit() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()

enum Bad:
    Two

def check(g: &Guard) -> Result[i64, Bad]:
    if g.n == 2:
        return Err(Bad.Two)
    Ok(g.n)

def total(p: &mut Iter[Guard]) -> Result[i64, Bad]:
    mut t = 0
    for g in p:
        t += check(&g)?
    Ok(t)

def stop_at(p: &mut Iter[Guard], n: i64) -> i64:
    for g in p:
        if g.n == n:
            return g.n
    -1

def first(p: &mut Iter[Guard]) -> ():
    for g in p:
        print("saw", g.n).unwrap()
        break

def main() -> ():
    values = [Guard(0), Guard(1), Guard(2), Guard(3), Guard(4), Guard(5), Guard(6)].unwrap()
    mut p = iter(values)
    print(total(&mut p).is_err()).unwrap()
    print(stop_at(&mut p, 3)).unwrap()
    first(&mut p)
    print("next", next(p).is_some()).unwrap()
    print("count", p.count()).unwrap()
    print("end").unwrap()
"#,
        "drop 0\ndrop 1\ndrop 2\nTrue\ndrop 3\n3\nsaw 4\ndrop 4\n\
next True\ndrop 5\ndrop 6\ncount 1\nend\n",
    );
}

#[test]
fn lending_keeps_borrow_and_signature_rules() {
    for (source, expected) in [
        (
            "def f(p: &mut Iter[i64]) -> ():\n    pass\ndef main() -> ():\n    p = iter(range(3))\n    f(&mut p)\n",
            "requires a mutable binding",
        ),
        (
            "def f(p: &mut Iter[i64]) -> ():\n    pass\ndef main() -> ():\n    mut p = iter(range(3))\n    f(p)\n",
            "lend a pipeline with `&mut`",
        ),
        (
            "def f(p: &mut Iter[i64]) -> ():\n    pass\ndef main() -> ():\n    mut p = iter(range(3))\n    print(p.count()).unwrap()\n    f(&mut p)\n",
            "use of moved binding `p`",
        ),
        (
            "def f(p: &mut Iter[i64], q: &mut Iter[i64]) -> ():\n    pass\ndef main() -> ():\n    mut p = iter(range(3))\n    f(&mut p, &mut p)\n",
            "conflicting borrow",
        ),
        (
            "def f(p: &mut Iter[i64]) -> ():\n    pass\ndef g(p: &mut Iter[i64]) -> ():\n    for x in p:\n        f(&mut p)\ndef main() -> ():\n    mut p = iter(range(3))\n    g(&mut p)\n",
            "conflicting borrow",
        ),
        (
            "def f(p: &mut Iter[i64]) -> ():\n    pass\ndef main() -> Result[(), AllocError]:\n    mut xs = [1, 2]?\n    mut p = iter(&xs).map(def(x: &i64) -> i64: *x)\n    f(&mut p)\n    xs.append(3)?\n    drop(next(p))\n    Ok(())\n",
            "conflicting borrow",
        ),
        (
            "def g(p: &mut Iter[i64]) -> i64:\n    q = *p\n    q.count()\ndef main() -> ():\n    mut p = iter(range(3))\n    drop(g(&mut p))\n",
            "cannot escape their creating scope",
        ),
        (
            "def g(p: &mut Iter[i64]) -> i64:\n    h = def [p]() -> i64:\n        next(p).unwrap_or(0)\n    h()\ndef main() -> ():\n    mut p = iter(range(3))\n    drop(g(&mut p))\n",
            "cannot escape their creating scope",
        ),
        (
            "def g(p: &mut Iter[i64]) -> Generator[i64]:\n    for x in p:\n        yield x\ndef main() -> ():\n    mut p = iter(range(3))\n    drop(g(&mut p))\n",
            "generators cannot capture references",
        ),
        (
            "def f(p: &mut Iter[&i64]) -> &i64:\n    for x in p:\n        return x\n    p.count()\ndef main() -> Result[(), AllocError]:\n    xs = [1, 2]?\n    mut p = iter(&xs)\n    drop(*f(&mut p))\n    Ok(())\n",
            "an item of a lent pipeline cannot be returned",
        ),
        (
            "def same(x: &i64) -> &i64:\n    x\ndef f(k: &i64, p: &mut Iter[&i64]) -> &i64:\n    for x in p:\n        return same(x)\n    k\ndef main() -> Result[(), AllocError]:\n    xs = [1, 2]?\n    k = 0\n    mut p = iter(&xs)\n    drop(*f(&k, &mut p))\n    Ok(())\n",
            "an item of a lent pipeline cannot be returned",
        ),
        (
            "def f(p: &mut Iter[&i64]) -> ():\n    for x in p:\n        break\ndef main() -> Result[(), AllocError]:\n    mut xs = [1, 2]?\n    mut p = iter(&xs)\n    f(&mut p)\n    xs.append(3)?\n    drop(p.count())\n    Ok(())\n",
            "conflicting borrow",
        ),
        (
            "def f(p: &mut Iter[&i64]) -> Closure[[], i64]:\n    for x in p:\n        return def [&x]() -> i64:\n            x\n    def() -> i64:\n        0\ndef main() -> Result[(), AllocError]:\n    xs = [1, 2]?\n    mut p = iter(&xs)\n    drop(f(&mut p)())\n    Ok(())\n",
            "cannot escape its function",
        ),
        (
            "def f(p: &mut Iter[&i64]) -> Option[&i64]:\n    next(p)\n",
            "references cannot be stored in enum payloads",
        ),
        (
            "def f(p: &mut Iter[&i64]) -> i64:\n    match next(p):\n        case Some(x):\n            *x\n        case Nothing:\n            0\ndef main() -> Result[(), AllocError]:\n    xs = [1, 2]?\n    mut p = iter(&xs)\n    drop(f(&mut p))\n    Ok(())\n",
            "references cannot be stored in a next result",
        ),
        (
            "def f[T](p: &mut Iter[T]) -> i64:\n    p.count()\ndef main() -> Result[(), AllocError]:\n    xs = [1, 2]?\n    mut p = iter(&xs)\n    drop(f(&mut p))\n    Ok(())\n",
            "reference type arguments are not supported",
        ),
        (
            "def g(p: &mut Iter[i64]) -> &mut Iter[i64]:\n    p\n",
            "cannot be returned",
        ),
        ("def g(p: &Iter[i64]) -> ():\n    pass\n", "must be `&mut"),
    ] {
        let error = support::check_source(source).expect_err(source).to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

/// A stored closure that borrows its captures is a reference inside the
/// plan's state, which a borrower cannot receive; the same closure written
/// inside the adapter call is spliced, and its captures become lent loans.
#[test]
fn lending_a_stored_borrowing_closure_explains_the_fix() {
    let error = support::check_file_error(
        r#"def first(p: &mut Iter[i64]) -> Option[i64]:
    next(p)

def main() -> Result[(), AllocError | IoError]:
    limit = 3
    keep = def [&limit](x: &i64) -> bool:
        *x > limit
    mut p = iter(range(9)).filter(keep)
    print(first(&mut p))?
    Ok(())
"#,
    );
    assert!(
        error.contains(":8:35: this pipeline cannot be lent (at line 9): it holds closure `keep`, which is stored in a variable and borrows its captures; write the closure literal inside the adapter call instead"),
        "{error}"
    );
    output(
        r#"
def first(p: &mut Iter[i64]) -> Option[i64]:
    next(p)
limit = 3
mut p = iter(range(9)).filter(def [&limit](x: &i64) -> bool: *x > limit)
print(first(&mut p)).unwrap()
print(p.count()).unwrap()
"#,
        "Option[i64].Some(4)\n4\n",
    );
}

/// A lent pipeline of references: the borrower's items borrow the lender's
/// source, which the lender keeps borrowed across the call.
#[test]
fn lent_pipelines_of_reference_items() {
    output(
        r#"
def show(p: &mut Iter[&str]) -> Result[i64, IoError]:
    mut n = 0
    for name in p:
        print(name)?
        n += 1
        if n == 2:
            break
    Ok(n)

def total(p: &mut Iter[&i64]) -> i64:
    mut t = 0
    for x in p:
        t += *x
    t

def bump(p: &mut Iter[&mut i64]) -> ():
    for x in p:
        *x += 10
        break

def inner(p: &mut Iter[&str]) -> Result[i64, IoError]:
    mut n = 0
    for x in p:
        print("inner", x)?
        n += len(x)
    Ok(n)

def outer(p: &mut Iter[&str]) -> Result[i64, IoError]:
    for x in p:
        print("outer", x)?
        break
    inner(p)

def scaled(p: &mut Iter[&i64]) -> i64:
    mut t = 0
    for x in p:
        times = def [&x](k: i64) -> i64:
            x * k
        t += times(10)
    t

def main() -> Result[(), AllocError | IoError]:
    names = ["ada", "bob", "ada", "cal", "ada"]?
    mut p = iter(&names).filter_value("ada")
    print(show(&mut p)?)?
    print(p.count())?
    mut xs = [1, 2, 3, 4]?
    mut q = iter(&xs).skip(1)
    print(total(&mut q))?
    xs.append(5)?
    mut r = iter(&mut xs)
    bump(&mut r)
    bump(&mut r)
    print(xs)?
    mut s = iter(&names).filter(def(n: &str) -> bool: n != "bob")
    print(outer(&mut s)?)?
    mut t = iter(&xs).take(2)
    print(scaled(&mut t))?
    text = "abc"
    mut chars = iter(&text)
    print(show(&mut chars)?, chars.count())?
    Ok(())
"#,
        "ada\nada\n2\n1\n9\n[11, 12, 3, 4, 5]\nouter ada\ninner ada\ninner cal\ninner ada\n9\n230\na\nb\n2 1\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn lending_reference_items_does_not_allocate_or_leak() {
    support::assert_visible_stdout(
        r#"
def longest(p: &mut Iter[&str]) -> i64:
    mut best = 0
    for s in p:
        if len(s) == 0:
            break
        if len(s) > best:
            best = len(s)
    best
words = ["x", "yy", "zzz", "", "last"].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
mut p = iter(&words).filter(def(s: &str) -> bool: s != "yy")
n = longest(&mut p)
m = p.count()
print("__test_end_no_allocations__").unwrap()
print("__test_restore_allocations__").unwrap()
print(n, m, len(words)).unwrap()
"#,
        "3 1 5",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn lending_does_not_allocate_or_leak() {
    support::assert_visible_stdout(
        r#"
def skip(p: &mut Iter[str]) -> ():
    _ = next(p)
def longest(p: &mut Iter[str]) -> i64:
    mut best = 0
    for s in p:
        if len(s) == 0:
            break
        if len(s) > best:
            best = len(s)
    best
words = ["x", "yy", "zzz", "", "last"].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
mut p = iter(words).filter(def(s: &str) -> bool: s != "yy")
skip(&mut p)
n = longest(&mut p)
m = p.count()
print("__test_end_no_allocations__").unwrap()
print("__test_restore_allocations__").unwrap()
print(n, m).unwrap()
"#,
        "3 1",
    );
}

#[test]
fn lent_owned_state_survives_early_exits() {
    output(
        r#"
enum Bad:
    Empty
def check(s: &str) -> Result[i64, Bad]:
    if len(s) == 0:
        return Err(Bad.Empty)
    Ok(len(s))
def total(p: &mut Iter[str]) -> Result[i64, Bad]:
    mut t = 0
    for s in p:
        t += check(&s)?
    Ok(t)
def first(p: &mut Iter[str]) -> Option[str]:
    for s in p:
        return Some(s)
    Nothing
def main() -> Result[(), AllocError | IoError]:
    skip = ("skip" + "me")?
    words = ["aa", "skipme", "b", "", "ccc", "dddd", ""]?
    mut p = iter(words).filter(def [skip](s: &str) -> bool: s != skip)
    print(total(&mut p).is_err())?
    print(first(&mut p))?
    print(total(&mut p).is_err())?
    print(first(&mut p))?
    Ok(())
"#,
        "True\nOption[str].Some(\"ccc\")\nTrue\nOption[str].Nothing\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn lent_pipeline_resumes_after_allocation_failure_in_borrower() {
    support::assert_visible_stdout(
        r#"
def take_three(p: &mut Iter[str], out: &mut list[str]) -> Result[(), AllocError]:
    mut n = 0
    for s in p:
        out.append(s)?
        n += 1
        if n == 3:
            break
    Ok(())
words = ["a", "bb", "ccc", "dddd", "eeeee"].unwrap()
mut p = iter(words).filter(def(s: &str) -> bool: s != "bb")
mut out: list[str] = [].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
r = take_three(&mut p, &mut out)
print("__test_restore_allocations__").unwrap()
print(r.is_err(), len(out)).unwrap()
take_three(&mut p, &mut out).unwrap()
print(out).unwrap()
"#,
        "True 0\n[\"ccc\", \"dddd\", \"eeeee\"]",
    );
}
