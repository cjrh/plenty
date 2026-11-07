//! `Submitter`: nonblocking submission from worker threads (issue #789).
mod support;

fn runs(source: &str, expected: &str) {
    let output = support::assert_success(support::run(source));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn rejects(source: &str, expected: &str) {
    let error = support::check_error(source);
    assert!(error.contains(expected), "expected {expected}: {error}");
}

#[test]
fn workers_fan_out_recursively_and_run_full_jobs_inline() {
    runs(
        r#"
from std.atomic import Atomic, Ordering
from std.time import milliseconds, sleep

def visit(submitter: Submitter, visited: Shared[Atomic[u64]], depth: i64) -> ():
    if depth > 0:
        for _ in range(2):
            handle = submitter.share()
            counter = visited.share()
            child = def once [handle, counter, depth]() -> ():
                # depth > 0, so this cannot overflow.
                unchecked:
                    visit(handle, counter, depth - 1)
            match submitter.submit_nowait(child):
                case Ok(_):
                    pass
                case Err(SubmitError.Full(job) | SubmitError.Shutdown(job) | SubmitError.OutOfMemory(job) | SubmitError.CapacityOverflow(job)):
                    job()
    _ = visited.fetch_add(1, Ordering.Release)

def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()]]:
    visited = Shared[Atomic[u64]](Atomic(0))?
    pause = milliseconds(1)
    with ThreadPoolExecutor(3, 2)? as pool:
        submitter = pool.submitter()
        submitter.submit_nowait(visit, submitter.share(), visited.share(), 4).drop_job()?
        while visited.load(Ordering.Acquire) < 31:
            sleep(&pause)
    print(visited.load(Ordering.Acquire))?
    Ok(())
"#,
        "31\n",
    );
}

#[test]
fn a_submitter_outlives_its_pool_and_reports_shutdown() {
    runs(
        r#"
def hello() -> ():
    pass

def main() -> Result[(), AllocError | IoError | PoolError]:
    mut submitter: Option[Submitter] = Nothing
    with ThreadPoolExecutor(1, 1)? as pool:
        submitter = Some(pool.submitter())
    match submitter:
        case Some(handle):
            match handle.submit_nowait(hello):
                case Ok(_):
                    print("accepted")?
                case Err(SubmitError.Shutdown(job)):
                    print("shutdown")?
                    job()
                case Err(_):
                    print("other")?
        case Nothing:
            pass
    Ok(())
"#,
        "shutdown\n",
    );
}

#[test]
fn shared_submitters_reach_jobs_and_threads() {
    runs(
        r#"
from std.atomic import Atomic, Ordering
from std.time import milliseconds, sleep

def bump(count: Shared[Atomic[u64]]) -> ():
    _ = count.fetch_add(1, Ordering.Release)

def relay(handles: Shared[Submitter], count: Shared[Atomic[u64]]) -> ():
    match handles.submit_nowait(bump, count):
        case Ok(_):
            pass
        case Err(_):
            pass

def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()]]:
    count = Shared[Atomic[u64]](Atomic(0))?
    pause = milliseconds(1)
    with ThreadPoolExecutor(2, 8)? as pool:
        handles = Shared[Submitter](pool.submitter())?
        for _ in range(3):
            pool.submit_nowait(relay, handles.share(), count.share()).drop_job()?
        while count.load(Ordering.Acquire) < 3:
            sleep(&pause)
    print(count.load(Ordering.Acquire))?
    Ok(())
"#,
        "3\n",
    );
    runs(
        r#"
def hello() -> ():
    _ = print("hello")
def feed(handle: Submitter) -> ():
    _ = handle.submit_nowait(hello).drop_job()
def main() -> Result[(), AllocError | IoError | PoolError | SpawnError[()]]:
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        with spawn(feed, handle).drop_job()? as task:
            task.join()
    Ok(())
"#,
        "hello\n",
    );
}

#[test]
fn cancel_pending_cancels_handle_jobs() {
    runs(
        r#"
from std.atomic import Atomic, Ordering
from std.time import milliseconds, sleep
def gated(gate: Shared[Atomic[bool]]) -> ():
    while not gate.load(Ordering.Acquire):
        sleep(&milliseconds(1))
def loud(text: str) -> ():
    _ = print(text)
def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()]]:
    gate = Shared[Atomic[bool]](Atomic(False))?
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        # The only worker runs or queues `gated` ahead of "never".
        handle.submit_nowait(gated, gate.share()).drop_job()?
        handle.submit_nowait(loud, "never").drop_job()?
        pool.cancel_pending()
        gate.store(True, Ordering.Release)
        handle.submit_nowait(loud, "kept").drop_job()?
    Ok(())
"#,
        "kept\n",
    );
}

