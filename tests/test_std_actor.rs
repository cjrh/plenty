//! `std.actor`: supervised actors with per-actor heaps. See
//! book/src/design/stdlib/actor.md.
mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::process::{Command, Stdio};
use std::time::Duration;

/// One acceptor and one actor per connection; see the example's header.
const ECHO_SERVER: &str = include_str!("../examples/actor_echo.plenty");

/// Three children of one supervisor: one exhausts its heap, one replies with
/// text built in its own heap and stops, one traps. Then mailbox overflow and
/// sends to a stopped actor.
const SUPERVISOR: &str = include_str!("fixtures/actor/supervise.plenty");

/// Timers, request-reply and restart under one lead actor.
const TIMERS: &str = include_str!("fixtures/actor/timers.plenty");

/// Admission under injected allocation failure.
const ADMISSION: &str = include_str!("fixtures/actor/admit.plenty");

/// One actor watching two sockets; see the fixture's header.
const WATCH: &str = include_str!("fixtures/actor/watch.plenty");

/// Compiles with `--test-hooks`, which the library API does not expose.
fn build_with_hooks(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    support::write_main(dir.path(), source);
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(["--compile", "main.plenty", "--test-hooks", "-o", "program"])
        .current_dir(dir.path())
        .env("TMPDIR", dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let executable = dir.path().join("program");
    (dir, executable)
}

/// Deterministic bytes that never start with `X`.
fn payload(seed: u64, length: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut bytes = Vec::with_capacity(length);
    for _ in 0..length {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        bytes.push((state >> 33) as u8);
    }
    bytes[0] = b'a';
    bytes
}

/// Sends `data` while reading the echo, then returns everything read.
fn echo(port: u16, data: Vec<u8>) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    let sender = std::thread::spawn(move || {
        for chunk in data.chunks(7919) {
            writer.write_all(chunk).unwrap();
        }
        writer.shutdown(Shutdown::Write).unwrap();
    });
    let mut received = Vec::new();
    stream.read_to_end(&mut received).unwrap();
    sender.join().unwrap();
    received
}

#[test]
fn echo_server_serves_many_connections_and_contains_a_trap() {
    const GOOD: usize = 40;
    let source = ECHO_SERVER.replace(
        "const connections: i64 = 3\n",
        &format!("const connections: i64 = {}\n", GOOD + 1),
    );
    assert_ne!(source, ECHO_SERVER);
    let (_dir, executable) = support::build_file(&source);
    let mut child = Command::new(executable)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap());
    let mut port = String::new();
    lines.read_line(&mut port).unwrap();
    let port: u16 = port.trim().parse().unwrap();

    // The faulty connection traps while the others are open; discarding its
    // heap closes its socket.
    let mut bad = TcpStream::connect(("127.0.0.1", port)).unwrap();
    bad.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    bad.write_all(b"Xboom").unwrap();
    let clients: Vec<_> = (0..GOOD)
        .map(|index| {
            std::thread::spawn(move || {
                let data = payload(index as u64, 50_000 + index * 997);
                let received = echo(port, data.clone());
                assert!(
                    received == data,
                    "connection {index} was not echoed exactly"
                );
            })
        })
        .collect();
    let mut rest = Vec::new();
    match bad.read_to_end(&mut rest) {
        Ok(count) => assert_eq!(count, 0),
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset),
    }
    for client in clients {
        client.join().unwrap();
    }
    let mut summary = String::new();
    lines.read_to_string(&mut summary).unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "{status:?}: {stderr}");
    assert!(stderr.contains("unwrap on Nothing"), "{stderr}");
    // The acceptor and every connection started; every heap is gone.
    let prefix = format!(
        "std.actor.Stats(started={}, stopped={}, failed=0, trapped=1, restarted=0,",
        GOOD + 2,
        GOOD + 1
    );
    assert!(summary.starts_with(&prefix), "{summary}");
    assert!(summary.trim_end().ends_with("lost=0) 0 0 0"), "{summary}");
}

