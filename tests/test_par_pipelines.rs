//! Parallel pipelines: `iter(items).par(&pool)` (B75 stage 4).
mod support;

fn output(source: &str) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("par-test");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    std::process::Command::new("timeout")
        .arg("15s")
        .arg(executable)
        .output()
        .unwrap()
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

fn traps(source: &str, stdout: &str, message: &str) {
    let output = output(source);
    assert!(!output.status.success(), "{source} should trap");
    assert_eq!(String::from_utf8_lossy(&output.stdout), stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(message), "{source}\n{stderr}");
}

const MAIN: &str = "def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError]:\n";

#[test]
fn sources_steps_and_collecting_terminals_match_serial_results() {
    runs(
        &format!(
            r#"
def double(x: i64) -> i64:
    x * 2
def odd(x: &i64) -> bool:
    *x % 2 == 1
def big(x: &i64) -> bool:
    *x > 100
def pair(index: i64, value: i64) -> (i64, i64):
    (index, value * 10)
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
    x * 2
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
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError]:
    pools = [ThreadPoolExecutor(2, 1)?]?
    limit = 10
    print(enumerate(items(), note("start", 0)).par(&pools[note("pool", 0)], chunks=note("chunks", 2)).filter(def [limit](i: &i64, v: &i64) -> bool: *v < limit).count()?)?
    print(iter(items()).par(&pools[note("pool", 0)], chunks=note("chunks", 3)).filter_value(note("value", 2)).map(def [limit](x: i64) -> i64: x + limit).any(def [limit](x: &i64) -> bool: *x > limit)?)?
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
        out = iter(range(0, 8)).par(&pool).filter(def [limit](x: &i64) -> bool: *x < limit).map(def [scale](x: i64) -> i64: x * scale.factor).collect()?
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
    Ok(x * 10)
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        for chunks in [1, 2, 4, 8]?:
            r = iter(range(0, 8)).par(&pool, chunks=chunks).map(check).try_collect()
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

#[test]
fn reduce_matches_reduce_tree_and_empty_input_gives_nothing() {
    runs(
        &format!(
            r#"
def shape(a: i64, b: i64) -> i64:
    a * 3 - b
{MAIN}    with ThreadPoolExecutor(3, 1)? as pool:
        print(iter([5, 1, 4, 2, 8, 7, 3]?).par(&pool).reduce(shape)?)?
        print(pool.reduce_tree(shape, [5, 1, 4, 2, 8, 7, 3]?)?)?
        print(iter(range(0, 0)).par(&pool).reduce(shape)?)?
        print(iter(range(4, 5)).par(&pool).reduce(shape)?)?
    Ok(())
"#
        ),
        "Option[i64].Some(48)\nOption[i64].Some(48)\nOption[i64].Nothing\nOption[i64].Some(4)\n",
    );
}

#[test]
fn sums_match_serial_results_and_overflow_like_serial_sum() {
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
    // Serial `sum` traps when any running total overflows, even if a later
    // item brings it back in range; the parallel sum must agree.
    for (items, chunks) in [
        ("[100i8, 100i8, -100i8]", 3),
        ("[100i8, 100i8, -100i8]", 1),
        ("[-100i8, -100i8, 100i8]", 2),
        ("[i64.MAX, 1, -1]", 1),
        ("[200u8, 100u8]", 2),
        ("[0, 0, 1, i64.MAX]", 2),
        ("[0, 0, -2, i64.MIN + 1]", 2),
        ("[i64.MAX, i64.MAX, i64.MIN, i64.MIN, 1]", 5),
    ] {
        traps(
            &format!(
                "{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:\n        print(\"start\")?\n        print(iter({items}?).par(&pool, chunks={chunks}).sum()?)?\n    Ok(())\n"
            ),
            "start\n",
            "integer overflow",
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
def total(pool: &ThreadPoolExecutor, chunks: i64) -> Result[f64, AllocError | PoolMapError]:
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
    for chunks in ["0", "-3"] {
        traps(
            &format!(
                "{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:\n        n = {chunks}\n        print(\"start\")?\n        print(iter(range(0, 4)).par(&pool, chunks=n).count()?)?\n    Ok(())\n"
            ),
            "start\n",
            "integer cast out of range",
        );
    }
}

#[test]
fn shut_down_pool_reports_pool_map_error() {
    runs(
        &format!(
            r#"
def double(x: i64) -> i64:
    x * 2
{MAIN}    with ThreadPoolExecutor(2, 1)? as pool:
        pool.shutdown()
        print(str.repr(iter(range(0, 4)).par(&pool).map(double).collect())?)?
    Ok(())
"#
        ),
        "Result[list[i64], PoolMapError].Err(PoolMapError.Shutdown)\n",
    );
}

#[test]
fn par_rejects_unsupported_shapes() {
    let header = "def add(a: i64, b: i64) -> i64:\n    a + b\ndef double(x: i64) -> i64:\n    x * 2\ndef show(x: i64) -> ():\n    pass\ndef main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | ParallelError[AllocError]]:\n    with ThreadPoolExecutor(3, 1)? as pool:\n        xs = [1, 2, 3]?\n";
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
            "print(iter(&xs).par(&pool).count()?)?",
            "par needs an owned list or range",
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
            "chunks must be a positive i64",
        ),
        (
            "print(iter(xs).par(&pool, chunks=-2).count()?)?",
            "chunks must be a positive i64",
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
        let source = format!("{header}        {body}\n    Ok(())\n");
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
        result = iter(xs).par(&pool).map(def [capture](x: i64) -> i64: x * capture.factor).collect()
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
