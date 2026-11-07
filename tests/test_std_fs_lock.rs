//! Owned flock locks: actual competing processes and deterministic release.
#![cfg(target_os = "linux")]
mod support;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HEADER: &str = r#"
import std.fs.lock as lock
import std.path as path
import std.time
import std.test
from std.fs.lock import Mode, LockError
from std.sync import CancellationToken
from std.time import deadline_after, deadline_at, milliseconds, monotonic_now
"#;

fn compile(dir: &Path, name: &str, body: &str, optimize: bool) -> PathBuf {
    let source = dir.join(format!("{name}.plenty"));
    std::fs::write(&source, format!("{HEADER}\n{body}")).unwrap();
    let exe = dir.join(name);
    plenty::compile_file_to_executable_with_options(
        &source,
        &exe,
        None,
        &plenty::CompileOptions {
            optimize,
            test_hooks: true,
            ..Default::default()
        },
    )
    .unwrap_or_else(|error| panic!("{body}\n{error}"));
    exe
}

fn output(exe: &Path, dir: &Path) -> String {
    let out = Command::new(exe).current_dir(dir).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}

#[test]
fn shared_and_exclusive_contention_close_and_owner_death() {
    let dir = tempfile::tempdir().unwrap();
    for optimize in [false, true] {
        let probe = compile(
            dir.path(),
            "probe",
            r#"
def main() -> Result[(), IoError]:
    p = path.from_text("resource.lock")?
    shared = lock.acquire_now(&p, Mode.Shared).is_ok()
    exclusive = lock.acquire_now(&p, Mode.Exclusive).is_ok()
    print(shared, exclusive)?
    Ok(())
"#,
            optimize,
        );
        for mode in ["Shared", "Exclusive"] {
            let holder = compile(
                dir.path(),
                "holder",
                &format!(
                    r#"
def main() -> Result[(), IoError | LockError]:
    mut owner = lock.acquire_now(&path.from_text("resource.lock")?, Mode.{mode})?
    print("ready")?
    flush_stdout()?
    drop(input()?)
    owner.close()?
    owner.close()?
    print(owner.is_closed())?
    Ok(())
"#
                ),
                optimize,
            );
            for kill in [false, true] {
                let mut child = Command::new(&holder)
                    .current_dir(dir.path())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let mut stdout = BufReader::new(child.stdout.take().unwrap());
                let mut line = String::new();
                stdout.read_line(&mut line).unwrap();
                assert_eq!(line.trim(), "ready");
                assert_eq!(
                    output(&probe, dir.path()),
                    if mode == "Shared" {
                        "True False"
                    } else {
                        "False False"
                    }
                );
                if kill {
                    child.kill().unwrap();
                    child.wait().unwrap();
                } else {
                    child.stdin.take().unwrap().write_all(b"\n").unwrap();
                    line.clear();
                    stdout.read_line(&mut line).unwrap();
                    assert_eq!(line.trim(), "True");
                    assert!(child.wait().unwrap().success());
                }
                assert_eq!(output(&probe, dir.path()), "True True");
            }
        }
    }
}

