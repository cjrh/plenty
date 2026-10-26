//! Linux filesystem notifications: bounded batches, lifecycle, byte paths,
//! readiness and fallible ownership. Each program has its own working dir.
#![cfg(target_os = "linux")]

mod support;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

const HEADER: &str = r#"
import std.fs
import std.fs.watch
import std.path
import std.ready
from std.fs.watch import WatchError
from std.ready import WaitError
from std.sync import CancellationToken
from std.time import deadline_at, monotonic_now
"#;

fn run(body: &str) -> String {
    let lines: String = body.lines().map(|line| format!("    {line}\n")).collect();
    let source = format!(
        "{HEADER}\ndef main() -> Result[(), IoError | AllocError | WatchError | WaitError]:\n{lines}    Ok(())\n"
    );
    run_source(&source)
}

fn run_source(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source_path = support::write_main(dir.path(), source);
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&source_path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(executable)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn worker_ownership_and_drop_release_descriptors() {
    let source = format!(
        r#"{HEADER}
def descriptors() -> Result[i64, IoError]:
    mut entries = std.fs.read_dir(&std.path.from_text("/proc/self/fd")?)?
    mut count = 0
    while entries.next()?.is_some():
        count += 1
    Ok(count)

def observe(watcher: std.fs.watch.Watcher, path: std.path.Path) -> bool:
    mut owned = watcher
    first = owned.add(&path).unwrap()
    second = owned.add(&path).unwrap()
    owned.poll().unwrap() == 0 and first == second

def main() -> Result[(), IoError | AllocError | WatchError | ThreadError]:
    before = descriptors()?
    for index in range(20):
        watcher = std.fs.watch.watcher()?
        with spawn(observe, watcher, std.path.from_text(".")?)? as task:
            if not task.join():
                print("unexpected")?
    print(descriptors()? == before)?
    Ok(())
"#
    );
    assert_eq!(run_source(&source), "True");
}

#[test]
fn changes_include_rename_cookies_and_are_not_recursive() {
    assert_eq!(
        run(r#"
mut watcher = std.fs.watch.watcher()?
path = std.path.from_text(".")?
id = watcher.add(&path)?
print(watcher.add(&path)? == id)?
root = std.fs.root(&path)?
mut file = open("alpha", "w")?
file.write("hello")?
file.close()?
root.move(&std.path.from_text("alpha")?, &std.path.from_text("beta")?)?
root.remove_file(&std.path.from_text("beta")?)?
mut created = False
mut modified = False
mut closed = False
mut deleted = False
mut from_cookie = 0u32
mut to_cookie = 0u32
count = watcher.poll()?
for index in range(count):
    event = watcher.event(index)?
    if event.watch != id or event.is_directory():
        print("wrong watch")?
    if event.created():
        created = event.name.to_text()? == "alpha"
    if event.modified():
        modified = True
    if event.closed_after_write():
        closed = True
    if event.deleted():
        deleted = event.name.to_text()? == "beta"
    if event.moved_from():
        from_cookie = event.cookie
    if event.moved_to():
        to_cookie = event.cookie
print(created, modified, closed, deleted, from_cookie != 0u32 and from_cookie == to_cookie)?
root.create_dir(&std.path.from_text("child")?)?
print(watcher.poll()? == 1, watcher.event(0)?.is_directory())?
mut nested = open("child/inside", "w")?
nested.write("unwatched")?
nested.close()?
print(watcher.poll()?)?
"#),
        "True\nTrue True True True True\nTrue True\n0"
    );
}

#[test]
fn small_batches_keep_remaining_events_and_native_filename_bytes() {
    assert_eq!(
        run(r#"
mut watcher = std.fs.watch.watcher(272)?
path = std.path.from_text(".")?
watcher.add(&path)?
for index in range(40):
    mut file = open(str.from(index)?, "w")?
    file.close()?
mut names = set[str].with_capacity(40)?
mut batches = 0
while True:
    count = watcher.poll()?
    if count == 0:
        break
    if count > 17:
        print("unbounded")?
    batches += 1
    for index in range(count):
        event = watcher.event(index)?
        if event.created():
            names.add(event.name.to_text()?)?
print(len(names), batches > 1)?
raw = std.path.from_bytes([255u8]?)
root = std.fs.root(&path)?
root.create_dir(&raw)?
print(watcher.poll()?)?
saved = watcher.event(0)?
print(saved.name.as_bytes() == raw.as_bytes(), saved.created(), saved.is_directory())?
watcher.close()?
print(saved.name.as_bytes() == raw.as_bytes())?
"#),
        "40 True\n1\nTrue True True\nTrue"
    );
}

#[test]
fn watch_lifecycle_and_invalid_inputs_are_explicit() {
    assert_eq!(
        run(r#"
for size in [0, 271, 65537]?:
    match std.fs.watch.watcher(size):
        case Err(WatchError.OutOfRange):
            print("range")?
        case _:
            print("unexpected")?
mut watcher = std.fs.watch.watcher()?
for path in [std.path.from_bytes([0u8]?), std.path.from_text("missing")?]?:
    match watcher.add(&path):
        case Err(WatchError.Io(_)):
            print("io")?
        case _:
            print("unexpected")?
mut file = open("owned", "w")?
file.close()?
path = std.path.from_text("owned")?
id = watcher.add(&path)?
root = std.fs.root(&std.path.from_text(".")?)?
root.remove_file(&path)?
count = watcher.poll()?
mut deleted = False
mut ignored = False
for index in range(count):
    event = watcher.event(index)?
    if event.watch == id and len(event.name.as_bytes()) == 0:
        deleted = deleted or event.deleted_self()
        ignored = ignored or event.ignored()
print(deleted, ignored)?
dir_id = watcher.add(&std.path.from_text(".")?)?
watcher.remove(dir_id)?
print(watcher.poll()? == 1, watcher.event(0)?.ignored())?
match watcher.event(1):
    case Err(WatchError.OutOfRange):
        print("index")?
    case _:
        print("unexpected")?
watcher.close()?
match watcher.poll():
    case Err(WatchError.Io(_)):
        print("closed")?
    case _:
        print("unexpected")?
print(watcher.count())?
"#),
        "range\nrange\nrange\nio\nio\nTrue True\nTrue True\nindex\nclosed\n0"
    );
}

#[test]
fn deadlines_cancellation_and_selectors_reuse_existing_readiness() {
    assert_eq!(
        run(r#"
mut watcher = std.fs.watch.watcher()?
watcher.add(&std.path.from_text(".")?)?
token = CancellationToken()?
deadline = deadline_at(&monotonic_now())
match watcher.wait(&deadline, &token):
    case Err(WatchError.Wait(WaitError.TimedOut)):
        print("timeout")?
    case _:
        print("unexpected")?
mut selector = std.ready.selector(1)?
watcher.register(&mut selector, 42u64)?
mut file = open("notice", "w")?
file.close()?
print(selector.wait(&deadline, &token)?, selector.event(0)?.key)?
print(watcher.wait(&deadline, &token)? > 0)?
token.cancel()
match watcher.wait(&deadline, &token):
    case Err(WatchError.Wait(WaitError.Cancelled)):
        print("cancelled")?
    case _:
        print("unexpected")?
watcher.unregister(&mut selector)?
watcher.close()?
print(watcher.count())?
"#),
        "timeout\n1 42\nTrue\ncancelled\n0"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn polling_does_not_allocate_and_failed_event_copy_can_retry() {
    assert_eq!(
        run(r#"
mut watcher = std.fs.watch.watcher()?
path = std.path.from_text(".")?
watcher.add(&path)?
mut file = open("event-name", "w")?
file.close()?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
count = watcher.poll()?
print("__test_end_no_allocations__")?
failure = watcher.event(0)
add_failure = watcher.add(&path)
print("__test_restore_allocations__")?
print(count > 0, watcher.count() == count)?
match failure:
    case Err(WatchError.Allocation(_)):
        print("copy allocation")?
    case _:
        print("unexpected")?
match add_failure:
    case Err(WatchError.Io(_)):
        print("add allocation")?
    case _:
        print("unexpected")?
print(watcher.event(0)?.name.to_text()?)?
for index in range(12):
    number = str.from(index)?
    prefix = ("__test_fail_allocations_after_" + number)?
    marker = (prefix + "__")?
    print(marker)?
    result = std.fs.watch.watcher()
    print("__test_restore_allocations__")?
    drop(result)
print("done")?
"#),
        "True True\ncopy allocation\nadd allocation\nevent-name\ndone"
    );
}

#[test]
fn kernel_queue_overflow_is_a_visible_event() {
    let limit: usize = std::fs::read_to_string("/proc/sys/fs/inotify/max_queued_events")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // Do not create millions of filesystem objects on hosts configured for
    // unusually large production queues. Ordinary Linux defaults are 16,384.
    if limit > 65_536 {
        eprintln!("overflow probe skipped: host queue limit is {limit}");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = format!(
        r#"{HEADER}
def main() -> Result[(), IoError | WatchError]:
    mut watcher = std.fs.watch.watcher()?
    watcher.add(&std.path.from_text(".")?)?
    print("ready")?
    flush_stdout()?
    drop(input()?)
    mut overflow = False
    while True:
        count = watcher.poll()?
        if count == 0:
            break
        for index in range(count):
            event = watcher.event(index)?
            if event.overflowed():
                overflow = event.watch == -1i32 and len(event.name.as_bytes()) == 0
    print(overflow)?
    Ok(())
"#
    );
    let path = support::write_main(dir.path(), &source);
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&path, &executable, None).unwrap();
    let mut child = Command::new(executable)
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line, "ready\n");
    let event_path = dir.path().join("changed");
    // Alternating create/delete records cannot be coalesced into one event.
    for _ in 0..=limit / 2 {
        std::fs::create_dir(&event_path).unwrap();
        std::fs::remove_dir(&event_path).unwrap();
    }
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    line.clear();
    stdout.read_line(&mut line).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(line, "True\n");
}
