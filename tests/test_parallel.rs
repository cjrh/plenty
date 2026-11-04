mod support;

fn runs(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("parallel-test");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = support::output_within(
        &mut std::process::Command::new(executable),
        b"",
        std::time::Duration::from_secs(15),
    );
    assert!(
        output.status.success(),
        "status {:?}; stdout {}; stderr {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn maps_share_owned_immutable_closures_including_generic_callable_workers() {
    parallel_runs_in_both_modes(
        r#"
class Capture:
    values: list[i64]
    def __del__(self) -> ():
        print("capture dropped").unwrap()
def mapped[F: Callable[[i64], i64]](pool: &ThreadPoolExecutor, worker: F) -> Result[list[i64], PoolMapError]:
    pool.map(worker, range(4))
def fallible[F: Callable[[i64], Result[i64, str]]](pool: &ThreadPoolExecutor, worker: F) -> Result[list[i64], ParallelError[str]]:
    pool.map_result(worker, range(3))
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError]:
    with ThreadPoolExecutor(3, 1)? as pool:
        capture = Capture([2, 3]?)
        scale = def [capture](n: i64) -> i64:
            # Fixed fixture: n is 0..3 and capture length is two.
            result = checked:
                n * len(capture.values)
            result.unwrap()
        print(mapped(&pool, scale)?)?
        label = "error"
        work = def [label](n: i64) -> Result[i64, str]:
            if n == 1:
                Err(label)
            else:
                Ok(n)
        print(str.repr(fallible(&pool, work))?)?
        offset = 10
        shift = def [offset](n: i64) -> i64:
            # Fixed fixture: n is 0..2 and offset is ten; retain map's i64 ABI.
            result = checked:
                n + offset
            result.unwrap()
        print(pool.map(shift, range(3))?)?
    Ok(())
"#,
        "capture dropped\n[0, 2, 4, 6]\nResult[list[i64], ParallelError[str]].Err(ParallelError[str].Worker(\"error\"))\n[10, 11, 12]\n",
    );
}

#[test]
fn mapping_rejects_unsafe_callback_ownership_and_uncertified_effects() {
    // Fixed n=0..2/value=2 fixtures retain their i64 ABI so arithmetic
    // diagnostics cannot mask the intended ownership/eligibility rejection.
    for (worker, diagnostic) in [
        (
            "job = def once [value](n: i64) -> i64:\n    result = checked:\n        n + value\n    result.unwrap()\n",
            "reusable immutable closure",
        ),
        (
            "mut job = def [mut value](n: i64) -> i64:\n    result = checked:\n        value + n\n    value = result.unwrap()\n    value\n",
            "reusable immutable closure",
        ),
        (
            "job = def [&value](n: i64) -> i64:\n    result = checked:\n        n + value\n    result.unwrap()\n",
            "must own their captures",
        ),
        (
            "def seven() -> i64:\n    path = std.env.current_dir().unwrap()\n    std.env.set_current_dir(&path).unwrap()\n    7\njob = def [value](n: i64) -> i64:\n    f: Callable[[], i64] = seven\n    result = checked:\n        n + value + f()\n    result.unwrap()\n",
            "changing the working directory",
        ),
        (
            "def identity(n: i64) -> i64:\n    n\njob = identity\n",
            "lack worker eligibility evidence",
        ),
        (
            "job = def [value](n: &i64) -> i64:\n    result = checked:\n        *n + value\n    result.unwrap()\n",
            "one owned input",
        ),
    ] {
        let source = format!(
            "value = 2\n{worker}with ThreadPoolExecutor(2, 1).unwrap() as pool:\n    drop(pool.map(job, range(3)))\n"
        );
        let source = format!("import std.env\n{}", support::program(&source));
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
    }
}

