//! Native handles on worker threads and std.ready's bounded waits. Child
//! processes from `/bin/sh` provide local pipes, so no test needs a network.
use std::path::Path;
use std::process::{Command, Stdio};

fn written(dir: &Path, source: &str) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    std::fs::write(&path, source).unwrap();
    path
}

fn runs(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&written(dir.path(), source), &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(executable)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status {:?}; {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn rejected(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let error = plenty::check_file(&written(dir.path(), source), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains(expected), "{error}");
}

const SHOW: &str = r#"
import std.bytes
import std.io
import std.process
from std.io import FileStream
from std.process import Stdio
from std.ready import Interest, Readiness, WaitError, read_before, wait, wait_until, write_before
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now, sleep

def show(result: Result[Readiness, WaitError]) -> Result[(), Failure]:
    match result:
        case Ok(r):
            print(r.readable, r.writable, r.hangup, r.error)?
        case Err(WaitError.TimedOut):
            print("timed out")?
        case Err(WaitError.Cancelled):
            print("cancelled")?
        case Err(_):
            print("stream error")?
    Ok(())

def piped(script: &str) -> Result[std.process.Child, Failure]:
    mut command = std.process.shell(script)?
    command.stdout(Stdio.Piped)
    Ok(command.spawn()?)
"#;

fn with_show(body: &str) -> String {
    format!("{SHOW}{body}")
}

#[test]
fn waits_report_readiness_timeout_hangup_and_cancellation() {
    runs(
        &with_show(
            r#"
def main() -> Result[(), Failure]:
    mut child = piped("sleep 0.2; printf hi")?
    mut out = std.io.from_file(child.take_stdout().unwrap())
    token = CancellationToken()?
    start = monotonic_now()
    short = deadline_after(&start, &milliseconds(50))?
    show(wait(&out.file, Interest.Read, &short, &token))?
    show(wait_until(&out.file, Interest.Read, &short))?
    long = deadline_after(&start, &milliseconds(5000))?
    mut buffer = std.bytes.with_capacity(16)?
    buffer.resize(16, 0u8)?
    match read_before(&mut out, &mut buffer, 0, 16, &long, &token):
        case Ok(n):
            print(n)?
        case Err(_):
            print("read failed")?
    show(wait_until(&out.file, Interest.Read, &long))?
    token.cancel()
    show(wait(&out.file, Interest.Read, &long, &token))?
    child.wait()?
    Ok(())
"#,
        ),
        "timed out\ntimed out\n2\nFalse False True False\ncancelled\n",
    );
}

#[test]
fn writes_wait_for_space_and_closed_handles_are_errors() {
    runs(
        &with_show(
            r#"
def main() -> Result[(), Failure]:
    mut command = std.process.shell("head -c 3 >/dev/null")?
    command.stdin(Stdio.Piped)
    mut child = command.spawn()?
    mut input = std.io.from_file(child.take_stdin().unwrap())
    token = CancellationToken()?
    start = monotonic_now()
    deadline = deadline_after(&start, &milliseconds(5000))?
    show(wait(&input.file, Interest.Write, &deadline, &token))?
    mut source = std.bytes.with_capacity(10000)?
    source.resize(10000, 120u8)?
    print(write_before(&mut input, &source, 0, 10000, &deadline, &token).unwrap())?
    input.file.close()?
    show(wait(&input.file, Interest.Write, &deadline, &token))?
    child.wait()?
    Ok(())
"#,
        ),
        "False True False False\n4096\nstream error\n",
    );
}

#[test]
fn workers_own_and_borrow_handles_through_deadlines_and_cancellation() {
    runs(
        &with_show(
            r#"
# The pipe moves into the worker, which drains it under a deadline.
def drain(stream: FileStream, cancel: CancellationToken) -> Result[i64, WaitError]:
    mut owned = stream
    mut buffer = std.bytes.with_capacity(64).unwrap()
    buffer.resize(64, 0u8).unwrap()
    start = monotonic_now()
    deadline = deadline_after(&start, &milliseconds(5000)).unwrap()
    mut total = 0
    while True:
        count = read_before(&mut owned, &mut buffer, 0, 64, &deadline, &cancel)?
        if count == 0:
            return Ok(total)
        total = total + count
    Ok(total)

def attempt(stream: &mut FileStream, buffer: &mut std.bytes.Bytes, deadline: &Deadline, cancel: &CancellationToken) -> str:
    match read_before(stream, buffer, 0, 4, deadline, cancel):
        case Err(WaitError.TimedOut):
            "timed out"
        case Err(WaitError.Cancelled):
            "cancelled"
        case _:
            "other"

def main() -> Result[(), Failure]:
    mut child = piped("printf abcdef; sleep 0.05; printf ghi")?
    stream = std.io.from_file(child.take_stdout().unwrap())
    cancel = CancellationToken()?
    with spawn(drain, stream, cancel.share())? as task:
        print(task.join().unwrap())?
    child.wait()?

    # The borrowed stream and buffer stay usable after the worker's deadline.
    mut quiet_child = piped("sleep 5")?
    mut quiet = std.io.from_file(quiet_child.take_stdout().unwrap())
    mut buffer = std.bytes.with_capacity(4)?
    buffer.resize(4, 7u8)?
    now = monotonic_now()
    deadline = deadline_after(&now, &milliseconds(30))?
    with spawn(attempt, &mut quiet, &mut buffer, &deadline, &cancel)? as task:
        print(task.join())?
    print(buffer.get(0)?, buffer.get(3)?, quiet.file.closed)?

    # Cancellation from the parent reaches a worker blocked in a long wait.
    stop = CancellationToken()?
    observer = stop.share()
    long = deadline_after(&now, &milliseconds(60000))?
    with spawn(attempt, &mut quiet, &mut buffer, &long, &observer)? as task:
        sleep(&milliseconds(20))
        requested = monotonic_now()
        stop.cancel()
        print(task.join())?
        print(monotonic_now().saturating_duration_since(&requested).as_nanoseconds() < 1000000000u64)?
    # A token cancelled before the wait wins over a ready handle.
    with spawn(attempt, &mut quiet, &mut buffer, &long, &observer)? as task:
        print(task.join())?
    quiet_child.kill()?
    quiet_child.wait()?
    Ok(())
"#,
        ),
        "9\ntimed out\n7 7 False\ncancelled\nTrue\ncancelled\n",
    );
}

#[test]
fn waiting_workers_stay_idle_and_release_every_descriptor() {
    runs(
        &with_show(
            r#"
import std.fs
import std.path

def count(directory: &str) -> Result[i64, Failure]:
    mut entries = std.fs.read_dir(&std.path.from_text(directory)?)?
    mut total = 0
    while entries.next()?.is_some():
        total = total + 1
    Ok(total)

# User plus system CPU ticks. The test program's name has no spaces, so
# fields 14 and 15 of /proc/self/stat are at fixed positions.
def cpu_ticks() -> Result[i64, Failure]:
    stat = read_text("/proc/self/stat")?
    fields = stat.split(" ")?
    Ok(i64.parse(fields[13])? + i64.parse(fields[14])?)

def idle(file: &File, cancel: &CancellationToken) -> bool:
    now = monotonic_now()
    deadline = deadline_after(&now, &milliseconds(400)).unwrap()
    match wait(file, Interest.Read, &deadline, cancel):
        case Err(WaitError.TimedOut):
            True
        case _:
            False

def consume(file: File) -> bool:
    mut owned = file
    owned.close().is_ok()

def main() -> Result[(), Failure]:
    descriptors = count("/proc/self/fd")?
    threads = count("/proc/self/task")?
    mut child = piped("sleep 5")?
    pipe = child.take_stdout().unwrap()
    cancel = CancellationToken()?
    before = cpu_ticks()?
    with spawn(idle, &pipe, &cancel)? as first:
        with spawn(idle, &pipe, &cancel)? as second:
            print(first.join(), second.join())?
    # Two waiters idle for 400 ms each; 10 ms ticks allow scheduler noise.
    print(cpu_ticks()? - before < 10)?
    child.kill()?
    child.wait()?
    for _ in range(20):
        mut short = piped("printf x")?
        with spawn(consume, short.take_stdout().unwrap())? as task:
            task.join()
        short.wait()?
    print(count("/proc/self/fd")? - descriptors, count("/proc/self/task")? - threads)?
    Ok(())
"#,
        ),
        "True True\nTrue\n1 0\n",
    );
}

#[test]
fn workers_may_not_change_the_working_directory_or_hold_foreign_pointers() {
    rejected(
        r#"
import std.env
import std.path
def worker() -> ():
    std.env.set_current_dir(&std.path.from_text("/").unwrap()).unwrap()
def main() -> Result[(), Failure]:
    with spawn(worker)?:
        pass
    Ok(())
"#,
        "changing the working directory affects every thread",
    );
    // A destructor that runs on the worker is certified like its body.
    rejected(
        r#"
import std.env
import std.path
class Resource:
    value: i64
    def __del__(self: &mut Resource) -> ():
        std.env.set_current_dir(&std.path.from_text("/").unwrap()).unwrap()
def worker(value: Resource) -> i64:
    value.value
def main() -> Result[(), Failure]:
    with spawn(worker, Resource(1))?:
        pass
    Ok(())
"#,
        "changing the working directory affects every thread",
    );
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("native.plentyi"),
        "pub opaque Token\npub extern def token() -> Token = \"token\"\n",
    )
    .unwrap();
    let path = root.path().join("main.plenty");
    std::fs::write(
        &path,
        "import native\ndef worker(token: native.Token) -> ():\n    pass\ndef main() -> Result[(), Failure]:\n    with spawn(worker, native.token())?:\n        pass\n    Ok(())\n",
    )
    .unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(
        error.contains("has no cross-thread ownership contract"),
        "{error}"
    );
}

#[test]
fn shared_file_loans_still_exclude_concurrent_mutation() {
    rejected(
        r#"
def reader(file: &mut File) -> ():
    file.read().unwrap()
    pass
def main() -> Result[(), Failure]:
    mut file = open("/dev/null", "r")?
    with spawn(reader, &mut file)?:
        file.close()?
    Ok(())
"#,
        "borrow",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_start_closes_a_moved_handle_once() {
    runs(
        &with_show(
            r#"
import std.fs
import std.path

def count(directory: &str) -> Result[i64, Failure]:
    mut entries = std.fs.read_dir(&std.path.from_text(directory)?)?
    mut total = 0
    while entries.next()?.is_some():
        total = total + 1
    Ok(total)

def consume(file: File) -> bool:
    file.closed

def attempt(file: File) -> Result[bool, Failure]:
    with spawn(consume, file)? as task:
        return Ok(task.join())

def main() -> Result[(), Failure]:
    descriptors = count("/proc/self/fd")?
    mut child = piped("printf x")?
    pipe = child.take_stdout().unwrap()
    print("__test_fail_thread_starts__")?
    print(str.repr(attempt(pipe))?)?
    print("__test_restore_thread_starts__")?
    child.wait()?
    print(count("/proc/self/fd")? - descriptors)?
    Ok(())
"#,
        ),
        "__test_fail_thread_starts__\nResult[bool, Failure].Err(Failure.Unspecified)\n__test_restore_thread_starts__\n0\n",
    );
}
