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
fn future_result_tracks_drop_job_submission_results() {
    // Issue #348: `drop_job()` keeps the submission's job on the future.
    for (body, holder) in [
        (
            "future = pool.submit(produce, sender).drop_job()?\n        future.result()?",
            "`receiver`",
        ),
        (
            "future = pool.submit(produce, sender).drop_job().unwrap()\n        future.result()?",
            "`receiver`",
        ),
        (
            "(pool.submit(produce, sender).drop_job()?).result()?",
            "`receiver`",
        ),
        (
            "match pool.submit(produce, sender).drop_job():\n            case Ok(future):\n                future.result()?\n            case Err(error):\n                drop(error)",
            "`receiver`",
        ),
        (
            "match pool.submit(produce, sender).drop_job():\n            case Ok(future):\n                kept = (receiver, future.result()?)\n                drop(kept)\n                return Ok(())\n            case Err(error):\n                drop(error)",
            "the expression being evaluated",
        ),
    ] {
        let source = format!(
            "{PRODUCE}def main() -> Result[(), ChannelError | FutureError | PoolError | SubmitError[()]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        {body}
        drop(receiver)
    Ok(())
"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains(&format!(
                "this result() waits for a job that holds `Sender[i64]` while {holder} \
                 still holds `Receiver[i64]`"
            )) && error.contains("the job is submitted here"),
            "{body}\n{error}"
        );
    }
}