#[test]
fn deadlines_cancellation_worker_drop_and_failures() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile(
        dir.path(),
        "wait",
        r#"
def cancel_later(token: &CancellationToken) -> ():
    std.time.sleep(&milliseconds(20))
    token.cancel()

def acquire_in_worker(p: &path.Path) -> Result[lock.Lock, LockError]:
    lock.acquire_now(p, Mode.Exclusive)

def release(owner: lock.Lock) -> ():
    std.time.sleep(&milliseconds(20))
    drop(owner)

def main() -> Result[(), IoError | AllocError | LockError | RangeError | SpawnError[()] | std.test.TestError]:
    p = path.from_text("resource.lock")?
    owner = lock.acquire_now(&p, Mode.Exclusive)?
    token = CancellationToken()?
    now = monotonic_now()
    match lock.acquire_before(&p, Mode.Exclusive, &deadline_after(&now, &milliseconds(20))?, &token):
        case Err(LockError.TimedOut):
            print("timeout")?
        case _:
            std.test.check(False, "unexpected lock result")?
    with spawn(cancel_later, &token)? as task:
        match lock.acquire_before(&p, Mode.Shared, &deadline_after(&monotonic_now(), &milliseconds(5000))?, &token):
            case Err(LockError.Cancelled):
                print("cancelled")?
            case _:
                std.test.check(False, "unexpected lock result")?
        task.join()
    fresh = CancellationToken()?
    with spawn(release, owner).drop_job()? as task:
        waiting = lock.acquire_before(&p, Mode.Shared, &deadline_after(&monotonic_now(), &milliseconds(5000))?, &fresh)?
        drop(waiting)
        task.join()
    match lock.acquire_before(&path.from_text("cancelled-no-file")?, Mode.Shared, &deadline_at(&monotonic_now()), &token):
        case Err(LockError.Cancelled):
            print("pre-cancelled")?
        case _:
            std.test.check(False, "unexpected lock result")?
    mut again = lock.acquire_now(&p, Mode.Exclusive)?
    again.close()?
    with spawn(acquire_in_worker, &p)? as task:
        acquired = task.join()?
        std.test.check(lock.acquire_now(&p, Mode.Shared).is_err(), "worker result must retain lock")?
        drop(acquired)
    match lock.acquire_before(&p, Mode.Shared, &deadline_at(&monotonic_now()), &fresh):
        case Err(LockError.TimedOut):
            print("expired")?
        case _:
            std.test.check(False, "unexpected lock result")?
    print(lock.acquire_now(&path.from_text("")?, Mode.Shared).is_err())?
    print(lock.acquire_now(&path.from_text("bad\0path")?, Mode.Shared).is_err())?
    print(lock.acquire_now(&path.from_text(".")?, Mode.Shared).is_err())?
    print(lock.acquire_now(&path.from_text("link")?, Mode.Shared).is_err())?
    print(lock.acquire_now(&path.from_text("fifo")?, Mode.Shared).is_err())?
    Ok(())
"#,
        true,
    );
    std::os::unix::fs::symlink("resource.lock", dir.path().join("link")).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(dir.path().join("fifo"))
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        output(&exe, dir.path()),
        "timeout\ncancelled\npre-cancelled\nexpired\nTrue\nTrue\nTrue\nTrue\nTrue"
    );
    assert!(!dir.path().join("cancelled-no-file").exists());
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_releases_opened_owners() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile(
        dir.path(),
        "allocation",
        r#"
def main() -> Result[(), IoError | AllocError | LockError | std.test.TestError]:
    p = path.from_text("resource.lock")?
    mut saw_failure = False
    for number in ["0", "1", "2", "3", "4", "5", "6", "7"]?:
        prefix = ("__test_fail_allocations_after_" + number)?
        marker = (prefix + "__")?
        print(marker)?
        result = lock.acquire_now(&p, Mode.Exclusive)
        print("__test_restore_allocations__")?
        match result:
            case Ok(owner):
                drop(owner)
            case Err(LockError.Allocation(_)):
                saw_failure = True
            case Err(_):
                std.test.check(False, "unexpected lock result")?
        # Either success or any failed allocation must have released the fd
        # and flock before a fresh owner can acquire the same inode.
        owner = lock.acquire_now(&p, Mode.Exclusive)?
        print("__test_begin_no_allocations__")?
        drop(owner)
        print("__test_end_no_allocations__")?
    std.test.check(saw_failure, "failure injection must exercise an allocation error")?
    print("ok")?
    Ok(())
"#,
        true,
    );
    let out = output(&exe, dir.path());
    let lines: Vec<_> = out
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect();
    assert_eq!(lines, ["ok"]);
}

