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

/// Admission under injected allocation failure.
const ADMISSION: &str = include_str!("fixtures/actor/admit.plenty");

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
        "std.actor.Stats(started={}, stopped={}, failed=0, trapped=1,",
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
        "0 got reply from 2\n\
         down 1 std.actor.Exit[AllocError].Failed(AllocError.OutOfMemory)\n\
         down 2 std.actor.Exit[AllocError].Stopped\n\
         down 3 std.actor.Exit[AllocError].Trapped\n\
         std.actor.Stats(started=4, stopped=2, failed=1, trapped=1, delivered=7, full=0, gone=0, lost=0) 0 0 0\n\
         gone Note.Text(\"late\")\n\
         sent\n\
         full Note.Text(\"second\")\n\
         9 got first\n\
         gone Note.Text(\"third\")\n\
         std.actor.Stats(started=1, stopped=1, failed=0, trapped=0, delivered=1, full=1, gone=1, lost=0) 0 0\n"
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

/// The spawn hook runs the parent's code with the child's heap current, so it
/// can store child-heap values in the parent or hand parent values to the
/// child. Neither is rejected; actor.md documents the rule. This test pins
/// the escape: delete it when the checker confines the hook.
#[test]
fn spawn_hook_can_move_values_between_heaps() {
    let source = r#"
from std.actor import Address, Context, Mail, stage

pub class Node:
    pub kept: list[list[i64]]

    pub def started(self: &mut Node, context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, AllocError], context: &mut Context[i64, AllocError]) -> Result[(), AllocError]:
        Ok(())

    pub def spawn(self: &mut Node, tag: i64, child: Address, context: &mut Context[i64, AllocError]) -> Result[Option[Node], AllocError]:
        # Allocated in the child's heap, kept by the parent.
        self.kept.append([tag]?)?
        # Owned by the parent, moved into the child.
        kept = replace(self.kept, []?)
        Ok(Some(Node(kept)))

def main() -> Result[(), AllocError]:
    mut actors = stage[Node, i64, AllocError](4, 4, 4).unwrap()
    actors.start(Node([]?), 65536).unwrap()
    Ok(())
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = support::write_main(dir.path(), source);
    plenty::check_file(&path, None).unwrap_or_else(|error| panic!("{error}"));
}
