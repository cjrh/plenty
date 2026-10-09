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
        print(pool.map_result(square, range(0)))?
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
        inputs = [Input(n, signal.share(), wait.share(), dropped.share())? for n in range(8)]?
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
