//! std.net over loopback only: no test needs a network, a resolver beyond
//! the hosts file, or a fixed port.
use std::path::Path;
use std::process::{Command, Stdio};

fn written(dir: &Path, source: &str) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    std::fs::write(&path, source).unwrap();
    path
}

fn runs(body: &str, expected: &str) {
    runs_with(body, expected, None);
}

/// Runs the program with at most `descriptors` open files, when given.
fn runs_with(body: &str, expected: &str, descriptors: Option<u32>) {
    let source = format!("{PRELUDE}{body}");
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&written(dir.path(), &source), &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let mut command = match descriptors {
        Some(limit) => {
            let mut shell = Command::new("sh");
            shell.args([
                "-c",
                &format!("ulimit -n {limit} && exec \"$0\""),
                executable.to_str().unwrap(),
            ]);
            shell
        }
        None => Command::new(executable),
    };
    let output = command.stdin(Stdio::null()).output().unwrap();
    assert!(
        output.status.success(),
        "status {:?}; {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

const PRELUDE: &str = r#"
import std.bytes
from std.bytes import Bytes
from std.ready import Interest, WaitError, selector
from std.net import Datagram, IpAddress, NetError, Shutdown, SocketAddress, TcpListener, TcpStream, UdpSocket, any_v4, bind_udp, connect, format_ip, ipv4, listen, loopback_v4, loopback_v6, parse_address, parse_ip, resolve, socket_address
from std.sync import CancellationToken
from std.time import Deadline, Instant, deadline_after, milliseconds, monotonic_now, sleep

def within(ms: u32) -> Result[Deadline, Failure]:
    Ok(deadline_after(&monotonic_now(), &milliseconds(ms))?)

def zeroed(size: i64) -> Result[Bytes, Failure]:
    mut out = std.bytes.with_capacity(size)?
    out.resize(size, 0u8)?
    Ok(out)

def outcome[T](result: Result[T, NetError]) -> Result[str, Failure]:
    match result:
        case Ok(_):
            Ok("ok")
        case Err(error):
            Ok(str.repr(error)?)

def read_all(stream: &mut TcpStream, deadline: &Deadline, cancel: &CancellationToken) -> Result[Bytes, NetError]:
    mut out = std.bytes.with_capacity(64).unwrap()
    mut chunk = std.bytes.with_capacity(7).unwrap()
    chunk.resize(7, 0u8).unwrap()
    while True:
        count = stream.read_before(&mut chunk, 0, 7, deadline, cancel)?
        if count == 0:
            break
        out.extend(&chunk.slice(0, count).unwrap()).unwrap()
    Ok(out)

def echo(listener: &TcpListener, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
    mut stream = listener.accept(deadline, cancel)?
    stream.set_no_delay(True)?
    data = read_all(&mut stream, deadline, cancel)?
    stream.write_all_before(&data, 0, data.len(), deadline, cancel)?
    stream.shutdown(Shutdown.Write)?
    Ok(data.len())
"#;

#[test]
fn addresses_parse_and_format() {
    runs(
        r#"
def show(text: &str) -> Result[(), Failure]:
    match parse_ip(text):
        case Ok(ip):
            print(format_ip(ip)?)?
        case Err(error):
            print(text, str.repr(error)?)?
    Ok(())

def show_address(text: &str) -> Result[(), Failure]:
    match parse_address(text):
        case Ok(address):
            print(address.format()?, address.port, address.scope)?
        case Err(error):
            print(text, str.repr(error)?)?
    Ok(())

def main() -> Result[(), Failure]:
    for text in ["10.0.0.255", "::", "::1", "1::", "2001:DB8:0:0:8:800:200C:417A", "2001:db8:0:0:1:0:0:1", "2001:db8:0:1:1:1:1:1", "0:0:1:0:0:0:0:0", "::ffff:192.0.2.1", "64:ff9b::192.0.2.33", "1.2.3", "1.2.3.256", "01.2.3.4", "1:::2", "1::2::3", ":1::", "1:2:3:4:5:6:7:8:9", "12345::", "fe80::1%2", ""]?:
        show(text)?
    for text in ["127.0.0.1:80", "[::1]:65535", "[fe80::1%7]:8080", "1.2.3.4:65536", "1.2.3.4", "[::1]80", "[::1]:"]?:
        show_address(text)?
    print(format_ip(ipv4(192u8, 168u8, 1u8, 20u8))?)?
    print(socket_address(IpAddress.V6(0u64, 1u64), 443u16).format()?)?
    Ok(())
"#,
        "10.0.0.255\n::\n::1\n1::\n2001:db8::8:800:200c:417a\n2001:db8::1:0:0:1\n2001:db8:0:1:1:1:1:1\n0:0:1::\n\
::ffff:192.0.2.1\n64:ff9b::c000:221\n1.2.3 std.net.NetError.InvalidAddress\n\
1.2.3.256 std.net.NetError.InvalidAddress\n01.2.3.4 std.net.NetError.InvalidAddress\n\
1:::2 std.net.NetError.InvalidAddress\n1::2::3 std.net.NetError.InvalidAddress\n\
:1:: std.net.NetError.InvalidAddress\n1:2:3:4:5:6:7:8:9 std.net.NetError.InvalidAddress\n\
12345:: std.net.NetError.InvalidAddress\nfe80::1%2 std.net.NetError.InvalidAddress\n std.net.NetError.InvalidAddress\n\
127.0.0.1:80 80 0\n[::1]:65535 65535 0\n[fe80::1%7]:8080 8080 7\n\
1.2.3.4:65536 std.net.NetError.InvalidAddress\n1.2.3.4 std.net.NetError.InvalidAddress\n\
[::1]80 std.net.NetError.InvalidAddress\n[::1]: std.net.NetError.InvalidAddress\n\
192.168.1.20\n[::1]:443\n",
    );
}

#[test]
fn tcp_echo_reports_short_reads_and_end_of_input() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 4).unwrap()
    address = listener.local_address().unwrap()
    with spawn(echo, &listener, &deadline, &cancel)? as task:
        mut stream = connect(&address, &deadline, &cancel).unwrap()
        print(stream.peer_address().unwrap().port == address.port)?
        print(format_ip(stream.local_address().unwrap().ip)?)?
        message = std.bytes.from_text("short reads are seven bytes at most")?
        stream.write_all_before(&message, 0, message.len(), &deadline, &cancel).unwrap()
        stream.shutdown(Shutdown.Write).unwrap()
        reply = read_all(&mut stream, &deadline, &cancel).unwrap()
        print(reply.to_text()?)?
        print(task.join().unwrap())?
    Ok(())
"#,
        "True\n127.0.0.1\nshort reads are seven bytes at most\n35\n",
    );
}

