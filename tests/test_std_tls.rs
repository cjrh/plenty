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
        .replace("std.tls.", "")
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

const SESSION: &str = r#"
from std.tls import ClientBuilder, ClientCertificates, CipherSuite, ServerBuilder, TlsVersion, TrustRoots

def roots(name: &str) -> Result[TrustRoots, Failure]:
    Ok(std.tls.roots_from_pem(&fixture(name)?).unwrap())

def client_builder(name: &str) -> Result[ClientBuilder, Failure]:
    Ok(std.tls.client_builder(&roots(name)?).unwrap())

def server_builder(chain: &str) -> Result[ServerBuilder, Failure]:
    Ok(std.tls.server_builder(&fixture(chain)?, &fixture("leaf.key")?).unwrap())

def report(stream: &TlsStream) -> Result[(), Failure]:
    print(stream.application_protocol().unwrap())?
    print(stream.server_name().unwrap())?
    match stream.peer_certificate().unwrap():
        case Some(der):
            print(der.len() > 100 and der[0] == 48u8)?
        case Nothing:
            print("no peer certificate")?
    Ok(())

# Reports the session, then echoes until close_notify. The report is printed
# before the echo, so before the client prints anything.
def serve(listener: &TcpListener, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, TlsError]:
    mut stream = handshake(listener, config, deadline, cancel)?
    match report(&stream):
        case _:
            pass
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

def ping(stream: &mut TlsStream, deadline: &Deadline, cancel: &CancellationToken) -> Result[(), TlsError]:
    message = std.bytes.from_text("ping").unwrap()
    stream.write_all_before(&message, 0, 4, deadline, cancel)?
    mut reply = std.bytes.with_capacity(16).unwrap()
    reply.resize(16, 0u8).unwrap()
    stream.read_before(&mut reply, 0, 16, deadline, cancel)?
    Ok(())

# One session: the server's report, then the client's, then the server's
# result. A failure prints where it happened.
def session(server: &ServerConfig, client: &ClientConfig, name: &str) -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(10000))?
    listener = listen(&loopback_v4(0u16), 8).unwrap()
    address = listener.local_address().unwrap()
    with spawn(serve, &listener, server, &deadline, &cancel)? as worker:
        tcp = connect(&address, &deadline, &cancel).unwrap()
        match std.tls.connect(tcp, client, name, &deadline, &cancel):
            case Ok(opened):
                mut stream = opened
                match ping(&mut stream, &deadline, &cancel):
                    case Ok(_):
                        report(&stream)?
                        print(stream.version())?
                        print(stream.cipher_suite())?
                        stream.close_before(&deadline, &cancel).unwrap()
                    case Err(error):
                        print(error)?
            case Err(error):
                print(error)?
        match worker.join():
            case Ok(count):
                print(count)?
            case Err(error):
                print(error)?
    Ok(())
"#;

#[test]
fn servers_verify_client_certificates_when_asked() {
    native(
        SESSION,
        r#"
mut required = server_builder("leaf.pem")?
required.verify_clients(&roots("client-ca.pem")?, ClientCertificates.Required)
required_config = required.build().unwrap()
mut optional = server_builder("leaf.pem")?
optional.verify_clients(&roots("client-ca.pem")?, ClientCertificates.Optional)
optional_config = optional.build().unwrap()
mut presenting = client_builder("ca.pem")?
presenting.certificate(&fixture("client.pem")?, &fixture("client.key")?).unwrap()
presenting_config = presenting.build().unwrap()
anonymous = client_for("ca.pem")?
mut stranger = client_builder("ca.pem")?
stranger.certificate(&fixture("leaf.pem")?, &fixture("leaf.key")?).unwrap()
stranger_config = stranger.build().unwrap()
print("-- required, presented")?
session(&required_config, &presenting_config, "localhost")?
print("-- required, absent")?
session(&required_config, &anonymous, "localhost")?
print("-- required, a server certificate")?
session(&required_config, &stranger_config, "localhost")?
print("-- optional, absent")?
session(&optional_config, &anonymous, "localhost")?
"#,
        r#"
-- required, presented
Option[str].Nothing
Option[str].Some("localhost")
True
Option[str].Nothing
Option[str].Nothing
True
Option[TlsVersion].Some(TlsVersion.Tls13)
Option[CipherSuite].Some(CipherSuite.Tls13Aes256GcmSha384)
4
-- required, absent
TlsError.Rejected
TlsError.CertificateRequired
-- required, a server certificate
TlsError.Rejected
TlsError.InvalidCertificate
-- optional, absent
Option[str].Nothing
Option[str].Some("localhost")
no peer certificate
Option[str].Nothing
Option[str].Nothing
True
Option[TlsVersion].Some(TlsVersion.Tls13)
Option[CipherSuite].Some(CipherSuite.Tls13Aes256GcmSha384)
4
"#,
    );
}