#[test]
fn supervisor_receives_errors_traps_and_stops() {
    let (_dir, executable) = support::build_file(SUPERVISOR);
    let output = Command::new(executable).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("unwrap on Nothing"), "{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0 got reply from 12\n\
         down 1 11 std.actor.Exit[AllocError].Failed(AllocError.OutOfMemory)\n\
         down 2 12 std.actor.Exit[AllocError].Stopped\n\
         down 3 13 std.actor.Exit[AllocError].Trapped(std.actor.TrapKind.Unwrap)\n\
         std.actor.Stats(started=4, stopped=2, failed=1, trapped=1, restarted=0, delivered=7, full=0, gone=0, lost=0) 0 0 0\n\
         gone Note.Text(\"late\")\n\
         sent\n\
         full Note.Text(\"second\")\n\
         9 got first\n\
         gone Note.Text(\"third\")\n\
         std.actor.Stats(started=1, stopped=1, failed=0, trapped=0, restarted=0, delivered=1, full=1, gone=1, lost=0) 0 0\n"
    );
}

/// Timer, reply and exit order depends on timing, so lines are compared
/// sorted; each counter's own lines stay in order.
#[test]
fn timers_requests_and_restarts() {
    let (_dir, executable) = support::build_file(TIMERS);
    let output = Command::new(executable).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(stderr.matches("unwrap on Nothing").count(), 3, "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines: Vec<&str> = stdout.lines().collect();
    for ordered in [["tick 1", "tick 2", "tick 3"], ["up 1", "up 2", "up 3"]] {
        let positions: Vec<usize> = ordered
            .iter()
            .map(|line| lines.iter().position(|l| l == line).unwrap())
            .collect();
        assert!(positions.is_sorted(), "{stdout}");
    }
    lines.sort_unstable();
    let exit = "std.actor.Exit[AllocError | IoError]";
    let mut expected = vec![
        "cancelled True False".to_string(),
        "restart True False".into(),
        "again True".into(),
        "asked answer 49".into(),
        "waited timed out".into(),
        "abandoned gone".into(),
        "late reply refused True".into(),
        "timer 1".into(),
        "tick 1".into(),
        "tick 2".into(),
        "tick 3".into(),
        "up 1".into(),
        "up 2".into(),
        "up 3".into(),
        format!("down server {exit}.Stopped"),
        format!("down slow {exit}.Stopped"),
        format!("down quitter {exit}.Stopped"),
        format!("down phoenix {exit}.Trapped(std.actor.TrapKind.Unwrap)"),
        "std.actor.Stats(started=5, stopped=4, failed=0, trapped=3, restarted=2, delivered=21, full=0, gone=2, lost=0) 0".into(),
        "no timer".into(),
        "zero period".into(),
    ];
    expected.sort_unstable();
    assert_eq!(lines, expected, "{stdout}");
}

/// The two sockets' readiness arrives in epoll order, so those lines are
/// compared sorted.
#[test]
fn ready_mail_names_its_handle_and_is_queued_once() {
    let (_dir, executable) = support::build_file(WATCH);
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines[1..3].sort_unstable();
    assert_eq!(
        lines,
        [
            "same True False",
            "ready first 1",
            "ready second 1",
            "done",
            "3 0 0"
        ],
        "{stdout}"
    );
}

#[test]
fn admission_reports_allocation_failure() {
    let (_dir, executable) = build_with_hooks(ADMISSION);
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "refused 0 0\nstarted 2\nthen admitted 0 0\nstd.actor.StartError.Limit\n"
    );
}

#[test]
fn raw_actor_builtins_are_reserved_for_std() {
    for call in [
        "__actor_heap(0, 0)",
        "__actor_forget(5)",
        "__actor_run(0, main)",
    ] {
        let name = call.split('(').next().unwrap();
        let error = support::check_file_error(&format!("def main() -> ():\n    x = {call}\n"));
        assert!(
            error.contains(&format!("`{name}` is reserved for the standard library")),
            "{error}"
        );
    }
}

