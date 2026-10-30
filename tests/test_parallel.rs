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
    runs(
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
            n * len(capture.values)
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
            n + offset
        print(pool.map(shift, range(3))?)?
    Ok(())
"#,
        "capture dropped\n[0, 2, 4, 6]\nResult[list[i64], ParallelError[str]].Err(ParallelError[str].Worker(\"error\"))\n[10, 11, 12]\n",
    );
}

#[test]
fn mapping_rejects_unsafe_callback_ownership_and_uncertified_effects() {
    for (worker, diagnostic) in [
        (
            "job = def once [value](n: i64) -> i64:\n    n + value\n",
            "reusable immutable closure",
        ),
        (
            "mut job = def [mut value](n: i64) -> i64:\n    value = value + n\n    value\n",
            "reusable immutable closure",
        ),
        (
            "job = def [&value](n: i64) -> i64:\n    n + value\n",
            "must own their captures",
        ),
        (
            "def seven() -> i64:\n    7\njob = def [value](n: i64) -> i64:\n    f: Callable[[], i64] = seven\n    n + value + f()\n",
            "indirect callable effects",
        ),
        (
            "def identity(n: i64) -> i64:\n    n\njob = identity\n",
            "lack worker eligibility evidence",
        ),
        (
            "job = def [value](n: &i64) -> i64:\n    *n + value\n",
            "one owned input",
        ),
    ] {
        let source = format!(
            "value = 2\n{worker}with ThreadPoolExecutor(2, 1).unwrap() as pool:\n    drop(pool.map(job, range(3)))\n"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
    }
}

#[test]
fn fallible_map_drains_shared_callback_before_destroying_its_capture() {
    runs(
        r#"
class Capture:
    sender: Sender[i64]
    def __del__(self) -> ():
        self.sender.send(99).unwrap()
def main() -> Result[(), AllocError | ChannelError | IoError | PoolError | RecvError]:
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
    runs(
        r#"
def callback() -> Closure[[range[i64]], range[i64]]:
    print("callback").unwrap()
    offset = 1
    def [offset](value: range[i64]) -> range[i64]:
        range(len(value) + offset)
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
        runs(
            &format!(
                r#"
class Capture:
    message: str
    def __del__(self) -> ():
        print(self.message).unwrap()
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    capture = Capture("dropped")
    work = def [capture](n: i64) -> i64:
        n + len(capture.message)
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
    runs(
        r#"
def square[T: IntType](n: T) -> Result[T, str]:
    Ok(n * n)
def fail(n: i64) -> Result[i64, range[i64]]:
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
def main() -> Result[(), IoError | ParallelError[str] | PoolError]:
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
        "[0, 1, 4, 9, 16]\n[]\nrange(2, 4, 1)\nResult[list[i64], ParallelError[str]].Err(ParallelError[str].Shutdown)\n",
    );
}

#[test]
fn lowest_input_error_wins_even_when_a_later_failure_finishes_first() {
    runs(
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
def main() -> Result[(), AllocError | ChannelError | IoError | PoolError | RecvError]:
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
    left - right
def add(left: f64, right: f64) -> f64:
    left + right
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError]:
    for workers in range[u64](1, 5):
        with ThreadPoolExecutor(workers, 1)? as pool:
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
            "def work(a: i64, b: i64) -> i64:\n    pool = ThreadPoolExecutor(1, 1).unwrap()\n    a + b\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    drop(pool.reduce_tree(work, range(3)))\n",
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
