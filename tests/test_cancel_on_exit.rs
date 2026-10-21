//! `with token.cancel_on_exit():` and the check that rejects an exit that
//! joins a task, or shuts down a pool, before the function's later cancel of
//! a token that the task or job holds (issue #355).
mod support;
use std::process::Command;
use std::time::{Duration, Instant};
use support::check_file_error as rejected;

const HEADER: &str = "\
from std.sync import CancellationToken
from std.concurrent import ThreadPoolExecutor, Future, SubmitError, PoolError, FutureError

def spin(stop: CancellationToken) -> ():
    while not stop.is_cancelled():
        drop(stop.wait_timeout(50u64))

def watch(stop: &CancellationToken) -> ():
    stop.wait()

def fails() -> Result[(), str]:
    Err(\"failed\")

def quick() -> ():
    pass
";

fn program(body: &str) -> String {
    format!("{HEADER}\n{body}")
}

fn accepted(body: &str) {
    let source = program(body);
    if let Err(error) = support::check_source(&source) {
        panic!("rejected:\n{source}\n{error}");
    }
}

fn rejects(body: &str, expected: &[&str]) {
    let error = rejected(&program(body));
    for line in expected {
        assert!(error.contains(line), "missing {line:?} in:\n{error}");
    }
}

/// Runs a program that must fail promptly with `main returned "failed"`.
/// A hang is the bug this guards against, so the run has a deadline.
fn fails_promptly(body: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    support::compile_source_to_executable(&program(body), &executable).unwrap();
    let mut child = Command::new(&executable)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(20) {
            child.kill().unwrap();
            panic!("program hung:\n{body}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("main returned \"failed\""), "{stderr}");
}

const JOINS: &str =
    "leaving this `with` block joins a task that holds `stop` before this function cancels it";

#[test]
fn a_question_mark_before_the_cancel_is_rejected() {
    rejects(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())?:
        fails()?
        stop.cancel()
    Ok(())
"#,
        &[
            JOINS,
            "`?` leaves the block here",
            "`stop` is cancelled here",
            "`with spawn(...)?, stop.cancel_on_exit():`",
        ],
    );
}

#[test]
fn a_borrowed_token_is_matched_too() {
    rejects(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(watch, &stop)?:
        fails()?
        stop.cancel()
    Ok(())
"#,
        &[JOINS],
    );
}

#[test]
fn return_break_and_continue_before_the_cancel_are_rejected() {
    rejects(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())?:
        if True:
            return Err("early")
        stop.cancel()
    Ok(())
"#,
        &[JOINS],
    );
    rejects(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    for i in range(3):
        with spawn(spin, stop.share())?:
            if i == 0:
                break
            stop.cancel()
    Ok(())
"#,
        &[JOINS],
    );
    rejects(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    for i in range(3):
        with spawn(spin, stop.share())?:
            if i == 0:
                continue
            stop.cancel()
    Ok(())
"#,
        &[JOINS],
    );
}

#[test]
fn a_failed_later_start_is_rejected() {
    rejects(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())? as a, spawn(quick)? as b:
        stop.cancel()
    Ok(())
"#,
        &[JOINS, "`?` leaves the block here"],
    );
}

#[test]
fn a_guard_before_the_task_is_rejected() {
    rejects(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with stop.cancel_on_exit(), spawn(spin, stop.share())?:
        fails()?
    Ok(())
"#,
        &[
            JOINS,
            "put `stop.cancel_on_exit()` directly after each task that holds it",
            "`stop.cancel_on_exit()` cancels it here, too late",
        ],
    );
}

#[test]
fn an_explicit_join_before_the_cancel_is_rejected() {
    rejects(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())? as task, stop.cancel_on_exit():
        task.join()
        stop.cancel()
    Ok(())
"#,
        &[
            "this join waits for a task that holds `stop` before this function cancels it",
            "cancel `stop` before joining",
        ],
    );
}

#[test]
fn pool_shutdown_and_result_before_the_cancel_are_rejected() {
    rejects(
        r#"
def main() -> Result[(), AllocError | PoolError | SubmitError[()] | str]:
    stop = CancellationToken()?
    with ThreadPoolExecutor(1, 1)? as pool:
        job = pool.submit(spin, stop.share()).drop_job()?
        fails()?
        stop.cancel()
    Ok(())
"#,
        &[
            "shuts down a pool running a job that holds `stop` before this function cancels it",
            "the job is submitted here",
            "`with ThreadPoolExecutor(...)? as pool, stop.cancel_on_exit():`",
        ],
    );
    rejects(
        r#"
def main() -> Result[(), AllocError | PoolError | SubmitError[()] | FutureError | str]:
    stop = CancellationToken()?
    with ThreadPoolExecutor(1, 1)? as pool, stop.cancel_on_exit():
        job = pool.submit(spin, stop.share()).drop_job()?
        job.result()?
        stop.cancel()
    Ok(())
"#,
        &[
            "this result() waits for a job that holds `stop` before this function cancels it",
            "cancel `stop` before waiting for the result",
        ],
    );
}

