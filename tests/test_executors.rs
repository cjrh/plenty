mod support;

fn runs(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("executor-test");
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
fn named_generic_and_owned_closure_jobs_return_typed_results() {
    runs(
        r#"
def identity[T](value: T) -> T:
    value
def square(value: i64) -> i64:
    value * value
def later() -> Result[Future[list[i64]], Failure]:
    with ThreadPoolExecutor(2, 1)? as pool:
        values = [3, 4]?
        job = def once [values]() -> list[i64]:
            values
        return Ok(pool.submit(job)?)
def main() -> Result[(), Failure]:
    with ThreadPoolExecutor(2, 2)? as pool:
        a = pool.submit(square, 6)?
        b = pool.submit(identity, "hello")?
        n = 7
        job = def [n]() -> i64:
            n + 1
        c = pool.submit(job)?
        print(a.result()?)?
        print(b.result()?)?
        print(c.result()?)?
    future = later()?
    print(future.done())?
    print(future.result()?)?
    Ok(())
"#,
        "36\nhello\n8\nTrue\n[3, 4]\n",
    );
}

#[test]
fn map_is_ordered_for_ranges_owned_lists_and_inline_results() {
    runs(r#"
def square[T: IntType](value: T) -> T:
    value * value
def owned(values: list[i64]) -> list[i64]:
    values
def span(n: i64) -> Option[range[i64]]:
    Some(range(n, n + 3))
with ThreadPoolExecutor(3, 1).unwrap() as pool:
    print(pool.map(square, range[u8](8)).unwrap()).unwrap()
    values = [[1, 2].unwrap(), [3].unwrap()].unwrap()
    print(pool.map(owned, values).unwrap()).unwrap()
    print(pool.map(span, range(4)).unwrap()).unwrap()
    print(pool.map(square, range(0)).unwrap()).unwrap()
"#, "[0, 1, 4, 9, 16, 25, 36, 49]\n[[1, 2], [3]]\n[Option[range].Some(range(0, 3, 1)), Option[range].Some(range(1, 4, 1)), Option[range].Some(range(2, 5, 1)), Option[range].Some(range(3, 6, 1))]\n[]\n");
}

#[test]
fn pending_cancel_and_saturation_are_deterministic() {
    runs(
        r#"
def block(ready: Sender[i64], gate: Receiver[i64]) -> i64:
    ready.send(1).unwrap()
    gate.recv().unwrap()
def value() -> i64:
    9
def full[F: OnceCallable[[], i64]](pool: &ThreadPoolExecutor, job: F) -> i64:
    match pool.submit_nowait(job):
        case Ok(future):
            future.result().unwrap()
        case Err(error):
            match error:
                case SubmitError[F].Full(job):
                    job()
                case SubmitError[F].Shutdown(job):
                    -1
                case SubmitError[F].OutOfMemory(job):
                    -2
                case SubmitError[F].CapacityOverflow(job):
                    -3
ready, started = channel[i64](1).unwrap()
signal, gate = channel[i64](1).unwrap()
with ThreadPoolExecutor(1, 1).unwrap() as pool:
    running = pool.submit(block, ready, gate).unwrap()
    started.recv().unwrap()
    print(running.done()).unwrap()
    print(running.cancel()).unwrap()
    pending = pool.submit(value).unwrap()
    n = 42
    fallback = def once [n]() -> i64:
        n
    print(full(pool, fallback)).unwrap()
    print(pending.cancel()).unwrap()
    print(pending.done()).unwrap()
    print(pending.result()).unwrap()
    signal.send(7).unwrap()
    print(running.result().unwrap()).unwrap()
"#,
        "False\nFalse\n42\nTrue\nTrue\nResult[i64, FutureError].Err(FutureError.Cancelled)\n7\n",
    );
}

#[test]
fn mapping_workers_overlap_and_temporary_futures_compose() {
    runs(
        r#"
class Work:
    id: i64
    sender: Sender[i64]
    receiver: Receiver[i64]
def run(work: Work) -> i64:
    if work.id == 0:
        work.receiver.recv().unwrap()
    else:
        work.sender.send(1).unwrap()
    work.id
def square(n: i64) -> i64:
    n * n
def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    inputs = [Work(0, sender.share(), receiver.share()), Work(1, sender, receiver)]?
    with ThreadPoolExecutor(2, 1)? as pool:
        print(pool.map(run, inputs)?)?
        print((pool.submit(square, 9)?).result()?)?
    Ok(())
"#,
        "[0, 1]\n81\n",
    );
}

#[test]
fn nested_inline_executor_errors_support_construction_copy_and_borrowing() {
    runs(
        r#"
def code(error: &PoolError) -> i32:
    match error:
        case PoolError.Thread(problem):
            match problem:
                case ThreadError.System(value):
                    *value
        case PoolError.Allocation(_):
            -1i32
        case PoolError.InvalidSize:
            -2i32
mut error = PoolError.Thread(ThreadError.System(13))
match &mut error:
    case PoolError.Thread(problem):
        match problem:
            case ThreadError.System(value):
                *value = *value + 2i32
    case PoolError.Allocation(_):
        pass
    case PoolError.InvalidSize:
        pass
print(code(&error)).unwrap()
print(copy(error).unwrap() == error).unwrap()
print(error).unwrap()
"#,
        "15\nTrue\nPoolError.Thread(ThreadError.System(15))\n",
    );
}

#[test]
fn unit_jobs_and_closed_map_have_ordinary_error_handling() {
    runs(
        r#"
def nothing() -> ():
    pass
def identity(value: i64) -> i64:
    value
def main() -> Result[(), Failure]:
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(nothing)?
        future.result()?
        pool.shutdown()
        print(pool.map(identity, [1, 2]?))?
    Ok(())
"#,
        "Result[list[i64], PoolMapError].Err(PoolMapError.Shutdown)\n",
    );
}

#[test]
fn handles_are_affine_and_worker_dependencies_are_rejected() {
    for (source, expected) in [
        ("def f() -> i64:\n    1\npool = ThreadPoolExecutor(1, 1).unwrap()\nfuture = pool.submit(f).unwrap()\nfuture.result().unwrap()\nfuture.result().unwrap()\n", "moved"),
        ("pool = ThreadPoolExecutor(1, 1).unwrap()\ncopy(pool).unwrap()\n", "copied"),
        ("n = 1\njob = def [&n]() -> i64:\n    n\npool = ThreadPoolExecutor(1, 1).unwrap()\npool.submit(job).unwrap()\n", "wholly owned"),
        ("def work() -> ():\n    pool = ThreadPoolExecutor(1, 1).unwrap()\n    pass\npool = ThreadPoolExecutor(1, 1).unwrap()\npool.submit(work).unwrap()\n", "cannot enter worker"),
        ("def work() -> ():\n    f = open(\"unused\", \"r\").unwrap()\n    pass\npool = ThreadPoolExecutor(1, 1).unwrap()\npool.submit(work).unwrap()\n", "File"),
        ("def work(future: Future[i64]) -> i64:\n    future.result().unwrap()\ndef one() -> i64:\n    1\npool = ThreadPoolExecutor(1, 1).unwrap()\nf = pool.submit(one).unwrap()\npool.submit(work, f).unwrap()\n", "cannot enter worker"),
        ("def borrowed(f: &Future[i64]) -> &Future[i64]:\n    f\ndef one() -> i64:\n    1\npool = ThreadPoolExecutor(1, 1).unwrap()\nf = pool.submit(one).unwrap()\nborrowed(&f).result().unwrap()\n", "owned Future"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_returns_the_entire_job_for_retry() {
    runs(r#"
def fallback[F: OnceCallable[[], i64]](pool: &ThreadPoolExecutor, job: F) -> i64:
    match pool.submit(job):
        case Ok(future):
            future.result().unwrap()
        case Err(error):
            match error:
                case SubmitError[F].OutOfMemory(job):
                    job()
                case SubmitError[F].CapacityOverflow(job):
                    job()
                case SubmitError[F].Shutdown(job):
                    job()
                case SubmitError[F].Full(job):
                    job()
with ThreadPoolExecutor(1, 1).unwrap() as pool:
    values = [range(20, 24)].unwrap()
    job = def once [values]() -> i64:
        values[0][2]
    print("__test_fail_allocations_after_0__").unwrap()
    answer = fallback(pool, job)
    print("__test_restore_allocations__").unwrap()
    print(answer).unwrap()
    pool.shutdown()
    n = 42
    other = def once [n]() -> i64:
        n
    print("__test_begin_no_allocations__").unwrap()
    second_answer = fallback(pool, other)
    print("__test_end_no_allocations__").unwrap()
    print(second_answer).unwrap()
"#, "__test_fail_allocations_after_0__\n__test_restore_allocations__\n22\n__test_begin_no_allocations__\n__test_end_no_allocations__\n42\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn creation_failures_are_inline_and_partial_native_startup_is_reclaimed() {
    runs(r#"
print(ThreadPoolExecutor(0, 1)).unwrap()
print(ThreadPoolExecutor(1, 0)).unwrap()
print(ThreadPoolExecutor(1, 18446744073709551615u64)).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
failed = ThreadPoolExecutor(2, 2)
print("__test_restore_allocations__").unwrap()
print(failed).unwrap()
print("__test_one_thread_start__").unwrap()
partial = ThreadPoolExecutor(3, 2)
print("__test_restore_thread_starts__").unwrap()
print(partial).unwrap()
"#, "Result[ThreadPoolExecutor, PoolError].Err(PoolError.InvalidSize)\nResult[ThreadPoolExecutor, PoolError].Err(PoolError.InvalidSize)\nResult[ThreadPoolExecutor, PoolError].Err(PoolError.Allocation(AllocError.CapacityOverflow))\n__test_fail_allocations_after_0__\n__test_restore_allocations__\nResult[ThreadPoolExecutor, PoolError].Err(PoolError.Allocation(AllocError.OutOfMemory))\n__test_one_thread_start__\n__test_restore_thread_starts__\nResult[ThreadPoolExecutor, PoolError].Err(PoolError.Thread(ThreadError.System(11)))\n");
}

#[test]
fn future_collections_and_error_results_keep_ownership() {
    runs(r#"
def make(n: i64) -> Result[list[i64], i64]:
    if n == 1:
        return Err(n)
    Ok([n].unwrap())
class Holder:
    future: Future[Result[list[i64], i64]]
    def get(self: &mut Holder) -> bool:
        self.future.done()
with ThreadPoolExecutor(2, 2).unwrap() as pool:
    mut futures: list[Future[Result[list[i64], i64]]] = [].unwrap()
    for n in range(3):
        futures.append(pool.submit(make, n).unwrap()).unwrap()
    for future in futures:
        print(future.result().unwrap()).unwrap()
    print(pool.map(make, range(3)).unwrap()).unwrap()
"#, "Result[list[i64], i64].Ok([0])\nResult[list[i64], i64].Err(1)\nResult[list[i64], i64].Ok([2])\n[Result[list[i64], i64].Ok([0]), Result[list[i64], i64].Err(1), Result[list[i64], i64].Ok([2])]\n");
}

#[test]
fn early_exits_join_and_release_unclaimed_results_exactly_once() {
    runs(
        r#"
class Guard:
    sender: Sender[i64]
    def __del__(self) -> ():
        self.sender.send(1).unwrap()
        pass
def work(guard: Guard) -> Guard:
    guard
def leave(sender: &Sender[i64], mode: i64) -> Result[(), Failure]:
    with ThreadPoolExecutor(2, 1)? as pool:
        pool.submit(work, Guard(sender.share()))?
        if mode == 0:
            return Ok(())
        failed: Result[(), Failure] = Err(Failure.Unspecified)
        failed?
    Ok(())
sender, receiver = channel[i64](8).unwrap()
leave(&sender, 0).unwrap()
drop(leave(&sender, 1))
for n in range(3):
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        pool.submit(work, Guard(sender.share())).unwrap()
        if n == 0:
            continue
        break
drop(sender)
mut count = 0
while True:
    match receiver.recv():
        case Ok(_):
            count = count + 1
        case Err(_):
            break
print(count).unwrap()
"#,
        "4\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn waiting_and_cleanup_after_submission_do_not_allocate() {
    runs(r#"
def work(values: list[range[i64]]) -> list[range[i64]]:
    values
with ThreadPoolExecutor(2, 2).unwrap() as pool:
    future = pool.submit(work, [range(8)].unwrap()).unwrap()
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    values = future.result().unwrap()
    drop(values)
    pool.shutdown()
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn map_failure_at_each_allocation_reclaims_inputs_jobs_and_partial_results() {
    for budget in 0..10 {
        let source = format!(
            r#"
def work(values: list[range[i64]]) -> list[range[i64]]:
    values
with ThreadPoolExecutor(2, 1).unwrap() as pool:
    inputs = [[range(3)].unwrap(), [range(4)].unwrap(), [range(5)].unwrap(), [range(6)].unwrap()].unwrap()
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = pool.map(work, inputs)
    print("__test_restore_allocations__").unwrap()
    drop(result)
"#
        );
        runs(
            &source,
            &format!("__test_fail_allocations_after_{budget}__\n__test_restore_allocations__\n"),
        );
    }
}
