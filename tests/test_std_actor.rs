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
        "down 1 11 std.actor.Exit[AllocError].Failed(AllocError.OutOfMemory)\n\
         0 got reply from 12\n\
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
    let exit = "std.actor.Exit[AllocError | IoError | OverflowError[i64]]";
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
        "refused 0 0\nstarted 2\nthen admitted 0 0\n"
    );
}

/// A heap limit has a type for its lower bound; `max_heap_limit` is a fixed
/// ceiling, so a larger limit is a programming error and traps.
#[test]
fn heap_limits_reject_zero_and_trap_above_the_ceiling() {
    let start = "    refused = actors.start(Node(1), 65536)\n";
    let error = support::check_file_error(
        &ADMISSION.replace(start, "    refused = actors.start(Node(1), 0)\n"),
    );
    assert!(
        error.contains("literal is not a `Positive[i64]`"),
        "{error}"
    );
    let error = support::check_file_error(
        &ADMISSION.replace(start, "    refused = actors.start(Node(1), 67108865)\n"),
    );
    assert!(
        error.contains("constant argument `limit` is 67108865"),
        "{error}"
    );
    let (_dir, executable) = build_with_hooks(&ADMISSION.replace(
        start,
        "    refused = actors.start(Node(1), Positive.try_from(67108865).unwrap())\n",
    ));
    let output = Command::new(executable).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains(": std.actor.Stage.start: limit is above max_heap_limit\n"),
        "{stderr}"
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

type Fault = AllocError | OverflowError[i64]

pub class Node:
    pub count: i64
    pub values: list[i64]

    pub def started(self: &mut Node, context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        self.count += 1
        self.values = [self.count]?
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, Fault], context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match mail:
            case Mail.Message(value):
                if value < 0:
                    absent: Option[i64] = Nothing
                    drop(absent.unwrap())
                self.count += value
            case _:
                pass
        Ok(())

    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, Fault]) -> Result[Option[Node], Fault]:
        Ok(Nothing)

def main() -> ():
    mut actors = stage[Node, i64, Fault](1, 4, 1).unwrap()
    first = actors.start(Node(40, list[i64].new().unwrap()), 65536).unwrap()
    BODY
"#;

/// A capacity that cannot be allocated is `CapacityOverflow`, negative
/// included (B92 invalid-argument policy).
#[test]
fn impossible_stage_capacities_are_capacity_overflow() {
    let source = OBSERVED_NODE
        .replace(
            "    BODY",
            r#"    print(str.repr(stage[Node, i64, Fault](-1, 4, 1))?)?
    print(str.repr(stage[Node, i64, Fault](1048577, 4, 1))?)?"#,
        )
        .replace(
            "def main() -> ():",
            "def main() -> Result[(), AllocError | IoError]:",
        );
    let source = format!("{source}    Ok(())\n");
    let output = support::run_file_stdout(&source);
    let line = "Result[std.actor.Stage[Node, i64, AllocError | OverflowError[i64]], std.actor.StageError].Err(std.actor.StageError.Allocation(AllocError.CapacityOverflow))";
    assert_eq!(output, format!("{line}\n{line}"), "{source}");
}

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

def descriptors() -> Result[i64, IoError | OverflowError[i64]]:
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
                # This constructor fixture makes at most four attempts.
                unchecked:
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

def descriptors() -> Result[i64, IoError | OverflowError[i64]]:
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

#[test]
fn child_exits_follow_prior_messages_without_starving_behind_new_mail() {
    let source = r#"
from std.actor import Address, Context, Mail, heaps_live, stage
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

pub class Node:
    pub parent: Option[Address]
    pub tag: i64
    pub last_message: i64
    pub exits: i64
    pub address: Option[Address]

    pub def started(self: &mut Node, context: &mut Context[i64, IoError]) -> Result[(), IoError]:
        match self.parent:
            case Some(parent):
                context.send(parent, self.tag).unwrap()
                context.stop()
            case Nothing:
                self.address = Some(context.address())
                context.send(context.address(), -1).unwrap()
                for tag in range(8):
                    drop(context.spawn_isolated(tag, 65536).unwrap())
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, IoError], context: &mut Context[i64, IoError]) -> Result[(), IoError]:
        match mail:
            case Mail.Message(value):
                if value < 0:
                    # Never let the parent's ordinary mailbox empty.
                    context.send(context.address(), -1).unwrap()
                else:
                    self.last_message = value
                    print("message", value)?
            case Mail.Down(_, tag, _):
                print("down", tag, self.last_message == tag)?
                # Only the eight non-restarting children above can exit.
                unchecked:
                    self.exits += 1
                if self.exits == 8:
                    context.stop()
            case _:
                pass
        Ok(())

    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, IoError]) -> Result[Option[Node], IoError]:
        Ok(Some(Node(self.address, tag, -1, 0, Nothing)))

