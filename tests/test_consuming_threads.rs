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
        ("n = 1\njob = def once [n]() -> i64:\n    n\nwith spawn(job).unwrap():\n    job()\n", "moved"),
        ("n = 1\njob = def [&n]() -> i64:\n    n\nwith spawn(job).unwrap():\n    pass\n", "wholly owned"),
        ("n = 1\njob = def [n]() -> i64:\n    file = open(\"unused\", \"r\").unwrap()\n    drop(file)\n    n\nwith spawn(job).unwrap():\n    pass\n", "File"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_owned_start_returns_the_unmodified_job_without_allocating() {
    runs(r#"
def attempt[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[F]]:
    with spawn(job)? as task:
        return Ok(task.join())
def retry[F: OnceCallable[[], i64]](job: F) -> i64:
    match attempt(job):
        case Ok(value):
            value
        case Err(error):
            match error:
                case SpawnError[F].Unavailable(job):
                    print("__test_restore_thread_starts__").unwrap()
                    attempt(job).unwrap()
                case SpawnError[F].PermissionDenied(job):
                    job()
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
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_fail_thread_starts__\n__test_restore_thread_starts__\n__test_restore_allocations__\n__test_end_no_allocations__\n13\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_residuals_and_unclaimed_worker_results_destroy_exactly_once() {
    runs(r#"
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
"#, "__test_fail_thread_starts__\ndrop\nResult[(), Failure].Err(Failure.Unspecified)\n__test_restore_thread_starts__\ndrop\ndrop\n2\n");
}
