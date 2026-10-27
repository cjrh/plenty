//! std.net over loopback only: no test needs a network, a resolver beyond
//! the hosts file, or a fixed port.

mod support;
use std::process::{Command, Stdio};
use support::write_main as written;

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
    // A regression to blocking in an immediate operation must fail the test,
    // not leave the whole suite waiting for input that will never be sent.
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("network program did not finish: {:?}", output);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
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

def within(ms: u32) -> Result[Deadline, RangeError]:
    Ok(deadline_after(&monotonic_now(), &milliseconds(ms))?)

def zeroed(size: i64) -> Result[Bytes, std.bytes.BytesError]:
    mut out = std.bytes.with_capacity(size)?
    out.resize(size, 0u8)?
    Ok(out)

def outcome[T](result: Result[T, NetError]) -> Result[str, AllocError]:
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
    stream.acknowledge_now()?
    stream.write_all_before(&data, 0, data.len(), deadline, cancel)?
    stream.shutdown(Shutdown.Write)?
    Ok(data.len())
"#;

#[test]
fn immediate_tcp_operations_handle_stale_readiness_and_eof() {
    runs(
        r#"
def main() -> Result[(), std.bytes.BytesError | NetError | WaitError | RangeError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    mut listener = listen(&loopback_v4(0u16), 4)?
    print(outcome(listener.accept_now())?)?
    mut chosen = selector(4)?
    listener.watch(&mut chosen, 1u64)?
    mut client = connect(&listener.local_address()?, &deadline, &cancel)?
    chosen.wait(&deadline, &cancel)?
    mut server = listener.accept_now()?
    # Another accept consumes the readiness report without waiting again.
    print(outcome(listener.accept_now())?)?
    listener.unwatch(&mut chosen)?
    server.watch(&mut chosen, 2u64, Interest.Read)?
    mut buffer = zeroed(4)?
    print(outcome(server.read_now(&mut buffer, 1, 3))?, buffer[1])?
    print(client.write_now(&b"!x!", 1, 2)?)?
    chosen.wait(&deadline, &cancel)?
    print(server.read_now(&mut buffer, 1, 3)?, buffer[0], buffer[1], buffer[2])?
    print(outcome(server.read_now(&mut buffer, 1, 3))?)?
    print(server.read_now(&mut buffer, 2, 2)?, client.write_now(&b"", 0, 0)?)?
    client.shutdown(Shutdown.Write)?
    chosen.wait(&deadline, &cancel)?
    print(server.read_now(&mut buffer, 0, 4)?)?
    server.close().unwrap()
    print(outcome(server.read_now(&mut buffer, 0, 1))?)?
    print(outcome(server.write_now(&b"x", 0, 1))?)?
    listener.close()?
    print(outcome(listener.accept_now())?)?
    Ok(())
"#,
        "std.net.NetError.WouldBlock\nstd.net.NetError.WouldBlock\n\
std.net.NetError.WouldBlock 0\n1\n1 0 120 0\nstd.net.NetError.WouldBlock\n0 0\n0\n\
std.net.NetError.Stream(std.io.StreamError.Io(IoError.Closed))\n\
std.net.NetError.Stream(std.io.StreamError.Io(IoError.Closed))\n\
std.net.NetError.Stream(std.io.StreamError.Io(IoError.Closed))\n",
    );
}

#[test]
fn immediate_tcp_ranges_are_checked_before_attempting_io() {
    runs(
        r#"
def main() -> Result[(), std.bytes.BytesError | NetError | RangeError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 1)?
    mut client = connect(&listener.local_address()?, &deadline, &cancel)?
    mut server = listener.accept_now()?
    mut buffer = zeroed(4)?
    for bounds in [(-1, 1), (3, 2), (0, 5), (1, i64.MAX)]?:
        (start, stop) = bounds
        print(outcome(server.read_now(&mut buffer, start, stop))?)?
        print(outcome(client.write_now(&buffer, start, stop))?)?
    print(outcome(server.read_now(&mut buffer, 0, 4))?)?
    print(buffer[0], buffer[3])?
    Ok(())
"#,
        &format!(
            "{}std.net.NetError.WouldBlock\n0 0\n",
            "std.net.NetError.Stream(std.io.StreamError.OutOfRange)\n".repeat(8)
        ),
    );
}

#[test]
fn immediate_tcp_writes_report_backpressure_and_resume_after_drain() {
    runs(
        r#"
def main() -> Result[(), std.bytes.BytesError | NetError | RangeError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(10000u32)?
    listener = listen(&loopback_v4(0u16), 1)?
    mut client = connect(&listener.local_address()?, &deadline, &cancel)?
    mut server = listener.accept_now()?
    data = zeroed(8388608)?
    mut sent = 0
    mut short = False
    mut larger_than_page = False
    while True:
        match client.write_now(&data, 0, data.len()):
            case Ok(count):
                if count <= 0 or count > data.len() or sent >= 67108864:
                    return Err(IoError.InvalidInput)
                short = short or count < data.len()
                larger_than_page = larger_than_page or count > 4096
                sent += count
            case Err(NetError.WouldBlock):
                break
            case Err(error):
                return Err(error)
    mut buffer = zeroed(65536)?
    mut received = 0
    while received < sent:
        count = server.read_before(&mut buffer, 0, 65536, &deadline, &cancel)?
        if count == 0:
            return Err(IoError.InvalidInput)
        received += count
    print(short, larger_than_page, received == sent)?
    print(outcome(server.read_now(&mut buffer, 0, 1))?)?
    mut chosen = selector(1).unwrap()
    client.watch(&mut chosen, 1u64, Interest.Write)?
    chosen.wait(&deadline, &cancel).unwrap()
    print(client.write_now(&b"x", 0, 1)?)?
    print(server.read_before(&mut buffer, 0, 1, &deadline, &cancel)?, buffer[0])?
    Ok(())
"#,
        "True True True\nstd.net.NetError.WouldBlock\n1\n1 120\n",
    );
}

#[test]
fn addresses_parse_and_format() {
    runs(
        r#"
def show(text: &str) -> Result[(), NetError | AllocError | IoError]:
    match parse_ip(text):
        case Ok(ip):
            print(format_ip(ip)?)?
        case Err(error):
            print(text, str.repr(error)?)?
    Ok(())

def show_address(text: &str) -> Result[(), NetError | AllocError | IoError]:
    match parse_address(text):
        case Ok(address):
            print(address.format()?, address.port, address.scope)?
        case Err(error):
            print(text, str.repr(error)?)?
    Ok(())

def main() -> Result[(), NetError | AllocError | IoError]:
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

/// Issue #256. The runtime writes whole address entries, so a truncated reply
/// needs a partial write; this compiles the private decoder from the module
/// source to feed it short entries directly.
#[test]
fn truncated_address_entries_are_invalid() {
    let module = include_str!("../stdlib/std/net.plenty");
    let start = module.find("def number(").unwrap();
    let end = module.find("def address_of(").unwrap();
    let decoder = &module[start..end];
    runs(
        &format!(
            r#"
const v4_size: i64 = 7
const v6_size: i64 = 23

{decoder}
def show(data: &list[u8]) -> Result[(), NetError | AllocError | IoError]:
    match decoded(data, 0):
        case Ok(pair):
            (address, next) = pair
            print(address.format()?, next)?
        case Err(error):
            print(str.repr(error)?)?
    Ok(())

def main() -> Result[(), NetError | AllocError | IoError]:
    show(&[4u8]?)?
    show(&[6u8, 0u8]?)?
    show(&[4u8, 0u8, 80u8, 127u8, 0u8, 0u8]?)?
    show(&[6u8, 0u8, 80u8, 0u8, 0u8, 0u8, 0u8, 0u8]?)?
    show(&[4u8, 0u8, 80u8, 127u8, 0u8, 0u8, 1u8]?)?
    Ok(())
"#
        ),
        "std.net.NetError.InvalidAddress\nstd.net.NetError.InvalidAddress\n\
std.net.NetError.InvalidAddress\nstd.net.NetError.InvalidAddress\n127.0.0.1:80 7\n",
    );
}

#[test]
fn tcp_echo_reports_short_reads_and_end_of_input() {
    runs(
        r#"
def main() -> Result[(), std.bytes.BytesError | NetError | RangeError | ThreadError | AllocError | IoError]:
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
fn large_transfers_offer_the_remaining_range_without_a_page_cap() {
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

def main() -> Result[(), std.bytes.BytesError | RangeError | ThreadError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(10000u32)?
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    address = listener.local_address().unwrap()
    with spawn(sink, &listener, &deadline, &cancel)? as task:
        mut stream = connect(&address, &deadline, &cancel).unwrap()
        data = zeroed(4000000)?
        mut sent = 0
        mut larger_than_page = False
        while sent < data.len():
            count = stream.write_before(&data, sent, data.len(), &deadline, &cancel).unwrap()
            if count <= 0 or count > data.len() - sent:
                return Err(IoError.InvalidInput)
            larger_than_page = larger_than_page or count > 4096
            sent += count
        stream.close().unwrap()
        print(sent, larger_than_page)?
        print(task.join().unwrap())?
    Ok(())
"#,
        "4000000 True\n4000000\n",
    );
}

#[test]
fn tcp_writes_validate_the_whole_requested_range() {
    runs(
        r#"
def main() -> Result[(), std.bytes.BytesError | RangeError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    mut stream = connect(&listener.local_address().unwrap(), &deadline, &cancel).unwrap()
    mut peer = listener.accept(&deadline, &cancel).unwrap()
    data = zeroed(8192)?
    print(outcome(stream.write_before(&data, 0, 8193, &deadline, &cancel))?)?
    print(outcome(stream.write_before(&data, 1, i64.MAX, &deadline, &cancel))?)?
    print(outcome(stream.write_before(&data, -1, i64.MAX, &deadline, &cancel))?)?
    print(outcome(stream.write_before(&data, 8193, 8192, &deadline, &cancel))?)?
    print(stream.write_before(&data, 8192, 8192, &deadline, &cancel).unwrap())?
    stream.shutdown(Shutdown.Write).unwrap()
    mut received = zeroed(1)?
    print(peer.read_before(&mut received, 0, 1, &deadline, &cancel).unwrap())?
    Ok(())
"#,
        "std.net.NetError.Stream(std.io.StreamError.OutOfRange)\n\
std.net.NetError.Stream(std.io.StreamError.OutOfRange)\n\
std.net.NetError.Stream(std.io.StreamError.OutOfRange)\n\
std.net.NetError.Stream(std.io.StreamError.OutOfRange)\n0\n0\n",
    );
}

#[test]
fn connection_failures_are_named() {
    runs(
        r#"
def main() -> Result[(), std.bytes.BytesError | RangeError | AllocError | IoError]:
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
fn plain_stream_io_still_waits_on_nonblocking_descriptors() {
    runs(
        r#"
import std.io

def delayed_sink(listener: &TcpListener, deadline: &Deadline, cancel: &CancellationToken) -> ():
    mut stream = listener.accept(deadline, cancel).unwrap()
    sleep(&milliseconds(100u32))
    mut buffer = zeroed(65536).unwrap()
    mut count = 0
    while count < 8000000:
        count += stream.read(&mut buffer, 0, 65536).unwrap()
    sleep(&milliseconds(30u32))
    std.io.write_all(&mut stream, &b"done", 0, 4).unwrap()

def main() -> Result[(), std.bytes.BytesError | RangeError | ThreadError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    with spawn(delayed_sink, &listener, &deadline, &cancel)?:
        mut stream = connect(&listener.local_address().unwrap(), &deadline, &cancel).unwrap()
        data = zeroed(8000000)?
        std.io.write_all(&mut stream, &data, 0, data.len()).unwrap()
        mut reply = zeroed(4)?
        std.io.read_exact(&mut stream, &mut reply, 0, 4).unwrap()
        print(reply.to_text()?)?
    Ok(())
"#,
        "done\n",
    );
}

#[test]
fn backpressured_writes_obey_deadlines_and_cancellation() {
    runs(
        r#"
def cancel_later(token: CancellationToken) -> ():
    sleep(&milliseconds(30u32))
    token.cancel()

def main() -> Result[(), std.bytes.BytesError | RangeError | ThreadError | AllocError | IoError]:
    cancel = CancellationToken()?
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    deadline = within(5000u32)?
    mut stream = connect(&listener.local_address().unwrap(), &deadline, &cancel).unwrap()
    accepted = listener.accept(&deadline, &cancel).unwrap()
    data = zeroed(8000000)?
    short = within(50u32)?
    mut result = stream.write_before(&data, 0, data.len(), &short, &cancel)
    while result.is_ok():
        result = stream.write_before(&data, 0, data.len(), &short, &cancel)
    print(outcome(result)?)?
    with spawn(cancel_later, cancel.share())?:
        mut cancelled = stream.write_before(&data, 0, data.len(), &deadline, &cancel)
        while cancelled.is_ok():
            cancelled = stream.write_before(&data, 0, data.len(), &deadline, &cancel)
        print(outcome(cancelled)?)?
    Ok(())
"#,
        "std.net.NetError.TimedOut\nstd.net.NetError.Cancelled\n",
    );
}

/// Feed stale-readiness outcomes directly to the production retry policy;
/// reproducing a kernel readiness race by timing threads is nondeterministic.
#[test]
fn stale_transfer_readiness_rechecks_the_original_budget() {
    let module = include_str!("../stdlib/std/net.plenty");
    let blocked = &module
        [module.find("def blocked(").unwrap()..module.find("# Plain Reader/Writer").unwrap()];
    let retry = &module[module.find("def retry_transfer(").unwrap()
        ..module.find("# Address encoding shared").unwrap()];
    let streamed =
        &module[module.find("def streamed(").unwrap()..module.find("def waited(").unwrap()];
    runs(
        &format!(
            r#"
from std.io import StreamError
const would_block: i32 = 11
def named(error: IoError) -> NetError:
    NetError.Stream(StreamError.Io(error))
{blocked}
{streamed}
{retry}
def main() -> Result[(), RangeError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    again = StreamError.Io(IoError.System(11i32))
    print(outcome(retry_transfer(again, &deadline, &cancel))?)?
    print(outcome(retry_transfer(StreamError.Interrupted, &deadline, &cancel))?)?
    print(outcome(retry_transfer(again, &within(0u32)?, &cancel))?)?
    cancel.cancel()
    print(outcome(retry_transfer(again, &deadline, &cancel))?)?
    print(outcome(retry_transfer(StreamError.Io(IoError.Closed), &deadline, &cancel))?)?
    Ok(())
"#
        ),
        "ok\nok\nstd.net.NetError.TimedOut\nstd.net.NetError.Cancelled\nstd.net.NetError.Stream(std.io.StreamError.Io(IoError.Closed))\n",
    );
}

/// Kernel TCP timeouts take minutes and depend on packet loss. Exercise the
/// production error policy directly so the regression needs neither timing
/// tricks nor privileged network configuration.
#[test]
fn kernel_connection_timeouts_are_not_deadline_expiry() {
    let module = include_str!("../stdlib/std/net.plenty");
    let constants = &module
        [module.find("const would_block:").unwrap()..module.find("const max_port:").unwrap()];
    let errors =
        &module[module.find("def named(").unwrap()..module.find("def ready_for(").unwrap()];
    let blocked = &module
        [module.find("def blocked(").unwrap()..module.find("# Plain Reader/Writer").unwrap()];
    let retry = &module[module.find("def retry_transfer(").unwrap()
        ..module.find("# Address encoding shared").unwrap()];
    let client = include_str!("../stdlib/std/http/client.plenty");
    let deadline_policy = client
        [client.find("def timed_out(").unwrap()..client.find("# Copies `body`").unwrap()]
        .replacen("def timed_out(", "def client_deadline_passed(", 1);
    runs(
        &format!(
            r#"
from std.io import StreamError, classify
from std.tls import TlsError
from std.http.client import ClientError
from std.http.https import secure_error
{constants}
{errors}
{blocked}
{retry}
{deadline_policy}
def main() -> Result[(), RangeError | AllocError | IoError]:
    kernel = IoError.System(110i32)
    print(named(kernel))?
    print(streamed(StreamError.Io(kernel)))?
    print(waited(WaitError.Stream(StreamError.Io(kernel))))?
    print(waited(WaitError.TimedOut))?
    cancel = CancellationToken()?
    cancel.cancel()
    print(outcome(retry_transfer(StreamError.Io(kernel), &within(0u32)?, &cancel))?)?
    failed = secure_error(TlsError.Net(named(kernel)))
    expired = secure_error(TlsError.Net(waited(WaitError.TimedOut)))
    print(failed, expired)?
    print(client_deadline_passed(&failed), client_deadline_passed(&expired))?
    print(named(IoError.System(11i32)))?
    print(streamed(StreamError.Interrupted))?
    print(named(IoError.System(103i32)))?
    Ok(())
"#
        ),
        "std.net.NetError.ConnectionTimedOut\nstd.net.NetError.ConnectionTimedOut\n\
std.net.NetError.ConnectionTimedOut\nstd.net.NetError.TimedOut\n\
std.net.NetError.ConnectionTimedOut\n\
std.http.client.ClientError.Net(std.net.NetError.ConnectionTimedOut) std.http.client.ClientError.Net(std.net.NetError.TimedOut)\n\
False True\nstd.net.NetError.WouldBlock\n\
std.net.NetError.Stream(std.io.StreamError.Interrupted)\n\
std.net.NetError.Stream(std.io.StreamError.Io(IoError.System(103)))\n",
    );
}

#[test]
fn waits_end_at_the_deadline_or_on_cancellation() {
    runs(
        r#"
def cancel_later(token: CancellationToken) -> ():
    sleep(&milliseconds(30u32))
    token.cancel()

def main() -> Result[(), std.bytes.BytesError | RangeError | ThreadError | AllocError | IoError]:
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
def main() -> Result[(), std.bytes.BytesError | RangeError | AllocError | IoError]:
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

/// Selectors other than 0 (local) and 1 (peer) are rejected (issue #278).
#[test]
fn address_selectors_outside_local_and_peer_are_invalid() {
    runs(
        r#"
def main() -> Result[(), AllocError | IoError]:
    socket = __net_open([4u8, 0u8, 0u8, 127u8, 0u8, 0u8, 1u8]?, 2u8, 0).unwrap()
    for which in [0u8, 2u8, 255u8]?:
        match __net_address(&socket, which):
            case Ok(data):
                print(len(data))?
            case Err(error):
                print(str.repr(error)?)?
    Ok(())
"#,
        "7\nIoError.InvalidInput\nIoError.InvalidInput\n",
    );
}

/// Writes take any `std.bytes.ByteSource`, so literals need no copy (issue #156).
#[test]
fn byte_literals_are_sent_in_place() {
    runs(
        r#"
def main() -> Result[(), std.bytes.BytesError | RangeError | ThreadError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    listener = listen(&loopback_v4(0u16), 4).unwrap()
    address = listener.local_address().unwrap()
    with spawn(echo, &listener, &deadline, &cancel)? as task:
        mut stream = connect(&address, &deadline, &cancel).unwrap()
        stream.write_all_before(b"literal, ", 0, 9, &deadline, &cancel).unwrap()
        print(stream.write_before(b"then more", 5, 9, &deadline, &cancel).unwrap())?
        stream.write(b"!", 0, 1).unwrap()
        stream.shutdown(Shutdown.Write).unwrap()
        print(read_all(&mut stream, &deadline, &cancel).unwrap().to_text()?)?
        print(task.join().unwrap())?
    mut sender = bind_udp(&loopback_v4(0u16)).unwrap()
    mut receiver = bind_udp(&loopback_v4(0u16)).unwrap()
    target = receiver.local_address().unwrap()
    print(sender.send_to(b"datagram", 4, 8, &target, &deadline, &cancel).unwrap())?
    print(outcome(sender.send_to(b"datagram", 4, 9, &target, &deadline, &cancel))?)?
    mut buffer = zeroed(8)?
    got = receiver.receive_from(&mut buffer, 0, 8, &deadline, &cancel).unwrap()
    print(buffer.slice(0, got.stored)?.to_text()?)?
    Ok(())
"#,
        "4\nliteral, more!\n14\n4\n\
std.net.NetError.Stream(std.io.StreamError.Io(IoError.InvalidInput))\ngram\n",
    );
}

#[test]
fn ipv6_loopback_carries_streams_and_datagrams() {
    // A host without IPv6 reports that instead of failing the test.
    runs(
        r#"
def main() -> Result[(), NetError | RangeError | AllocError | IoError]:
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
def main() -> Result[(), NetError | RangeError | AllocError | IoError]:
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
def main() -> Result[(), RangeError | AllocError | IoError]:
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

def count() -> Result[i64, IoError]:
    mut entries = std.fs.read_dir(&std.path.from_text("/proc/self/fd")?)?
    mut total = 0
    while entries.next()?.is_some():
        total += 1
    Ok(total)

def round(deadline: &Deadline, cancel: &CancellationToken) -> ():
    listener = listen(&loopback_v4(0u16), 1).unwrap()
    stream = connect(&listener.local_address().unwrap(), deadline, cancel).unwrap()
    accepted = listener.accept(deadline, cancel).unwrap()
    socket = bind_udp(&any_v4(0u16)).unwrap()

def main() -> Result[(), RangeError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = within(5000u32)?
    # The first wait gives the token its wake descriptor, which it keeps.
    round(&deadline, &cancel)
    before = count()?
    for _ in range(50):
        round(&deadline, &cancel)
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

def main() -> Result[(), std.bytes.BytesError | RangeError | ThreadError | AllocError | IoError]:
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
def ping(stream: &mut TcpStream, deadline: &Deadline, cancel: &CancellationToken) -> Result[(), std.bytes.BytesError]:
    message = std.bytes.from_text("ping")?
    stream.write_all_before(&message, 0, 4, deadline, cancel).unwrap()
    stream.shutdown(Shutdown.Write).unwrap()
    Ok(())

def main() -> Result[(), std.bytes.BytesError | RangeError | ThreadError | AllocError | IoError]:
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
def main() -> Result[(), std.bytes.BytesError | RangeError | AllocError | IoError]:
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

def main() -> Result[(), ThreadError | AllocError | IoError]:
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
def main() -> Result[(), RangeError | AllocError | IoError]:
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
def main() -> Result[(), RangeError | AllocError | IoError]:
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