#[test]
fn a_trap_in_a_handle_job_stops_the_program() {
    let output = support::run(
        r#"
def boom(xs: list[i64]) -> ():
    _ = xs[5]
def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        handle = pool.submitter()
        handle.submit_nowait(boom, [1, 2]?).drop_job()?
    print("after")?
    Ok(())
"#,
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("index out of bounds"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn handle_operations_are_nonblocking_and_jobs_return_unit() {
    rejects(
        r#"
def five() -> i64:
    5
def main() -> Result[(), PoolError]:
    with ThreadPoolExecutor(1, 1)? as pool:
        handle = pool.submitter()
        _ = handle.submit_nowait(five)
    Ok(())
"#,
        "a Submitter job must return (), not i64",
    );
    rejects(
        r#"
def hello() -> ():
    pass
def main() -> Result[(), PoolError]:
    with ThreadPoolExecutor(1, 1)? as pool:
        handle = pool.submitter()
        _ = handle.submit(hello)
    Ok(())
"#,
        "Submitter has no blocking submit",
    );
    rejects(
        r#"
def main() -> Result[(), PoolError]:
    with ThreadPoolExecutor(1, 1)? as pool:
        handle = pool.submitter()
        handle.shutdown()
    Ok(())
"#,
        "Submitter supports share() and submit_nowait(job)",
    );
}

#[test]
fn a_job_cannot_wait_on_a_queue_its_submitted_jobs_can_reach() {
    rejects(
        r#"
def child(out: Sender[i64]) -> ():
    _ = out.send(1)

def parent(handle: Submitter) -> Result[(), AllocError]:
    sender, receiver = channel[i64](1)?
    match handle.submit_nowait(child, sender):
        case Ok(_):
            _ = receiver.recv()
        case Err(_):
            pass
    Ok(())

def main() -> Result[(), AllocError | PoolError | SubmitError[()]]:
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle).drop_job()?
    Ok(())
"#,
        "executor worker `parent` can submit, through a Submitter, a job that can send on a `Sender[i64]`, and `parent` receives from a `Receiver[i64]` with a blocking or timed operation",
    );
    // Nonblocking operations on the same queue, and the child's own wait, are
    // allowed: the child cannot submit.
    support::check_source(
        r#"
def child(out: Sender[i64]) -> ():
    _ = out.send(1)

def parent(handle: Submitter) -> Result[(), AllocError]:
    sender, receiver = channel[i64](1)?
    match handle.submit_nowait(child, sender):
        case Ok(_):
            _ = receiver.recv_nowait()
        case Err(_):
            pass
    Ok(())

def main() -> Result[(), AllocError | PoolError | SubmitError[()]]:
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle).drop_job()?
    Ok(())
"#,
    )
    .unwrap();
}

#[test]
fn a_submitting_job_can_send_blocking_to_a_receiver_its_jobs_cannot_reach() {
    // `split` jobs hold only senders; `main` drains the receiver, so a full
    // queue drains without any pool job. The receive loop also ends only once
    // every job has dropped its sender, so the pool is quiescent at shutdown.
    runs(
        r#"
def split(handle: Submitter, out: Sender[i64], depth: i64) -> ():
    if depth > 0:
        mut child = 0
        # depth > 0, so this cannot overflow.
        unchecked:
            child = depth - 1
        for _ in range(2):
            match handle.submit_nowait(split, handle.share(), out.share(), child):
                case Ok(_):
                    pass
                case Err(SubmitError.Full(job) | SubmitError.Shutdown(job) | SubmitError.OutOfMemory(job) | SubmitError.CapacityOverflow(job)):
                    job()
    _ = out.send(1)

def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()] | OverflowError[i64]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(2, 2)? as pool, receiver as inbox:
        handle = pool.submitter()
        handle.submit_nowait(split, handle.share(), sender, 4).drop_job()?
        mut total = 0
        while True:
            match inbox.recv():
                case Ok(n):
                    total = total + n
                case Err(_):
                    break
        print(total)?
    Ok(())
"#,
        "31\n",
    );
    rejects(
        r#"
def child(inbox: Receiver[i64]) -> ():
    _ = inbox.recv()

def parent(handle: Submitter) -> Result[(), AllocError]:
    sender, receiver = channel[i64](1)?
    match handle.submit_nowait(child, receiver):
        case Ok(_):
            _ = sender.send(1)
        case Err(_):
            pass
    Ok(())

def main() -> Result[(), AllocError | PoolError | SubmitError[()]]:
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle).drop_job()?
    Ok(())
"#,
        "a job that can receive from a `Receiver[i64]`, and `parent` sends on a `Sender[i64]`",
    );
}

