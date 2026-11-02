mod support;

fn runs(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "status {:?}; {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn owned_reusable_and_once_jobs_transfer_captures_and_results() {
    runs(
        r#"
def submit[F: OnceCallable[[], list[i64]]](job: F) -> Result[list[i64], SpawnError[F]]:
    with spawn(job)? as task:
        return Ok(task.join())
values = [3, 4].unwrap()
job = def once [values]() -> list[i64]:
    values
print(submit(job).unwrap()).unwrap()
span = range(10, 15)
other = def [mut span]() -> i64:
    span[2]
with spawn(other).unwrap() as task:
    print(task.join()).unwrap()
"#,
        "[3, 4]\n12\n",
    );
}

#[test]
fn consuming_submission_rejects_moved_jobs_borrowed_environments_and_effects() {
    for (source, expected) in [
        (
            "n = 1\njob = def once [n]() -> i64:\n    n\nwith spawn(job).unwrap():\n    job()\n",
            "moved",
        ),
        (
            "n = 1\njob = def [&n]() -> i64:\n    n\nwith spawn(job).unwrap():\n    pass\n",
            "wholly owned",
        ),
        (
            "def seven() -> i64:\n    path = std.env.current_dir().unwrap()\n    std.env.set_current_dir(&path).unwrap()\n    7\nn = 1\njob = def [n]() -> i64:\n    f: Callable[[], i64] = seven\n    n + f()\nwith spawn(job).unwrap():\n    pass\n",
            "changing the working directory",
        ),
    ] {
        let source = format!("import std.env\n{}", support::program(source));
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn spawn_error_has_only_the_unavailable_variant() {
    let error = support::check_source(
        r#"
def attempt[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[F]]:
    with spawn(job)? as task:
        return Ok(task.join())
def run[F: OnceCallable[[], i64]](job: F) -> i64:
    match attempt(job):
        case Ok(value):
            value
        case Err(error):
            match error:
                case SpawnError.Unavailable(job, _):
                    job()
                case SpawnError.PermissionDenied(job):
                    job()
n = 1
job = def once [n]() -> i64:
    n
print(run(job)).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("unknown variant `PermissionDenied`"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_owned_start_returns_the_unmodified_job_without_allocating() {
    runs(
        r#"
def attempt[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[F]]:
    with spawn(job)? as task:
        return Ok(task.join())
def retry[F: OnceCallable[[], i64]](job: F) -> i64:
    match attempt(job):
        case Ok(value):
            value
        case Err(error):
            match error:
                case SpawnError.Unavailable(job, _):
                    print("__test_restore_thread_starts__").unwrap()
                    attempt(job).unwrap()
span = range(10, 20)
values = [span].unwrap()
job = def once [values]() -> i64:
    values[0][3]
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_fail_thread_starts__").unwrap()
answer = retry(job)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(answer).unwrap()
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_fail_thread_starts__\n__test_restore_thread_starts__\n__test_restore_allocations__\n__test_end_no_allocations__\n13\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_residuals_and_unclaimed_worker_results_destroy_exactly_once() {
    // Tests `Failure` erasure itself: converting the failed start to
    // `Failure` drops the unstarted job, which this test observes. A precise
    // `SpawnError[F]` returns the job instead (covered by
    // `failed_owned_start_in_error_sum_returns_the_job`); `drop_job()` drops
    // it at the start (`drop_job_drops_a_failed_start_before_returning`).
    runs(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
def make() -> OnceClosure[[], Guard]:
    guard = Guard(1)
    def once [guard]() -> Guard:
        guard
def attempt[F: OnceCallable[[], Guard]](job: F) -> Result[(), Failure]:
    with spawn(job)?:
        pass
    Ok(())
job = make()
print("__test_fail_thread_starts__").unwrap()
print(str.repr(attempt(job)).unwrap()).unwrap()
print("__test_restore_thread_starts__").unwrap()
with spawn(make()).unwrap():
    pass
guard = Guard(2)
other = def [guard]() -> i64:
    guard.id
with spawn(other).unwrap() as task:
    print(task.join()).unwrap()
"#,
        "__test_fail_thread_starts__\ndrop\nResult[(), Failure].Err(Failure.Unspecified)\n__test_restore_thread_starts__\ndrop\ndrop\n2\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn drop_job_drops_a_failed_start_before_returning() {
    // The job-free error carries only the variant; the job's captures are
    // destroyed at the start, before the function prints or returns.
    runs(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        if self.id == 1:
            write_stdout("job\n").unwrap()
        else:
            write_stdout("frame\n").unwrap()
        pass
def make() -> OnceClosure[[], Guard]:
    guard = Guard(1)
    def once [guard]() -> Guard:
        guard
def attempt[F: OnceCallable[[], Guard]](job: F) -> Result[(), SpawnError[()]]:
    started = Guard(3)
    with spawn(job).drop_job()?:
        pass
    drop(started)
    Ok(())
job = make()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_thread_starts__").unwrap()
failed = attempt(job)
print("__test_end_no_allocations__").unwrap()
print(str.repr(failed).unwrap()).unwrap()
print("__test_restore_thread_starts__").unwrap()
print(str.repr(attempt(make())).unwrap()).unwrap()
"#,
        "__test_begin_no_allocations__\n__test_fail_thread_starts__\njob\nframe\n__test_end_no_allocations__\nResult[(), SpawnError[()]].Err(SpawnError[()].Unavailable((), 11))\n__test_restore_thread_starts__\njob\nframe\nResult[(), SpawnError[()]].Ok(())\n",
    );
}

#[test]
fn owned_start_error_widens_into_declared_error_sum() {
    runs(
        r#"
def submit[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[F] | IoError]:
    with spawn(job)? as task:
        print("started")?
        return Ok(task.join())
n = 4
job = def once [n]() -> i64:
    n + 1
print(submit(job).unwrap()).unwrap()
"#,
        "started\n5\n",
    );
    let error = support::check_source(
        "def submit[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[()] | IoError]:\n    with spawn(job)? as task:\n        return Ok(task.join())\nn = 1\njob = def once [n]() -> i64:\n    n\nsubmit(job).unwrap()\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("spawn propagation requires a Result whose error accepts SpawnError[OnceClosure[[], i64]"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_owned_start_in_error_sum_returns_the_job() {
    runs(
        r#"
def attempt[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[F] | IoError]:
    with spawn(job)? as task:
        return Ok(task.join())
def retry[F: OnceCallable[[], i64]](job: F) -> i64:
    match attempt(job):
        case Ok(value):
            value
        case Err(error):
            match error:
                case failed: SpawnError[F]:
                    match failed:
                        case SpawnError.Unavailable(job, _):
                            print("__test_restore_thread_starts__").unwrap()
                            attempt(job).unwrap()
                case _:
                    0
values = [7, 8].unwrap()
job = def once [values]() -> i64:
    values[1]
print("__test_fail_thread_starts__").unwrap()
print(retry(job)).unwrap()
"#,
        "__test_fail_thread_starts__\n__test_restore_thread_starts__\n8\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_named_start_with_owned_arguments_returns_them_with_the_code() {
    // Issue #630: owned arguments bind a one-shot job, so the caller can
    // retry the start or run the job locally; the error keeps the native code.
    runs(
        r#"
def total(values: list[i64], scale: i64) -> i64:
    values[0] * scale + values[1] * scale
def attempt(values: list[i64]) -> Result[i64, SpawnError[OnceClosure[[], i64]]]:
    with spawn(total, values, 10)? as task:
        return Ok(task.join())
print("__test_fail_thread_starts__").unwrap()
match attempt([1, 2].unwrap()):
    case Ok(value):
        print(value).unwrap()
    case Err(SpawnError.Unavailable(job, code)):
        print("__test_restore_thread_starts__").unwrap()
        print(code).unwrap()
        print(job()).unwrap()
"#,
        "__test_fail_thread_starts__\n__test_restore_thread_starts__\n11\n30\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_borrowing_named_start_reports_job_free_spawn_error() {
    // A borrowed argument cannot be returned in an error, so this start
    // reports the job-free `SpawnError[()]`.
    runs(
        r#"
def total(values: &list[i64], scale: i64) -> i64:
    values[0] * scale
def attempt(values: &list[i64]) -> Result[i64, SpawnError[()]]:
    with spawn(total, values, 10)? as task:
        return Ok(task.join())
values = [4, 5].unwrap()
print("__test_fail_thread_starts__").unwrap()
print(str.repr(attempt(&values)).unwrap()).unwrap()
print("__test_restore_thread_starts__").unwrap()
print(attempt(&values).unwrap()).unwrap()
"#,
        "__test_fail_thread_starts__\nResult[i64, SpawnError[()]].Err(SpawnError[()].Unavailable((), 11))\n__test_restore_thread_starts__\n40\n",
    );
}

#[test]
fn owned_named_start_declared_with_thread_error_names_drop_job() {
    let error = support::check_error(
        "def total(values: list[i64]) -> i64:\n    values[0]\ndef main() -> Result[(), ThreadError | AllocError]:\n    with spawn(total, [1].unwrap())? as task:\n        task.join()\n    Ok(())\n",
    );
    assert!(
        error.contains("call `drop_job()` on the start and declare the job-free `SpawnError[()]`"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn dropping_a_stored_spawn_error_keeps_its_code_and_releases_the_job_once() {
    runs(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def result[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[F]]:
    with spawn(job)? as task:
        return Ok(task.join())
def make(label: str) -> OnceClosure[[], i64]:
    guard = Guard(label)
    def once [guard]() -> i64:
        len(guard.label)
def main() -> ():
    print("__test_fail_thread_starts__").unwrap()
    held = result(make("error"))
    match held:
        case Ok(_):
            print("unexpected worker").unwrap()
        case Err(error):
            free = error.drop_job()
            print(str.repr(free).unwrap()).unwrap()
            drop(free)
    other = result(make("result"))
    converted = other.drop_job()
    print(str.repr(converted).unwrap()).unwrap()
    drop(converted)
    print("__test_restore_thread_starts__").unwrap()
"#,
        "__test_fail_thread_starts__\nerror\nSpawnError[()].Unavailable((), 11)\nresult\nResult[i64, SpawnError[()]].Err(SpawnError[()].Unavailable((), 11))\n__test_restore_thread_starts__\n",
    );
}
