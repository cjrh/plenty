//! `std.tls`: verified TLS streams over loopback TCP. Certificates come from
//! `tests/fixtures/tls` (see its `generate.sh`); nothing leaves 127.0.0.1.

fn fixtures() -> String {
    format!("{}/tests/fixtures/tls", env!("CARGO_MANIFEST_DIR"))
}

const HEADER: &str = r#"import std.bytes
import std.tls
from std.bytes import Bytes
from std.net import NetError, TcpListener, connect, listen, loopback_v4
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now
from std.tls import ClientConfig, ServerConfig, TlsError, TlsStream

def fixture(name: &str) -> Result[str, Failure]:
    Ok(read_text(&("FIXTURES/" + name)?)?)

def client_for(roots: &str) -> Result[ClientConfig, Failure]:
    pem = fixture(roots)?
    Ok(std.tls.client_config(&std.tls.roots_from_pem(&pem).unwrap()).unwrap())

def server_for(chain: &str) -> Result[ServerConfig, Failure]:
    Ok(std.tls.server_config(&fixture(chain)?, &fixture("leaf.key")?).unwrap())

def handshake(listener: &TcpListener, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[TlsStream, TlsError]:
    match listener.accept(deadline, cancel):
        case Ok(tcp):
            std.tls.accept(tcp, config, deadline, cancel)
        case Err(error):
            Err(TlsError.Net(error))

# Reads until close_notify, returning the byte count.
def drain(stream: &mut TlsStream, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, TlsError]:
    mut buffer = std.bytes.with_capacity(4096).unwrap()
    buffer.resize(4096, 0u8).unwrap()
    mut total = 0
    while True:
        count = stream.read_before(&mut buffer, 0, 4096, deadline, cancel)?
        if count == 0:
            return Ok(total)
        total += count
    Ok(total)

"#;

fn program(declarations: &str, body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{}{declarations}\ndef main() -> Result[(), Failure]:\n    cancel = CancellationToken()?\n    deadline = deadline_after(&monotonic_now(), &milliseconds(10000))?\n    listener = listen(&loopback_v4(0u16), 8).unwrap()\n    address = listener.local_address().unwrap()\n{body}    Ok(())\n",
        HEADER.replace("FIXTURES", &fixtures())
    )
}

fn compile(source: &str, dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    executable
}

fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(compile(source, dir.path()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.tls.TlsError", "TlsError")
        .replace("std.net.NetError", "NetError")
}

fn native(declarations: &str, body: &str, expected: &str) {
    assert_eq!(
        output(&program(declarations, body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

const ECHO: &str = r#"
def echo(listener: &TcpListener, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, TlsError]:
    mut stream = handshake(listener, config, deadline, cancel)?
    mut buffer = std.bytes.with_capacity(64).unwrap()
    buffer.resize(64, 0u8).unwrap()
    mut total = 0
    while True:
        count = stream.read_before(&mut buffer, 0, 64, deadline, cancel)?
        if count == 0:
            stream.close_before(deadline, cancel)?
            return Ok(total)
        stream.write_all_before(&buffer, 0, count, deadline, cancel)?
        total += count
    Ok(total)
"#;

#[test]
fn a_verified_client_and_server_exchange_data_and_close_cleanly() {
    native(
        ECHO,
        r#"
server = server_for("leaf.pem")?
client = client_for("ca.pem")?
with spawn(echo, &listener, &server, &deadline, &cancel)? as worker:
    tcp = connect(&address, &deadline, &cancel).unwrap()
    mut stream = std.tls.connect(tcp, &client, "localhost", &deadline, &cancel).unwrap()
    message = std.bytes.from_text("hello")?
    stream.write_all_before(&message, 0, 5, &deadline, &cancel).unwrap()
    mut reply = std.bytes.with_capacity(16)?
    reply.resize(16, 0u8)?
    count = stream.read_before(&mut reply, 0, 16, &deadline, &cancel).unwrap()
    print(reply.slice(0, count)?.to_text()?)?
    stream.close_before(&deadline, &cancel).unwrap()
    print(worker.join().unwrap())?
"#,
        "hello\n5",
    );
}

#[test]
fn an_ip_address_name_is_checked_against_the_certificate() {
    native(
        ECHO,
        r#"
server = server_for("leaf.pem")?
client = client_for("ca.pem")?
with spawn(echo, &listener, &server, &deadline, &cancel)? as worker:
    tcp = connect(&address, &deadline, &cancel).unwrap()
    mut stream = std.tls.connect(tcp, &client, "127.0.0.1", &deadline, &cancel).unwrap()
    stream.close_before(&deadline, &cancel).unwrap()
    print(worker.join().unwrap())?
"#,
        "0",
    );
}

const VERIFY: &str = r#"
def attempt(chain: &str, roots: &str, name: &str) -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(10000))?
    listener = listen(&loopback_v4(0u16), 8).unwrap()
    address = listener.local_address().unwrap()
    server = server_for(chain)?
    client = client_for(roots)?
    with spawn(handshake, &listener, &server, &deadline, &cancel)? as worker:
        tcp = connect(&address, &deadline, &cancel).unwrap()
        match std.tls.connect(tcp, &client, name, &deadline, &cancel):
            case Ok(_):
                print("connected")?
            case Err(error):
                print(error)?
        match worker.join():
            case Ok(_):
                print("accepted")?
            case Err(error):
                print(error)?
    Ok(())
"#;

#[test]
fn certificates_that_fail_verification_are_named_on_both_sides() {
    native(
        VERIFY,
        r#"
attempt("expired.pem", "ca.pem", "localhost")?
attempt("untrusted.pem", "ca.pem", "localhost")?
attempt("leaf.pem", "ca.pem", "example.test")?
attempt("leaf.pem", "ca.pem", "127.0.0.2")?
attempt("leaf.pem", "other-ca.pem", "localhost")?
"#,
        "TlsError.ExpiredCertificate\nTlsError.Rejected\n\
         TlsError.UntrustedCertificate\nTlsError.Rejected\n\
         TlsError.WrongHost\nTlsError.Rejected\n\
         TlsError.WrongHost\nTlsError.Rejected\n\
         TlsError.UntrustedCertificate\nTlsError.Rejected",
    );
}

#[test]
fn configuration_errors_are_reported_before_connecting() {
    native(
        "",
        r#"
ca = fixture("ca.pem")?
other = fixture("other-ca.pem")?
print(std.tls.roots_from_pem(&ca).unwrap().len())?
print(std.tls.roots_from_pem(&(ca + other)?).unwrap().len())?
match std.tls.roots_from_pem(""):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected")?
match std.tls.roots_from_pem("-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n"):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected")?
# Well-formed PEM whose certificate webpki cannot use is skipped.
match std.tls.roots_from_pem("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n"):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected")?
# The leaf's key does not match the CA certificate.
match std.tls.server_config(&ca, &fixture("leaf.key")?):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected")?
match std.tls.server_config(&fixture("leaf.pem")?, "not a key"):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected")?
client = client_for("ca.pem")?
tcp = connect(&address, &deadline, &cancel).unwrap()
match std.tls.connect(tcp, &client, "not a name!", &deadline, &cancel):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected")?
"#,
        "1\n2\nTlsError.NoTrustRoots\nTlsError.InvalidCertificate\nTlsError.NoTrustRoots\n\
         TlsError.InvalidConfiguration\nTlsError.InvalidConfiguration\nTlsError.InvalidName",
    );
}

#[test]
fn a_silent_server_times_the_handshake_out_and_cancellation_stops_it() {
    // The listener's backlog completes the TCP connection, but nobody
    // answers the ClientHello.
    native(
        "",
        r#"
client = client_for("ca.pem")?
soon = deadline_after(&monotonic_now(), &milliseconds(200))?
tcp = connect(&address, &deadline, &cancel).unwrap()
match std.tls.connect(tcp, &client, "localhost", &soon, &cancel):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected")?
cancelled = CancellationToken()?
cancelled.cancel()
again = connect(&address, &deadline, &cancel).unwrap()
match std.tls.connect(again, &client, "localhost", &deadline, &cancelled):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected")?
"#,
        "TlsError.Net(NetError.TimedOut)\nTlsError.Net(NetError.Cancelled)",
    );
}

#[test]
fn a_connection_cut_without_close_notify_is_truncated() {
    native(
        r#"
def cut(listener: &TcpListener, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[(), TlsError]:
    mut stream = handshake(listener, config, deadline, cancel)?
    message = std.bytes.from_text("partial").unwrap()
    stream.write_all_before(&message, 0, 7, deadline, cancel)?
    # Dropping the stream closes the socket without close_notify.
    Ok(())
"#,
        r#"
server = server_for("leaf.pem")?
client = client_for("ca.pem")?
with spawn(cut, &listener, &server, &deadline, &cancel)? as worker:
    tcp = connect(&address, &deadline, &cancel).unwrap()
    mut stream = std.tls.connect(tcp, &client, "localhost", &deadline, &cancel).unwrap()
    worker.join().unwrap()
    match drain(&mut stream, &deadline, &cancel):
        case Err(error):
            print(error)?
        case Ok(count):
            print(count)?
"#,
        "TlsError.Truncated",
    );
}

#[test]
fn large_transfers_cross_many_records_through_both_interfaces() {
    // `write_before` sends at most one 16 KiB record; `write` and `read` are
    // the std.io Writer and Reader methods.
    native(
        r#"
def count(listener: &TcpListener, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, TlsError]:
    mut stream = handshake(listener, config, deadline, cancel)?
    total = drain(&mut stream, deadline, cancel)?
    stream.close_before(deadline, cancel)?
    Ok(total)
"#,
        r#"
server = server_for("leaf.pem")?
client = client_for("ca.pem")?
mut data = std.bytes.with_capacity(200000)?
data.resize(200000, 120u8)?
with spawn(count, &listener, &server, &deadline, &cancel)? as worker:
    tcp = connect(&address, &deadline, &cancel).unwrap()
    mut stream = std.tls.connect(tcp, &client, "localhost", &deadline, &cancel).unwrap()
    print(stream.write_before(&data, 0, 200000, &deadline, &cancel).unwrap())?
    stream.write_all_before(&data, 16384, 100000, &deadline, &cancel).unwrap()
    mut at = 100000
    while at < 200000:
        at += stream.write(&data, at, 200000).unwrap()
    stream.flush().unwrap()
    stream.close().unwrap()
    print(worker.join().unwrap())?
"#,
        "16384\n200000",
    );
}

#[test]
fn reads_through_the_reader_interface_see_close_notify_as_zero() {
    native(
        ECHO,
        r#"
server = server_for("leaf.pem")?
client = client_for("ca.pem")?
with spawn(echo, &listener, &server, &deadline, &cancel)? as worker:
    tcp = connect(&address, &deadline, &cancel).unwrap()
    mut stream = std.tls.connect(tcp, &client, "localhost", &deadline, &cancel).unwrap()
    message = std.bytes.from_text("abc")?
    stream.write(&message, 0, 3).unwrap()
    mut reply = std.bytes.with_capacity(8)?
    reply.resize(8, 0u8)?
    print(stream.read(&mut reply, 0, 8).unwrap())?
    print(stream.read(&mut reply, 0, 0).unwrap())?
    stream.close().unwrap()
    print(worker.join().unwrap())?
    match stream.write(&message, 0, 3):
        case Err(_):
            print("closed")?
        case Ok(_):
            print("unexpected")?
"#,
        "3\n0\n3\nclosed",
    );
}

/// The TLS stack sits behind its own runtime entry, so programs that never
/// use `std.tls` link none of rustls or ring.
#[test]
fn programs_without_tls_do_not_link_the_tls_stack() {
    use object::{Object, ObjectSymbol};
    let symbols = |source: &str| {
        let dir = tempfile::tempdir().unwrap();
        let data = std::fs::read(compile(source, dir.path())).unwrap();
        let file = object::File::parse(&*data).unwrap();
        file.symbols()
            .filter_map(|symbol| symbol.name().ok().map(str::to_owned))
            .filter(|name| name == "plenty_tls" || name.contains("ring_core_0_17_14_"))
            .count()
    };
    let plain = "def main() -> Result[(), Failure]:\n    print(\"hi\")?\n    Ok(())\n";
    assert_eq!(symbols(plain), 0);
    assert!(symbols(&program("", "client_for(\"ca.pem\")?\n")) > 1);
}

#[test]
fn system_roots_read_the_platform_bundle() {
    let bundles = [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
        "/etc/ssl/ca-bundle.pem",
        "/etc/ssl/cert.pem",
    ];
    let expected = if bundles
        .iter()
        .any(|path| std::fs::read_to_string(path).is_ok())
    {
        "found"
    } else {
        "TlsError.NoTrustRoots"
    };
    native(
        "",
        r#"
match std.tls.system_roots():
    case Ok(roots):
        if roots.len() > 0:
            print("found")?
    case Err(error):
        print(error)?
"#,
        expected,
    );
}