#[test]
fn a_worker_that_skips_its_final_cancel_is_rejected() {
    rejects(
        r#"
def client(stop: CancellationToken) -> Result[(), str]:
    fails()?
    stop.cancel()
    Ok(())

def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(client, stop.share())? as task:
        stop.wait()
        task.join()?
    Ok(())
"#,
        &[
            "`?` leaves this function without cancelling `stop`, which its normal end cancels",
            "putting the body in `with stop.cancel_on_exit():`",
        ],
    );
}

#[test]
fn guarded_tasks_pools_and_workers_are_accepted() {
    accepted(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())?, stop.cancel_on_exit():
        fails()?
    with spawn(watch, &stop)?, stop.cancel_on_exit():
        fails()?
    Ok(())
"#,
    );
    accepted(
        r#"
def main() -> Result[(), AllocError | PoolError | SubmitError[()] | FutureError | str]:
    stop = CancellationToken()?
    with ThreadPoolExecutor(1, 1)? as pool, stop.cancel_on_exit():
        job = pool.submit(spin, stop.share()).drop_job()?
        fails()?
        stop.cancel()
        job.result()?
    Ok(())
"#,
    );
    // Two tasks hold `stop`: a guard after each cancels before each join.
    accepted(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())?, stop.cancel_on_exit(), spawn(spin, stop.share())?, stop.cancel_on_exit():
        fails()?
    Ok(())
"#,
    );
    // A nested guard ends before the join inside the outer block.
    accepted(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())? as task:
        with stop.cancel_on_exit():
            fails()?
        task.join()
    Ok(())
"#,
    );
    accepted(
        r#"
def client(stop: CancellationToken) -> Result[(), str]:
    with stop.cancel_on_exit():
        fails()?
    Ok(())

def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(client, stop.share())? as task:
        stop.wait()
        task.join()?
    Ok(())
"#,
    );
}

#[test]
fn unrelated_or_missing_cancels_are_accepted() {
    // The function never cancels `stop`, so an early exit skips nothing.
    accepted(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(quick)?:
        fails()?
    Ok(())
"#,
    );
    // The later cancel is of a token that the task does not hold.
    accepted(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    other = CancellationToken()?
    with spawn(spin, stop.share())? as task, stop.cancel_on_exit():
        fails()?
    with spawn(quick)?:
        fails()?
        other.cancel()
    Ok(())
"#,
    );
    // A cancel already made before the exit is enough.
    accepted(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())?:
        stop.cancel()
        fails()?
        stop.cancel()
    Ok(())
"#,
    );
    // A worker whose cancel is conditional is not held to it.
    accepted(
        r#"
def worker(stop: CancellationToken, n: i64) -> Result[(), str]:
    fails()?
    if n > 0:
        stop.cancel()
    Ok(())

def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    worker(stop.share(), 1)?
    Ok(())
"#,
    );
}

#[test]
fn cancel_on_exit_is_a_with_guard_with_no_value() {
    let error = rejected(&program(
        r#"
def main() -> Result[(), AllocError]:
    stop = CancellationToken()?
    stop.cancel_on_exit()
    Ok(())
"#,
    ));
    assert!(
        error.contains("cancel_on_exit() is a `with` guard: write `with token.cancel_on_exit():`"),
        "{error}"
    );
    let error = rejected(&program(
        r#"
def main() -> Result[(), AllocError]:
    stop = CancellationToken()?
    with stop.cancel_on_exit() as guard:
        pass
    Ok(())
"#,
    ));
    assert!(
        error.contains("cancel_on_exit() has no value to bind; remove `as`"),
        "{error}"
    );
}

#[test]
fn guarded_error_paths_do_not_hang() {
    fails_promptly(
        r#"
def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(spin, stop.share())?, stop.cancel_on_exit():
        fails()?
    Ok(())
"#,
    );
    fails_promptly(
        r#"
def main() -> Result[(), AllocError | PoolError | SubmitError[()] | FutureError | str]:
    stop = CancellationToken()?
    with ThreadPoolExecutor(1, 1)? as pool, stop.cancel_on_exit():
        job = pool.submit(spin, stop.share()).drop_job()?
        fails()?
        stop.cancel()
        job.result()?
    Ok(())
"#,
    );
    fails_promptly(
        r#"
def client(stop: CancellationToken) -> Result[(), str]:
    with stop.cancel_on_exit():
        fails()?
    Ok(())

def main() -> Result[(), AllocError | ThreadError | str]:
    stop = CancellationToken()?
    with spawn(client, stop.share())? as task:
        stop.wait()
        task.join()?
    Ok(())
"#,
    );
}