def main() -> ():
    mut actors = stage[Node, i64, IoError](9, 16, 1).unwrap()
    drop(actors.start(Node(Nothing, -1, -1, 0, Nothing), 65536).unwrap())
    result = actors.run(&deadline_after(&monotonic_now(), &seconds(1)).unwrap(), &CancellationToken().unwrap())
    print(result.is_ok(), actors.stats().stopped, heaps_live()).unwrap()
"#;
    let (_dir, executable) = support::build_file(source);
    let output = support::output_within(&mut Command::new(executable), &[], Duration::from_secs(5));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = (0..8)
        .map(|tag| format!("message {tag}\ndown {tag} True\n"))
        .collect::<String>()
        + "True 9 0\n";
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn retired_slots_keep_one_runnable_ticket_when_reused_before_polling() {
    let source = r#"
from std.actor import Address, Context, Mail, heaps_live, stage
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

pub enum Fault:
    Failed

pub class Node:
    pub kind: i64

    pub def started(self: &mut Node, context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        context.send(context.address(), 1).unwrap()
        if self.kind == 0:
            context.stop()
        elif self.kind == 1:
            return Err(Fault.Failed)
        elif self.kind == 2:
            empty: Option[i64] = Nothing
            empty.unwrap()
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, Fault], context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        print("observer").unwrap()
        context.stop()
        Ok(())

    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, Fault]) -> Result[Option[Node], Fault]:
        Ok(Nothing)

def main() -> ():
    mut actors = stage[Node, i64, Fault](2, 2, 1).unwrap()
    drop(actors.start(Node(-1), 65536).unwrap())
    # Reuse the other slot before the queued observer or retired actors run.
    # Its existing scheduling ticket must not be appended a second time.
    for index in range(128):
        # index is in 0..128; the fixed positive divisor cannot fail.
        mode = unchecked:
            index % 3
        drop(actors.start(Node(mode), 65536).unwrap())
    result = actors.run(&deadline_after(&monotonic_now(), &seconds(1)).unwrap(), &CancellationToken().unwrap())
    stats = actors.stats()
    print(result.is_ok(), actors.live(), stats.started, stats.stopped, stats.failed, stats.trapped, heaps_live()).unwrap()
    actors.shutdown()
    # Shutdown discards any remaining retired-slot ticket and its queued flag.
    drop(actors.start(Node(-1), 65536).unwrap())
    again = actors.run(&deadline_after(&monotonic_now(), &seconds(1)).unwrap(), &CancellationToken().unwrap())
    print(again.is_ok(), actors.live(), heaps_live()).unwrap()
"#;
    let (_dir, executable) = support::build_file(source);
    let output = support::output_within(&mut Command::new(executable), &[], Duration::from_secs(5));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "observer\nTrue 0 129 44 43 42 0\nobserver\nTrue 0 0\n"
    );
}

fn actor_probe_stdout(source: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&input, source).unwrap();
    let options = plenty::CompileOptions {
        optimize,
        test_hooks: true,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&input, &executable, None, &options)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Exercise the actual private helpers at representation limits without large
