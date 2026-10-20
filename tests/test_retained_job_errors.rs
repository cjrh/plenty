//! Nongeneric signatures that name retained-job errors through closure
//! signatures (issue #336). `?` resolves the concrete job from the error it
//! propagates, as a returned closure resolves a declared closure return.
mod support;

const GUARD: &str = r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
"#;

#[cfg(feature = "runtime-checks")]
#[test]
fn main_propagates_a_failed_closure_start_unchanged() {
    let output = support::run(
        r#"
def main() -> Result[(), AllocError | IoError | SpawnError[OnceClosure[[], list[i64]]]]:
    values = [3, 4]?
    print("__test_fail_thread_starts__")?
    with spawn(def once [values]() -> list[i64]: values)? as task:
        print(task.join())?
    Ok(())
"#,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("SpawnError[OnceClosure[[], list[i64]]"),
        "{stderr}"
    );
    assert!(stderr.contains(".Unavailable(<closure>)"), "{stderr}");
}

#[test]
fn main_propagates_a_rejected_closure_submission_unchanged() {
    let output = support::run_file(&format!(
        r#"
from std.concurrent import ThreadPoolExecutor, SubmitError, PoolError
{GUARD}
def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[OnceClosure[[], list[i64]]]]:
    values = [3, 4]?
    guard = Guard(1)
    with ThreadPoolExecutor(1, 1)? as pool:
        pool.shutdown()
        future = pool.submit(def once [values, guard]() -> list[i64]: values)?
        print(future.result().unwrap())?
    Ok(())
"#
    ));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("SubmitError[OnceClosure[[], list[i64]]"),
        "{stderr}"
    );
    assert!(stderr.contains(".Shutdown(<closure>)"), "{stderr}");
    // The returned error owns the job; reporting it destroys the captures once.
    assert_eq!(String::from_utf8_lossy(&output.stdout), "drop\n");
}

#[test]
fn successful_starts_and_submissions_run_with_inferred_error_signatures() {
    let stdout = support::run_file_stdout(
        r#"
from std.concurrent import ThreadPoolExecutor, SubmitError, PoolError

def main() -> Result[(), AllocError | IoError | PoolError | SpawnError[OnceClosure[[], i64]] | SubmitError[OnceClosure[[], i64]]]:
    with spawn(def once () -> i64: 3)? as task:
        print(task.join())?
    with ThreadPoolExecutor(1, 1)? as pool:
        print(pool.submit(def once () -> i64: 4)?.result().unwrap())?
    Ok(())
"#,
    );
    assert_eq!(stdout, "3\n4");
}

#[test]
fn generic_and_nongeneric_propagation_compose() {
    support::assert_stdout(
        r#"
def attempt[F: OnceCallable[[], list[i64]]](job: F) -> Result[list[i64], SpawnError[F]]:
    with spawn(job)? as task:
        return Ok(task.join())

def make(values: list[i64]) -> OnceClosure[[], list[i64]]:
    def once [values]() -> list[i64]:
        values

def run(values: list[i64]) -> Result[(), IoError | SpawnError[OnceClosure[[], list[i64]]]]:
    print(attempt(make(values))?)?
    Ok(())

def returned(values: list[i64]) -> Result[list[i64], IoError | SpawnError[OnceClosure[[], list[i64]]]]:
    match attempt(make(values)):
        case Ok(values):
            Ok(values)
        case Err(error):
            Err(error)

def main() -> Result[(), AllocError | IoError | SpawnError[OnceClosure[[], list[i64]]]]:
    run([1, 2]?)?
    print(returned([3]?)?)?
    Ok(())
"#,
        "[1, 2]\n[3]\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn recovered_jobs_run_and_destroy_their_captures_once() {
    support::assert_stdout(
        &format!(
            r#"{GUARD}
def make(id: i64) -> OnceClosure[[], i64]:
    guard = Guard(id)
    def once [guard]() -> i64:
        guard.id

def run(id: i64) -> Result[i64, IoError | SpawnError[OnceClosure[[], i64]]]:
    with spawn(make(id))? as task:
        return Ok(task.join())

def main() -> Result[(), IoError]:
    print("__test_fail_thread_starts__")?
    match run(7):
        case Ok(value):
            print(value)?
        case Err(error):
            match error:
                case failed: SpawnError[OnceClosure[[], i64]]:
                    match failed:
                        case SpawnError.Unavailable(job):
                            print("recovered")?
                            print(job())?
                case other: IoError:
                    return Err(other)
    match run(8):
        case Ok(value):
            print(value)?
        case Err(_):
            print("dropped")?
    print("__test_restore_thread_starts__")?
    print(run(9).unwrap())?
    Ok(())
"#
        ),
        "__test_fail_thread_starts__\nrecovered\ndrop\n7\ndropped\ndrop\n__test_restore_thread_starts__\ndrop\n9\n",
    );
}

#[test]
fn a_declared_job_member_takes_one_producer() {
    let error = support::check_error(
        r#"
def main() -> Result[(), SpawnError[OnceClosure[[], i64]]]:
    with spawn(def once () -> i64: 3)? as task:
        task.join()
    with spawn(def once () -> i64: 4)? as task:
        task.join()
    Ok(())
"#,
    );
    assert!(error.contains("already names another closure"), "{error}");
    let error = support::check_file_error(
        r#"
from std.concurrent import ThreadPoolExecutor, SubmitError, PoolError
def main() -> Result[(), PoolError | SubmitError[OnceClosure[[], i64]]]:
    with ThreadPoolExecutor(1, 1)? as pool:
        pool.submit(def once () -> i64: 4)?.result().unwrap()
        pool.submit(def once () -> i64: 5)?.result().unwrap()
    Ok(())
"#,
    );
    assert!(error.contains("already names another closure"), "{error}");
    // One factory gives every job the same producer.
    support::check_source(
        r#"
def make(n: i64) -> OnceClosure[[], i64]:
    def once [n]() -> i64:
        n
def main() -> Result[(), SpawnError[OnceClosure[[], i64]]]:
    with spawn(make(3))? as task:
        task.join()
    with spawn(make(4))? as task:
        task.join()
    Ok(())
"#,
    )
    .unwrap();
}

#[test]
fn closure_sum_members_resolve_only_in_return_types() {
    let error = support::check_error(
        "def main() -> Result[(), SpawnError[OnceClosure[[], i64]]]:\n    Ok(())\n",
    );
    assert!(
        error.contains("cannot infer the concrete one-shot closure returned by this function"),
        "{error}"
    );
    for source in [
        "def f(x: i64 | OnceClosure[[], i64]) -> ():\n    pass\n",
        "def f() -> Result[(), IoError]:\n    x: i64 | OnceClosure[[], i64] = 3\n    Ok(())\n",
    ] {
        let error = support::check_error(source);
        assert!(
            error.contains("only a function's declared return type can infer it"),
            "{error}"
        );
    }
    let error = support::check_error("class C:\n    x: i64 | OnceClosure[[], i64]\n");
    assert!(
        error.contains("cannot be stored in class fields"),
        "{error}"
    );
}