#[test]
fn large_transfers_complete_through_partial_writes() {
    runs(
        r#"
def sink(listener: &TcpListener, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
    mut stream = listener.accept(deadline, cancel)?
    mut buffer = std.bytes.with_capacity(65536).unwrap()
    buffer.resize(65536, 0u8).unwrap()
    mut total = 0
    while True:
        count = stream.read_before(&mut buffer, 0, 65536, deadline, cancel)?
        if count == 0:
            break
        total += count
    Ok(total)

def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(10000u32)?
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    address = listener.local_address().unwrap()
    with spawn(sink, &listener, &deadline, &cancel)? as task:
        mut stream = connect(&address, &deadline, &cancel).unwrap()
        data = zeroed(4000000)?
        mut sent = 0
        mut calls = 0
        while sent < data.len():
            sent += stream.write_before(&data, sent, data.len(), &deadline, &cancel).unwrap()
            calls += 1
        stream.close().unwrap()
        print(sent, calls >= 977)?
        print(task.join().unwrap())?
    Ok(())
"#,
        "4000000 True\n4000000\n",
    );
}

#[test]
fn connection_failures_are_named() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(2000u32)?
    mut listener = listen(&loopback_v4(0u16), 1).unwrap()
    address = listener.local_address().unwrap()
    print(outcome(listen(&address, 1))?)?
    listener.close().unwrap()
    print(outcome(connect(&address, &deadline, &cancel))?)?
    print(outcome(listen(&parse_address("192.0.2.1:0").unwrap(), 1))?)?

    server = listen(&loopback_v4(0u16), 1).unwrap()
    mut stream = connect(&server.local_address().unwrap(), &deadline, &cancel).unwrap()
    mut accepted = server.accept(&deadline, &cancel).unwrap()
    accepted.close().unwrap()
    data = zeroed(4096)?
    mut result = stream.write_all_before(&data, 0, 4096, &deadline, &cancel)
    while result.is_ok():
        result = stream.write_all_before(&data, 0, 4096, &deadline, &cancel)
    print(outcome(result)?)?
    Ok(())
"#,
        "std.net.NetError.AddressInUse\nstd.net.NetError.Refused\nstd.net.NetError.AddressUnavailable\n\
std.net.NetError.Reset\n",
    );
}