/// mailboxes/heaps. Both codegen modes must preserve each opt-out's bounds.
#[test]
fn bounded_ring_and_timer_arithmetic_at_representation_limits() {
    let actor = include_str!("../stdlib/std/actor.plenty");
    let helpers = &actor[actor.find("def ring_at(").unwrap()..];
    let order_down = actor
        .split("    def order_down(")
        .nth(1)
        .unwrap()
        .split("    def accepts(")
        .next()
        .unwrap()
        .replace("Context[M, E]", "HeapProbe");
    let order_up = actor
        .split("    def order_up(")
        .nth(1)
        .unwrap()
        .split("    def order_down(")
        .next()
        .unwrap()
        .replace("Context[M, E]", "HeapProbe");
    let timer_error = actor
        .split("pub enum TimerError:\n")
        .nth(1)
        .unwrap()
        .split("# A request that was not sent")
        .next()
        .unwrap();
    let source = format!(
        r#"enum TimerError:
{timer_error}
{helpers}
class HeapProbe:
    _order_count: i64
    swapped: bool
    def order_due(self, at: i64) -> u64:
        u64(at)
    def order_swap(self: &mut HeapProbe, a: i64, b: i64) -> ():
        self.swapped = True
    def order_down({order_down}
    def order_up({order_up}
def main() -> Result[(), IoError]:
    mut heap = HeapProbe(i64.MAX, False)
    print("__test_begin_no_allocations__")?
    heap.order_down(9223372036854775806)
    heap.order_down(4611686018427387902)
    heap.order_up(9223372036854775806)
    a = ring_at(0, 1, 1)
    b = ring_at(9223372036854775806, 9223372036854775806, i64.MAX)
    c = ring_at(9223372036854775805, 1, i64.MAX)
    d = ring_at(5, 7, 12)
    e = timer_due(u64.MAX, 0u64).unwrap()
    f = timer_due(u64.MAX, 1u64).is_err()
    g = timer_due(0u64, u64.MAX).unwrap()
    h = request_due(u64.MAX, 1u64)
    i = repeat_due(10u64, 5u64, 14u64)
    j = repeat_due(10u64, 5u64, 15u64)
    k = repeat_due(u64.MAX, 1u64, u64.MAX)
    l = count_event(9223372036854775806)
    m = count_event(i64.MAX)
    n = next_generation(4294967294u32).unwrap()
    o = next_generation(u32.MAX).is_none()
    p = rebase_fence(8u64, 9u64)
    q = rebase_fence(9u64, 9u64)
    r = rebase_fence(u64.MAX, 9u64)
    print("__test_end_no_allocations__")?
    print(a, b, c, d)?
    print(e, f, g, h)?
    print(i, j, k)?
    print(l, m, n, o, p, q, r)?
    print(heap.swapped)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        let actual = actor_probe_stdout(&source, optimize);
        assert_eq!(
            actual,
            "0 9223372036854775805 9223372036854775806 0\n18446744073709551615 True 18446744073709551615 18446744073709551615\n15 20 18446744073709551615\n9223372036854775807 9223372036854775807 4294967295 True 0 0 18446744073709551606\nFalse"
        );
    }
}

#[test]
fn mailbox_rebase_preserves_all_linked_down_fences() {
    let actor = include_str!("../stdlib/std/actor.plenty");
    let method = actor
        .split("    def rebase_mail(")
        .nth(1)
        .unwrap()
        .split("    def enqueue(")
        .next()
        .unwrap();
    let push = actor
        .split("    def push(")
        .nth(1)
        .unwrap()
        .split("    # Rebase every fence")
        .next()
        .unwrap();
    let clear = actor
        .split("    def clear(")
        .nth(1)
        .unwrap()
        .split("    def free(")
        .next()
        .unwrap();
    let unlink = actor
        .split("    def unlink_down(")
        .nth(1)
        .unwrap()
        .split("    # Returns a zombie")
        .next()
        .unwrap();
    let bury = actor
        .split("    def bury(")
        .nth(1)
        .unwrap()
        .split("    def prepare(")
        .next()
        .unwrap();
    let helpers = &actor[actor.find("def ring_at(").unwrap()..];
    let source = format!(
        r#"enum TimerError:
    Range
    Capacity
enum Mail[M, E]:
    Message(M)
    Failure(E)
class Payload:
    id: i64
    def __del__(self) -> ():
        print("dropped", self.id).unwrap()
class ProbeMailbox[M, E]:
    mail_read: u64
    mail_written: u64
    down_head: i64
    down_after: u64
    down_next: i64
    items: list[Option[Mail[M, E]]]
    head: i64
    count: i64
    ready: i64
    exit: Option[Payload]
    heap: i64
    zombie: bool
    down_tail: i64
class Stats:
    lost: i64
class Context[M, E]:
    _boxes: list[ProbeMailbox[M, E]]
    _free: list[i64]
    _stats: Stats
    def rebase_mail({method}
    def push({push}
    def enqueue(self: &mut Context[M, E], index: i64) -> ():
        pass
class Stage[N, M, E]:
    _context: Context[M, E]
    def clear({clear}
    def unlink_down({unlink}
    def bury({bury}
{helpers}
def child(fence: u64, next: i64, heap: i64, id: i64) -> ProbeMailbox[i64, i64]:
    ProbeMailbox[i64, i64](0u64, 0u64, -1, fence, next, [].unwrap(), 0, 0, 0, Some(Payload(id)), heap, True, -1)
def main() -> Result[(), IoError]:
    # Four linked Down records: past, equal, future, and MAX.
    # The fourth has an exhausted slot; the fifth belongs to another parent.
    mut stage = Stage[i64, i64, i64](Context[i64, i64]([
        ProbeMailbox[i64, i64](18446744073709551612u64, u64.MAX, 1, 0u64, -1,
            [Some(Mail[i64, i64].Message(1)), Some(Mail[i64, i64].Message(2)), Some(Mail[i64, i64].Message(3)), Nothing].unwrap(), 0, 3, 0, Nothing, 0, False, 4),
        child(7u64, 2, -1, 1),
        child(18446744073709551612u64, 3, -1, 2),
        child(18446744073709551614u64, 4, -1, 3),
        child(u64.MAX, -1, -2, 4),
        child(u64.MAX, -1, -1, 5)].unwrap(), [].unwrap(), Stats(0)))
    print("__test_begin_no_allocations__")?
    stage._context.push(0, Some(Mail[i64, i64].Message(9)))
    print("__test_end_no_allocations__")?
    print(stage._context._boxes[0].mail_read, stage._context._boxes[0].mail_written, stage._context._boxes[0].count)?
    print(stage._context._boxes[1].down_after, stage._context._boxes[2].down_after, stage._context._boxes[3].down_after, stage._context._boxes[4].down_after, stage._context._boxes[5].down_after)?
    # clear is used by both stop and restart; it must drop every owned Down
    # exactly once before reserve resets the counters for reuse.
    stage.clear(0)
    print(stage._context._boxes[0].mail_read, stage._context._boxes[0].mail_written, stage._context._boxes[0].count, stage._context._stats.lost, len(stage._context._free))?
    # A cleared/restarted mailbox can accept more mail without stale fences.
    stage._context.push(0, Some(Mail[i64, i64].Message(10)))
    print(stage._context._boxes[0].mail_read, stage._context._boxes[0].mail_written, stage._context._boxes[0].count, stage._context._boxes[0].down_head)?
    stage.clear(0)
    drop(replace(stage._context._boxes[5].exit, Nothing))
    # Cross MAX-1 -> MAX and then rebase on the following enqueue.
    mut edge = Context[i64, i64]([ProbeMailbox[i64, i64](18446744073709551614u64, 18446744073709551614u64, -1, 0u64, -1,
        [Nothing, Nothing].unwrap(), 0, 0, 0, Nothing, 0, False, -1)].unwrap(), [].unwrap(), Stats(0))
    edge.push(0, Some(Mail[i64, i64].Message(1)))
    edge.push(0, Some(Mail[i64, i64].Message(2)))
    print(edge._boxes[0].mail_read, edge._boxes[0].mail_written, edge._boxes[0].count)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        let actual = actor_probe_stdout(&source, optimize);
        assert_eq!(
            actual,
            "0 4 4\n0 0 2 3 18446744073709551615\ndropped 1\ndropped 2\ndropped 3\ndropped 4\n4 4 0 4 3\n4 5 1 -1\ndropped 5\n0 2 2"
        );
    }
}

#[test]
fn exhausted_slot_generation_retires_after_zombie_cleanup() {
    let actor = include_str!("../stdlib/std/actor.plenty");
    let free = actor
        .split("    def free(")
        .nth(1)
        .unwrap()
        .split("# Allocates a stage")
        .next()
        .unwrap();
    let bury = actor
        .split("    def bury(")
        .nth(1)
        .unwrap()
        .split("    def prepare(")
        .next()
        .unwrap();
    let generation = actor
        .split("def next_generation(")
        .nth(1)
        .unwrap()
        .split("def rebase_fence(")
        .next()
        .unwrap();
    let source = format!(
        r#"enum Mail[M, E]:
    Message(M)
    Failure(E)
class ProbeMailbox[M, E]:
    generation: u32
    heap: i64
    pending: bool
    zombie: bool
    down_next: i64
    items: list[Option[Mail[M, E]]]
class Context[M, E]:
    _boxes: list[ProbeMailbox[M, E]]
    _free: list[i64]
class Stage[N, M, E]:
    _context: Context[M, E]
    def free({free}
    def bury({bury}
def next_generation({generation}
def main() -> Result[(), IoError]:
    mut stage = Stage[i64, i64, i64](Context[i64, i64]([
        ProbeMailbox[i64, i64](4294967294u32, 0, False, False, -1, [] .unwrap()),
        ProbeMailbox[i64, i64](u32.MAX, 0, False, True, 7, [] .unwrap()),
        ProbeMailbox[i64, i64](u32.MAX, 0, False, False, -1, [] .unwrap())].unwrap(), [] .unwrap()))
    stage.free(0)
    stage.free(1)
    # The exhausted zombie stays reserved until Down delivery/cleanup.
    print(len(stage._context._free), stage._context._boxes[1].zombie)?
    stage.bury(1)
    stage.free(2)
    print(len(stage._context._free), stage._context._boxes[0].generation, stage._context._boxes[0].heap)?
    print(stage._context._boxes[1].heap, stage._context._boxes[1].zombie, stage._context._boxes[1].down_next, stage._context._boxes[2].heap)?
    Ok(())
"#
    )
    .replace("[] .unwrap()", "[].unwrap()");
    for optimize in [false, true] {
        let actual = actor_probe_stdout(&source, optimize);
        assert_eq!(actual, "1 True\n1 4294967295 -1\n-2 False -1 -2");
    }
}

#[test]
fn timer_record_key_capacity_is_checked_before_mutation() {
    let source = r#"
from std.actor import Address, Context, Mail, stage
pub class Node:
    pub def started(self: &mut Node, context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        Ok(())
    pub def receive(self: &mut Node, mail: Mail[i64, AllocError], context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        Ok(())
    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, AllocError]) -> Result[Option[Node], AllocError]:
        Ok(Nothing)

def main() -> Result[(), IoError]:
    mut actors = stage[Node, i64, AllocError](0, 1, 1).unwrap()
    print(str.repr(actors.add_timers(4294967297)).unwrap())?
    print(str.repr(actors.add_timers(i64.MAX)).unwrap())?
    print(actors.add_timers(1).is_ok())?
    Ok(())
"#;
    for optimize in [false, true] {
        assert_eq!(
            actor_probe_stdout(source, optimize),
            "Result[(), AllocError].Err(AllocError.CapacityOverflow)\nResult[(), AllocError].Err(AllocError.CapacityOverflow)\nTrue"
        );
    }
}

#[test]
fn exhausted_clock_generation_unlinks_without_reentering_free_list() {
    let actor = include_str!("../stdlib/std/actor.plenty");
    let free_clock = actor
        .split("    def free_clock(")
        .nth(1)
        .unwrap()
        .split("    # Removes a request")
        .next()
        .unwrap();
    let generation = actor
        .split("def next_generation(")
        .nth(1)
        .unwrap()
        .split("def rebase_fence(")
        .next()
        .unwrap();
    let source = format!(
        r#"class ProbeClock:
    generation: u32
    owner: i64
    owner_prev: i64
    owner_next: i64
    position: i64
    noticed: bool
class ProbeMailbox:
    clocks_head: i64
class Context[M, E]:
    _clocks: list[ProbeClock]
    _boxes: list[ProbeMailbox]
    _clock_free: i64
    removed: bool
    target_unlinked: bool
    notice_unlinked: bool
    def order_remove(self: &mut Context[M, E], record: i64) -> ():
        self._clocks[record].position = -1
        self.removed = True
    def unlink_target(self: &mut Context[M, E], record: i64) -> ():
        self.target_unlinked = True
    def unlink_notice(self: &mut Context[M, E], record: i64) -> ():
        self._clocks[record].noticed = False
        self.notice_unlinked = True
    def free_clock({free_clock}
def next_generation({generation}
def main() -> Result[(), IoError]:
    mut context = Context[i64, i64]([
        ProbeClock(u32.MAX, 0, -1, 1, 0, True),
        ProbeClock(4294967294u32, 0, 0, -1, -1, False)].unwrap(),
        [ProbeMailbox(0)].unwrap(), -1, False, False, False)
    print("__test_begin_no_allocations__")?
    context.free_clock(0)
    print("__test_end_no_allocations__")?
    print(context._clocks[0].owner, context._clock_free, context._boxes[0].clocks_head, context._clocks[1].owner_prev, context.removed, context.target_unlinked, context.notice_unlinked)?
    context.free_clock(1)
    print(context._clock_free, context._clocks[1].generation, context._clocks[1].owner, context._boxes[0].clocks_head)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        let actual = actor_probe_stdout(&source, optimize);
        assert_eq!(actual, "-2 -1 1 -1 True True True\n1 4294967295 -1 -1");
    }
}
