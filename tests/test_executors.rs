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
def later() -> Result[Future[list[i64]], AllocError | PoolError | SubmitError[OnceClosure[[], list[i64]]]]:
    with ThreadPoolExecutor(2, 1)? as pool:
        values = [3, 4]?
        job = def once [values]() -> list[i64]:
            values
        return Ok(pool.submit(job)?)
def main() -> Result[(), AllocError | FutureError | IoError | PoolError | SubmitError[Closure[[], i64]] | SubmitError[OnceClosure[[], i64]] | SubmitError[OnceClosure[[], list[i64]]] | SubmitError[OnceClosure[[], str]]]:
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
                case SubmitError.Full(job):
                    job()
                case SubmitError.Shutdown(job):
                    -1
                case SubmitError.OutOfMemory(job):
                    -2
                case SubmitError.CapacityOverflow(job):
                    -3
ready, started = channel[i64](1).unwrap()
signal, gate = channel[i64](1).unwrap()
with ThreadPoolExecutor(1, 1).unwrap() as pool:
    running = pool.submit(block, ready, gate).unwrap()
    started.recv().unwrap()
    drop(started)
    print(running.done()).unwrap()
    print(running.cancel()).unwrap()
    pending = pool.submit(value).unwrap()
    n = 42
    fallback = def once [n]() -> i64:
        n
    print(full(pool, fallback)).unwrap()
    print(pending.cancel()).unwrap()
    print(pending.done()).unwrap()
    print(str.repr(pending.result()).unwrap()).unwrap()
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
def main() -> Result[(), AllocError | ChannelError | FutureError | IoError | PoolError | PoolMapError | SubmitError[OnceClosure[[], i64]]]:
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
def main() -> Result[(), AllocError | FutureError | IoError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(nothing)?
        future.result()?
        pool.shutdown()
        print(str.repr(pool.map(identity, [1, 2]?)).unwrap())?
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
        ("def seven() -> i64:\n    7\ndef work() -> ():\n    f: Callable[[], i64] = seven\n    f()\n    pass\npool = ThreadPoolExecutor(1, 1).unwrap()\npool.submit(work).unwrap()\n", "indirect callable"),
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
                case SubmitError.OutOfMemory(job):
                    job()
                case SubmitError.CapacityOverflow(job):
                    job()
                case SubmitError.Shutdown(job):
                    job()
                case SubmitError.Full(job):
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
print(str.repr(ThreadPoolExecutor(0, 1)).unwrap()).unwrap()
print(str.repr(ThreadPoolExecutor(1, 0)).unwrap()).unwrap()
print(str.repr(ThreadPoolExecutor(1, 18446744073709551615u64)).unwrap()).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
failed = ThreadPoolExecutor(2, 2)
print("__test_restore_allocations__").unwrap()
print(str.repr(failed).unwrap()).unwrap()
print("__test_one_thread_start__").unwrap()
partial = ThreadPoolExecutor(3, 2)
print("__test_restore_thread_starts__").unwrap()
print(str.repr(partial).unwrap()).unwrap()
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
        print(str.repr(future.result().unwrap()).unwrap()).unwrap()
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
enum Exit:
    Requested
def leave(sender: &Sender[i64], mode: i64) -> Result[(), Exit | PoolError | SubmitError[OnceClosure[[], Guard]]]:
    with ThreadPoolExecutor(2, 1)? as pool:
        pool.submit(work, Guard(sender.share()))?
        if mode == 0:
            return Ok(())
        failed: Result[(), Exit] = Err(Exit.Requested)
        failed?
    Ok(())
def count(receiver: Receiver[i64]) -> i64:
    mut count = 0
    while True:
        match receiver.recv():
            case Ok(_):
                count = count + 1
            case Err(_):
                break
    count
sender, receiver = channel[i64](8).unwrap()
with spawn(count, receiver).unwrap() as counter:
    leave(&sender, 0).unwrap()
    drop(leave(&sender, 1))
    for n in range(3):
        with ThreadPoolExecutor(1, 1).unwrap() as pool:
            pool.submit(work, Guard(sender.share())).unwrap()
            if n == 0:
                continue
            break
    drop(sender)
    print(counter.join()).unwrap()
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

/// A job that sends until the queue disconnects; `main` starts on line 8.
const PRODUCE: &str = "def produce(sender: Sender[i64]) -> ():
    for n in range(10000):
        match sender.send(n):
            case Ok(_):
                pass
            case Err(_):
                return
";

/// The diagnostic for a shutdown that a surviving receiver could block forever.
fn blocked_shutdown(body: &str) -> String {
    let error = support::check_source(&format!("{PRODUCE}{body}"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("could block the job's send"), "{error}");
    error
}

fn accepted(body: &str) {
    support::check_source(&format!("{PRODUCE}{body}")).unwrap_or_else(|e| panic!("{body}\n{e}"));
}

#[test]
fn future_result_rejects_a_live_receiver_at_the_wait() {
    for (binding, wait) in [
        ("", "future.result()"),
        ("        moved = future\n", "moved.result()"),
    ] {
        let source = format!(
            "{PRODUCE}def main() -> Result[(), ChannelError | FutureError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
{binding}        {wait}?
        drop(receiver)
    Ok(())
"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        let line = if binding.is_empty() { "12:" } else { "13:" };
        assert!(error.starts_with(line), "{error}");
        assert!(error.contains("this result() waits for a job"), "{error}");
        assert!(
            error.contains("`receiver` still holds `Receiver[i64]`"),
            "{error}"
        );
        assert!(error.contains("the job is submitted here"), "{error}");
        assert!(error.contains("before waiting for the result"), "{error}");
    }
}

#[test]
fn future_result_tracks_unwrapped_and_matched_submission_results() {
    for binding in [
        "pool.submit(produce, sender).unwrap().result().unwrap()",
        "future = pool.submit(produce, sender).unwrap()\n    future.result().unwrap()",
        "submitted = pool.submit(produce, sender)\n    future = submitted.unwrap()\n    future.result().unwrap()",
        "match pool.submit(produce, sender):\n        case Ok(future):\n            future.result().unwrap()\n        case Err(error):\n            drop(error)",
    ] {
        let source = format!(
            "{PRODUCE}def main() -> ():
    pool = ThreadPoolExecutor(1, 1).unwrap()
    sender, receiver = channel[i64](1).unwrap()
    {binding}
    drop(receiver)
"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("this result() waits for a job"), "{error}");
    }
}

#[test]
fn a_temporary_future_wait_is_checked_after_try() {
    let error = support::check_source(&format!(
        "{PRODUCE}def main() -> Result[(), ChannelError | FutureError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        sender, receiver = channel[i64](1)?
        (pool.submit(produce, sender)?).result()?
        drop(receiver)
    Ok(())
"
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains("this result() waits for a job"), "{error}");
}

#[test]
fn future_waits_keep_distinct_jobs_and_message_types_separate() {
    // The three submissions bind different functions returning `()`; one
    // job-free `SubmitError[()]` member covers all of them.
    accepted(
        "def noop() -> ():
    pass
def words(sender: Sender[str]) -> ():
    drop(sender)
def main() -> Result[(), ChannelError | FutureError | PoolError | SubmitError[()]]:
    with ThreadPoolExecutor(3, 3)? as pool:
        sender, receiver = channel[i64](1)?
        texts, lines = channel[str](1)?
        blocked = pool.submit(produce, sender).drop_job()?
        other = pool.submit(words, texts).drop_job()?
        plain = pool.submit(noop).drop_job()?
        plain.result()?
        drop(lines)
        other.result()?
        drop(receiver)
        blocked.result()?
    Ok(())
",
    );
}

#[test]
fn future_result_accepts_dropped_or_ended_receiver_guards_and_early_exits() {
    for release in [
        "drop(receiver)",
        "with receiver as inbox:\n            pass",
        "if leave:\n            drop(receiver)\n        else:\n            return Ok(())",
    ] {
        accepted(&format!(
            "def main() -> Result[(), ChannelError | FutureError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    leave = True
    with ThreadPoolExecutor(1, 1)? as pool:
        sender, receiver = channel[i64](1)?
        future = pool.submit(produce, sender)?
        {release}
        future.result()?
    Ok(())
"
        ));
    }
}

#[test]
fn timed_and_nonblocking_future_operations_allow_live_receivers() {
    runs(
        &format!(
            "{PRODUCE}def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        sender, receiver = channel[i64](1)?
        future = pool.submit(produce, sender)?
        future.done()
        future.wait_timeout(1)
        future.cancel()
        drop(receiver)
        match future.result():
            case Ok(_):
                pass
            case Err(_):
                pass
    Ok(())
"
        ),
        "",
    );
}

#[test]
fn receiver_guards_and_partial_drops_do_not_discharge_future_waits() {
    for before in [
        "with receiver as inbox:\n            future.result().unwrap()",
        "if flag:\n            drop(receiver)\n        future.result().unwrap()",
    ] {
        let error = support::check_source(&format!(
            "{PRODUCE}def main() -> ():
    flag = True
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        sender, receiver = channel[i64](1).unwrap()
        future = pool.submit(produce, sender).unwrap()
        {before}
"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("this result() waits for a job"), "{error}");
    }
}

#[test]
fn result_errors_remain_recoverable_after_receiver_cleanup() {
    runs(
        r#"
def fail(sender: Sender[str]) -> Result[(), i64]:
    drop(sender)
    Err(42)
def main() -> Result[(), ChannelError | FutureError | IoError | PoolError | SubmitError[OnceClosure[[], Result[(), i64]]]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        sender, receiver = channel[str](1)?
        future = pool.submit(fail, sender)?
        drop(receiver)
        match future.result()?:
            case Ok(_):
                pass
            case Err(error):
                print(error)?
    Ok(())
"#,
        "42\n",
    );
}

#[test]
fn receiver_outside_the_pool_block_cannot_reach_the_shutdown() {
    // Issue #74: without the check this program hangs when the pool shuts down.
    let error = blocked_shutdown(
        "def main() -> Result[(), ChannelError | IoError | PoolError | RecvError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(def once [sender]() -> (): produce(sender))?
        print(receiver.recv()?)?
    Ok(())
",
    );
    assert_eq!(
        error,
        "10:5: leaving its scope shuts down this pool after a job that holds `Sender[i64]` was \
         submitted, while `receiver` still holds `Receiver[i64]`: a full queue could block the \
         job's send, and the shutdown, forever\n  \
         9:5: note: `receiver` holds `Receiver[i64]`; drop it before the pool shuts down, or \
         guard it directly after the pool: `with ThreadPoolExecutor(...)? as pool, receiver as \
         inbox:`\n  \
         11:30: note: the job is submitted here\n  \
         12:15: note: `?` leaves the pool's scope here"
    );
}

#[test]
fn failed_job_start_cannot_keep_its_receiver_alive_through_the_shutdown() {
    // Issue #344: the `SpawnError` keeps the consumer and its receiver until
    // the function returns, after the pool drains the producer.
    let error = blocked_shutdown(
        "def main() -> Result[(), ChannelError | PoolError | SpawnError[OnceClosure[[], Result[i64, RecvError]]] | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    consumer = def once [receiver]() -> Result[i64, RecvError]:
        receiver.recv()
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        with spawn(consumer)?:
            pass
    Ok(())
",
    );
    assert!(
        error.contains(
            "while the error that `?` propagates still holds `Receiver[i64]`: a full queue could \
             block the job's send, and the shutdown, forever"
        ),
        "{error}"
    );
    assert!(
        error.contains(
            "14:14: note: the error that `?` propagates here holds `Receiver[i64]` until this \
             function returns"
        ),
        "{error}"
    );
}

#[test]
fn receiver_guard_order_decides_whether_it_outlives_the_shutdown() {
    runs(
        &format!(
            "{PRODUCE}def main() -> Result[(), ChannelError | IoError | PoolError | RecvError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool, receiver as inbox:
        future = pool.submit(produce, sender)?
        print(inbox.recv()?)?
    print(\"done\")?
    Ok(())
"
        ),
        "0\ndone\n",
    );
    // Contexts exit in reverse order: this guard outlives the pool.
    let error = blocked_shutdown(
        "def main() -> Result[(), ChannelError | IoError | PoolError | RecvError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    with receiver as inbox, ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        print(inbox.recv()?)?
    Ok(())
",
    );
    assert!(
        error.contains("note: `receiver` holds `Receiver[i64]`"),
        "{error}"
    );
    // A guard inside the pool's block ends before the pool does.
    runs(
        &format!(
            "{PRODUCE}def main() -> Result[(), ChannelError | IoError | PoolError | RecvError | SubmitError[OnceClosure[[], ()]]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        sender, receiver = channel[i64](1)?
        with receiver as inbox:
            future = pool.submit(produce, sender)?
            print(inbox.recv()?)?
    print(\"done\")?
    Ok(())
"
        ),
        "0\ndone\n",
    );
}

#[test]
fn every_way_a_pool_shuts_down_is_checked_for_surviving_receivers() {
    // The end of a block names no exit.
    let error = blocked_shutdown(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(produce, sender).unwrap()
",
    );
    assert!(
        error.starts_with("10:10: leaving its scope shuts down this pool"),
        "{error}"
    );
    assert!(
        !error.contains("leaves") && !error.contains("returns"),
        "{error}"
    );

    // Locals are released last declared first, so this pool outlives nothing.
    let error = blocked_shutdown(
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    pool = ThreadPoolExecutor(1, 1).unwrap()
    future = pool.submit(produce, sender).unwrap()
",
    );
    assert!(
        error.starts_with("10:5: leaving its scope shuts down this pool"),
        "{error}"
    );

    for (exit, note) in [
        ("return Ok(())", "14:17: note: the function returns here"),
        ("break", "14:17: note: `break` leaves the pool's scope here"),
        (
            "continue",
            "14:17: note: `continue` leaves the pool's scope here",
        ),
        ("print(n)?", "14:23: note: `?` leaves the pool's scope here"),
    ] {
        let error = blocked_shutdown(&format!(
            "def main() -> Result[(), ChannelError | IoError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    for n in range(2):
        with ThreadPoolExecutor(1, 1)? as pool:
            future = pool.submit(produce, sender.share())?
            if n == 0:
                {exit}
    drop(receiver)
    Ok(())
"
        ));
        assert!(
            error.starts_with("11:9: leaving its scope shuts down this pool"),
            "{error}"
        );
        assert!(error.contains(note), "{error}");
    }

    for (shutdown, at) in [
        ("pool.shutdown()", "13:5"),
        ("pool.shutdown(True)", "13:5"),
        ("drop(pool)", "13:10"),
        ("pool = ThreadPoolExecutor(1, 1)?", "13:5"),
    ] {
        let error = blocked_shutdown(&format!(
            "def main() -> Result[(), ChannelError | IoError | PoolError | RecvError | SubmitError[OnceClosure[[], ()]]]:
    mut pool = ThreadPoolExecutor(1, 1)?
    sender, receiver = channel[i64](1)?
    future = pool.submit(produce, sender)?
    print(receiver.recv()?)?
    {shutdown}
    drop(receiver)
    Ok(())
"
        ));
        assert_eq!(
            error,
            format!(
                "{at}: this shuts down a pool after a job that holds `Sender[i64]` was \
                 submitted, while `receiver` still holds `Receiver[i64]`: a full queue could \
                 block the job's send, and the shutdown, forever\n  \
                 10:5: note: `receiver` holds `Receiver[i64]`; drop it, or end its `with` \
                 block, before shutting the pool down\n  \
                 11:35: note: the job is submitted here"
            )
        );
    }

    // A temporary pool shuts down at the end of its statement.
    let error = blocked_shutdown(
        "def main() -> Result[(), ChannelError | IoError | PoolError | RecvError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    future = ThreadPoolExecutor(1, 1)?.submit(produce, sender)?
    print(receiver.recv()?)?
    Ok(())
",
    );
    assert!(
        error.starts_with("10:5: leaving its scope shuts down this pool"),
        "{error}"
    );
}

#[test]
fn shutdown_of_a_borrowed_pool_counts_receivers_the_caller_keeps() {
    let error = blocked_shutdown(
        "def run(pool: &ThreadPoolExecutor, inbox: &Receiver[i64], sender: Sender[i64]) -> Result[(), SubmitError[OnceClosure[[], ()]]]:
    future = pool.submit(produce, sender)?
    pool.shutdown()
    Ok(())
",
    );
    assert!(
        error.contains(
            "note: `inbox` holds `Receiver[i64]`; its owner outlives this call, so take the \
             receiver by value"
        ),
        "{error}"
    );
}

#[test]
fn a_job_submitted_in_one_iteration_is_seen_by_a_shutdown_in_the_next() {
    let error = blocked_shutdown(
        "def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    mut pool = ThreadPoolExecutor(1, 1)?
    for n in range(3):
        pool.shutdown()
        pool = ThreadPoolExecutor(1, 1)?
        future = pool.submit(produce, sender.share())?
    drop(receiver)
    Ok(())
",
    );
    assert!(error.starts_with("12:9: this shuts down a pool"), "{error}");
    assert!(
        error.contains("14:39: note: the job is submitted here"),
        "{error}"
    );
}

#[test]
fn shutdowns_without_a_matching_live_receiver_are_accepted() {
    for body in [
        // The pool is declared first, so the receiver is released before it.
        "def main() -> Result[(), ChannelError | IoError | PoolError | RecvError | SubmitError[OnceClosure[[], ()]]]:
    pool = ThreadPoolExecutor(1, 1)?
    sender, receiver = channel[i64](1)?
    future = pool.submit(produce, sender)?
    print(receiver.recv()?)?
    Ok(())
",
        // The receiver is dropped on every path that reaches the shutdown.
        "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(produce, sender).unwrap()
        drop(receiver)
",
        // A `submit(...)?` that fails has not moved its job into the pool.
        "def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        sender, receiver = channel[i64](1)?
        future = pool.submit(produce, sender)?
        drop(receiver)
    Ok(())
",
        // The job is submitted after the earlier pool has shut down.
        "def one() -> i64:
    1
def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]] | SubmitError[OnceClosure[[], i64]]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as setup:
        ready = setup.submit(one)?
    with ThreadPoolExecutor(1, 1)? as pool, receiver as inbox:
        future = pool.submit(produce, sender)?
    Ok(())
",
        // The live receiver carries a different message type.
        "def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    texts, lines = channel[str](1)?
    with ThreadPoolExecutor(1, 1)? as pool, receiver as inbox:
        future = pool.submit(produce, sender)?
    drop(texts)
    Ok(())
",
        // The job holds the receiver and the parent the sender.
        "def consume(inbox: Receiver[i64]) -> ():
    pass
def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(consume, receiver)?
        pool.shutdown()
    drop(sender)
    Ok(())
",
    ] {
        accepted(body);
    }
}