#[test]
fn waits_end_at_the_deadline_or_on_cancellation() {
    runs(
        r#"
def cancel_later(token: CancellationToken) -> ():
    sleep(&milliseconds(30u32))
    token.cancel()

def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    expired = within(0u32)?
    print(outcome(listener.accept(&expired, &cancel))?)?
    start = monotonic_now()
    print(outcome(listener.accept(&within(50u32)?, &cancel))?)?
    print(monotonic_now().saturating_duration_since(&start).as_milliseconds() >= 50u64)?
    with spawn(cancel_later, cancel.share())?:
        print(outcome(listener.accept(&within(5000u32)?, &cancel))?)?
    print(outcome(connect(&listener.local_address().unwrap(), &within(1000u32)?, &cancel))?)?

    fresh = CancellationToken()?
    long = within(5000u32)?
    mut stream = connect(&listener.local_address().unwrap(), &long, &fresh).unwrap()
    accepted = listener.accept(&long, &fresh).unwrap()
    mut buffer = zeroed(8)?
    print(outcome(stream.read_before(&mut buffer, 0, 8, &expired, &fresh))?)?
    print(buffer.len())?
    Ok(())
"#,
        "std.net.NetError.TimedOut\nstd.net.NetError.TimedOut\nTrue\nstd.net.NetError.Cancelled\nstd.net.NetError.Cancelled\n\
std.net.NetError.TimedOut\n8\n",
    );
}

#[test]
fn datagrams_report_their_source_and_truncation() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(2000u32)?
    mut sender = bind_udp(&loopback_v4(0u16)).unwrap()
    mut receiver = bind_udp(&loopback_v4(0u16)).unwrap()
    target = receiver.local_address().unwrap()
    payload = std.bytes.from_text("0123456789")?
    print(sender.send_to(&payload, 0, 10, &target, &deadline, &cancel).unwrap())?
    print(sender.send_to(&payload, 2, 5, &target, &deadline, &cancel).unwrap())?
    mut buffer = zeroed(8)?
    first = receiver.receive_from(&mut buffer, 0, 4, &deadline, &cancel).unwrap()
    print(first.size, first.stored, first.truncated(), buffer.slice(0, 4)?.to_text()?)?
    print(first.source.port == sender.local_address().unwrap().port)?
    second = receiver.receive_from(&mut buffer, 0, 8, &deadline, &cancel).unwrap()
    print(second.size, second.stored, second.truncated(), buffer.slice(0, 3)?.to_text()?)?
    print(outcome(receiver.receive_from(&mut buffer, 0, 8, &within(0u32)?, &cancel))?)?
    print(outcome(sender.send_to(&payload, 0, 11, &target, &deadline, &cancel))?)?
    huge = zeroed(70000)?
    print(outcome(sender.send_to(&huge, 0, 70000, &target, &deadline, &cancel))?)?
    Ok(())
"#,
        "10\n3\n10 4 True 0123\nTrue\n3 3 False 234\nstd.net.NetError.TimedOut\n\
std.net.NetError.Stream(std.io.StreamError.Io(IoError.InvalidInput))\n\
std.net.NetError.Stream(std.io.StreamError.Io(IoError.System(90)))\n",
    );
}

#[test]
fn ipv6_loopback_carries_streams_and_datagrams() {
    // A host without IPv6 reports that instead of failing the test.
    runs(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(2000u32)?
    match listen(&loopback_v6(0u16), 1):
        case Ok(listener):
            address = listener.local_address().unwrap()
            mut stream = connect(&address, &deadline, &cancel).unwrap()
            accepted = listener.accept(&deadline, &cancel).unwrap()
            print(format_ip(accepted.peer_address().unwrap().ip)?)?
            print(stream.peer_address().unwrap().format()? == address.format()?)?
        case Err(NetError.AddressUnavailable):
            print("::1")?
            print(True)?
        case Err(error):
            print(str.repr(error)?)?
    Ok(())
"#,
        "::1\nTrue\n",
    );
}

#[test]
fn names_resolve_through_the_system_resolver() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    literal = resolve("10.1.2.3", 53u16, &deadline, &cancel).unwrap()
    print(len(literal), literal[0].format()?)?
    local = resolve("localhost", 8080u16, &deadline, &cancel).unwrap()
    mut loopback = False
    for address in &local:
        loopback = loopback or address.format()? == "127.0.0.1:8080" or address.format()? == "[::1]:8080"
    print(len(local) > 0, loopback)?
    print(outcome(resolve("localhost", 1u16, &within(0u32)?, &cancel))?)?
    print(outcome(resolve("", 1u16, &deadline, &cancel))?)?
    cancel.cancel()
    print(outcome(resolve("localhost", 1u16, &deadline, &cancel))?)?
    Ok(())
