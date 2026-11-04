//! Parallel pipelines: `iter(items).par(&pool)` (B75 stage 4).
mod support;

fn output(source: &str) -> std::process::Output {
    output_with_optimization(source, true)
}

fn output_with_optimization(source: &str, optimize: bool) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("par-test");
    let path = directory.path().join("main.plenty");
    std::fs::write(
        &path,
        format!("{FIXTURE_ARITHMETIC}\n{}", support::program(source)),
    )
    .unwrap();
    let options = plenty::CompileOptions {
        optimize,
        test_hooks: true,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&path, &executable, None, &options)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    support::output_within(
        &mut std::process::Command::new(executable),
        b"",
        std::time::Duration::from_secs(15),
    )
}

fn runs(source: &str, expected: &str) {
    let output = output(source);
    assert!(
        output.status.success(),
        "status {:?}; stdout {}; stderr {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

// Old scalar-callback fixtures intentionally assert that their fixed, small
// inputs fit. Checked+unwrap preserves that assertion without changing callback
// signatures (in particular, this does not implement fallible reduce).
const FIXTURE_ARITHMETIC: &str = r#"
def fixture_product(a: i64, b: i64) -> i64:
    result = checked:
        a * b
    result.unwrap()
def fixture_sum(a: i64, b: i64) -> i64:
    result = checked:
        a + b
    result.unwrap()
def fixture_shape(a: i64, b: i64) -> i64:
    result = checked:
        a * 3 - b
    result.unwrap()
"#;

const MAIN: &str = "def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | OverflowError[i64] | OverflowError[i8] | OverflowError[u8] | ZeroDivisionError]:\n";

#[test]
fn sources_steps_and_collecting_terminals_match_serial_results() {
    runs(
        &format!(
            r#"
def double(x: i64) -> i64:
    fixture_product(x, 2)
def odd(x: &i64) -> bool:
    two: NonZero[i64] = 2
    *x % two == 1
def big(x: &i64) -> bool:
    *x > 100
def pair(index: i64, value: i64) -> (i64, i64):
    (index, fixture_product(value, 10))
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        print(iter([1, 2, 3, 4, 5, 6, 7]?).par(&pool).map(double).collect()?)?
        print(iter(range(0, 10)).par(&pool).filter(odd).map(double).collect()?)?
        print(iter(range(20, 0, -3)).par(&pool).collect()?)?
        print(iter(range(5, 5)).par(&pool).collect()?)?
        print(enumerate([1, 2, 3]?, 5).par(&pool).map(pair).collect()?)?
        print(iter(["a", "b"]?).enumerate().par(&pool).collect()?)?
        typed: list[i64] = iter(range(0, 3)).par(&pool).map(double).collect()?
        print(typed)?
        print(iter(range(0, 9)).par(&pool).filter(odd).count()?)?
        print(iter(range(0, 9)).par(&pool).any(big)?)?
        print(iter(range(0, 9)).par(&pool).all(odd)?)?
        print(iter(range(0, 0)).par(&pool).all(odd)?)?
    Ok(())
"#
        ),
        "[2, 4, 6, 8, 10, 12, 14]\n[2, 6, 10, 14, 18]\n[20, 17, 14, 11, 8, 5, 2]\n[]\n[(5, 10), (6, 20), (7, 30)]\n[(0, \"a\"), (1, \"b\")]\n[0, 2, 4]\n4\nFalse\nFalse\nTrue\n",
    );
}

#[test]
fn filter_value_keeps_equal_items_like_the_serial_form() {
    runs(
        &format!(
            r#"
def double(x: i64) -> i64:
    fixture_product(x, 2)
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        print(iter([1, 2, 3, 2, 5, 2]?).par(&pool).filter_value(2).count()?)?
        print(iter(range(0, 6)).par(&pool).map(double).filter_value(4).collect()?)?
        print(iter([True, False, True]?).par(&pool).filter_value().count()?)?
        print(iter(["a", "b", "a"]?).par(&pool).filter_value("a").collect()?)?
        nan = 0.0 / 0.0
        print(iter([nan, 0.0, -0.0]?).par(&pool).filter_value(nan).count()?)?
        print(iter([nan, 0.0, -0.0]?).par(&pool).filter_value(0.0).count()?)?
    Ok(())
"#
        ),
        "3\n[4]\n2\n[\"a\", \"a\"]\n0\n2\n",
    );
}

#[test]
fn arguments_are_evaluated_in_written_order() {
    // Source, enumerate start, the pool and chunk count, then step values
    // and captures, left to right, as in a serial pipeline.
    runs(
        r#"
def note(label: str, n: i64) -> i64:
    print(label).unwrap()
    n
def items() -> list[i64]:
    print("source").unwrap()
    [1, 2, 3, 2].unwrap()
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | OverflowError[i64] | OverflowError[i8] | OverflowError[u8] | ZeroDivisionError]:
    pools = [ThreadPoolExecutor(2, 1)?]?
    limit = 10
    print(enumerate(items(), note("start", 0)).par(&pools[note("pool", 0)], chunks=Positive.try_from(note("chunks", 2)).unwrap()).filter(def [limit](i: &i64, v: &i64) -> bool: *v < limit).count()?)?
    print(iter(items()).par(&pools[note("pool", 0)], chunks=Positive.try_from(note("chunks", 3)).unwrap()).filter_value(note("value", 2)).map(def [limit](x: i64) -> i64: fixture_sum(x, limit)).any(def [limit](x: &i64) -> bool: *x > limit)?)?
    Ok(())
"#,
        "source\nstart\npool\nchunks\n4\nsource\npool\nchunks\nvalue\nTrue\n",
    );
}

#[test]
fn closures_own_their_captures_and_drop_once() {
    runs(
        &format!(
            r#"
class Scale:
    factor: i64
    def __del__(self) -> ():
        print("scale dropped").unwrap()
{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:
        scale = Scale(3)
        limit = 4
        out = iter(range(0, 8)).par(&pool).filter(def [limit](x: &i64) -> bool: *x < limit).map(def [scale](x: i64) -> i64: fixture_product(x, scale.factor)).collect()?
        print(out)?
    Ok(())
"#
        ),
        "scale dropped\n[0, 3, 6, 9]\n",
    );
}

#[test]
fn try_collect_reports_the_lowest_index_error() {
    runs(
        &format!(
            r#"
def check(x: i64) -> Result[i64, i64]:
    if x == 3 or x == 6:
        return Err(x)
    Ok(fixture_product(x, 10))
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        for chunks in [1, 2, 4, 8]?:
            r = iter(range(0, 8)).par(&pool, chunks=Positive.try_from(chunks).unwrap()).map(check).try_collect()
            print(str.repr(r)?)?
        ok = iter(range(0, 3)).par(&pool).map(check).try_collect()
        print(str.repr(ok)?)?
    Ok(())
"#
        ),
        &format!(
            "{}Result[list[i64], ParallelError[AllocError | i64]].Ok([0, 10, 20])\n",
            "Result[list[i64], ParallelError[AllocError | i64]].Err(ParallelError[AllocError | i64].Worker(3))\n"
                .repeat(4)
        ),
    );
}

/// Chunks [5, 1, 4], [2, 8], [7, 3] fold to 38, -2 and 18, then 330;
/// `reduce_tree` groups the same items differently.
#[test]
fn reduce_folds_chunks_in_order_and_empty_input_gives_nothing() {
    runs(
        &format!(
            r#"
def shape(a: i64, b: i64) -> i64:
    fixture_shape(a, b)
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        print(iter([5, 1, 4, 2, 8, 7, 3]?).par(&pool).reduce(shape)?)?
        print(pool.reduce_tree(shape, [5, 1, 4, 2, 8, 7, 3]?)?)?
        print(iter(range(0, 0)).par(&pool).reduce(shape)?)?
        print(iter(range(4, 5)).par(&pool).reduce(shape)?)?
    Ok(())
"#
        ),
        "Option[i64].Some(330)\nOption[i64].Some(48)\nOption[i64].Nothing\nOption[i64].Some(4)\n",
    );
}

#[test]
fn any_and_all_without_a_predicate_test_bool_items() {
    runs(
        &format!(
            r#"
def positive(x: i64) -> bool:
    x > 0
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        print(iter([False, False, True, False]?).par(&pool).any()?)?
        print(iter([True, True, False]?).par(&pool, chunks=3).all()?)?
        print(iter(range(1, 9)).par(&pool).map(positive).all()?)?
        print(iter(range(-3, 0)).par(&pool).map(positive).any()?)?
        print(iter(list[bool].with_capacity(0)?).par(&pool).all()?)?
    Ok(())
"#
        ),
        "True\nFalse\nTrue\nFalse\nTrue\n",
    );
}

/// The grouping must be exactly the level-by-level adjacent-pair tree for
/// every length and worker count, even though one job now reduces a whole
/// aligned subtree. `mix` is not associative, so its result records the
/// grouping.
#[test]
fn reduce_tree_groups_exactly_like_the_level_by_level_pair_tree() {
    runs(
        &format!(
            r#"
def mix(a: i64, b: i64) -> i64:
    a.wrapping_mul(1000003).wrapping_add(b.wrapping_mul(31)).wrapping_add(17)

def inputs(n: i64) -> Result[list[i64], AllocError | OverflowError[i64]]:
    mut items = list[i64].with_capacity(n)?
    for i in range(0, n):
        items.append(i * 7 + 1)?
    Ok(items)

def reference(n: i64) -> Result[Option[i64], AllocError | OverflowError[i64] | ZeroDivisionError]:
    # The reduce_tree contract, evaluated serially one level at a time.
    if n == 0:
        return Ok(Nothing)
    mut level = inputs(n)?
    while len(level) > 1:
        count = len(level)
        mut next = list[i64].with_capacity(count // 2 + 1)?
        mut i = 0
        while i + 1 < count:
            next.append(mix(level[i], level[i + 1]))?
            i += 2
        if count % 2 == 1:
            next.append(level[count - 1])?
        level = next
    Ok(Some(level[0]))

{MAIN}    mut lengths = inputs(0)?
    for n in range(0, 71):
        lengths.append(n)?
    for n in [127, 128, 129, 1000, 1025]?:
        lengths.append(n)?
    mut checked = 0
    for workers in [1, 2, 3, 8]?:
        with ThreadPoolExecutor(Positive.try_from(workers).unwrap(), 2)? as pool:
            for n in &lengths:
                if pool.reduce_tree(mix, inputs(*n)?)? != reference(*n)?:
                    print("reduce_tree differs")?
                    print(workers)?
                    print(*n)?
                checked += 1
    print(checked)?
    Ok(())
"#
        ),
        "304\n",
    );
}

/// A parallel `reduce` folds each chunk left to right, then folds the
/// non-empty chunk results in input order (not integer `sum` grouping).
#[test]
fn par_reduce_folds_each_chunk_then_the_chunk_results() {
    runs(
        &format!(
            r#"
def mix(a: i64, b: i64) -> i64:
    a.wrapping_mul(1000003).wrapping_add(b.wrapping_mul(31)).wrapping_add(17)

def keep(x: &i64) -> bool:
    three: NonZero[i64] = 3
    *x % three != 0

def inputs(n: i64) -> Result[list[i64], AllocError | OverflowError[i64]]:
    mut items = list[i64].with_capacity(n)?
    for i in range(0, n):
        items.append(i * 7 + 1)?
    Ok(items)

def fold(acc: Option[i64], x: i64) -> Option[i64]:
    match acc:
        case Some(a):
            Some(mix(a, x))
        case Nothing:
            Some(x)

def reference(n: i64, parts: i64, filtered: bool) -> Result[Option[i64], OverflowError[i64] | ZeroDivisionError]:
    # Chunk sizes as the split makes them: the first n % c chunks get one more.
    mut c = n
    if parts < n:
        c = parts
    if c == 0:
        return Ok(Nothing)
    base = n // c
    extra = n % c
    mut total: Option[i64] = Nothing
    mut start = 0
    for k in range(0, c):
        mut size = base
        if k < extra:
            size += 1
        mut chunk: Option[i64] = Nothing
        for i in range(start, start + size):
            x = i * 7 + 1
            if not filtered or keep(&x):
                chunk = fold(chunk, x)
        start += size
        match chunk:
            case Some(value):
                total = fold(total, value)
            case Nothing:
                ()
    Ok(total)

{MAIN}    mut lengths = inputs(0)?
    for n in range(0, 71):
        lengths.append(n)?
    for n in [127, 128, 129, 1000, 1025]?:
        lengths.append(n)?
    mut checked = 0
    for workers in [1, 2, 3, 8]?:
        with ThreadPoolExecutor(Positive.try_from(workers).unwrap(), 2)? as pool:
            w = i64(workers)
            for n in &lengths:
                if iter(inputs(*n)?).par(&pool).reduce(mix)? != reference(*n, w, False)?:
                    print("par reduce differs")?
                    print(workers)?
                    print(*n)?
                if iter(inputs(*n)?).par(&pool, chunks=5).reduce(mix)? != reference(*n, 5, False)?:
                    print("par reduce (5 chunks) differs")?
                    print(workers)?
                    print(*n)?
                if iter(inputs(*n)?).par(&pool, chunks=4).filter(keep).reduce(mix)? != reference(*n, 4, True)?:
                    print("filtered par reduce differs")?
                    print(workers)?
                    print(*n)?
                checked += 3
    print(checked)?
    Ok(())
"#
        ),
        "912\n",
    );
}

/// Chunks read the source's rows in place; every owned item moves once and
/// drops once, including the rest of a chunk whose worker stopped early, which
/// the caller drops with the source list.
#[test]
fn list_chunks_move_owned_items_once() {
    runs(
        &format!(
            r#"
class Tracked:
    id: i64
    def __del__(self) -> ():
        print("drop").unwrap()
def stop(item: &Tracked) -> bool:
    item.id == 2
{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:
        ids = iter([Tracked(1), Tracked(2), Tracked(3), Tracked(4), Tracked(5)]?).par(&pool, chunks=3).map(def(t: Tracked) -> i64: t.id).collect()?
        print(ids)?
        found = iter([Tracked(1), Tracked(2), Tracked(3), Tracked(4)]?).par(&pool, chunks=2).any(stop)?
        print(found)?
        spans = iter([Some(range(1, 3)), Nothing, Some(range(4, 6)), Some(range(7, 9)), Nothing]?).par(&pool, chunks=2).collect()?
        print(spans)?
        nested = iter([[1, 2]?, [3]?, [4, 5, 6]?]?).enumerate(10).par(&pool, chunks=2).collect()?
        print(nested)?
    Ok(())
"#
        ),
        "drop\ndrop\ndrop\ndrop\ndrop\n[1, 2, 3, 4, 5]\ndrop\ndrop\ndrop\ndrop\nTrue\n[Option[range].Some(range(1, 3, 1)), Option[range].Nothing, Option[range].Some(range(4, 6, 1)), Option[range].Some(range(7, 9, 1)), Option[range].Nothing]\n[(10, [1, 2]), (11, [3]), (12, [4, 5, 6])]\n",
    );
}

/// `iter(&xs).par(&pool)` yields `&T` items like serial `iter(&xs)`; chunk
/// jobs read the caller's rows in place and leave the list unchanged.
#[test]
fn borrowed_sources_match_serial_results() {
    runs(
        &format!(
            r#"
def odd(x: &i64) -> bool:
    two: NonZero[i64] = 2
    *x % two == 1
def shout(s: &str) -> str:
    s.upper().unwrap()
def short(s: &str) -> bool:
    len(*s) < 2
def add(a: i64, b: i64) -> i64:
    fixture_sum(a, b)
def at(index: i64, value: &i64) -> i64:
    fixture_sum(fixture_product(index, 100), *value)
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        xs = [5, 2, 7, 1, -4, 9, 3]?
        print(iter(&xs).par(&pool).map(def(x: &i64) -> i64: fixture_product(*x, 2)).collect()?)?
        print(iter(&xs).par(&pool, chunks=2).filter(odd).count()?)?
        print(iter(&xs).par(&pool).filter_value(7).count()?)?
        print(iter(&xs).par(&pool).sum()?)?
        print(iter(&xs).par(&pool).min()?)?
        print(iter(&xs).par(&pool).max()?)?
        print(iter(&xs).par(&pool).any(odd)?)?
        print(iter(&xs).par(&pool).all(odd)?)?
        print(iter(&xs).par(&pool).map(def(x: &i64) -> i64: *x).reduce(add)?)?
        print(enumerate(&xs, 10).par(&pool, chunks=3).map(at).collect()?)?
        print(xs)?
        words = ["a", "bb", "c"]?
        print(iter(&words).par(&pool).map(shout).collect()?)?
        print(iter(&words).par(&pool).filter(short).count()?)?
        print(iter(&words).par(&pool).filter_value("bb").count()?)?
        print(iter(&words).par(&pool).max()?)?
        view = &words
        print(iter(view).par(&pool).count()?)?
        print(words)?
        flags = [False, True]?
        print(iter(&flags).par(&pool).any()?)?
    Ok(())
"#
        ),
        "[10, 4, 14, 2, -8, 18, 6]\n5\n1\n23\nOption[i64].Some(-4)\nOption[i64].Some(9)\nTrue\nFalse\nOption[i64].Some(23)\n[1005, 1102, 1207, 1301, 1396, 1509, 1603]\n[5, 2, 7, 1, -4, 9, 3]\n[\"A\", \"BB\", \"C\"]\n2\n1\nOption[str].Some(\"c\")\n3\n[\"a\", \"bb\", \"c\"]\nTrue\n",
    );
}

#[test]
fn sums_match_serial_results_and_report_first_overflow() {
    runs(
        &format!(
            r#"
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        print(iter(range(0, 10)).par(&pool).sum()?)?
        print(iter([i64.MAX, -1, 1]?).par(&pool, chunks=3).sum()?)?
        print(iter([-i64.MAX, -1, i64.MAX, i64.MAX]?).par(&pool, chunks=2).sum()?)?
        print(iter([100i8, -100i8, 100i8]?).par(&pool, chunks=3).sum()?)?
        print(iter([200u8, 50u8]?).par(&pool).sum()?)?
        print(iter(range(0, 0)).par(&pool).sum()?)?
        print(iter([1.5, 2.5]?).par(&pool).sum()?)?
        # A chunk's own running total leaves i64 while the serial one does not.
        print(iter([1, 2, -2, i64.MIN + 1]?).par(&pool, chunks=2).sum()?)?
        print(iter([-3, 2, i64.MAX - 1]?).par(&pool, chunks=2).sum()?)?
        print(iter([i64.MIN + 1, -1, i64.MAX, i64.MAX, i64.MIN, 1]?).par(&pool, chunks=3).sum()?)?
    Ok(())
"#
        ),
        "45\n9223372036854775807\n9223372036854775806\n100\n250\n0\n4.0\n-9223372036854775806\n9223372036854775805\n-1\n",
    );
    // Even cancellation after the failing prefix cannot erase its payload.
    for (items, chunks, ty, wrapped) in [
        ("[100i8, 100i8, -100i8]", 3, "i8", "-56"),
        ("[100i8, 100i8, -100i8]", 1, "i8", "-56"),
        ("[-100i8, -100i8, 100i8]", 2, "i8", "56"),
        ("[i64.MAX, 1, -1]", 1, "i64", "-9223372036854775808"),
        ("[200u8, 100u8]", 2, "u8", "44"),
        ("[0, 0, 1, i64.MAX]", 2, "i64", "-9223372036854775808"),
        ("[0, 0, -2, i64.MIN + 1]", 2, "i64", "9223372036854775807"),
        ("[i64.MAX, i64.MAX, i64.MIN, i64.MIN, 1]", 5, "i64", "-2"),
    ] {
        runs(
            &format!(
                "{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:\n        print(str.repr(iter({items}?).par(&pool, chunks={chunks}).sum())?)?\n    Ok(())\n"
            ),
            &format!(
                "Result[{ty}, OverflowError[{ty}] | PoolMapError].Err(OverflowError[{ty}](wrapped={wrapped}))\n"
            ),
        );
    }
}

#[test]
fn min_and_max_match_serial_nan_and_signed_zero_rules() {
    runs(
        &format!(
            r#"
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        nan = 0.0 / 0.0
        print(iter([1.0, nan, 3.0, -nan]?).par(&pool, chunks=4).max()?)?
        print(iter([1.0, nan, 3.0]?).par(&pool, chunks=3).min()?)?
        print(iter([0.0, -0.0, 0.0]?).par(&pool, chunks=3).min()?)?
        print(iter([-0.0, 0.0, -0.0]?).par(&pool, chunks=3).max()?)?
        print(iter(["b", "a", "c"]?).par(&pool).min()?)?
        print(iter(range(3, 9)).par(&pool).max()?)?
        print(iter(range(0, 0)).par(&pool).max()?)?
    Ok(())
"#
        ),
        "Option[f64].Some(NaN)\nOption[f64].Some(NaN)\nOption[f64].Some(-0.0)\nOption[f64].Some(0.0)\nOption[str].Some(\"a\")\nOption[i64].Some(8)\nOption[i64].Nothing\n",
    );
}

#[test]
fn chunk_count_defaults_to_pool_workers_and_clamps_to_input_length() {
    // Float sums depend on chunk boundaries: [1, 1e16 | -1e16, 1] gives 0.0
    // while one chunk, or one chunk per item, gives the serial 1.0.
    runs(
        &format!(
            r#"
def items() -> Result[list[f64], AllocError]:
    [1.0, 1e16, -1e16, 1.0]
def total(pool: &ThreadPoolExecutor, chunks: Positive[i64]) -> Result[f64, AllocError | PoolMapError]:
    Ok(iter(items()?).par(pool, chunks=chunks).sum()?)
{MAIN}    print(iter(items()?).sum())?
    with ThreadPoolExecutor(2, 1)? as two:
        print(iter(items()?).par(&two).sum()?)?
        print(iter(items()?).par(&two, chunks=1).sum()?)?
        print(total(&two, 4)?)?
        print(total(&two, 100)?)?
    with ThreadPoolExecutor(1, 1)? as one:
        print(iter(items()?).par(&one).sum()?)?
        print(iter(items()?).par(&one, chunks=2).sum()?)?
    with ThreadPoolExecutor(64, 1)? as wide:
        print(iter(items()?).par(&wide).sum()?)?
        print(iter(range(0, 3)).par(&wide).collect()?)?
    Ok(())
"#
        ),
        "1.0\n0.0\n1.0\n1.0\n1.0\n1.0\n0.0\n1.0\n[0, 1, 2]\n",
    );
}

#[test]
fn runtime_chunk_counts_must_be_positive() {
    runs(
        r#"
print(Positive.try_from(0).is_err(), Positive.try_from(-3).is_err()).unwrap()
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    n = Positive.try_from(3).unwrap()
    print(iter(range(0, 4)).par(&pool, chunks=n).count().unwrap()).unwrap()
"#,
        "True True\n4\n",
    );
}

#[test]
fn shut_down_pool_reports_pool_map_error() {
    runs(
        &format!(
            r#"
def double(x: i64) -> i64:
    fixture_product(x, 2)
{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:
        pool.shutdown()
        print(str.repr(iter(range(0, 4)).par(&pool).map(double).collect())?)?
    Ok(())
"#
        ),
        "Result[list[i64], PoolMapError].Err(PoolMapError.Shutdown)\n",
    );
}

/// A borrowed chunk's items are checked like any worker local, though the
/// job's argument is only a span of row addresses.
#[test]
fn borrowed_items_must_be_worker_eligible() {
    let source = "def one() -> i64:\n    1\ndef keep(f: &Future[i64]) -> bool:\n    True\ndef main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | SubmitError[OnceClosure[[], i64]]]:\n    with ThreadPoolExecutor(2, 4)? as pool:\n        fs = [pool.submit(one)?]?\n        print(iter(&fs).par(&pool).filter(keep).count()?)?\n    Ok(())\n";
    let directory = tempfile::tempdir().unwrap();
    let error = support::compile_source_to_executable(source, &directory.path().join("x"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("future handles cannot enter worker threads"),
        "{error}"
    );
}

#[test]
fn par_rejects_unsupported_shapes() {
    let header = "def add(a: i64, b: i64) -> i64:\n    fixture_sum(a, b)\ndef double(x: i64) -> i64:\n    fixture_product(x, 2)\ndef show(x: i64) -> ():\n    pass\ndef main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | ParallelError[AllocError]]:\n    with ThreadPoolExecutor(3, 1)? as pool:\n        xs = [1, 2, 3]?\n";
    for (body, diagnostic) in [
        (
            "for x in iter(xs).par(&pool):\n            print(x)?",
            "must end in collect",
        ),
        ("p = iter(xs).par(&pool)", "must end in collect"),
        (
            "print(iter(xs).par(&pool).fold(0, add))?",
            "fold is not available after .par",
        ),
        (
            "iter(xs).par(&pool).for_each(show)",
            "for_each is not available after .par",
        ),
        (
            "print(iter(xs).par(&pool).take(2).count()?)?",
            "take cannot follow .par",
        ),
        (
            "print(iter(xs).par(&pool).any()?)?",
            "any() tests bool items, but this pipeline yields i64; pass a predicate",
        ),
        (
            "print(enumerate([True]?).par(&pool).all()?)?",
            "all() tests bool items, but this pipeline yields tuple[i64, bool]; pass a predicate",
        ),
        (
            "print(iter([[1]?]?).par(&pool).filter_value([1]?).count()?)?",
            "filter_value after .par needs copyable items",
        ),
        (
            "print(enumerate(xs).par(&pool).filter_value((0, 1)).count()?)?",
            "filter_value does not accept pair items",
        ),
        (
            "print(iter(xs).par(&pool).filter_value(1u8).count()?)?",
            "filter_value compares i64 items with a u8 value",
        ),
        (
            "p = ThreadPoolExecutor(1, 1)?\n        print(iter(xs).par(&p).map(def [p](x: i64) -> i64: x).count()?)?",
            "cannot move `p` while it is borrowed",
        ),
        (
            "print(iter(xs).par(&pool).enumerate().count()?)?",
            "enumerate must come before .par",
        ),
        (
            "print(iter(xs).map(double).par(&pool).count()?)?",
            "only enumerate may come before .par",
        ),
        (
            "owned = ThreadPoolExecutor(1, 1)?\n        print(iter(xs).par(owned).count()?)?",
            "par takes a borrowed ThreadPoolExecutor",
        ),
        (
            "mut ys = [1]?\n        print(iter(&mut ys).par(&pool).count()?)?",
            "par reads a borrowed list through a shared reference",
        ),
        (
            "print(iter(&xs).par(&pool).collect()?)?",
            "collect after .par cannot store borrowed items, found &i64",
        ),
        (
            "print(iter(&xs).par(&pool).reduce(add)?)?",
            "reduce after .par cannot store borrowed items",
        ),
        (
            "print(iter(range(0u8, 3u8)).par(&pool).count()?)?",
            "par splits range[i64] sources",
        ),
        (
            "mut n = 0\n        print(iter(xs).par(&pool).map(def [mut n](x: i64) -> i64: x + n).count()?)?",
            "cannot have mutable captures",
        ),
        (
            "s = [1]?\n        print(iter(xs).par(&pool).map(def once [s](x: i64) -> i64: x + len(s)).count()?)?",
            "one-shot closure cannot be a parallel callback",
        ),
        (
            "print(iter(xs).par(&pool, chunks=0).count()?)?",
            "a Positive value must be greater than zero",
        ),
        (
            "print(iter(xs).par(&pool, chunks=-2).count()?)?",
            "a Positive value must be greater than zero",
        ),
        (
            "print(iter(xs).par(&pool).reduce(def(a: i64, b: i64) -> i64: a + b)?)?",
            "a parallel reduce takes a named function",
        ),
        (
            "print(iter(xs).par(&pool).find(def(x: &i64) -> bool: *x == 1))?",
            "find is not available on a parallel pipeline",
        ),
        (
            "print(iter([\"a\"]?).par(&pool).sum()?)?",
            "sum requires numeric items",
        ),
    ] {
        let source = format!("{FIXTURE_ARITHMETIC}\n{header}        {body}\n    Ok(())\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_reports_errors_and_drops_captures_once() {
    let mut failures = 0;
    for budget in 0..12 {
        let output = output(&format!(
            r#"
class Capture:
    factor: i64
    def __del__(self) -> ():
        print("dropped").unwrap()
{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:
        xs = [1, 2, 3, 4, 5]?
        capture = Capture(2)
        print("__test_fail_allocations_after_{budget}__").unwrap()
        result = iter(xs).par(&pool).map(def [capture](x: i64) -> i64: fixture_product(x, capture.factor)).collect()
        print("__test_restore_allocations__").unwrap()
        match result:
            case Ok(values):
                print(values)?
            case Err(_):
                print("failed")?
    Ok(())
"#
        ));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "budget {budget}: {stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            stdout.matches("dropped").count(),
            1,
            "budget {budget}: {stdout}"
        );
        assert!(
            stdout.ends_with("[2, 4, 6, 8, 10]\n") || stdout.ends_with("failed\n"),
            "budget {budget}: {stdout}"
        );
        failures += usize::from(stdout.ends_with("failed\n"));
    }
    assert!(failures > 0, "no budget made the pipeline fail");
}

/// Each owned item drops exactly once whatever stops the pipeline: success,
/// a worker error, or a failed submission. The runtime's allocation balance
/// check at exit catches a leaked `str` or a double release.
#[cfg(feature = "runtime-checks")]
#[test]
fn owned_affine_items_drop_once_on_every_path() {
    let program = |body: &str| {
        format!(
            r#"
class Tracked:
    name: str
    def __del__(self) -> ():
        print("dropped").unwrap()
def checked(t: Tracked) -> Result[i64, str]:
    if t.name == "b":
        return Err("first")
    if t.name == "d":
        return Err("second")
    Ok(len(t.name))
def tracked() -> Result[list[Tracked], AllocError]:
    [Tracked("a"), Tracked("b"), Tracked("c"), Tracked("d"), Tracked("e")]
{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:
{body}
    Ok(())
"#
        )
    };
    for (body, expected) in [
        (
            "        print(iter(tracked()?).par(&pool, chunks=3).map(def(t: Tracked) -> i64: len(t.name)).collect()?)?",
            "[1, 1, 1, 1, 1]\n",
        ),
        (
            "        print(str.repr(iter(tracked()?).par(&pool, chunks=5).map(checked).try_collect())?)?",
            "Err(ParallelError[AllocError | str].Worker(\"first\"))\n",
        ),
        (
            "        pool.shutdown()\n        print(str.repr(iter(tracked()?).par(&pool, chunks=2).count())?)?",
            "Result[i64, PoolMapError].Err(PoolMapError.Shutdown)\n",
        ),
    ] {
        let output = output(&program(body));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{body}\n{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(stdout.matches("dropped").count(), 5, "{body}\n{stdout}");
        assert!(stdout.ends_with(expected), "{body}\n{stdout}");
    }
}

/// Allocation failures at every point, including a submission that fails
/// after earlier chunks were submitted, leave each item dropped once.
#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_drops_each_owned_item_once() {
    let mut failures = 0;
    for budget in 0..24 {
        let output = output(&format!(
            r#"
class Tracked:
    name: str
    def __del__(self) -> ():
        print("dropped").unwrap()
{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:
        xs = [Tracked("a"), Tracked("bb"), Tracked("c"), Tracked("dd"), Tracked("e")]?
        print("__test_fail_allocations_after_{budget}__").unwrap()
        result = iter(xs).par(&pool, chunks=5).map(def(t: Tracked) -> i64: len(t.name)).collect()
        print("__test_restore_allocations__").unwrap()
        match result:
            case Ok(values):
                print(values)?
            case Err(_):
                print("failed")?
    Ok(())
"#
        ));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "budget {budget}: {stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            stdout.matches("dropped").count(),
            5,
            "budget {budget}: {stdout}"
        );
        assert!(
            stdout.ends_with("[1, 2, 1, 2, 1]\n") || stdout.ends_with("failed\n"),
            "budget {budget}: {stdout}"
        );
        failures += usize::from(stdout.ends_with("failed\n"));
    }
    assert!(failures > 2, "too few budgets made the pipeline fail");
}

// The public type annotation is intentional: no caller-mode or hidden callback
// effect participates in the sum ABI. Each width exercises empty/success,
// worker-local and cross-chunk overflow, and precise non-sentinel payloads.
#[test]
fn integer_sum_all_widths_and_caller_modes_preserve_first_payload() {
    let mut declarations = String::new();
    let mut body = String::new();
    let widths = ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"];
    for ty in widths {
        declarations.push_str(&format!(r#"
def overflow_{ty}(result: Result[{ty}, PoolMapError | OverflowError[{ty}]]) -> bool:
    match result:
        case Err(error):
            match error:
                case overflow: OverflowError[{ty}]:
                    overflow.wrapped == {ty}.MAX.wrapping_add({ty}(2))
                case pool: PoolMapError:
                    False
        case Ok(_):
            False

def sum_{ty}(pool: &ThreadPoolExecutor) -> Result[bool, AllocError]:
    success = iter([{ty}(1), {ty}(2)]?).par(pool).sum().unwrap() == {ty}(3)
    empty = iter(list[{ty}].with_capacity(0)?).par(pool).sum().unwrap() == {ty}(0)
    mut precise = True
    for chunks in [1, 2, 3]?:
        values = [{ty}.MAX, {ty}(2), {ty}(0)]?
        normal = overflow_{ty}(iter(&values).par(pool, chunks=Positive.try_from(chunks).unwrap()).sum())
        local = checked:
            mode_marker = 0 + 0
            overflow_{ty}(iter(&values).par(pool, chunks=Positive.try_from(chunks).unwrap()).sum())
        deliberate = unchecked:
            overflow_{ty}(iter(&values).par(pool, chunks=Positive.try_from(chunks).unwrap()).sum())
        precise = precise and normal and local.unwrap() and deliberate
    Ok(success and empty and precise)
"#));
        body.push_str(&format!(
            r#"        normal_{ty} = sum_{ty}(&pool)?
        local_{ty} = checked:
            mode_marker = 0 + 0
            sum_{ty}(&pool)
        trapping_{ty} = unchecked:
            sum_{ty}(&pool)
        print(normal_{ty}, local_{ty}.unwrap()?, trapping_{ty}?)?
"#
        ));
    }
    let source = format!(
        "{declarations}\n{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:\n{body}    Ok(())\n"
    );
    for optimize in [false, true] {
        let result = output_with_optimization(&source, optimize);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "True True True\n".repeat(8)
        );
    }
}

#[test]
fn signed_sum_offsets_rescue_chunk_prefixes_without_replaying_callbacks() {
    let source = format!(
        r#"
class Item:
    value: i8
    calls: Sender[i64]
    def __del__(self) -> ():
        self.calls.send(10).unwrap()
class Capture:
    calls: Sender[i64]
    def __del__(self) -> ():
        self.calls.send(100).unwrap()
def keep(item: &Item) -> bool:
    item.calls.send(1).unwrap()
    True
{MAIN}    calls, events = channel[i64](32)?
    with ThreadPoolExecutor(3, 1)? as pool:
        # Chunk two's [100, 100] overflows from zero, not from -100.
        items = [Item(-100i8, calls.share()), Item(0i8, calls.share()), Item(100i8, calls.share()), Item(100i8, calls.share())]?
        capture = Capture(calls.share())
        mapped = def [capture](item: Item) -> i8:
            capture.calls.send(2).unwrap()
            item.value
        result = iter(items).par(&pool, chunks=2).filter(keep).map(mapped).sum()
        print(result.unwrap())?
        mut total = 0
        for _ in range(13):
            total += events.recv().unwrap()
        print(total)?
    Ok(())
"#
    );
    let overflowing = source
        .replace(
            "Item(-100i8, calls.share()), Item(0i8",
            "Item(100i8, calls.share()), Item(0i8",
        )
        .replace(
            "Item(100i8, calls.share())]?",
            "Item(-100i8, calls.share())]?",
        )
        .replace("print(result.unwrap())?", "print(str.repr(result)?)?");
    for optimize in [false, true] {
        for (program, expected) in [
            (&source, "100\n152\n"),
            (
                &overflowing,
                "Result[i8, OverflowError[i8] | PoolMapError].Err(OverflowError[i8](wrapped=-56))\n152\n",
            ),
        ] {
            let result = output_with_optimization(program, optimize);
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&result.stdout), expected);
        }
    }
}

#[test]
fn integer_sum_drains_blocked_jobs_before_owned_drop_or_borrowed_source_reuse() {
    let source = format!(
        r#"
class Item:
    index: i64
    value: i8
    signal: Sender[i64]
    wait: Receiver[i64]
    dropped: Sender[i64]
    def __del__(self) -> ():
        self.dropped.send(self.index).unwrap()
def read(item: &Item) -> i8:
    if item.index == 0:
        item.wait.recv().unwrap()
    if item.index == 2:
        item.signal.send(1).unwrap()
    item.value
{MAIN}    signal, wait = channel[i64](1)?
    dropped, deaths = channel[i64](8)?
    with ThreadPoolExecutor(3, 1)? as pool:
        mut items = [Item(0, 100i8, signal.share(), wait.share(), dropped.share()), Item(1, 100i8, signal.share(), wait.share(), dropped.share()), Item(2, -100i8, signal.share(), wait.share(), dropped.share())]?
        result = iter(&items).par(&pool, chunks=3).map(read).sum()
        match result:
            case Err(error):
                match error:
                    case overflow: OverflowError[i8]:
                        print(overflow.wrapped)?
                    case pool: PoolMapError:
                        print("unexpected pool failure")?
            case Ok(_):
                print("missed overflow")?
        # The signal has been consumed and every borrow ended before return.
        print(len(items), items[2].value)?
        items.append(Item(3, 0i8, signal.share(), wait.share(), dropped.share())).unwrap()
        drop(items)
        mut total = 0
        for _ in range(4):
            total += deaths.recv().unwrap()
        print(total)?
    Ok(())
"#
    );
    let owned = source
        .replace("def read(item: &Item)", "def read(item: Item)")
        .replace("iter(&items)", "iter(items)")
        .replace(
            "        print(len(items), items[2].value)?\n        items.append(Item(3, 0i8, signal.share(), wait.share(), dropped.share())).unwrap()\n        drop(items)\n",
            "",
        )
        .replace("for _ in range(4):", "for _ in range(3):");
    // Exercise both an early blocked chunk and a later blocked chunk whose
    // rows must remain alive even though earlier values already overflow.
    let later_blocked = source
        .replace("if item.index == 0:", "if item.index == 7:")
        .replace("if item.index == 2:", "if item.index == 0:")
        .replace("if item.index == 7:", "if item.index == 2:");
    let owned_later_blocked = owned
        .replace("if item.index == 0:", "if item.index == 7:")
        .replace("if item.index == 2:", "if item.index == 0:")
        .replace("if item.index == 7:", "if item.index == 2:");
    for optimize in [false, true] {
        for (program, expected) in [
            (&source, "-56\n3 -100\n6\n"),
            (&later_blocked, "-56\n3 -100\n6\n"),
            (&owned, "-56\n3\n"),
            (&owned_later_blocked, "-56\n3\n"),
        ] {
            let result = output_with_optimization(program, optimize);
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&result.stdout), expected);
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn integer_sum_allocation_and_submission_failures_drop_rows_and_captures_once() {
    let mut failures = 0;
    let mut successes = 0;
    let mut partial_failures = 0;
    for optimize in [false, true] {
        for budget in 0..40 {
            let source = format!(
                r#"
class Item:
    value: i8
    def __del__(self) -> ():
        print("row dropped").unwrap()
class Capture:
    value: i8
    def __del__(self) -> ():
        print("capture dropped").unwrap()
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        items = [Item(1i8), Item(2i8), Item(3i8), Item(4i8), Item(5i8)]?
        capture = Capture(0i8)
        mapped = def [capture](item: Item) -> i8:
            drop(capture.value)
            print("mapped").unwrap()
            item.value
        print("__test_fail_allocations_after_{budget}__").unwrap()
        result = iter(items).par(&pool, chunks=5).map(mapped).sum()
        print("__test_restore_allocations__").unwrap()
        match result:
            case Ok(value):
                print(value)?
            case Err(error):
                match error:
                    case pool: PoolMapError:
                        print(pool)?
                    case overflow: OverflowError[i8]:
                        print("unexpected overflow")?
    Ok(())
"#
            );
            let result = output_with_optimization(&source, optimize);
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert!(
                result.status.success(),
                "budget {budget}: {stdout}\n{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(stdout.matches("row dropped").count(), 5, "{stdout}");
            assert_eq!(stdout.matches("capture dropped").count(), 1, "{stdout}");
            assert!(
                stdout.ends_with("15\n")
                    || stdout.ends_with("PoolMapError.Allocation(AllocError.OutOfMemory)\n"),
                "{stdout}"
            );
            failures += usize::from(stdout.contains("PoolMapError.Allocation"));
            successes += usize::from(stdout.ends_with("15\n"));
            partial_failures += usize::from(
                stdout.contains("PoolMapError.Allocation") && stdout.contains("mapped"),
            );
        }
    }
    assert!(
        partial_failures > 0,
        "no failed submission followed accepted work"
    );
    assert!(
        failures > 5 && successes > 0,
        "failures {failures}, successes {successes}"
    );
}

#[test]
fn enumerate_indices_keep_their_explicit_trapping_contract_in_every_mode() {
    for optimize in [false, true] {
        for chain in ["", ".par(&pool, chunks=1)", ".par(&pool, chunks=2)"] {
            for mode in ["", "checked", "unchecked"] {
                let expression = format!("drop(enumerate([1, 2]?, i64.MAX){chain}.count())");
                let body = if mode.is_empty() {
                    format!("        {expression}\n")
                } else {
                    format!(
                        "        value = {mode}:\n            mode_marker = 0 + 0\n            {expression}\n            0\n        drop(value)\n"
                    )
                };
                let source = format!(
                    "{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:\n{body}    Ok(())\n"
                );
                let result = output_with_optimization(&source, optimize);
                assert!(!result.status.success(), "{source}");
                assert!(
                    String::from_utf8_lossy(&result.stderr).contains("integer overflow"),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
        }
    }
}

#[test]
fn split_ranges_and_enumerate_boundaries_remain_exact() {
    let source = format!(
        r#"
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        positive: NonZero[i64] = 9223372036854775807
        negative: NonZero[i64] = -9223372036854775808
        print(iter(range(i64.MIN, i64.MAX, positive)).par(&pool, chunks=2).collect()?)?
        print(iter(range(i64.MAX, i64.MIN, negative)).par(&pool, chunks=2).collect()?)?
        print(enumerate([1]?, i64.MAX).par(&pool).collect()?)?
        print(enumerate([1, 2]?, i64.MIN).par(&pool, chunks=2).collect()?)?
        print(enumerate([1, 2]?, 0).par(&pool, chunks=2).collect()?)?
        print(iter([1.0 / 0.0, 1.0]?).par(&pool).sum()?)?
        print(iter([0.0 / 0.0, 1.0]?).par(&pool).sum()?)?
        print(iter([1.0f32, 2.0f32]?).par(&pool).sum()?)?
        print(iter(list[f32].with_capacity(0)?).par(&pool).sum()?)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        let result = output_with_optimization(&source, optimize);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "[-9223372036854775808, -1, 9223372036854775806]\n[9223372036854775807, -1]\n[(9223372036854775807, 1)]\n[(-9223372036854775808, 1), (-9223372036854775807, 2)]\n[(0, 1), (1, 2)]\ninf\nNaN\n3.0\n0.0\n"
        );
    }
}

#[test]
fn generated_split_and_count_helpers_cover_representation_limits() {
    let templates = include_str!("../src/frontend/parallel_pipelines.plenty");
    let split = &templates[..templates.find("def collect[").unwrap()];
    let count =
        &templates[templates.find("def count[").unwrap()..templates.find("def any[").unwrap()];
    let source = format!(
        r#"{split}
{count}
class CountMarker:
    value: i64
{MAIN}    # Only chunk metadata is allocated; fictitious row pointers are never read.
    single = split_rows(0, 16, i64.MAX, 0, 1)?
    print(single[0])?
    pair = split_rows(0, 16, i64.MAX, 0, 2)?
    print(pair[0], pair[1])?
    spans = split_range(i64.MIN, 1, i64.MAX, 0, 2)?
    print(spans[0][1], spans[1][1])?
    with ThreadPoolExecutor(2, 1)? as pool:
        chunks: Result[list[i64], AllocError] = Ok([9223372036854775806, 1]?)
        marker = CountMarker(0)
        # A concrete capturing closure retains executor eligibility through F;
        # stored capture-free callbacks currently erase it to Callable.
        worker = def [marker](value: i64) -> i64:
            drop(marker.value)
            value
        print(count(&pool, chunks, worker)?)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        let result = output_with_optimization(&source, optimize);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "(0, 0, 16, 9223372036854775807)\n(0, 0, 16, 4611686018427387904) (4611686018427387904, 0, 16, 4611686018427387903)\nrange(-9223372036854775808, -4611686018427387904, 1) range(-4611686018427387904, -1, 1)\n9223372036854775807\n"
        );
    }
}

#[test]
fn integer_sum_preserves_shutdown_error_family() {
    runs(
        &format!(
            r#"
{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:
        pool.shutdown()
        print(str.repr(iter([1i8, 2i8]?).par(&pool).sum())?)?
    Ok(())
"#
        ),
        "Result[i8, OverflowError[i8] | PoolMapError].Err(PoolMapError.Shutdown)\n",
    );
}

#[test]
fn parallel_pipeline_tutorial_examples_run() {
    let page = include_str!("../book/src/tutorial/11-concurrency/parallel-pipelines.md");
    let mut count = 0;
    for example in page.split("```plenty\n").skip(1) {
        let (source, rest) = example.split_once("```").unwrap();
        let expected = rest
            .split_once("```output\n")
            .unwrap()
            .1
            .split_once("```")
            .unwrap()
            .0;
        runs(source, expected);
        count += 1;
    }
    assert_eq!(count, 2);
}

#[test]
fn fallible_reduce_keeps_chunk_grouping_and_prioritizes_chunk_errors_before_combining() {
    let source = r#"
def shape(a: i64, b: i64) -> Result[i64, str]:
    Ok(a.wrapping_sub(b))
def fail(a: i64, b: i64) -> Result[i64, str]:
    if b == 5:
        return Err("chunk error")
    if a == 0 and b == 2:
        return Err("combine error")
    Ok(a)
def add[T: IntType](a: T, b: T) -> Result[T, OverflowError[T]]:
    Ok(a + b)
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | str | OverflowError[i64]]:
    with ThreadPoolExecutor(3, 1)? as pool:
        print(iter(range(1, 6)).par(&pool, chunks=2).reduce(shape)?)?
        result = iter(range(6)).par(&pool, chunks=3).reduce(fail)
        match result:
            case Err(error):
                match error:
                    case text: str:
                        print(text)?
                    case infrastructure: PoolMapError:
                        print("unexpected infrastructure")?
            case Ok(_):
                print("missed error")?
        values = [100i8, 100i8]?
        overflow_result = iter(&values).par(&pool, chunks=2).map(def(n: &i8) -> i8: *n).reduce(add)
        match overflow_result:
            case Err(error):
                match error:
                    case overflow: OverflowError[i8]:
                        print(overflow.wrapped)?
                    case infrastructure: PoolMapError:
                        print("unexpected infrastructure")?
            case Ok(_):
                print("missed overflow")?
        print(len(values))?
        start = 1
        nested = checked:
            stop = start + 5
            iter(range(start, stop)).par(&pool, chunks=2).reduce(add)
        print(nested??)?
        unchecked:
            print(iter(range(0)).par(&pool).reduce(add)?)?
    Ok(())
"#;
    for optimize in [false, true] {
        let output = output_with_optimization(source, optimize);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "Option[i64].Some(-3)\nchunk error\n-56\n2\nOption[i64].Some(15)\nOption[i64].Nothing\n"
        );
    }
}

#[test]
fn fallible_reduce_drains_gated_chunks_before_source_reuse_and_moves_managed_errors() {
    let source = r#"
class Row:
    value: i64
    def __del__(self) -> ():
        print("row dropped").unwrap()
class Capture:
    ready: Sender[i64]
    gate: Receiver[i64]
    def __del__(self) -> ():
        print("capture dropped").unwrap()
class ReduceError:
    payload: list[Option[range[i64]]]
    def __del__(self) -> ():
        print("error dropped").unwrap()
def reject(a: i64, b: i64) -> Result[i64, ReduceError | AllocError]:
    Err(ReduceError([Some(range(a, b))]?))
def release(ready: Receiver[i64], gate: Sender[i64]) -> ():
    ready.recv().unwrap()
    gate.send(1).unwrap()
def main() -> Result[(), AllocError | IoError | PoolError | SpawnError[()]]:
    ready, started = channel[i64](1)?
    signal, gate = channel[i64](1)?
    with ThreadPoolExecutor(2, 1)? as pool:
        mut rows = [Row(0), Row(1), Row(2), Row(3), Row(4), Row(5)]?
        capture = Capture(ready, gate)
        read = def [capture](row: &Row) -> i64:
            if row.value == 3:
                capture.ready.send(1).unwrap()
                capture.gate.recv().unwrap()
            row.value
        with spawn(release, started, signal).drop_job()? as releaser:
            result = iter(&rows).par(&pool, chunks=2).map(read).reduce(reject)
            match result:
                case Err(error):
                    match error:
                        case domain: ReduceError:
                            print(len(domain.payload))?
                        case allocation: AllocError:
                            print("unexpected allocation")?
                        case infrastructure: PoolMapError:
                            print("unexpected infrastructure")?
                case Ok(_):
                    print("missed error")?
        rows.append(Row(6))?
        print(len(rows))?
    Ok(())
"#;
    let owned = source
        .replace("row: &Row", "row: Row")
        .replace("iter(&rows)", "iter(rows)")
        .replace(
            "        rows.append(Row(6))?\n        print(len(rows))?\n",
            "",
        );
    for optimize in [false, true] {
        for (program, row_count) in [(source, 7), (owned.as_str(), 6)] {
            let result = output_with_optimization(program, optimize);
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert!(
                result.status.success(),
                "{stdout}\n{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(stdout.matches("row dropped").count(), row_count, "{stdout}");
            assert_eq!(stdout.matches("capture dropped").count(), 1, "{stdout}");
            assert_eq!(stdout.matches("error dropped").count(), 2, "{stdout}");
            assert!(stdout.contains("1\n"), "{stdout}");
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn fallible_reduce_submission_oom_drains_accepted_rows_and_captures() {
    let mut failures = 0;
    let mut successes = 0;
    let mut partial = 0;
    for optimize in [false, true] {
        for budget in 0..24 {
            let source = format!(
                r#"
class Row:
    value: i64
    def __del__(self) -> ():
        print("row dropped").unwrap()
class Capture:
    def __del__(self) -> ():
        print("capture dropped").unwrap()
def left(a: i64, b: i64) -> Result[i64, str]:
    print("combined").unwrap()
    Ok(a)
def main() -> Result[(), AllocError | IoError | PoolError]:
    with ThreadPoolExecutor(2, 1)? as pool:
        rows = [Row(1), Row(2), Row(3), Row(4), Row(5), Row(6)]?
        capture = Capture()
        read = def [capture](row: Row) -> i64:
            print("mapped").unwrap()
            row.value
        print("__test_fail_allocations_after_{budget}__").unwrap()
        result = iter(rows).par(&pool, chunks=3).map(read).reduce(left)
        print("__test_restore_allocations__").unwrap()
        match result:
            case Ok(value):
                print(value)?
            case Err(error):
                print(error)?
    Ok(())
"#
            );
            let result = output_with_optimization(&source, optimize);
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert!(
                result.status.success(),
                "budget {budget}: {stdout}\n{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(stdout.matches("row dropped").count(), 6, "{stdout}");
            assert_eq!(stdout.matches("capture dropped").count(), 1, "{stdout}");
            let failed = stdout.contains("PoolMapError.Allocation(AllocError.OutOfMemory)");
            assert!(stdout.matches("mapped").count() <= 6, "{stdout}");
            assert!(stdout.matches("combined").count() <= 5, "{stdout}");
            if stdout.ends_with("Option[i64].Some(1)\n") {
                assert_eq!(stdout.matches("mapped").count(), 6, "{stdout}");
                assert_eq!(stdout.matches("combined").count(), 5, "{stdout}");
            }
            failures += usize::from(failed);
            successes += usize::from(stdout.ends_with("Option[i64].Some(1)\n"));
            partial += usize::from(failed && stdout.contains("mapped"));
        }
    }
    assert!(
        failures > 0 && successes > 0 && partial > 0,
        "{failures} failures, {successes} successes, {partial} partial"
    );
}

#[test]
fn fallible_reduce_overlapping_errors_and_named_only_diagnostics() {
    let source = r#"
def reject(a: i64, b: i64) -> Result[i64, PoolMapError]:
    Err(PoolMapError.Shutdown)
def mixed(a: i64, b: i64) -> Result[i64, PoolMapError | str]:
    Err("domain error")
def main() -> Result[(), IoError | PoolError | PoolMapError]:
    with ThreadPoolExecutor(2, 1)? as pool:
        empty = iter(range(0)).par(&pool).reduce(reject)
        single = iter(range(9, 10)).par(&pool).reduce(reject)
        rejected = iter(range(4)).par(&pool, chunks=2).reduce(reject)
        union = iter(range(4)).par(&pool, chunks=2).reduce(mixed)
        print(empty?, single?, rejected.is_err(), union.is_err())?
        pool.shutdown()
        closed = iter(range(0)).par(&pool).reduce(mixed)
        print(closed.is_err())?
    Ok(())
"#;
    for optimize in [false, true] {
        let result = output_with_optimization(source, optimize);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "Option[i64].Nothing Option[i64].Some(9) True True\nTrue\n"
        );
    }
    for (terminal, diagnostic) in [
        (
            "pool.reduce_tree(def(a: i64, b: i64) -> Result[i64, str]: Ok(a), range(2))",
            "statically named function",
        ),
        (
            "iter(range(2)).par(&pool).reduce(def(a: i64, b: i64) -> Result[i64, str]: Ok(a))",
            "named function",
        ),
        (
            "pool.reduce_tree(wrong, range(2))",
            "return that same type or Result[T, E]",
        ),
        (
            "iter(range(2)).par(&pool).reduce(wrong)",
            "return T or Result[T, E]",
        ),
    ] {
        let program = format!(
            "def wrong(a: i64, b: i64) -> Result[bool, str]:\n    Ok(True)\ndef main() -> ():\n    with ThreadPoolExecutor(2, 1).unwrap() as pool:\n        drop({terminal})\n"
        );
        let error = support::check_source(&program).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}