/// A node for the spawn-hook checks: `FIELD`, `RECEIVER` and `BODY` vary.
const HOOK_NODE: &str = r#"
from std.actor import Address, Context, Mail, stage
from std.sync import Mutex

pub class Node:
    pub value: FIELD

    pub def started(self: &mut Node, context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, AllocError], context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        Ok(())

    pub def spawn(RECEIVER, tag: i64, child: Address, context: &mut Context[i64, AllocError]) -> Result[Option[Node], AllocError]:
        BODY

def main() -> Result[(), AllocError]:
    mut actors = stage[Node, i64, AllocError](4, 4, 4).unwrap()
    actors.start(Node(INIT), 65536).unwrap()
    Ok(())
"#;

fn hook_node(field: &str, receiver: &str, body: &str, init: &str) -> String {
    HOOK_NODE
        .replace("FIELD", field)
        .replace("RECEIVER", receiver)
        .replace("BODY", body)
        .replace("INIT", init)
}

/// The spawn hook runs with the child's heap current, so it borrows the
/// parent's node shared, and the node cannot hold values that a shared borrow
/// aliases or changes across heaps.
#[test]
fn spawn_hook_cannot_move_values_between_heaps() {
    // Allocated in the child's heap and kept by the parent, then moved out.
    let error = support::check_file_error(&hook_node(
        "list[list[i64]]",
        "self: &mut Node",
        "self.value.append([tag]?)?\n        Ok(Some(Node(replace(self.value, []?))))",
        "[]?",
    ));
    assert!(
        error.contains("run_spawn") && error.contains("cannot borrow shared reference as mutable"),
        "{error}"
    );
    // A `str` copy would share the parent's storage with the child.
    let error = support::check_file_error(&hook_node(
        "str",
        "self",
        "Ok(Some(Node(self.value)))",
        "\"root\"",
    ));
    assert!(
        error.contains("an actor node cannot hold `str` (in `Node`)"),
        "{error}"
    );
    // `Mutex.lock` gives `&mut` through a shared borrow.
    let error = support::check_file_error(&hook_node(
        "Mutex[list[i64]]",
        "self",
        "Ok(Nothing)",
        "Mutex[list[i64]]([]?)",
    ));
    assert!(
        error.contains("an actor node cannot hold `Atomic[u32]` (in `std.sync.Mutex[list[i64]]`)"),
        "{error}"
    );
    // `Bytes` is allowed.
    let source = hook_node(
        "std.bytes.Bytes",
        "self",
        "Ok(Some(Node(std.bytes.with_capacity(self.value.len()).unwrap())))",
        "std.bytes.with_capacity(4).unwrap()",
    )
    .replace("from std.sync import Mutex", "import std.bytes");
    let dir = tempfile::tempdir().unwrap();
    let path = support::write_main(dir.path(), &source);
    plenty::check_file(&path, None).unwrap_or_else(|error| panic!("{error}"));
}

const OBSERVED_NODE: &str = r#"
from std.actor import Address, Context, Mail, stage
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, milliseconds, seconds

pub class Node:
    pub count: i64
    pub values: list[i64]

    pub def started(self: &mut Node, context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        self.count += 1
        self.values = [self.count]?
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, AllocError], context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        match mail:
            case Mail.Message(value):
                if value < 0:
                    absent: Option[i64] = Nothing
                    drop(absent.unwrap())
                self.count += value
            case _:
                pass
        Ok(())

    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, AllocError]) -> Result[Option[Node], AllocError]:
        Ok(Nothing)

def main() -> ():
    mut actors = stage[Node, i64, AllocError](1, 4, 1).unwrap()
    first = actors.start(Node(40, list[i64].new().unwrap()), 65536).unwrap()
    BODY
"#;