#[test]
fn fallible_map_drains_shared_callback_before_destroying_its_capture() {
    parallel_runs_in_both_modes(
        r#"
class Capture:
    sender: Sender[i64]
    def __del__(self) -> ():
        self.sender.send(99).unwrap()
def main() -> Result[(), AllocError | IoError | PoolError | RecvError | OverflowError[i64]]:
    sender, receiver = channel[i64](16)?
    with ThreadPoolExecutor(3, 1)? as pool:
        capture = Capture(sender)
        work = def [capture](n: i64) -> Result[i64, i64]:
            capture.sender.send(n).unwrap()
            Err(n)
        match pool.map_result(work, range(8)):
            case Err(error):
                match error:
                    case ParallelError.Worker(value):
                        print(value)?
                    case _:
                        print(-1)?
            case Ok(values):
                print(values)?
        mut observed = 0
        while True:
            n = receiver.recv()?
            if n == 99:
                break
            observed = observed + 1
        print(observed > 0)?
        print(str.repr(receiver.recv_nowait())?)?
    Ok(())
"#,
        "0\nTrue\nResult[i64, RecvError].Err(RecvError.Disconnected)\n",
    );
}

#[test]
fn map_evaluates_the_callback_before_its_input_and_moves_inline_input_values() {
    parallel_runs_in_both_modes(
        r#"
def callback() -> Closure[[range[i64]], range[i64]]:
    print("callback").unwrap()
    offset = 1
    def [offset](value: range[i64]) -> range[i64]:
        # Fixed ranges have length three or two, and offset is one.
        size = checked:
            len(value) + offset
        range(size.unwrap())
def inputs() -> list[range[i64]]:
    print("inputs").unwrap()
    [range(3), range(4, 6)].unwrap()
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    print(pool.map(callback(), inputs()).unwrap()).unwrap()
"#,
        "callback\ninputs\n[range(0, 4, 1), range(0, 3, 1)]\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn map_failure_and_shutdown_drop_shared_captures_once_and_drain_jobs() {
    for budget in 0..8 {
        parallel_runs_in_both_modes(
            &format!(
                r#"
class Capture:
    message: str
    def __del__(self) -> ():
        print(self.message).unwrap()
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    capture = Capture("dropped")
    work = def [capture](n: i64) -> i64:
        # Fixed n=0..7 and seven-character capture; keep the infallible ABI.
        result = checked:
            n + len(capture.message)
        result.unwrap()
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = pool.map(work, range(8))
    print("__test_restore_allocations__").unwrap()
    drop(result)
    shutdown_capture = Capture("shutdown capture")
    shutdown_work = def [shutdown_capture](n: i64) -> i64:
        n
    pool.shutdown()
    drop(pool.map(shutdown_work, range(0)))
"#
            ),
            &format!(
                "__test_fail_allocations_after_{budget}__\ndropped\n__test_restore_allocations__\nshutdown capture\n"
            ),
        );
    }
}

#[test]
fn fallible_map_preserves_order_and_concrete_error() {
    parallel_runs_in_both_modes(
        r#"
def square[T: IntType](n: T) -> Result[T, OverflowError[T]]:
    Ok(n * n)
def fail(n: i64) -> Result[i64, range[i64] | OverflowError[i64]]:
    if n == 2 or n == 3:
        Err(range(n, n + 2))
    else:
        Ok(n)
def inspect[E](error: ParallelError[E]) -> E:
    match error:
        case ParallelError.Worker(value):
            value
        case _:
            fail_unreachable()
def fail_unreachable() -> range[i64]:
    range(0)
def main() -> Result[(), IoError | ParallelError[OverflowError[u8]] | ParallelError[OverflowError[i64]] | PoolError]:
    with ThreadPoolExecutor(3, 1)? as pool:
        print(pool.map_result(square, range[u8](5))?)?
        print(pool.map_result(square, range(0))?)?
        match pool.map_result(fail, range(6)):
            case Err(error):
                print(inspect(error))?
            case Ok(values):
                print(values)?
        pool.shutdown()
        print(str.repr(pool.map_result(square, range(0))).unwrap())?
    Ok(())
"#,
        "[0, 1, 4, 9, 16]\n[]\nrange(2, 4, 1)\nResult[list[i64], ParallelError[OverflowError[i64]]].Err(ParallelError[OverflowError[i64]].Shutdown)\n",
    );
}

#[test]
fn lowest_input_error_wins_even_when_a_later_failure_finishes_first() {
    parallel_runs_in_both_modes(
        r#"
class Input:
    index: i64
    signal: Sender[i64]
    wait: Receiver[i64]
    dropped: Sender[i64]
    def __del__(self) -> ():
        self.dropped.send(self.index).unwrap()
def work(value: Input) -> Result[Input, Input]:
    if value.index == 1:
        value.wait.recv().unwrap()
    if value.index == 2:
        value.signal.send(1).unwrap()
    if value.index >= 1:
        Err(value)
    else:
        Ok(value)
def main() -> Result[(), AllocError | IoError | PoolError | RecvError | OverflowError[i64]]:
    signal, wait = channel[i64](1)?
    dropped, count = channel[i64](8)?
    with ThreadPoolExecutor(3, 1)? as pool:
        inputs = [Input(n, signal.share(), wait.share(), dropped.share()) for n in range(8)]?
        match pool.map_result(work, inputs):
            case Err(error):
                match error:
                    case ParallelError.Worker(value):
                        print(value.index)?
                    case _:
                        print(-1)?
            case Ok(values):
                print(-2)?
        mut total = 0
        for n in range(8):
            total = total + count.recv()?
        print(total)?
    Ok(())
"#,
        "1\n28\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn fallible_map_allocation_failures_clean_owned_inputs_and_outputs() {
    for budget in 0..10 {
        runs(
            &format!(
                r#"
def work(value: list[range[i64]]) -> Result[list[range[i64]], list[range[i64]]]:
    if len(value[0]) == 5:
        Err(value)
    else:
        Ok(value)
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    inputs = [[range(3)].unwrap(), [range(4)].unwrap(), [range(5)].unwrap(), [range(6)].unwrap()].unwrap()
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = pool.map_result(work, inputs)
    print("__test_restore_allocations__").unwrap()
    drop(result)
"#
            ),
            &format!("__test_fail_allocations_after_{budget}__\n__test_restore_allocations__\n"),
        );
    }
}

#[test]
fn tree_grouping_is_fixed_across_pool_sizes_and_not_a_left_fold() {
    runs(
        r#"
def subtract[T: IntType](left: T, right: T) -> T:
    left.wrapping_sub(right)
def add(left: f64, right: f64) -> f64:
    left + right
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError]:
    for workers in range(1, 5):
        with ThreadPoolExecutor(Positive.try_from(workers).unwrap(), 1)? as pool:
            print(pool.reduce_tree(subtract, range(1, 6))?)?
            print(pool.reduce_tree(subtract, range[i32](1, 5))?)?
            print(pool.reduce_tree(add, [1e16, 1.0, -1e16, 1.0]?)?)?
    with ThreadPoolExecutor(2, 1)? as pool:
        print(pool.reduce_tree(subtract, range(0))?)?
        print(pool.reduce_tree(subtract, range(9, 10))?)?
        pool.shutdown()
        print(str.repr(pool.reduce_tree(subtract, range(0))).unwrap())?
    Ok(())
"#,
        &format!(
            "{}Option[i64].Nothing\nOption[i64].Some(9)\nResult[Option[i64], PoolMapError].Err(PoolMapError.Shutdown)\n",
            "Option[i64].Some(-5)\nOption[i32].Some(0)\nOption[f64].Some(0.0)\n".repeat(4)
        ),
    );
}

#[test]
fn tree_transfers_owned_and_nested_inline_values() {
    runs(
        r#"
def left[T](a: T, b: T) -> T:
    a
def callback(n: i64) -> Closure[[], i64]:
    def [n]() -> i64:
        n
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError]:
    with ThreadPoolExecutor(3, 2)? as pool:
        values = [[1, 2]?, [3]?, [4]?, [5]?, [6]?]?
        print(pool.reduce_tree(left, values)?)?
        spans = [Some(range(2, 5)), Nothing, Some(range(9)), Nothing, Some(range(1))]?
        print(pool.reduce_tree(left, spans)?)?
        print(pool.reduce_tree(left, [Some(range(4, 8))]?)?)?
        callbacks = [callback(42), callback(43), callback(44)]?
        chosen = (pool.reduce_tree(left, callbacks)?).unwrap()
        print(chosen())?
    Ok(())
"#,
        "Option[list[i64]].Some([1, 2])\nOption[Option[range]].Some(Option[range].Some(range(2, 5, 1)))\nOption[Option[range]].Some(Option[range].Some(range(4, 8, 1)))\n42\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reduction_failure_at_every_allocation_cleans_up_and_keeps_pool_usable() {
    for budget in 0..12 {
        runs(
            &format!(
                r#"
def left[T](a: T, b: T) -> T:
    a
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    inputs = [[range(3)].unwrap(), [range(4)].unwrap(), [range(5)].unwrap(), [range(6)].unwrap(), [range(7)].unwrap()].unwrap()
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = pool.reduce_tree(left, inputs)
    print("__test_restore_allocations__").unwrap()
    drop(result)
    print(pool.reduce_tree(left, range(2)).unwrap()).unwrap()
"#
            ),
            &format!(
                "__test_fail_allocations_after_{budget}__\n__test_restore_allocations__\nOption[i64].Some(0)\n"
            ),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn empty_and_singleton_reductions_do_not_allocate() {
    runs(
        r#"
def left[T](a: T, b: T) -> T:
    a
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    inputs = [Some(range(4, 8))].unwrap()
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    empty = pool.reduce_tree(left, range(0)).unwrap()
    single = pool.reduce_tree(left, inputs).unwrap()
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(empty).unwrap()
    print(single).unwrap()
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\nOption[i64].Nothing\nOption[Option[range]].Some(Option[range].Some(range(4, 8, 1)))\n",
    );
}

#[test]
fn parallel_operations_reject_uncertified_workers_and_wrong_signatures() {
    for (source, diagnostic) in [
        (
            "def work(n: i64) -> i64:\n    n\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    pool.map_result(work, range(3))\n",
            "map_result worker must return Result",
        ),
        (
            "def work(n: i64) -> Result[(), str]:\n    Ok(())\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    pool.map_result(work, range(3))\n",
            "non-unit T",
        ),
        (
            "def work(a: i64, b: i64) -> str:\n    return \"wrong\"\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    pool.reduce_tree(work, range(3))\n",
            "return that same type",
        ),
        (
            "def work(a: i64, b: i64) -> i64:\n    pool = ThreadPoolExecutor(1, 1).unwrap()\n    a.wrapping_add(b)\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    drop(pool.reduce_tree(work, range(3)))\n",
            "executor",
        ),
        (
            "def work(n: i64) -> Result[i64, Future[i64]]:\n    Ok(n)\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    drop(pool.map_result(work, range(3)))\n",
            "future handles cannot enter worker threads",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "expected {diagnostic}: {error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn earlier_worker_error_takes_precedence_over_later_submission_failure() {
    runs(
        r#"
def reject(n: i64) -> Result[i64, i64]:
    Err(n)
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    print("__test_fail_allocations_after_4__").unwrap()
    result = pool.map_result(reject, range(8))
    print("__test_restore_allocations__").unwrap()
    print(str.repr(result).unwrap()).unwrap()
"#,
        "__test_fail_allocations_after_4__\n__test_restore_allocations__\nResult[list[i64], ParallelError[i64]].Err(ParallelError[i64].Worker(0))\n",
    );
}

/// Public callbacks and native slot ownership must work with either backend mode.
fn parallel_runs_in_both_modes(source: &str, expected: &str) {
    for optimize in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reduce.plenty");
        let executable = directory.path().join("reduce");
        std::fs::write(&path, support::program(source)).unwrap();
        plenty::compile_file_to_executable_with_options(
            &path,
            &executable,
            None,
            &plenty::CompileOptions {
                optimize,
                test_hooks: true,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
        let output = support::output_within(
            &mut std::process::Command::new(executable),
            b"",
            std::time::Duration::from_secs(15),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn fallible_tree_public_results_preserve_grouping_payloads_and_caller_modes() {
    parallel_runs_in_both_modes(r#"
def add[T: IntType](a: T, b: T) -> Result[T, OverflowError[T]]:
    Ok(a + b)
def shape(a: i64, b: i64) -> Result[i64, str]:
    Ok(a.wrapping_sub(b))
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | OverflowError[i64] | str]:
    for workers in [1, 2, 3, 8]?:
        with ThreadPoolExecutor(Positive.try_from(workers).unwrap(), 1)? as pool:
            print(pool.reduce_tree(shape, range(1, 6))?)?
            print(pool.reduce_tree(add, range(1, 6))?)?
            print(pool.reduce_tree(add, range(0))?)?
            print(pool.reduce_tree(add, range(9, 10))?)?
            result = pool.reduce_tree(add, [100i8, 100i8]?)
            match result:
                case Err(error):
                    match error:
                        case overflow: OverflowError[i8]:
                            print(overflow.wrapped)?
                        case infrastructure: PoolMapError:
                            print("unexpected infrastructure")?
                case Ok(_):
                    print("missed overflow")?
            nested = checked:
                # A checked suite must contain its own fallible arithmetic.
                drop(workers + 1)
                pool.reduce_tree(add, [100i8, 100i8]?)
            print(nested?.is_err())?
            unchecked:
                unchecked_result = pool.reduce_tree(add, [100i8, 100i8]?)
                print(unchecked_result.is_err())?
    Ok(())
"#, &"Option[i64].Some(-5)\nOption[i64].Some(15)\nOption[i64].Nothing\nOption[i64].Some(9)\n-56\nTrue\nTrue\n".repeat(4));
}

#[test]
fn fallible_tree_selects_level_then_pair_and_accepts_overlapping_error_unions() {
    parallel_runs_in_both_modes(
        r#"
def levels(a: i64, b: i64) -> Result[i64, str]:
    if a == 4:
        return Err("level zero")
    if a == 0 and b == 2:
        return Err("level one")
    Ok(a)
def overlap(a: i64, b: i64) -> Result[i64, PoolMapError]:
    Err(PoolMapError.Shutdown)
def union(a: i64, b: i64) -> Result[i64, PoolMapError | str]:
    Err("union error")
def main() -> Result[(), AllocError | IoError | PoolError]:
    for workers in [1, 2, 3, 8]?:
        with ThreadPoolExecutor(Positive.try_from(workers).unwrap(), 1)? as pool:
            match pool.reduce_tree(levels, range(8)):
                case Err(error):
                    match error:
                        case message: str:
                            print(message)?
                        case infrastructure: PoolMapError:
                            print("unexpected infrastructure")?
                case Ok(_):
                    print("missed error")?
            print(pool.reduce_tree(overlap, range(2)).is_err())?
            print(pool.reduce_tree(union, range(2)).is_err())?
            pool.shutdown()
            print(pool.reduce_tree(union, range(0)).is_err())?
    Ok(())
"#,
        &"level zero\nTrue\nTrue\nTrue\n".repeat(4),
    );
}

#[test]
fn fallible_tree_moves_managed_errors_and_drops_unused_inputs_and_errors_once() {
    parallel_runs_in_both_modes(
        r#"
class Item:
    events: Sender[i64]
    def __del__(self) -> ():
        self.events.send(1).unwrap()
class ReduceFailure:
    payload: list[Option[range[i64]]]
    events: Sender[i64]
    def __del__(self) -> ():
        self.events.send(100).unwrap()
def fail(a: Item, b: Item) -> Result[Item, ReduceFailure | AllocError]:
    Err(ReduceFailure([Some(range(3))]?, a.events.share()))
def main() -> Result[(), AllocError | IoError | PoolError]:
    events, received = channel[i64](32)?
    with ThreadPoolExecutor(2, 1)? as pool:
        inputs = [Item(events.share()), Item(events.share()), Item(events.share()), Item(events.share()), Item(events.share()), Item(events.share()), Item(events.share()), Item(events.share())]?
        result = pool.reduce_tree(fail, inputs)
        match result:
            case Err(error):
                match error:
                    case failure: ReduceFailure:
                        print(len(failure.payload))?
                    case allocation: AllocError:
                        print("unexpected allocation")?
                    case infrastructure: PoolMapError:
                        print("unexpected infrastructure")?
            case Ok(_):
                print("missed error")?
        mut total = 0
        for _ in range(11):
            total = total.wrapping_add(received.recv().unwrap())
        print(total)?
    Ok(())
"#,
        "1\n308\n",
    );
}

#[test]
fn fallible_tree_tutorial_example_runs_in_both_modes() {
    let page = include_str!("../book/src/tutorial/11-concurrency/parallel-reduction.md");
    let source = page
        .split_once("```plenty\n")
        .unwrap()
        .1
        .split_once("```")
        .unwrap()
        .0;
    let expected = page
        .split_once("```output\n")
        .unwrap()
        .1
        .split_once("```")
        .unwrap()
        .0;
    parallel_runs_in_both_modes(source, expected);
}

#[cfg(feature = "runtime-checks")]
#[test]
fn fallible_tree_allocation_failures_preserve_errors_and_pool_reuse() {
    for budget in 0..18 {
        parallel_runs_in_both_modes(
            &format!(
                r#"
def left[T](a: T, b: T) -> Result[T, str]:
    Ok(a)
def reject(a: i64, b: i64) -> Result[i64, str]:
    Err("first callback")
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | str]:
    with ThreadPoolExecutor(2, 1)? as pool:
        values = [[Some(range(3))]?, [Some(range(4))]?, [Some(range(5))]?, [Some(range(6))]?, [Some(range(7))]?]?
        print("__test_fail_allocations_after_{budget}__").unwrap()
        result = pool.reduce_tree(left, values)
        print("__test_restore_allocations__").unwrap()
        drop(result)
        print(pool.reduce_tree(left, range(2))?)?
    Ok(())
"#
            ),
            &format!(
                "__test_fail_allocations_after_{budget}__\n__test_restore_allocations__\nOption[i64].Some(0)\n"
            ),
        );
    }
    // Values scratch + Pending + first job succeed; the next job fails.
    parallel_runs_in_both_modes(
        r#"
def reject(a: i64, b: i64) -> Result[i64, str]:
    Err("first callback")
def main() -> Result[(), IoError | PoolError]:
    with ThreadPoolExecutor(2, 1)? as pool:
        values = range(8)
        print("__test_fail_allocations_after_3__").unwrap()
        result = pool.reduce_tree(reject, values)
        print("__test_restore_allocations__").unwrap()
        match result:
            case Err(error):
                match error:
                    case text: str:
                        print(text)?
                    case infrastructure: PoolMapError:
                        print("unexpected infrastructure")?
            case Ok(_):
                print("missed error")?
    Ok(())
"#,
        "__test_fail_allocations_after_3__\n__test_restore_allocations__\nfirst callback\n",
    );
}

#[test]
fn fallible_tree_matches_ordinary_fixed_tree_for_odd_and_large_lengths() {
    parallel_runs_in_both_modes(
        r#"
def shape(a: i64, b: i64) -> i64:
    a.wrapping_mul(1000003).wrapping_add(b.wrapping_mul(31)).wrapping_add(17)
def fallible(a: i64, b: i64) -> Result[i64, str]:
    Ok(shape(a, b))
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | str]:
    mut lengths = [127, 128, 129, 1000, 1025]?
    for n in range(71):
        lengths.append(n)?
    mut comparisons = 0
    for workers in [1, 2, 3, 8]?:
        with ThreadPoolExecutor(Positive.try_from(workers).unwrap(), 2)? as pool:
            for n in &lengths:
                if pool.reduce_tree(fallible, range(*n))? != pool.reduce_tree(shape, range(*n))?:
                    print("wrong tree")?
                comparisons = comparisons.wrapping_add(1)
    print(comparisons)?
    Ok(())
"#,
        "304\n",
    );
}

#[test]
fn fallible_tree_drains_gated_accepted_pairs_before_unused_source_drops() {
    parallel_runs_in_both_modes(
        r#"
class Row:
    index: i64
    ready: Sender[i64]
    gate: Receiver[i64]
    deaths: Sender[i64]
    def __del__(self) -> ():
        self.deaths.send(1).unwrap()
def reject(a: Row, b: Row) -> Result[Row, i64]:
    if a.index == 2:
        a.ready.send(1).unwrap()
        a.gate.recv().unwrap()
    Err(a.index)
def release(ready: Receiver[i64], gate: Sender[i64]) -> ():
    ready.recv().unwrap()
    gate.send(1).unwrap()
def main() -> Result[(), AllocError | IoError | PoolError | SpawnError[()]]:
    ready, started = channel[i64](1)?
    signal, gate = channel[i64](1)?
    deaths, dead = channel[i64](16)?
    with ThreadPoolExecutor(2, 1)? as pool:
        mut rows: list[Row] = []?
        for n in range(8):
            rows.append(Row(n, ready.share(), gate.share(), deaths.share()))?
        drop(ready)
        drop(deaths)
        with spawn(release, started, signal).drop_job()?, gate as held_gate, dead as received:
            match pool.reduce_tree(reject, rows):
                case Err(error):
                    match error:
                        case index: i64:
                            print(index)?
                        case infrastructure: PoolMapError:
                            print("unexpected infrastructure")?
                case Ok(_):
                    print("missed error")?
            mut count = 0
            for _ in range(8):
                count = count.wrapping_add(received.recv().unwrap())
            print(count)?
    Ok(())
"#,
        "0\n8\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn fallible_tree_empty_singleton_never_call_or_allocate() {
    parallel_runs_in_both_modes(
        r#"
def reject[T](a: T, b: T) -> Result[T, str]:
    Err("must not run")
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | str]:
    with ThreadPoolExecutor(2, 1)? as pool:
        values = [Some(range(4, 8))]?
        print("__test_fail_allocations_after_0__").unwrap()
        empty = pool.reduce_tree(reject, range(0))?
        single = pool.reduce_tree(reject, values)?
        print("__test_restore_allocations__").unwrap()
        print(empty, single)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_restore_allocations__\nOption[i64].Nothing Option[Option[range]].Some(Option[range].Some(range(4, 8, 1)))\n",
    );
}

#[test]
fn fallible_tree_success_calls_each_pair_once_and_returns_owned_payload() {
    parallel_runs_in_both_modes(
        r#"
class Row:
    value: list[Option[range[i64]]]
    events: Sender[i64]
def left(a: Row, b: Row) -> Result[Row, str]:
    a.events.send(1).unwrap()
    Ok(a)
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | str]:
    events, received = channel[i64](32)?
    with ThreadPoolExecutor(3, 1)? as pool:
        mut rows: list[Row] = []?
        for n in range(7):
            rows.append(Row([Some(range(n))]?, events.share()))?
        drop(events)
        result = pool.reduce_tree(left, rows)?
        match result:
            case Some(row):
                print(row.value)?
            case Nothing:
                print("missing row")?
        mut calls = 0
        while True:
            match received.recv():
                case Ok(value):
                    calls = calls.wrapping_add(value)
                case Err(_):
                    break
        print(calls)?
    Ok(())
"#,
        "[Option[range].Some(range(0, 0, 1))]\n6\n",
    );
}

#[test]
fn fallible_tree_and_pipeline_relocate_inline_error_payloads() {
    parallel_runs_in_both_modes(
        r#"
def span(a: i64, b: i64) -> Result[i64, range[i64]]:
    Err(range(a, b))
def show(result: Result[Option[i64], PoolMapError | range[i64]]) -> Result[(), IoError]:
    match result:
        case Err(error):
            match error:
                case interval: range[i64]:
                    print(interval)?
                case infrastructure: PoolMapError:
                    print("unexpected infrastructure")?
        case Ok(_):
            print("missed error")?
    Ok(())
def main() -> Result[(), IoError | PoolError]:
    with ThreadPoolExecutor(2, 1)? as pool:
        show(pool.reduce_tree(span, range(4)))?
        result = iter(range(4)).par(&pool, chunks=2).reduce(span)
        show(result)?
    Ok(())
"#,
        "range(0, 1, 1)\nrange(0, 1, 1)\n",
    );
}