#[test]
fn a_drop_job_future_waits_after_the_receiver_is_dropped() {
    runs(
        &format!(
            "{PRODUCE}def main() -> Result[(), AllocError | IoError | ChannelError | FutureError | PoolError | SubmitError[()]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender).drop_job()?
        drop(receiver)
        future.result()?
    print(\"done\")?
    Ok(())
"
        ),
        "done\n",
    );
}

#[test]
fn a_successful_drop_job_submission_is_checked_at_shutdown() {
    blocked_shutdown(
        "def main() -> Result[(), ChannelError | PoolError | SubmitError[()]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        drop(pool.submit(produce, sender).drop_job()?)
    drop(receiver)
    Ok(())
",
    );
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
fn returned_receiver_cannot_outlive_the_shutdown_it_blocks() {
    // Issue #346: the returned value keeps the receiver until the function
    // returns, after its pools have drained the producer.
    let start = "def keep(receiver: Receiver[i64]) -> Receiver[i64]:
    receiver
def start(sender: Sender[i64], receiver: Receiver[i64]) -> Result[Receiver[i64], PoolError | SubmitError[OnceClosure[[], ()]]]:
";
    let main = "def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    drop(start(sender, receiver))
";
    for (body, note) in [
        (
            "    with ThreadPoolExecutor(1, 1)? as pool:\n        future = pool.submit(produce, sender)?\n        return Ok(receiver)\n",
            "13:9: note: the value returned here holds `Receiver[i64]`, moved from `receiver`, \
             until this function returns; drop the endpoint before the pool's scope ends instead \
             of returning it",
        ),
        // A pool binding shuts down as the frame is released.
        (
            "    pool = ThreadPoolExecutor(1, 1)?\n    future = pool.submit(produce, sender)?\n    return Ok(receiver)\n",
            "13:5: note: the value returned here",
        ),
        // So does the function's final expression.
        (
            "    pool = ThreadPoolExecutor(1, 1)?\n    future = pool.submit(produce, sender)?\n    Ok(receiver)\n",
            "13:5: note: the value returned here",
        ),
        // A returned call's arguments are in flight through the release too.
        (
            "    pool = ThreadPoolExecutor(1, 1)?\n    future = pool.submit(produce, sender)?\n    return Ok(keep(receiver))\n",
            "13:5: note: the value returned here",
        ),
    ] {
        let error = blocked_shutdown(&format!("{start}{body}{main}"));
        assert!(
            error.contains(
                "while the returned value still holds `Receiver[i64]`: a full queue could block \
                 the job's send, and the shutdown, forever"
            ) && error.contains(note),
            "{body}\n{error}"
        );
    }
    // A wait inside the returned expression sees the operands moved before it.
    let error = support::check_source(&format!(
        "{PRODUCE}def start(sender: Sender[i64], receiver: Receiver[i64]) -> Result[(Receiver[i64], ()), FutureError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        return Ok((receiver, future.result()?))
{main}"
    ))
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("this result() waits for a job that holds `Sender[i64]` while the returned value still holds `Receiver[i64]`")
            && error.contains("wait before moving the endpoint into the returned value"),
        "{error}"
    );
    accepted(&format!(
        "{start}    pool = ThreadPoolExecutor(1, 1)?\n    future = pool.submit(produce, sender)?\n    drop(receiver)\n    drop(pool)\n    unused, other = channel[i64](1).unwrap()\n    drop(unused)\n    Ok(other)\n{main}"
    ));
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

/// `main` for issue #347 probes: `sender` goes to the job, `receiver` stays.
fn building_main(body: &str) -> String {
    format!(
        "def keep(receiver: Receiver[i64], done: bool) -> bool:
    drop(receiver)
    done
def finished(result: Result[(), FutureError]) -> bool:
    result.is_ok()
def count(sender: Sender[i64]) -> i64:
    drop(sender)
    1
def main() -> Result[(), AllocError | ChannelError | FutureError | IoError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
{body}    print(\"done\")?
    Ok(())
"
    )
}

#[test]
fn a_wait_inside_a_statement_rejects_endpoints_its_value_already_holds() {
    // Issue #347: the tuple, or the call's arguments, hold the receiver from
    // its move until the statement completes, across the nested wait.
    let pool = "    with ThreadPoolExecutor(1, 1)? as pool:\n        future = pool.submit(produce, sender)?\n";
    for (statement, at) in [
        (
            "        pair = (receiver, future.result()?)\n        inbox, _ = pair\n        drop(inbox)\n",
            "20:16",
        ),
        ("        drop((receiver, future.result()?))\n", "20:9"),
        (
            "        if keep(receiver, finished(future.result())):\n            pass\n",
            "20:12",
        ),
        (
            "        match keep(receiver, finished(future.result())):\n            case True:\n                pass\n            case False:\n                pass\n",
            "20:15",
        ),
        (
            "        for done in [keep(receiver, finished(future.result()))]?:\n            pass\n",
            "20:64",
        ),
    ] {
        let error = blocked_shutdown(&building_main(&format!("{pool}{statement}")));
        assert!(
            error.contains(
                "this result() waits for a job that holds `Sender[i64]` while the expression \
                 being evaluated still holds `Receiver[i64]`"
            ) && error.contains(&format!(
                "{at}: note: the expression here holds `Receiver[i64]`, moved from \
                 `receiver`, until the statement completes; wait in an earlier statement, \
                 before moving the endpoint"
            )),
            "{statement}\n{error}"
        );
    }
}

/// `main` for issue #350 probes: `index` evaluates a lock's computed index.
fn lock_index_main(index: &str) -> String {
    format!(
        "from std.sync import Mutex
def keep(receiver: Receiver[i64], done: bool) -> i64:
    drop(receiver)
    0
def slot(done: bool) -> i64:
    0
def main() -> Result[(), AllocError | ChannelError | IoError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    cells = [Mutex[i64](0)]?
    flags = [Mutex[bool](False)]?
    shared = [Shared(Mutex[i64](0))?]?
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
{index}            *cell = 1
            print(*cell)?
    print(\"done\")?
    Ok(())
"
    )
}

#[test]
fn a_wait_inside_a_lock_place_rejects_endpoints_its_index_already_holds() {
    // Issue #350: the computed index holds the receiver from its move through
    // the nested wait, as any other `with` manager does.
    for (statement, at) in [
        (
            "        with cells[keep(receiver, future.result().is_ok())].lock() as cell:\n",
            "21:14",
        ),
        (
            "        with flags[0].lock() as flag, cells[keep(receiver, future.result().is_ok())].lock() as cell:\n",
            "21:39",
        ),
        (
            "        with shared[keep(receiver, future.result().is_ok())].lock() as cell:\n",
            "21:14",
        ),
    ] {
        let error = blocked_shutdown(&lock_index_main(statement));
        assert!(
            error.contains(
                "this result() waits for a job that holds `Sender[i64]` while the expression \
                 being evaluated still holds `Receiver[i64]`"
            ) && error.contains(&format!(
                "{at}: note: the expression here holds `Receiver[i64]`, moved from \
                 `receiver`, until the statement completes"
            )),
            "{statement}\n{error}"
        );
    }
    runs(
        &format!(
            "{PRODUCE}{}",
            lock_index_main(
                "        drop(receiver)\n        with cells[slot(future.result().is_ok())].lock() as cell:\n"
            )
        ),
        "1\ndone\n",
    );
}

#[test]
fn a_match_case_value_holds_its_endpoints_through_the_cases_shutdown() {
    // Issue #347: the case's pool shuts down after its final expression has
    // moved the receiver into the `match` value, before the binding stores it.
    let error = blocked_shutdown(&building_main(
        "    kept = match True:
        case True:
            pool = ThreadPoolExecutor(1, 1)?
            drop(pool.submit(produce, sender)?)
            receiver
        case False:
            drop(sender)
            receiver
    drop(kept)
",
    ));
    assert!(
        error.contains(
            "shuts down this pool after a job that holds `Sender[i64]` was submitted, while the \
             expression being evaluated still holds `Receiver[i64]`"
        ) && error.contains(
            "22:13: note: the expression here holds `Receiver[i64]`, moved from `receiver`, \
             until the statement completes; use the endpoint outside the pool's scope"
        ),
        "{error}"
    );
}

#[test]
fn statements_that_wait_before_moving_an_endpoint_are_accepted() {
    // Issue #347: a value that takes the endpoint after the wait, or only an
    // endpoint the job cannot block on, does not hold one through the wait.
    runs(
        &format!(
            "{PRODUCE}{}",
            building_main(
                "    drop(receiver)
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        pair = (future.result()?, 1)
        print(pair[1])?
",
            )
        ),
        "1\ndone\n",
    );
    runs(
        &format!(
            "{PRODUCE}{}",
            building_main(
                "    other, inbox = channel[str](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        drop(receiver)
        pair = (inbox, future.result()?)
        kept, _ = pair
        drop(kept)
        drop(other)
    kept = match True:
        case True:
            spare, unused = channel[i64](1)?
            drop(unused)
            spare
        case False:
            spare, unused = channel[i64](1)?
            drop(unused)
            spare
    print(count(kept))?
",
            )
        ),
        "1\ndone\n",
    );
}

/// Helpers that wait for a future's result or shut a pool down, reached by
/// direct, transitive, recursive, and indirect calls (issue #349).
const HELPERS: &str = "def finished(future: Future[()]) -> bool:
    future.result().is_ok()

def through(future: Future[()]) -> bool:
    finished(future)

def countdown(future: Future[()], n: i64) -> bool:
    if n == 0:
        return future.result().is_ok()
    countdown(future, n - 1)

def apply(wait: Callable[[Future[()]], bool], future: Future[()]) -> bool:
    wait(future)

def ignore(future: Future[()]) -> bool:
    drop(future)
    True

def keep(receiver: Receiver[i64], done: bool) -> i64:
    drop(receiver)
    0

def stop(pool: ThreadPoolExecutor) -> ():
    drop(pool)

def shut(pool: &ThreadPoolExecutor) -> ():
    pool.shutdown()

";

fn with_helpers(main: &str) -> String {
    format!("from std.concurrent import Future\n{PRODUCE}{HELPERS}{main}")
}

#[test]
fn calls_that_wait_for_a_passed_future_check_the_callers_receivers() {
    for (call, callee) in [
        ("finished(future)", "finished"),
        ("through(future)", "through"),
        ("countdown(future, 3)", "countdown"),
        // A call through a value is assumed to wait.
        ("apply(finished, future)", "apply"),
    ] {
        let error = support::check_source(&with_helpers(&format!(
            "def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        ok = {call}
        drop(receiver)
    Ok(())
"
        )))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains(&format!(
                "this call to `{callee}` can wait for the result of a job that holds \
                 `Sender[i64]` while `receiver` still holds `Receiver[i64]`"
            )),
            "{error}"
        );
        assert!(error.contains("before the call"), "{error}");
        assert!(error.contains("the job is submitted here"), "{error}");
    }
}

#[test]
fn a_call_that_waits_while_its_statement_holds_a_receiver_is_rejected() {
    let error = support::check_source(&with_helpers(
        "def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        index = keep(receiver, finished(future))
    Ok(())
",
    ))
    .unwrap_err()
    .to_string();
    assert!(
        error.contains(
            "this call to `finished` can wait for the result of a job that holds `Sender[i64]` \
             while the expression being evaluated still holds `Receiver[i64]`"
        ),
        "{error}"
    );
}

#[test]
fn calls_that_shut_down_a_passed_pool_check_the_callers_receivers() {
    for (call, callee) in [("stop(pool)", "stop"), ("shut(&pool)", "shut")] {
        let error = support::check_source(&with_helpers(&format!(
            "def main() -> Result[(), ChannelError | PoolError]:
    sender, receiver = channel[i64](1)?
    pool = ThreadPoolExecutor(1, 1)?
    drop(pool.submit(produce, sender).drop_job())
    {call}
    drop(receiver)
    Ok(())
"
        )))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains(&format!(
                "this call to `{callee}` can shut down a pool after a job that holds \
                 `Sender[i64]` was submitted, while `receiver` still holds `Receiver[i64]`"
            )),
            "{error}"
        );
    }
}

#[test]
fn calls_after_the_receiver_drops_or_that_do_not_wait_run() {
    for body in [
        "    drop(receiver)
    ok = finished(future)
    drop(other)",
        "    ok = ignore(future)
    drop(receiver)
    drop(other)",
        "    drop(receiver)
    stop(pool)
    drop(other)",
    ] {
        let source = with_helpers(&format!(
            "def main() -> Result[(), AllocError | IoError | ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    unrelated, other = channel[str](1)?
    pool = ThreadPoolExecutor(1, 1)?
    future = pool.submit(produce, sender)?
{body}
    drop(unrelated)
    print(\"done\")?
    Ok(())
"
        ));
        runs(&source, "done\n");
    }
}

/// Helpers that return a future they are handed, or one they submit
/// themselves (issue #352).
const RETURNING: &str =
    "def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(_):
            Err(\"submission failed\")

def same(future: Future[()]) -> Future[()]:
    future

def wrapped(future: Future[()]) -> Option[Future[()]]:
    Some(future)

def again[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    submitted(result)

def start(pool: &ThreadPoolExecutor, sender: Sender[i64]) -> Result[Future[()], str]:
    submitted(pool.submit(produce, sender))

def finished(future: Future[()]) -> bool:
    future.result().is_ok()

def keep(receiver: Receiver[i64], done: bool) -> bool:
    drop(receiver)
    done

";

/// `main` for issue #352 probes: `sender` goes to the job, `receiver` stays,
/// and `other` stays live without a matching sender in any job.
fn returning_main(body: &str) -> String {
    format!(
        "from std.concurrent import Future
{PRODUCE}{RETURNING}def main() -> Result[(), AllocError | IoError | ChannelError | PoolError | str]:
    with ThreadPoolExecutor(1, 1)? as pool:
        sender, receiver = channel[i64](1)?
        unrelated, other = channel[str](1)?
{body}        drop(unrelated)
        drop(other)
    print(\"done\")?
    Ok(())
"
    )
}

#[test]
fn a_future_returned_by_a_helper_keeps_its_job() {
    let waits = "this result() waits for a job that holds `Sender[i64]` \
                 while `receiver` still holds `Receiver[i64]`";
    for (statement, expected) in [
        ("future = submitted(pool.submit(produce, sender))?", waits),
        (
            "submission = pool.submit(produce, sender)\n        future = submitted(submission)?",
            waits,
        ),
        (
            "future = same(pool.submit(produce, sender).unwrap())",
            waits,
        ),
        (
            "future = wrapped(pool.submit(produce, sender).unwrap()).unwrap()",
            waits,
        ),
        ("future = again(pool.submit(produce, sender))?", waits),
        (
            "future = same(same(pool.submit(produce, sender).unwrap()))",
            waits,
        ),
        // The callee submits the job itself.
        ("future = start(&pool, sender)?", waits),
    ] {
        let source = returning_main(&format!(
            "        {statement}\n        ok = future.result().is_ok()\n        drop(receiver)\n"
        ));
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{statement}\n{error}");
        assert!(
            error.contains("the job is submitted here"),
            "{statement}\n{error}"
        );
    }
}

#[test]
fn a_returned_future_is_checked_where_a_call_or_statement_waits_for_it() {
    for (statement, expected) in [
        // A call handed the returned future (issue #349).
        (
            "future = submitted(pool.submit(produce, sender))?\n        ok = finished(future)",
            "this call to `finished` can wait for the result of a job that holds `Sender[i64]` \
             while `receiver` still holds `Receiver[i64]`",
        ),
        // The statement holds `receiver` across the wait (issue #347).
        (
            "ok = keep(receiver, submitted(pool.submit(produce, sender))?.result().is_ok())",
            "this result() waits for a job that holds `Sender[i64]` \
             while the expression being evaluated still holds `Receiver[i64]`",
        ),
    ] {
        let source = returning_main(&format!("        {statement}\n"));
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{statement}\n{error}");
    }
}

#[test]
fn futures_returned_by_helpers_run_once_the_receiver_drops() {
    for statement in [
        "drop(receiver)\n        future = submitted(pool.submit(produce, sender))?\n        ok = future.result().is_ok()",
        "future = start(&pool, sender)?\n        drop(receiver)\n        ok = same(future).result().is_ok()",
        "future = again(pool.submit(produce, sender))?\n        drop(receiver)\n        ok = finished(future)",
    ] {
        runs(&returning_main(&format!("        {statement}\n")), "done\n");
    }
}