#[test]
fn a_job_cannot_wait_on_a_token_its_submitted_jobs_can_reach() {
    rejects(
        r#"
def child(stop: CancellationToken) -> ():
    stop.cancel()

def parent(handle: Submitter) -> Result[(), AllocError]:
    stop = CancellationToken()?
    match handle.submit_nowait(child, stop.share()):
        case Ok(_):
            stop.wait()
        case Err(_):
            pass
    Ok(())

def main() -> Result[(), AllocError | PoolError | SubmitError[()]]:
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle).drop_job()?
    Ok(())
"#,
        "a job that can cancel a `CancellationToken`, and `parent` waits on a `CancellationToken`",
    );
}

#[test]
fn a_job_cannot_lock_a_mutex_type_its_submitted_jobs_can_reach() {
    rejects(
        r#"
from std.sync import Mutex
def child(m: Shared[Mutex[i64]]) -> ():
    with m.lock() as data:
        *data = 2

def parent(handle: Submitter, m: Shared[Mutex[i64]]) -> ():
    with m.lock() as data:
        *data = 1
    _ = handle.submit_nowait(child, m).drop_job()

def main() -> Result[(), AllocError | PoolError | SubmitError[()]]:
    m = Shared[Mutex[i64]](Mutex(0))?
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle, m).drop_job()?
    Ok(())
"#,
        "a job that can lock a `Mutex[i64]`, and `parent` locks a `Mutex[i64]`",
    );
    // A mutex of a type no submitted job can reach may be locked.
    runs(
        r#"
from std.sync import Mutex
def child(m: Shared[Mutex[i64]]) -> ():
    pass

def parent(handle: Submitter, m: Shared[Mutex[str]], n: Shared[Mutex[i64]]) -> ():
    with m.lock() as data:
        pass
    _ = handle.submit_nowait(child, n).drop_job()

def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()]]:
    m = Shared[Mutex[str]](Mutex("a"))?
    n = Shared[Mutex[i64]](Mutex(0))?
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle, m, n).drop_job()?
    print("ok")?
    Ok(())
"#,
        "ok\n",
    );
}

#[test]
fn a_job_cannot_join_a_thread_that_shares_a_queue_with_its_submitted_jobs() {
    rejects(
        r#"
def child(out: Sender[i64]) -> ():
    _ = out.send(1)

def wait_one(inbox: Receiver[i64]) -> ():
    _ = inbox.recv()

def parent(handle: Submitter) -> Result[(), AllocError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    with spawn(wait_one, receiver).drop_job()? as task:
        _ = handle.submit_nowait(child, sender).drop_job()
        task.join()
    Ok(())

def main() -> Result[(), AllocError | PoolError | SubmitError[()]]:
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle).drop_job()?
    Ok(())
"#,
        "and `parent` joins a thread that holds a channel endpoint for `i64` messages",
    );
    // A thread that shares nothing with the submitted jobs may be joined.
    runs(
        r#"
def child(out: Sender[i64]) -> ():
    _ = out.send(1)

def idle() -> ():
    pass

def parent(handle: Submitter) -> Result[(), AllocError | SpawnError[()]]:
    sender, _ = channel[i64](1)?
    with spawn(idle).drop_job()? as task:
        _ = handle.submit_nowait(child, sender).drop_job()
        task.join()
    Ok(())

def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()]]:
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle).drop_job()?
    print("ok")?
    Ok(())
"#,
        "ok\n",
    );
}

#[test]
fn a_shutdown_sees_the_senders_of_jobs_a_job_can_submit() {
    // `parent` holds no sender; the job it submits does.
    rejects(
        r#"
def child(out: Sender[i64]) -> ():
    _ = out.send(1)

def parent(handle: Submitter) -> Result[(), AllocError]:
    out, _ = channel[i64](1)?
    match handle.submit_nowait(child, out):
        case Ok(_):
            pass
        case Err(_):
            pass
    Ok(())

def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()] | RecvError]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 4)? as pool:
        handle = pool.submitter()
        pool.submit_nowait(parent, handle).drop_job()?
        print(receiver.recv_nowait().is_ok())?
    drop(sender)
    Ok(())
"#,
        "after a job that holds `Sender[i64]` was submitted, while `receiver` still holds `Receiver[i64]`",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn taking_sharing_and_refused_submissions_do_not_allocate() {
    runs(
        r#"
def hello() -> ():
    pass

def main() -> Result[(), AllocError | IoError | PoolError]:
    pool = ThreadPoolExecutor(1, 1)?
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    handle = pool.submitter()
    other = handle.share()
    drop(pool)
    refused = other.submit_nowait(hello).drop_job()
    drop(handle)
    drop(other)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(refused.is_err())?
    Ok(())
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\nTrue\n",
    );
}
