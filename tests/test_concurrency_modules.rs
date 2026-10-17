//! Explicit imports retain the prelude's identities and safety checks.
use std::process::Command;

mod support;

fn source_file(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    (dir, path)
}

fn runs(source: &str, expected: &str) {
    let (dir, path) = source_file(source);
    let exe = dir.path().join("program");
    plenty::compile_file_to_executable(&path, &exe, None).unwrap();
    let output = Command::new("timeout")
        .args(["30s"])
        .arg(exe)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let output = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(output, expected.trim());
}

fn rejected(source: &str, expected: &str) {
    let (_dir, path) = source_file(source);
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(error.contains(expected), "expected {expected}: {error}");
}

#[test]
fn aliases_and_qualified_imports_share_builtin_types_and_operations() {
    runs(
        r#"
import std.channel as ch
import std.thread
import std.concurrent as jobs
import std.sync as sync
from std.channel import channel as bounded, Sender as Outbox, Receiver, SendError as SendFailure
from std.thread import spawn
from std.concurrent import ThreadPoolExecutor, Future as Pending
from std.sync import CancellationToken

def publish(out: Outbox[str]) -> ():
    out.send("owned").unwrap()

def answer() -> str:
    return "answer"

def take(inbox: &Receiver[str]) -> str:
    inbox.recv().unwrap()

def main() -> Result[(), Failure]:
    sender, receiver = bounded[str](1)?
    with std.thread.spawn(publish, sender)?, receiver as inbox:
        print(take(&inbox))?
    left, right = ch.channel[i64](1)?
    left.send(7)?
    print(right.recv()?)?
    failure: SendError[i64] = SendFailure[i64].Full(9)
    match failure:
        case ch.SendError.Full(value):
            print(value)?
        case ch.SendError.Disconnected(value):
            print(value)?
    token: CancellationToken = sync.CancellationToken()?
    shared = token.share()
    shared.cancel()
    print(token.is_cancelled())?
    with ThreadPoolExecutor(1, 1)? as pool:
        future: Pending[str] = pool.submit(answer)?
        print(future.result()?)?
    with jobs.ThreadPoolExecutor(1, 1)? as pool:
        print(pool.submit(answer)?.result()?)?
    with spawn(answer)? as task:
        print(task.join())?
    Ok(())
"#,
        "owned\n7\n9\nTrue\nanswer\nanswer\nanswer",
    );
}

#[test]
fn channel_bounds_disconnect_selection_and_deadlines_keep_their_errors() {
    runs(
        r#"
from std.channel import channel, select_recv_nowait as select, select_recv_timeout
from std.channel import SendError, RecvError, RecvTimeoutError, SelectError
from std.time import deadline_at, instant_at

def main() -> Result[(), Failure]:
    left, inbox = channel[i64](1)?
    other, second = channel[i64](1)?
    left.send(1)?
    match left.send_nowait(2):
        case Err(SendError.Full(value)):
            print(value)?
        case _:
            print("wrong")?
    match select(&inbox, &second):
        case Ok(Selected.First(value)):
            print(value)?
        case _:
            print("wrong")?
    deadline = deadline_at(&instant_at(0))
    timeout = deadline.remaining(&instant_at(1)).as_timeout_ms()
    print(inbox.recv_timeout(timeout) == Result[i64, RecvTimeoutError].Err(RecvTimeoutError.TimedOut))?
    print(select_recv_timeout(&inbox, &second, timeout) == Result[Selected[i64, i64], SelectError].Err(SelectError.TimedOut))?
    drop(left)
    print(inbox.recv() == Result[i64, RecvError].Err(RecvError.Disconnected))?
    drop(other)
    Ok(())
"#,
        "2\n1\nTrue\nTrue\nTrue",
    );
}

#[test]
fn cancellation_can_be_shared_with_owned_jobs_without_new_worker_effects() {
    runs(
        r#"
from std.sync import CancellationToken as Token
from std.concurrent import ThreadPoolExecutor as Pool
from std.thread import spawn as start

def wait(token: Token) -> bool:
    token.wait()
    token.is_cancelled()

def main() -> Result[(), Failure]:
    token = Token()?
    child = token.share()
    with start(wait, child)? as task:
        token.cancel()
        print(task.join())?
    with Pool(1, 1)? as pool:
        shared = token.share()
        job = def [shared]() -> bool:
            shared.wait_timeout(0)
        future = pool.submit(job)?
        print(future.result()?)?
    Ok(())
"#,
        "True\nTrue",
    );
}

#[test]
fn aliases_preserve_both_channel_join_hang_diagnostics() {
    for (worker, endpoint, expected) in [
        ("def worker(rx: Receiver[i64]) -> ():\n    drop(rx.recv().unwrap())\n", "rx", "could block the worker's receive"),
        ("def worker(tx: Sender[i64]) -> ():\n    tx.send(1).unwrap()\n    tx.send(2).unwrap()\n", "tx", "could block the worker's send"),
    ] {
        let source = format!("from std.thread import spawn as start\nfrom std.channel import channel as bounded, Sender, Receiver\n{worker}\ndef main() -> Result[(), Failure]:\n    tx, rx = bounded[i64](1)?\n    with start(worker, {endpoint})? as task:\n        task.join()\n    Ok(())\n");
        rejected(&source, expected);
    }
}

