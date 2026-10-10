mod support;

fn runs(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("parallel-test");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = std::process::Command::new("timeout")
        .arg("15s")
        .arg(executable)
        .output()
        .unwrap();
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
fn fallible_map_preserves_order_and_concrete_error() {
    runs(r#"
def square[T: IntType](n: T) -> Result[T, str]:
    Ok(n * n)
def fail(n: i64) -> Result[i64, range[i64]]:
    if n == 2 or n == 3:
        Err(range(n, n + 2))
    else:
        Ok(n)
def inspect[E](error: ParallelError[E]) -> E:
    match error:
        case ParallelError[E].Worker(value):
            value
        case _:
            fail_unreachable()
def fail_unreachable() -> range[i64]:
    range(0)
def main() -> Result[(), Failure]:
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
"#, "[0, 1, 4, 9, 16]\n[]\nrange(2, 4, 1)\nResult[list[i64], ParallelError[str]].Err(ParallelError[str].Shutdown)\n");
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
def main() -> Result[(), Failure]:
    signal, wait = channel[i64](1)?
    dropped, count = channel[i64](8)?
    with ThreadPoolExecutor(3, 1)? as pool:
        inputs = [Input(n, signal.share(), wait.share(), dropped.share()) for n in range(8)]?
        match pool.map_result(work, inputs):
            case Err(error):
                match error:
                    case ParallelError[Input].Worker(value):
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
    runs(r#"
def subtract[T: IntType](left: T, right: T) -> T:
    left - right
def add(left: f64, right: f64) -> f64:
    left + right
def main() -> Result[(), Failure]:
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
"#, &format!("{}Option[i64].Nothing\nOption[i64].Some(9)\nResult[Option[i64], PoolMapError].Err(PoolMapError.Shutdown)\n",
    "Option[i64].Some(-5)\nOption[i32].Some(0)\nOption[f64].Some(0.0)\n".repeat(4)));
}

#[test]
fn tree_transfers_owned_and_nested_inline_values() {
    runs(r#"
def left[T](a: T, b: T) -> T:
    a
def callback(n: i64) -> Closure[[], i64]:
    def [n]() -> i64:
        n
def main() -> Result[(), Failure]:
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
"#, "Option[list[i64]].Some([1, 2])\nOption[Option[range]].Some(Option[range].Some(range(2, 5, 1)))\nOption[Option[range]].Some(Option[range].Some(range(4, 8, 1)))\n42\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reduction_failure_at_every_allocation_cleans_up_and_keeps_pool_usable() {
    for budget in 0..12 {
        runs(&format!(r#"
def left[T](a: T, b: T) -> T:
    a
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    inputs = [[range(3)].unwrap(), [range(4)].unwrap(), [range(5)].unwrap(), [range(6)].unwrap(), [range(7)].unwrap()].unwrap()
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = pool.reduce_tree(left, inputs)
    print("__test_restore_allocations__").unwrap()
    drop(result)
    print(pool.reduce_tree(left, range(2)).unwrap()).unwrap()
"#), &format!("__test_fail_allocations_after_{budget}__\n__test_restore_allocations__\nOption[i64].Some(0)\n"));
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn empty_and_singleton_reductions_do_not_allocate() {
    runs(r#"
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
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\nOption[i64].Nothing\nOption[Option[range]].Some(Option[range].Some(range(4, 8, 1)))\n");
}

#[test]
fn parallel_operations_reject_uncertified_workers_and_wrong_signatures() {
    for (source, diagnostic) in [
        ("def work(n: i64) -> i64:\n    n\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    pool.map_result(work, range(3))\n", "map_result worker must return Result"),
        ("def work(n: i64) -> Result[(), str]:\n    Ok(())\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    pool.map_result(work, range(3))\n", "non-unit T"),
        ("def work(a: i64, b: i64) -> str:\n    return \"wrong\"\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    pool.reduce_tree(work, range(3))\n", "return that same type"),
        ("def work(a: i64, b: i64) -> i64:\n    pool = ThreadPoolExecutor(1, 1).unwrap()\n    a + b\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    drop(pool.reduce_tree(work, range(3)))\n", "executor"),
        ("def work(n: i64) -> Result[i64, Future[i64]]:\n    Ok(n)\nwith ThreadPoolExecutor(1, 1).unwrap() as pool:\n    drop(pool.map_result(work, range(3)))\n", "future handles cannot enter worker threads"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "expected {diagnostic}: {error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn earlier_worker_error_takes_precedence_over_later_submission_failure() {
    runs(r#"
def reject(n: i64) -> Result[i64, i64]:
    Err(n)
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    print("__test_fail_allocations_after_4__").unwrap()
    result = pool.map_result(reject, range(8))
    print("__test_restore_allocations__").unwrap()
    print(str.repr(result).unwrap()).unwrap()
"#, "__test_fail_allocations_after_4__\n__test_restore_allocations__\nResult[list[i64], ParallelError[i64]].Err(ParallelError[i64].Worker(0))\n");
}