#[test]
fn revocation_lists_reject_revoked_certificates_on_both_sides() {
    native(
        SESSION,
        r#"
mut verifying = server_builder("leaf.pem")?
verifying.verify_clients(&roots("client-ca.pem")?, ClientCertificates.Required)
verifying.revocation_list(&fixture("client-ca.crl.pem")?).unwrap()
verifying_config = verifying.build().unwrap()
mut revoked_client = client_builder("ca.pem")?
revoked_client.certificate(&fixture("revoked-client.pem")?, &fixture("client.key")?).unwrap()
revoked_client_config = revoked_client.build().unwrap()
mut checking = client_builder("ca.pem")?
checking.revocation_list(&fixture("ca.crl.pem")?).unwrap()
checking_config = checking.build().unwrap()
print("-- revoked client")?
session(&verifying_config, &revoked_client_config, "localhost")?
print("-- revoked server")?
session(&server_for("revoked.pem")?, &checking_config, "localhost")?
print("-- listed authority, good server")?
session(&server_for("leaf.pem")?, &checking_config, "localhost")?
"#,
        r#"
-- revoked client
TlsError.Rejected
TlsError.RevokedCertificate
-- revoked server
TlsError.RevokedCertificate
TlsError.Rejected
-- listed authority, good server
Option[str].Nothing
Option[str].Some("localhost")
no peer certificate
Option[str].Nothing
Option[str].Nothing
True
Option[TlsVersion].Some(TlsVersion.Tls13)
Option[CipherSuite].Some(CipherSuite.Tls13Aes256GcmSha384)
4
"#,
    );
}

#[test]
fn servers_choose_a_certificate_by_name_and_a_protocol_by_preference() {
    native(
        SESSION,
        r#"
mut server = server_builder("leaf.pem")?
server.certificate(&fixture("second.pem")?, &fixture("leaf.key")?).unwrap()
server.application_protocol("h2").unwrap()
server.application_protocol("http/1.1").unwrap()
server_config = server.build().unwrap()
mut both = client_builder("ca.pem")?
both.application_protocol("http/1.1").unwrap()
both.application_protocol("h2").unwrap()
both_config = both.build().unwrap()
mut other = client_builder("ca.pem")?
other.application_protocol("spdy/3").unwrap()
other_config = other.build().unwrap()
print("-- second.test, server's preference")?
session(&server_config, &both_config, "second.test")?
print("-- by address, no protocol offered")?
session(&server_config, &client_for("ca.pem")?, "127.0.0.1")?
print("-- no shared protocol")?
session(&server_config, &other_config, "localhost")?
"#,
        r#"
-- second.test, server's preference
Option[str].Some("h2")
Option[str].Some("second.test")
no peer certificate
Option[str].Some("h2")
Option[str].Nothing
True
Option[TlsVersion].Some(TlsVersion.Tls13)
Option[CipherSuite].Some(CipherSuite.Tls13Aes256GcmSha384)
4
-- by address, no protocol offered
Option[str].Nothing
Option[str].Nothing
no peer certificate
Option[str].Nothing
Option[str].Nothing
True
Option[TlsVersion].Some(TlsVersion.Tls13)
Option[CipherSuite].Some(CipherSuite.Tls13Aes256GcmSha384)
4
-- no shared protocol
TlsError.Rejected
TlsError.Incompatible
"#,
    );
}

#[test]
fn versions_and_cipher_suites_can_be_restricted() {
    native(
        SESSION,
        r#"
mut tls12 = server_builder("leaf.pem")?
tls12.cipher_suite(CipherSuite.EcdheEcdsaChacha20Poly1305Sha256)
tls12.cipher_suite(CipherSuite.EcdheEcdsaAes128GcmSha256)
tls12_config = tls12.build().unwrap()
mut modern = client_builder("ca.pem")?
modern.minimum_version(TlsVersion.Tls13)
modern_config = modern.build().unwrap()
mut chosen = client_builder("ca.pem")?
chosen.cipher_suite(CipherSuite.Tls13Chacha20Poly1305Sha256)
chosen_config = chosen.build().unwrap()
print("-- TLS 1.2 server, default client")?
session(&tls12_config, &client_for("ca.pem")?, "localhost")?
print("-- TLS 1.2 server, TLS 1.3 client")?
session(&tls12_config, &modern_config, "localhost")?
print("-- one suite")?
session(&server_for("leaf.pem")?, &chosen_config, "localhost")?
mut impossible = client_builder("ca.pem")?
impossible.minimum_version(TlsVersion.Tls13)
impossible.cipher_suite(CipherSuite.EcdheRsaAes128GcmSha256)
match impossible.build():
    case Ok(_):
        print("built")?
    case Err(error):
        print(error)?
"#,
        r#"
-- TLS 1.2 server, default client
Option[str].Nothing
Option[str].Some("localhost")
no peer certificate
Option[str].Nothing
Option[str].Nothing
True
Option[TlsVersion].Some(TlsVersion.Tls12)
Option[CipherSuite].Some(CipherSuite.EcdheEcdsaAes128GcmSha256)
4
-- TLS 1.2 server, TLS 1.3 client
TlsError.Rejected
TlsError.Incompatible
-- one suite
Option[str].Nothing
Option[str].Some("localhost")
no peer certificate
Option[str].Nothing
Option[str].Nothing
True
Option[TlsVersion].Some(TlsVersion.Tls13)
Option[CipherSuite].Some(CipherSuite.Tls13Chacha20Poly1305Sha256)
4
TlsError.InvalidConfiguration
"#,
    );
}