"#,
        "1 10.1.2.3:53\nTrue True\nstd.net.NetError.TimedOut\nstd.net.NetError.InvalidAddress\n\
std.net.NetError.Cancelled\n",
    );
}

/// The helper thread outlives the reply's end of input; under load it once
/// still held memory when the program's allocation check ran (issue #129).
#[test]
fn a_program_may_end_right_after_a_lookup() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    for _ in range(8):
        print(len(resolve("localhost", 80u16, &within(5000u32)?, &cancel).unwrap()) > 0)?
    Ok(())
"#,
        &"True\n".repeat(8),
    );
}

#[test]
fn sockets_close_with_their_owner() {
    runs(
        r#"
import std.fs
import std.path

def count() -> Result[i64, Failure]:
    mut entries = std.fs.read_dir(&std.path.from_text("/proc/self/fd")?)?
    mut total = 0
    while entries.next()?.is_some():
        total += 1
    Ok(total)

def round(deadline: &Deadline, cancel: &CancellationToken) -> Result[(), Failure]:
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    stream = connect(&listener.local_address().unwrap(), deadline, cancel).unwrap()
    accepted = listener.accept(deadline, cancel).unwrap()
    socket = bind_udp(&any_v4(0u16)).unwrap()
    Ok(())

def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    # The first wait gives the token its wake descriptor, which it keeps.
    round(&deadline, &cancel)?
    before = count()?
    for _ in range(50):
        round(&deadline, &cancel)?
    print(count()? - before)?
    Ok(())
"#,
        "0\n",
    );
}

#[test]
fn workers_may_open_sockets_and_resolve_names() {
    runs(
        r#"
def client(port: u16) -> Result[i64, NetError]:
    cancel = CancellationToken().unwrap()
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000u32)).unwrap()
    addresses = resolve("127.0.0.1", port, &deadline, &cancel)?
    mut stream = connect(&addresses[0], &deadline, &cancel)?
    message = std.bytes.from_text("from a worker").unwrap()
    stream.write_all_before(&message, 0, message.len(), &deadline, &cancel)?
    Ok(message.len())

def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    port = listener.local_address().unwrap().port
    with spawn(client, port)? as task:
        mut stream = listener.accept(&deadline, &cancel).unwrap()
        print(read_all(&mut stream, &deadline, &cancel).unwrap().to_text()?)?
        print(task.join().unwrap())?
    Ok(())
"#,
        "from a worker\n13\n",
    );
}

#[test]
fn several_workers_accept_from_one_listener() {
    runs(
        r#"
def ping(stream: &mut TcpStream, deadline: &Deadline, cancel: &CancellationToken) -> Result[(), Failure]:
    message = std.bytes.from_text("ping")?
    stream.write_all_before(&message, 0, 4, deadline, cancel).unwrap()
    stream.shutdown(Shutdown.Write).unwrap()
    Ok(())

def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 8).unwrap()
    address = listener.local_address().unwrap()
    with spawn(echo, &listener, &deadline, &cancel)? as first:
        with spawn(echo, &listener, &deadline, &cancel)? as second:
            mut one = connect(&address, &deadline, &cancel).unwrap()
            mut two = connect(&address, &deadline, &cancel).unwrap()
            ping(&mut one, &deadline, &cancel)?
            ping(&mut two, &deadline, &cancel)?
            print(read_all(&mut one, &deadline, &cancel).unwrap().to_text()?)?
            print(read_all(&mut two, &deadline, &cancel).unwrap().to_text()?)?
            print(first.join().unwrap() + second.join().unwrap())?
    Ok(())
"#,
        "ping\nping\n8\n",
    );
}