#[test]
fn node_observation_tracks_lifetime_and_copies_outside_the_heap() {
    let source = OBSERVED_NODE.replace(
        "    BODY",
        r#"    print(actors.node(first).unwrap().count).unwrap()
    snapshot = copy(actors.node(first).unwrap().values).unwrap()
    token = CancellationToken().unwrap()
    actors.send(first, 1).unwrap()
    drop(actors.run(&deadline_after(&monotonic_now(), &milliseconds(1)).unwrap(), &token))
    print(actors.node(first).unwrap().count).unwrap()
    drop(actors.stop_actor(first))
    actors.run(&deadline_after(&monotonic_now(), &seconds(5)).unwrap(), &token).unwrap()
    print(actors.node(first).is_none()).unwrap()
    second = actors.start(Node(7, list[i64].new().unwrap()), 65536).unwrap()
    print(actors.node(first).is_none(), actors.node(second).unwrap().count).unwrap()
    actors.send(second, -1).unwrap()
    actors.run(&deadline_after(&monotonic_now(), &seconds(5)).unwrap(), &token).unwrap()
    print(actors.node(second).is_none(), actors.stats().trapped).unwrap()
    drop(actors)
    print(snapshot).unwrap()"#,
    );
    let (_dir, executable) = support::build_file(&source);
    let output = Command::new(executable).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("unwrap on Nothing"), "{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "41\n42\nTrue\nTrue 8\nTrue 1\n[41]\n"
    );
}

#[test]
fn borrowed_node_prevents_stage_mutation_or_destruction() {
    for action in [
        "drop(actors.stop_actor(first))",
        "drop(actors.send(first, 1))",
        "actors.shutdown()",
        "drop(actors.start(Node(0, list[i64].new().unwrap()), 65536))",
        "drop(actors.run(&deadline_after(&monotonic_now(), &seconds(1)).unwrap(), &CancellationToken().unwrap()))",
        "drop(actors)",
    ] {
        let body = format!(
            "observed = actors.node(first).unwrap()\n    {action}\n    print(observed.count).unwrap()"
        );
        let error = support::check_file_error(&OBSERVED_NODE.replace("BODY", &body));
        assert!(error.contains("borrow"), "{action}: {error}");
    }
}

#[test]
fn runnable_stage_respects_its_deadline() {
    let source = OBSERVED_NODE
        .replace("self.count += value", "self.count += value\n                context.send(context.address(), 1).unwrap()")
        .replace("    BODY", r#"    actors.send(first, 1).unwrap()
    result = actors.run(&deadline_after(&monotonic_now(), &milliseconds(20)).unwrap(), &CancellationToken().unwrap())
    match result:
        case Err(error):
            print(error).unwrap()
        case Ok(_):
            print("completed").unwrap()
    print(actors.node(first).unwrap().count > 41).unwrap()"#);
    let (_dir, executable) = support::build_file(&source);
    let output = support::output_within(&mut Command::new(executable), &[], Duration::from_secs(5));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TimedOut"), "{stdout}");
    assert!(stdout.ends_with("True\n"), "{stdout}");
}

const ISOLATED_SPAWN: &str = r#"
import std.fs
import std.path
from std.actor import Address, Context, Mail, heaps_live, heap_bytes_used, heap_bytes_committed, stage
from std.net import listen, loopback_v4
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

pub enum Fault:
    Detail(str)

def descriptors() -> Result[i64, IoError]:
    mut count = 0
    for entry in std.fs.entries(std.fs.read_dir(&std.path.from_text("/proc/self/fd")?)?):
        entry?
        count += 1
    Ok(count)

pub class Node:
    pub parent: bool
    pub attempt: i64
    pub child: Option[Address]

    pub def started(self: &mut Node, context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        if self.parent:
            # A full mailbox cannot prevent constructor failure notices.
            context.send(context.address(), 0).unwrap()
            child = context.SPAWN(0, 65536).unwrap()
            self.child = Some(child)
            if RESTART:
                drop(context.restart(child, 3, &seconds(60)))
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, Fault], context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match mail:
            case Mail.Message(_):
                if RESTART:
                    if self.parent:
                        self.attempt = 1
                        context.send(self.child.unwrap(), 0).unwrap()
                    else:
                        absent: Option[i64] = Nothing
                        drop(absent.unwrap())
            case Mail.Down(_, tag, exit):
                print(tag, exit).unwrap()
                timer = context.after(&seconds(60), 88).unwrap()
                drop(context.cancel(timer))
                self.attempt += 1
                if RESTART or self.attempt == 4:
                    context.stop()
                else:
                    drop(context.SPAWN(self.attempt, 65536).unwrap())
            case _:
                pass
        Ok(())

    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, Fault]) -> Result[Option[Node], Fault]:
        # A failed hook must reclaim its timers and tracked descriptors too.
        drop(context.after(&seconds(60), 99).unwrap())
        socket = listen(&loopback_v4(0u16), 1).unwrap()
        if RESTART:
            if self.attempt > 0:
                return Err(Fault.Detail(str.from(42).unwrap()))
            return Ok(Some(Node(False, 0, Nothing)))
        if tag == 0:
            return Err(Fault.Detail(str.from(42).unwrap()))
        if tag == 1:
            absent: Option[i64] = Nothing
            drop(absent.unwrap())
        if tag == 2:
            return Ok(Nothing)
        # No work; test successful construction and normal child retirement.
        Ok(Some(Node(False, 0, Nothing)))