const SHOWN: &str = r#"
def shown(result: Result[(), TlsError]) -> Result[(), Failure]:
    match result:
        case Ok(_):
            print("ok")?
        case Err(error):
            print(error)?
    Ok(())
"#;

#[test]
fn builder_settings_are_checked() {
    native(
        &format!("{SESSION}{SHOWN}"),
        r#"
mut client = client_builder("ca.pem")?
shown(client.application_protocol(""))?
long = "x".repeat(256)?
shown(client.application_protocol(&long))?
shown(client.revocation_list(&fixture("ca.pem")?))?
shown(client.certificate(&fixture("client.pem")?, &fixture("leaf.key")?))?
shown(client.certificate(&fixture("client.pem")?, &fixture("client.key")?))?
shown(client.certificate(&fixture("client.pem")?, &fixture("client.key")?))?
mut server = server_builder("leaf.pem")?
server.revocation_list(&fixture("ca.crl.pem")?).unwrap()
match server.build():
    case Ok(_):
        print("built")?
    case Err(error):
        print(error)?
mut unsigned = client_builder("ca.pem")?
unsigned.revocation_list(&fixture("client-ca.crl.pem")?).unwrap()
match unsigned.build():
    case Ok(_):
        print("built")?
    case Err(error):
        print(error)?
"#,
        r#"
TlsError.InvalidConfiguration
TlsError.InvalidConfiguration
TlsError.InvalidConfiguration
TlsError.InvalidConfiguration
ok
TlsError.InvalidConfiguration
TlsError.InvalidConfiguration
built
"#,
    );
}

const NONBLOCKING: &str = r#"
import std.ready

# Echoes with the `try_` methods, waiting only in the selector: each time,
# everything that can be done without waiting is done first.
def nonblocking_echo(listener: &TcpListener, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, TlsError]:
    tcp = listener.accept(deadline, cancel).unwrap()
    mut stream = std.tls.start_accept(tcp, config)?
    mut selector = std.ready.selector(4).unwrap()
    mut buffer = std.bytes.with_capacity(2048).unwrap()
    buffer.resize(2048, 0u8).unwrap()
    mut start = 0
    mut stop = 0
    mut total = 0
    while True:
        mut progressed = False
        if start < stop:
            match stream.try_write(&buffer, start, stop)?:
                case Some(count):
                    start += count
                    progressed = True
                case Nothing:
                    pass
        else:
            match stream.try_read(&mut buffer, 0, 2048)?:
                case Some(count):
                    if count == 0:
                        stream.close_before(deadline, cancel)?
                        return Ok(total)
                    start = 0
                    stop = count
                    total += count
                    progressed = True
                case Nothing:
                    pass
        if not progressed:
            stream.watch(&mut selector, 7u64, stream.interest())?
            selector.wait(deadline, cancel).unwrap()
            if selector.event(0).unwrap().key != 7u64:
                return Err(TlsError.Protocol)
    Ok(total)
"#;

#[test]
fn a_selector_drives_a_nonblocking_session() {
    native(
        NONBLOCKING,
        r#"
server = server_for("leaf.pem")?
client = client_for("ca.pem")?
with spawn(nonblocking_echo, &listener, &server, &deadline, &cancel)? as worker:
    tcp = connect(&address, &deadline, &cancel).unwrap()
    mut stream = std.tls.connect(tcp, &client, "localhost", &deadline, &cancel).unwrap()
    mut message = std.bytes.with_capacity(3000)?
    message.resize(3000, 97u8)?
    mut reply = std.bytes.with_capacity(3000)?
    reply.resize(3000, 0u8)?
    mut matched = True
    for round in range(20):
        stream.write_all_before(&message, 0, 3000, &deadline, &cancel).unwrap()
        mut received = 0
        while received < 3000:
            received += stream.read_before(&mut reply, received, 3000, &deadline, &cancel).unwrap()
        if reply[2999] != 97u8 or reply[0] != 97u8:
            matched = False
    stream.close_before(&deadline, &cancel).unwrap()
    print(matched)?
    print(worker.join().unwrap())?
"#,
        "True\n60000",
    );
}