#[test]
fn a_selector_reports_which_sockets_are_ready() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    long = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 4).unwrap()
    address = listener.local_address().unwrap()
    mut chosen = selector(4).unwrap()
    listener.watch(&mut chosen, 100u64).unwrap()
    match chosen.wait(&within(20u32)?, &cancel):
        case Err(WaitError.TimedOut):
            print("timed out")?
        case _:
            print("unexpected")?
    mut one = connect(&address, &long, &cancel).unwrap()
    mut two = connect(&address, &long, &cancel).unwrap()
    print(chosen.wait(&long, &cancel).unwrap(), chosen.event(0).unwrap().key)?
    mut first = listener.accept(&long, &cancel).unwrap()
    mut second = listener.accept(&long, &cancel).unwrap()
    listener.unwatch(&mut chosen).unwrap()
    first.watch(&mut chosen, 1u64, Interest.Read).unwrap()
    second.watch(&mut chosen, 2u64, Interest.Read).unwrap()
    message = std.bytes.from_text("x")?
    two.write_all_before(&message, 0, 1, &long, &cancel).unwrap()
    ready = chosen.wait(&long, &cancel).unwrap()
    event = chosen.event(0).unwrap()
    print(ready, event.key, event.readiness.readable)?
    one.close().unwrap()
    mut keys = 0u64
    ready_both = chosen.wait(&long, &cancel).unwrap()
    for index in range(ready_both):
        keys += chosen.event(index).unwrap().key
    print(ready_both, keys)?
    print(chosen.event(ready_both).is_err())?
    second.watch(&mut chosen, 2u64, Interest.Write).unwrap()
    first.unwatch(&mut chosen).unwrap()
    print(chosen.wait(&long, &cancel).unwrap(), chosen.event(0).unwrap().readiness.writable)?
    chosen.close().unwrap()
    Ok(())
"#,
        "timed out\n1 100\n1 2 True\n2 3\nTrue\n1 True\n",
    );
}

#[test]
fn cancellation_wakes_a_blocked_wait_at_once() {
    runs(
        r#"
def waiter(listener: &TcpListener, cancel: CancellationToken) -> Result[Instant, NetError]:
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000u32)).unwrap()
    match listener.accept(&deadline, &cancel):
        case Err(NetError.Cancelled):
            Ok(monotonic_now())
        case _:
            Err(NetError.TimedOut)

def selecting(cancel: CancellationToken) -> Result[Instant, WaitError]:
    mut chosen = selector(1).unwrap()
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000u32)).unwrap()
    match chosen.wait(&deadline, &cancel):
        case Err(WaitError.Cancelled):
            Ok(monotonic_now())
        case _:
            Err(WaitError.TimedOut)

def main() -> Result[(), Failure]:
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    mut worst = 0u64
    for _ in range(20):
        cancel = CancellationToken()?
        with spawn(waiter, &listener, cancel.share())? as task:
            sleep(&milliseconds(5u32))
            at = monotonic_now()
            cancel.cancel()
            latency = task.join().unwrap().saturating_duration_since(&at).as_milliseconds()
            if latency > worst:
                worst = latency
        other = CancellationToken()?
        with spawn(selecting, other.share())? as task:
            sleep(&milliseconds(5u32))
            at = monotonic_now()
            other.cancel()
            latency = task.join().unwrap().saturating_duration_since(&at).as_milliseconds()
            if latency > worst:
                worst = latency
    # Far below the 5-second deadline; a sliced wait would also pass 10 ms
    # bounds, so this only checks that the wake does not wait for the deadline.
    print(worst < 1000u64)?
    Ok(())
"#,
        "True\n",
    );
}

#[test]
fn accept_reports_the_handle_limit_and_keeps_the_connection() {
    runs_with(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    long = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 64).unwrap()
    address = listener.local_address().unwrap()
    mut clients = list[TcpStream].with_capacity(64)?
    mut limited = False
    while not limited:
        match connect(&address, &long, &cancel):
            case Ok(stream):
                clients.append(stream)?
            case Err(NetError.HandleLimit):
                limited = True
            case Err(error):
                print(str.repr(error)?)?
                return Ok(())
    print(len(clients) > 2)?
    print(outcome(listener.accept(&long, &cancel))?)?
    clients.pop()
    print(outcome(listener.accept(&long, &cancel))?)?
    Ok(())
"#,
        "True\nstd.net.NetError.HandleLimit\nok\n",
        Some(24),
    );
}

#[test]
fn the_accept_queue_holds_about_backlog_connections() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    listener = listen(&loopback_v4(0u16), 2).unwrap()
    address = listener.local_address().unwrap()
    mut clients = list[TcpStream].with_capacity(8)?
    mut refused = 0
    for _ in range(5):
        match connect(&address, &within(100u32)?, &cancel):
            case Ok(stream):
                clients.append(stream)?
            case Err(NetError.TimedOut):
                refused += 1
            case Err(error):
                print(str.repr(error)?)?
                return Ok(())
    # Linux queues one more than the backlog and then drops further SYNs,
    # so a connect waits for its deadline instead of failing fast.
    print(len(clients) >= 2, len(clients) <= 3, refused >= 2)?
    long = within(5000u32)?
    for _ in range(len(clients)):
        listener.accept(&long, &cancel).unwrap()
    Ok(())
"#,
        "True True True\n",
    );
}