def main() -> ():
    before = descriptors().unwrap()
    mut actors = stage[Node, i64, Fault](2, 1, 2).unwrap()
    actors.add_timers(1).unwrap()
    root = actors.start(Node(True, 0, Nothing), 65536).unwrap()
    RUN
    print(actors.stats().started, actors.stats().stopped, actors.stats().failed, actors.stats().trapped).unwrap()
    drop(actors)
    print(heaps_live(), heap_bytes_used(), heap_bytes_committed()).unwrap()
    print(descriptors().unwrap() == before).unwrap()
"#;

#[test]
fn isolated_construction_failures_notify_parent_and_recover_capacity() {
    let source = ISOLATED_SPAWN
        .replace("SPAWN", "spawn_isolated")
        .replace("RESTART", "False")
        .replace("        Ok(())\n\n    pub def receive", "        else:\n            context.stop()\n        Ok(())\n\n    pub def receive")
        .replace("RUN", "actors.run(&deadline_after(&monotonic_now(), &seconds(5)).unwrap(), &CancellationToken().unwrap()).unwrap()");
    let (_dir, executable) = support::build_file(&source);
    let output =
        support::output_within(&mut Command::new(executable), &[], Duration::from_secs(10));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 std.actor.Exit[Fault].Failed(Fault.Detail(\"42\"))"),
        "{stdout}"
    );
    assert!(
        stdout.contains("1 std.actor.Exit[Fault].Trapped(std.actor.TrapKind.Unwrap)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("2 std.actor.Exit[Fault].Stopped"),
        "{stdout}"
    );
    assert!(
        stdout.contains("3 std.actor.Exit[Fault].Stopped"),
        "{stdout}"
    );
    assert!(stdout.ends_with("5 3 1 1\n0 0 0\nTrue\n"), "{stdout}");
}

#[test]
fn isolated_restart_constructor_failure_cleans_new_timers_and_notifies_parent() {
    for (failure, exit, counts) in [
        (
            "return Err(Fault.Detail(str.from(42).unwrap()))",
            "Failed(Fault.Detail(\"42\"))",
            "2 1 1 1",
        ),
        ("return Ok(Nothing)", "Stopped", "2 2 0 1"),
        (
            "absent: Option[i64] = Nothing\n                drop(absent.unwrap())",
            "Trapped(std.actor.TrapKind.Unwrap)",
            "2 1 0 2",
        ),
    ] {
        let source = ISOLATED_SPAWN
            .replace("SPAWN", "spawn_isolated")
            .replace("RESTART", "True")
            .replace("if self.attempt > 0:\n                return Err(Fault.Detail(str.from(42).unwrap()))", &format!("if self.attempt > 0:\n                {failure}"))
            .replace("RUN", "actors.run(&deadline_after(&monotonic_now(), &seconds(5)).unwrap(), &CancellationToken().unwrap()).unwrap()");
        let (_dir, executable) = support::build_file(&source);
        let output =
            support::output_within(&mut Command::new(executable), &[], Duration::from_secs(10));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("0 std.actor.Exit[Fault].{exit}")),
            "{stdout}"
        );
        assert!(
            stdout.ends_with(&format!("{counts}\n0 0 0\nTrue\n")),
            "{stdout}"
        );
    }
}