#[test]
fn interrupted_attempts_retry_with_deadline_and_cancellation_checks() {
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("flock.c");
    let object = dir.path().join("flock.o");
    std::fs::write(
        &wrapper,
        r#"
#include <errno.h>
int __real_flock(int, int);
static unsigned calls;
int __wrap_flock(int fd, int operation) {
    if (calls < 6) ++calls;
    if (calls == 5) return __real_flock(fd, operation);
    errno = EINTR;
    return -1;
}
"#,
    )
    .unwrap();
    assert!(
        Command::new("cc")
            .args(["-c", "-O2"])
            .arg(&wrapper)
            .arg("-o")
            .arg(&object)
            .status()
            .unwrap()
            .success()
    );
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, format!(r#"{HEADER}
def cancel_later(token: &CancellationToken) -> ():
    std.time.sleep(&milliseconds(20))
    token.cancel()

def main() -> Result[(), IoError | LockError | AllocError | RangeError | std.test.TestError | SpawnError[()]]:
    p = path.from_text("interrupted.lock")?
    match lock.acquire_now(&p, Mode.Exclusive):
        case Err(LockError.Io(IoError.System(4i32))):
            print("now interrupted")?
        case _:
            std.test.check(False, "nonblocking acquisition must preserve EINTR")?
    cancel = CancellationToken()?
    owner = lock.acquire_before(&p, Mode.Exclusive, &deadline_after(&monotonic_now(), &milliseconds(5000))?, &cancel)?
    drop(owner)
    print("retried")?
    match lock.acquire_before(&p, Mode.Exclusive, &deadline_after(&monotonic_now(), &milliseconds(20))?, &cancel):
        case Err(LockError.TimedOut):
            print("timeout")?
        case _:
            std.test.check(False, "EINTR retry must check deadline")?
    with spawn(cancel_later, &cancel)? as task:
        match lock.acquire_before(&p, Mode.Exclusive, &deadline_after(&monotonic_now(), &milliseconds(5000))?, &cancel):
            case Err(LockError.Cancelled):
                print("cancelled")?
            case _:
                std.test.check(False, "EINTR retry must check cancellation")?
        task.join()
    Ok(())
"#)).unwrap();
    let exe = dir.path().join("program");
    plenty::compile_file_to_executable_with_options(
        &source,
        &exe,
        None,
        &plenty::CompileOptions {
            optimize: true,
            link_args: vec![object.into_os_string(), "-Wl,--wrap=flock".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        output(&exe, dir.path()),
        "now interrupted\nretried\ntimeout\ncancelled"
    );
}

#[test]
fn explicitly_inherited_lock_survives_runner_death_until_child_closes() {
    let dir = tempfile::tempdir().unwrap();
    for optimize in [false, true] {
        let probe = compile(
            dir.path(),
            "inherit-probe",
            r#"
def main() -> Result[(), IoError]:
    print(lock.acquire_now(&path.from_text("resource.lock")?, Mode.Exclusive).is_ok())?
    Ok(())
"#,
            optimize,
        );
        let holder = compile(
            dir.path(),
            "inherit-holder",
            r#"
import std.process
from std.process import Stdio, ProcessGroup

def main() -> Result[(), IoError | AllocError | LockError]:
    mut owner = lock.acquire_now(&path.from_text("resource.lock")?, Mode.Exclusive)?
    mut built = std.process.command("/usr/bin/python3")?
    built.arg("-c")?
    built.arg("import os,sys,signal; signal.alarm(10); assert os.getsid(0)==os.getpid(); print('child-ready',flush=True); sys.stdin.readline(); os.close(int(sys.argv[1])); print('released',flush=True)")?
    fd = owner.inherit_into(&mut built)?
    built.arg(&str.from(fd)?)?
    built.stderr(Stdio.File(open("child-errors.log", "a")?))
    built.process_group(ProcessGroup.NewSession)
    child = built.spawn()?
    # The child alone must keep the open description locked.
    owner.close()?
    built.clear_inherited_files()
    print("holder-ready")?
    flush_stdout()?
    std.time.sleep(&milliseconds(60000u32))
    # Unreachable in this fixture: the harness kills the runner.
    mut ended = child
    drop(ended.wait()?)
    Ok(())
"#,
            optimize,
        );
        let mut parent = Command::new(&holder)
            .current_dir(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(parent.stdout.take().unwrap()).lines();
        let mut ready = [
            lines.next().unwrap().unwrap(),
            lines.next().unwrap().unwrap(),
        ];
        ready.sort();
        assert_eq!(ready, ["child-ready", "holder-ready"]);
        assert_eq!(output(&probe, dir.path()), "False");
        // Rust Child::wait closes an attached stdin; retain the pipe so the
        // orphan cannot mistake runner death for the explicit release signal.
        let mut release = parent.stdin.take().unwrap();
        parent.kill().unwrap();
        parent.wait().unwrap();
        assert_eq!(output(&probe, dir.path()), "False");
        // The orphan still owns stdin/stdout. Release its lock explicitly;
        // its alarm only bounds fixture failures, not normal lock lifetime.
        release.write_all(b"release\n").unwrap();
        drop(release);
        assert_eq!(lines.next().unwrap().unwrap(), "released");
        assert_eq!(output(&probe, dir.path()), "True");
        assert!(lines.next().is_none());
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn inherited_lock_allocation_failures_preserve_ownership_and_release_copies() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile(
        dir.path(),
        "inherit-allocation",
        r#"
import std.process

def main() -> Result[(), IoError | AllocError | LockError | std.test.TestError]:
    p = path.from_text("resource.lock")?
    mut failures = 0
    mut successes = 0
    for number in ["0", "1", "2", "3", "4", "5", "6", "7"]?:
        mut owner = lock.acquire_now(&p, Mode.Exclusive)?
        mut built = std.process.command("/bin/true")?
        marker = (("__test_fail_allocations_after_" + number)? + "__")?
        print(marker)?
        inherited = owner.inherit_into(&mut built)
        print("__test_restore_allocations__")?
        owner.close()?
        match inherited:
            case Ok(_):
                failures_before_clear = lock.acquire_now(&p, Mode.Exclusive).is_err()
                std.test.check(failures_before_clear, "command copy keeps lock")?
                successes = unchecked:
                    successes + 1
            case Err(LockError.Allocation(_)):
                drop(lock.acquire_now(&p, Mode.Exclusive)?)
                failures = unchecked:
                    failures + 1
            case Err(_):
                std.test.check(False, "unexpected inheritance error")?
        built.clear_inherited_files()
        drop(lock.acquire_now(&p, Mode.Exclusive)?)
        std.test.check(owner.inherit_into(&mut built).is_err(), "closed source rejected")?
    std.test.check(failures > 0 and successes > 0, "sweep covers failure and success")?
    print("ok")?
    Ok(())
"#,
        true,
    );
    let out = output(&exe, dir.path());
    let lines: Vec<_> = out
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect();
    assert_eq!(lines, ["ok"]);
}
