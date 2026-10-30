mod support;

fn run(source: &str, expected: &str) {
    let output = support::run(source);
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
fn stored_jobs_borrow_disjoint_tuple_slots_and_exclusive_collection_entries() {
    run(
        r#"
def counter(n: i64) -> Closure[[], i64]:
    def [mut n]() -> i64:
        n = n + 1
        n
mut pair = (counter(1), counter(10))
with spawn(&mut pair[0]).unwrap() as first, spawn(&mut pair[1]).unwrap() as second:
    print(first.join()).unwrap()
    print(second.join()).unwrap()
mut callbacks = [counter(20)].unwrap()
with spawn(&mut callbacks[0]).unwrap() as task:
    print(task.join()).unwrap()
print(callbacks[0]()).unwrap()
mut named = {0: counter(30)}.unwrap()
match named.get_mut(0):
    case Some(callback):
        with spawn(&mut *callback).unwrap() as task:
            print(task.join()).unwrap()
    case Nothing:
        pass
"#,
        "2\n11\n21\n22\n31\n",
    );
}

#[test]
fn worker_joins_transfer_concrete_closures_and_callback_containers() {
    run(
        r#"
def make(n: i64) -> Closure[[], i64]:
    span = range(n, n + 3)
    def [span]() -> i64:
        span[2]
def take[F: Callable[[], i64]](jobs: &mut list[F]) -> Option[F]:
    jobs.pop(0)
def transfer[F](jobs: &mut list[F]) -> Result[list[F], AllocError]:
    mut result = list[F].new()?
    while len(jobs) > 0:
        result.append(jobs.pop(0).unwrap())?
    Ok(result)
def produce() -> Result[OnceClosure[[], list[i64]], AllocError]:
    values = [42]?
    job = def once [values]() -> list[i64]:
        values
    Ok(job)
with spawn(make, 10).unwrap() as task:
    callback = task.join()
    print(callback()).unwrap()
with spawn(produce).unwrap() as task:
    job = task.join().unwrap()
    print(job()).unwrap()
mut jobs = [make(20), make(30)].unwrap()
with spawn(take, &mut jobs).unwrap() as task:
    callback = task.join().unwrap()
    print(callback()).unwrap()
with spawn(transfer, &mut jobs).unwrap() as task:
    callbacks = task.join().unwrap()
    print(callbacks[0]()).unwrap()
"#,
        "12\n[42]\n22\n32\n",
    );
}

#[test]
fn stored_callback_capture_and_body_effects_are_checked_transitively() {
    let definitions = "def seven() -> i64:\n    7\ndef unsafe_job(n: i64) -> Closure[[], i64]:\n    def [n]() -> i64:\n        callback: Callable[[], i64] = seven\n        n + callback()\n";
    for body in [
        "callbacks = [unsafe_job(1)].unwrap()\nwith spawn(&callbacks[0]).unwrap():\n    pass",
        "class Holder[F]:\n    callback: F\nholder = Holder(unsafe_job(1))\nwith spawn(&holder.callback).unwrap():\n    pass",
        "with spawn(unsafe_job, 1).unwrap():\n    pass",
    ] {
        let error = support::check_source(&format!("{definitions}{body}\n"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("worker") && error.contains("indirect callable"),
            "{error}"
        );
    }
}

#[test]
fn worker_loans_prevent_callback_removal_replacement_and_repeated_consumption() {
    let definitions = "def make(n: i64) -> Closure[[], i64]:\n    def [mut n]() -> i64:\n        n = n + 1\n        n\nmut jobs = [make(1)].unwrap()\n";
    for action in [
        "jobs.pop(0)",
        "jobs[0] = make(2)",
        "jobs[0]()",
        "drop(jobs)",
    ] {
        let error = support::check_source(&format!("{definitions}with spawn(&mut jobs[0]).unwrap() as task:\n    task.join()\n    {action}\n")).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn worker_closure_results_and_unclaimed_capture_cleanup_need_no_allocation() {
    run(
        r#"
def make(n: i64) -> Closure[[], i64]:
    span = range(n, n + 3)
    inner = def [span]() -> i64:
        span[1]
    def [inner]() -> i64:
        inner()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
with spawn(make, 10).unwrap() as task:
    callback = task.join()
    value = callback()
    if value != 11:
        print("wrong").unwrap()
with spawn(make, 20).unwrap():
    pass
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_stored_job_start_preserves_state_and_unclaimed_outputs_drop_captures() {
    run(
        r#"
def make(n: i64) -> Closure[[], i64]:
    def [mut n]() -> i64:
        n = n + 1
        n
def attempt[F: Callable[[], i64]](callback: &mut F) -> Result[i64, ThreadError]:
    with spawn(&mut callback)? as task:
        return Ok(task.join())
class Guard:
    id: i64
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
def owned_result() -> Result[OnceClosure[[], Guard], AllocError]:
    guard = Guard(1)
    job = def once [guard]() -> Guard:
        guard
    Ok(job)
mut jobs = [make(10)].unwrap()
print("__test_fail_thread_starts__").unwrap()
match attempt(&mut jobs[0]):
    case Ok(_):
        print("wrong").unwrap()
    case Err(_):
        print("failed").unwrap()
print("__test_restore_thread_starts__").unwrap()
print(attempt(&mut jobs[0]).unwrap()).unwrap()
with spawn(owned_result).unwrap():
    pass
with spawn(owned_result).unwrap() as task:
    job = task.join().unwrap()
    guard = job()
    drop(guard)
"#,
        "__test_fail_thread_starts__\nfailed\n__test_restore_thread_starts__\n11\ndrop\ndrop\n",
    );
}

#[test]
fn callback_worker_results_do_not_escape_borrowed_captures_or_hide_destructors() {
    let source = r#"
def seven() -> i64:
    7
class Guard:
    id: i64
    def __del__(self) -> ():
        callback: Callable[[], i64] = seven
        callback()
        pass
def make() -> Result[Closure[[], i64], AllocError]:
    guard = Guard(1)
    callback = def [guard]() -> i64:
        guard.id
    Ok(callback)
with spawn(make).unwrap():
    pass
"#;
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains("worker") && error.contains("indirect callable"),
        "{error}"
    );
    let source = "def invalid(value: &i64) -> Closure[[], i64]:\n    def [&value]() -> i64:\n        value\nn = 1\nwith spawn(invalid, &n).unwrap():\n    pass\n";
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains("borrow") || error.contains("reference"),
        "{error}"
    );
}

const COUNTER_AND_READER: &str = "def counter(n: i64) -> Closure[[], i64]:\n    def [mut n]() -> i64:\n        n = n + 1\n        n\ndef reader(n: i64) -> Closure[[], i64]:\n    def [n]() -> i64:\n        n * 2\n";

/// Issue #585: a binding that holds a closure reference reborrows its referent
/// for the task's scope, without writing `&mut *callback`.
#[test]
fn reference_bindings_reborrow_their_closure_for_the_task() {
    run(
        &format!(
            r#"{COUNTER_AND_READER}
def run_mut(job: &mut Closure[[], i64]) -> Result[i64, ThreadError]:
    with spawn(job)? as task:
        return Ok(task.join())
def run_generic[F: Callable[[], i64]](job: &mut F) -> Result[i64, ThreadError]:
    with spawn(job)? as task:
        return Ok(task.join())
def run_shared(job: &Closure[[], i64]) -> Result[i64, ThreadError]:
    with spawn(job)? as first, spawn(job)? as second:
        return Ok(first.join() + second.join())
def main() -> Result[(), AllocError | IoError | ThreadError]:
    mut named = {{0: counter(30)}}?
    match named.get_mut(0):
        case Some(callback):
            with spawn(callback)? as task:
                print(task.join())?
        case Nothing:
            pass
    mut c = counter(40)
    print(run_mut(&mut c)?)?
    print(run_generic(&mut c)?)?
    print(c())?
    r = reader(5)
    print(run_shared(&r)?)?
    shared = &r
    with spawn(shared)? as task:
        print(r())?
        print(task.join())?
    Ok(())
"#
        ),
        "31\n41\n42\n43\n20\n10\n10\n",
    );
}

#[test]
fn reference_binding_workers_keep_closure_and_loan_rules() {
    let cases = [
        (
            "def run(job: &Closure[[], i64]) -> Result[i64, ThreadError]:\n    with spawn(job)? as task:\n        return Ok(task.join())\ndef main() -> Result[(), ThreadError]:\n    mut c = counter(1)\n    run(&c)?\n    Ok(())\n",
            "this worker closure has mutable captures and needs exclusive access, but this binding is a shared reference; bind or pass it as `&mut`",
        ),
        (
            "def run(job: &mut Closure[[], i64]) -> Result[i64, ThreadError]:\n    with spawn(job)? as task:\n        x = job()\n        return Ok(task.join() + x)\ndef main() -> Result[(), ThreadError]:\n    mut c = counter(1)\n    run(&mut c)?\n    Ok(())\n",
            "conflicting borrow: cannot modify or exclusively borrow `job` while it is exclusively borrowed",
        ),
        (
            "def main() -> Result[(), AllocError | ThreadError]:\n    mut named = {0: counter(1)}?\n    match named.get_mut(0):\n        case Some(callback):\n            with spawn(callback)? as task:\n                named.clear()\n                task.join()\n        case Nothing:\n            pass\n    Ok(())\n",
            "conflicting borrow: cannot modify or exclusively borrow `named` while it is exclusively borrowed",
        ),
        (
            "def main() -> Result[(), ThreadError]:\n    mut r = reader(1)\n    shared = &r\n    with spawn(shared)? as task:\n        r = reader(2)\n        task.join()\n    Ok(())\n",
            "borrow",
        ),
        (
            "def run(job: &mut OnceClosure[[], i64]) -> Result[i64, ThreadError]:\n    with spawn(job)? as task:\n        return Ok(task.join())\ndef main() -> Result[(), ThreadError]:\n    n = 1\n    mut c = def once [n]() -> i64:\n        n\n    run(&mut c)?\n    Ok(())\n",
            "spawn requires a reusable zero-argument closure",
        ),
    ];
    for (body, expected) in cases {
        let error = support::check_source(&format!("{COUNTER_AND_READER}{body}"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
}