#[test]
fn ordinary_constructor_failure_still_ends_parent() {
    let source = ISOLATED_SPAWN
        .replace("SPAWN", "spawn")
        .replace("RESTART", "False")
        .replace("RUN", "print(actors.node(root).is_none()).unwrap()");
    let (_dir, executable) = support::build_file(&source);
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "True\n1 0 1 0\n0 0 0\nTrue\n"
    );
}

#[test]
fn shutdown_ignores_full_mailboxes_is_idempotent_and_allows_stage_reuse() {
    let source = OBSERVED_NODE
        .replace("Mail, stage", "Mail, heaps_live, stage")
        .replace(
            "    BODY",
            r#"    for _ in range(4):
        actors.send(first, 1).unwrap()
    print(actors.stop_actor(first)).unwrap()
    actors.shutdown()
    print(actors.live(), heaps_live(), actors.node(first).is_none()).unwrap()
    actors.shutdown()
    second = actors.start(Node(9, list[i64].new().unwrap()), 65536).unwrap()
    print(actors.node(second).unwrap().count, actors.node(first).is_none()).unwrap()
    actors.shutdown()
    print(actors.stats().stopped, heaps_live()).unwrap()"#,
        );
    let (_dir, executable) = support::build_file(&source);
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "False\n0 0 True\n10 True\n2 0\n"
    );
}

#[test]
fn stage_drop_releases_live_heaps_and_contains_node_destructor_traps() {
    let source = r#"
import std.fs
import std.path
from std.actor import Address, Context, Mail, heaps_live, heap_bytes_used, heap_bytes_committed, stage
from std.net import TcpListener, listen, loopback_v4

def descriptors() -> Result[i64, IoError]:
    mut count = 0
    for entry in std.fs.entries(std.fs.read_dir(&std.path.from_text("/proc/self/fd")?)?):
        entry?
        count += 1
    Ok(count)

pub class Node:
    pub bad: bool
    pub values: list[i64]
    pub socket: Option[TcpListener]

    def __del__(self) -> ():
        if self.bad:
            absent: Option[i64] = Nothing
            drop(absent.unwrap())

    pub def started(self: &mut Node, context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        self.values = [1, 2, 3]?
        self.socket = Some(listen(&loopback_v4(0u16), 1).unwrap())
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, AllocError], context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        Ok(())

    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, AllocError]) -> Result[Option[Node], AllocError]:
        Ok(Nothing)

def main() -> ():
    before = descriptors().unwrap()
    for _ in range(3):
        mut actors = stage[Node, i64, AllocError](2, 2, 1).unwrap()
        first = actors.start(Node(True, list[i64].new().unwrap(), Nothing), 65536).unwrap()
        actors.send(first, 1).unwrap()
        actors.send(first, 2).unwrap()
        drop(actors.start(Node(False, list[i64].new().unwrap(), Nothing), 65536).unwrap())
        drop(actors)
        print(heaps_live(), heap_bytes_used(), heap_bytes_committed(), descriptors().unwrap() == before).unwrap()
"#;
    let (_dir, executable) = support::build_file(source);
    let output = support::output_within(&mut Command::new(executable), &[], Duration::from_secs(5));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(stderr.matches("unwrap on Nothing").count(), 3, "{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0 0 0 True\n0 0 0 True\n0 0 0 True\n"
    );
}