#[test]
fn aliases_preserve_future_result_hang_diagnostic() {
    rejected(
        r#"
import std.concurrent as work
import std.channel as pipe
def produce(sender: pipe.Sender[i64]) -> ():
    sender.send(1).unwrap()
    sender.send(2).unwrap()
def main() -> Result[(), Failure]:
    sender, receiver = pipe.channel[i64](1)?
    with work.ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        future.result()?
        drop(receiver)
    Ok(())
"#,
        "this result() waits for a job",
    );
}

#[test]
fn imported_operations_still_reject_borrowed_jobs_and_uncertified_effects() {
    rejected(
        r#"
from std.concurrent import ThreadPoolExecutor as Pool
def read(value: &i64) -> i64:
    *value
def main() -> Result[(), Failure]:
    value = 1
    with Pool(1, 1)? as pool:
        pool.submit(read, &value)?
    Ok(())
"#,
        "borrowed work belongs in scoped spawn",
    );
    let (_dir, path) = source_file(
        r#"
from std.thread import spawn as start
from std.sync import CancellationToken as Token
from native import foreign_call
def worker(token: Token) -> i64:
    token.cancel()
    foreign_call()
def main() -> Result[(), Failure]:
    with start(worker, Token()?)? as task:
        task.join()
    Ok(())
"#,
    );
    std::fs::write(
        path.with_file_name("native.plentyi"),
        "pub extern def foreign_call() -> i64 = \"external\"\n",
    )
    .unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(
        error.contains("foreign calls have no worker-thread effect contract"),
        "{error}"
    );
}

#[test]
fn imports_do_not_override_unrelated_bindings_or_allow_reexports() {
    for (source, expected) in [
        ("from std.sync import CancellationToken as print\ndef main() -> ():\n    pass\n", "conflicts with an existing name"),
        ("from std.sync import CancellationToken as Token\nclass Token:\n    value: i64\ndef main() -> ():\n    pass\n", "conflicts with an existing name"),
        ("pub from std.thread import spawn\ndef main() -> ():\n    pass\n", "re-exports are not supported"),
        ("from std.sync import Mutex\ndef main() -> ():\n    pass\n", "not a declared export"),
    ] {
        rejected(source, expected);
    }
    runs("from std.sync import CancellationToken as Token\ndef main() -> ():\n    Token = 7\n    print(Token).unwrap()\n", "7");
}

#[test]
fn imported_tokens_keep_loan_and_move_checks() {
    rejected(
        r#"
from std.sync import CancellationToken as Token
from std.thread import spawn as start
def wait(token: &Token) -> ():
    token.wait()
def main() -> Result[(), Failure]:
    token = Token()?
    with start(wait, &token)? as task:
        drop(token)
        task.join()
    Ok(())
"#,
        "conflicting borrow",
    );
    rejected(
        r#"
from std.sync import CancellationToken as Token
def main() -> Result[(), Failure]:
    token = Token()?
    moved = token
    token.cancel()
    Ok(())
"#,
        "moved",
    );
}

#[test]
fn imports_across_files_share_identity_without_reexporting_private_names() {
    let (dir, path) = source_file(
        r#"
from std.sync import CancellationToken as Token
import bridge
def main() -> Result[(), Failure]:
    token: Token = bridge.make()?
    token.cancel()
    print(bridge.cancelled(&token))?
    Ok(())
"#,
    );
    std::fs::write(
        dir.path().join("bridge.plenty"),
        r#"
import std.sync as sync
pub def make() -> Result[sync.CancellationToken, AllocError]:
    sync.CancellationToken()
pub def cancelled(token: &sync.CancellationToken) -> bool:
    token.is_cancelled()
"#,
    )
    .unwrap();
    let exe = dir.path().join("program");
    plenty::compile_file_to_executable(&path, &exe, None).unwrap();
    let output = Command::new("timeout")
        .arg("30s")
        .arg(exe)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("True"));
    std::fs::write(
        &path,
        "from bridge import sync\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(error.contains("not a declared export"), "{error}");
    std::fs::write(&path, "import std.sync\ndef main() -> ():\n    pass\n").unwrap();
    std::fs::create_dir(dir.path().join("std")).unwrap();
    std::fs::write(
        dir.path().join("std/sync.plenty"),
        "pub def replacement() -> ():\n    pass\n",
    )
    .unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(error.contains("reserved"), "{error}");
}

#[test]
fn relocated_compiler_imports_facilities_offline() {
    let dir = tempfile::tempdir().unwrap();
    let compiler = dir.path().join("plenty");
    support::install_executable(
        std::path::Path::new(env!("CARGO_BIN_EXE_plenty")),
        &compiler,
    );
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, "from std.thread import spawn as start\nfrom std.sync import CancellationToken as Token\nimport std.channel as ch\nimport std.concurrent as jobs\ndef main() -> Result[(), Failure]:\n    token = Token()?\n    token.cancel()\n    print(token.is_cancelled())?\n    Ok(())\n").unwrap();
    let output = Command::new("timeout")
        .arg("30s")
        .arg(compiler)
        .arg(&path)
        .current_dir(dir.path())
        .env("CARGO_HOME", dir.path().join("absent"))
        .env("RUSTUP_HOME", dir.path().join("absent"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("True"));
}
