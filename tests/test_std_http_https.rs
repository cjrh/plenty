//! `std.http.https`: `https` URLs through std.tls, verified against the
//! certificates in `tests/fixtures/tls`, directly and through a `CONNECT`
//! tunnel. Every socket is on 127.0.0.1.

fn fixtures() -> String {
    format!("{}/tests/fixtures/tls", env!("CARGO_MANIFEST_DIR"))
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
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\nstatus {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

/// A pooled client verifies the server by DNS name and by IP address, streams
/// a chunked upload, and names an untrusted root and a certificate for
/// another host. Through a proxy, `CONNECT` carries the proxy's credentials,
/// the TLS session inside it names the origin, and the origin's request is
/// origin-form without them; a refused tunnel reports the proxy's status.
#[test]
fn https_requests_are_verified_pooled_and_tunnelled() {
    let source = r##"import std.bytes
import std.http
import std.http.client as hc
import std.http.https
import std.http.server
import std.http.tls
import std.io
import std.net
import std.time
import std.tls
from std.bytes import Bytes
from std.http.client import ClientError
from std.http.server import Exchange
from std.http.tls import TlsTransport
from std.io import Reader
from std.net import NetError, TcpListener, TcpStream
from std.sync import CancellationToken
from std.time import Deadline
from std.tls import ClientConfig, ServerConfig

def fixture(name: &str) -> Result[str, Failure]:
    Ok(read_text(&("FIXTURES/" + name)?)?)

def trusting(name: &str) -> Result[ClientConfig, Failure]:
    Ok(std.tls.client_config(&std.tls.roots_from_pem(&fixture(name)?).unwrap()).unwrap())

def serving(chain: &str) -> Result[ServerConfig, Failure]:
    Ok(std.tls.server_config(&fixture(chain)?, &fixture("leaf.key")?).unwrap())

def later() -> Result[Deadline, Failure]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?)

def url(host: &str, port: u16, path: &str) -> Result[str, Failure]:
    Ok("https://".concat(host)?.concat(":")?.concat(&str.from(i64(port))?)?.concat(path)?)

pub class Echo:
    pub def handle(self, exchange: &mut Exchange[TlsTransport]) -> Result[(), Failure]:
        mut body = std.bytes.empty()?
        exchange.read_all(&mut body)?
        text = exchange.request.method.concat(" ")?.concat(&exchange.path)?.concat(" ")?.concat(&body.to_text()?)?
        exchange.send_text(200, &text)?
        Ok(())

def show(result: Result[hc.Fetched, ClientError]) -> Result[(), Failure]:
    match result:
        case Ok(fetched):
            print(str.from(fetched.status)?.concat(" ")?.concat(&fetched.body.to_text()?)?)?
        case Err(error):
            print(error)?
    Ok(())

def stats[T, C](client: &hc.Client[T, C]) -> Result[(), Failure]:
    s = client.stats()
    print("opened=".concat(&str.from(s.opened)?)?.concat(" reused=")?.concat(&str.from(s.reused)?)?)?
    Ok(())

# Reads one message head, a byte at a time so nothing after it is consumed.
def head[R: Reader](stream: &mut R) -> Result[str, Failure]:
    mut out = std.bytes.empty()?
    mut one = std.bytes.with_capacity(1)?
    one.resize(1, 0u8)?
    while not out.to_text()?.endswith("\r\n\r\n"):
        if stream.read(&mut one, 0, 1)? == 0:
            break
        out.extend(&one)?
    Ok(out.to_text()?.replace("\r\n", "|")?)

def visits(port: u16) -> Result[(), Failure]:
    never = CancellationToken()?
    mut client = std.http.https.client(hc.config()?, trusting("ca.pem")?)?
    show(client.get(&url("localhost", port, "/")?, &later()?, &never))?
    show(client.get(&url("127.0.0.1", port, "/")?, &later()?, &never))?
    show(client.get(&url("localhost", port, "/")?, &later()?, &never))?
    mut source = std.io.cursor(std.bytes.from_text("streamed over tls")?)
    match client.send_from(hc.request("PUT", &url("localhost", port, "/up")?)?, &mut source, &hc.chunked(), &later()?, &never):
        case Ok(response):
            mut owned = response
            mut body = std.bytes.empty()?
            owned.read_all(&mut body)?
            print(body.to_text()?)?
            client.finish(owned)
        case Err(error):
            print(error)?
    stats(&client)?
    mut strangers = std.http.https.client(hc.config()?, trusting("other-ca.pem")?)?
    show(strangers.get(&url("localhost", port, "/")?, &later()?, &never))?
    Ok(())

def visit(port: u16, stop: CancellationToken) -> Result[(), Failure]:
    outcome = visits(port)
    stop.cancel()
    outcome

def impostor(listener: TcpListener, tls: ServerConfig) -> Result[str, Failure]:
    never = CancellationToken()?
    tcp = listener.accept(&later()?, &never)?
    match std.tls.accept(tcp, &tls, &later()?, &never):
        case Ok(_):
            Ok("impostor accepted")
        case Err(_):
            Ok("impostor refused")

def proxy(listener: TcpListener, tls: ServerConfig) -> Result[str, Failure]:
    never = CancellationToken()?
    mut tcp = listener.accept(&later()?, &never)?
    mut log = head(&mut tcp)?.concat("\n")?
    established = std.bytes.from_text("HTTP/1.1 200 Connection Established\r\n\r\n")?
    tcp.write_all_before(&established, 0, established.len(), &later()?, &never)?
    mut stream = std.tls.accept(tcp, &tls, &later()?, &never).unwrap()
    log = log.concat("server name ")?.concat(&stream.server_name().unwrap().unwrap())?.concat("\n")?
    log = log.concat(&head(&mut stream)?)?.concat("\n")?
    answer = std.bytes.from_text("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\ntunnel")?
    stream.write_all_before(&answer, 0, answer.len(), &later()?, &never).unwrap()
    stream.close_before(&later()?, &never).unwrap()
    mut refused = listener.accept(&later()?, &never)?
    log = log.concat(&head(&mut refused)?)?
    denied = std.bytes.from_text("HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n")?
    refused.write_all_before(&denied, 0, denied.len(), &later()?, &never)?
    Ok(log)

def main() -> Result[(), Failure]:
    never = CancellationToken()?
    mut table = std.http.server.router()?
    table.add("GET", "/", 0)?
    table.add("PUT", "/up", 1)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    mut config = std.http.server.config()
    config.workers = 1
    with spawn(visit, port, stop.share())? as task:
        report = std.http.tls.serve(listener, serving("leaf.pem")?, &table, &Echo(), &config, &stop)?
        task.join()?
    print("--- the certificate must name the host")?
    other = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    other_port = other.local_address()?.port
    with spawn(impostor, other, serving("second.pem")?)? as task:
        mut client = std.http.https.client(hc.config()?, trusting("ca.pem")?)?
        show(client.get(&url("localhost", other_port, "/")?, &later()?, &never))?
        print(task.join()?)?
    print("--- https through a CONNECT tunnel")?
    tunnels = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    proxy_port = tunnels.local_address()?.port
    with spawn(proxy, tunnels, serving("leaf.pem")?)? as task:
        mut settings = hc.config()?
        mut via = hc.proxy(&"http://127.0.0.1:".concat(&str.from(i64(proxy_port))?)?)?
        via.authorization = Some("Basic cHJveHk=")
        settings.proxy = Some(via)
        mut client = std.http.https.client(settings, trusting("ca.pem")?)?
        mut request = hc.request("GET", "https://localhost:9/secret")?
        request.add("Authorization", "Bearer origin")?
        match client.fetch(request, &later()?, &never):
            case Ok(fetched):
                print(fetched.body.to_text()?)?
            case Err(error):
                print(error)?
        show(client.get("https://localhost:9/again", &later()?, &never))?
        print(task.join()?)?
    Ok(())
"##
    .replace("FIXTURES", &fixtures());
    assert_eq!(
        output(&source),
        r##"200 GET / 
200 GET / 
200 GET / 
PUT /up streamed over tls
opened=2 reused=2
std.http.client.ClientError.Tls(std.http.client.TlsFailure.UntrustedCertificate)
--- the certificate must name the host
std.http.client.ClientError.Tls(std.http.client.TlsFailure.WrongHost)
impostor refused
--- https through a CONNECT tunnel
tunnel
std.http.client.ClientError.TunnelRefused(407)
CONNECT localhost:9 HTTP/1.1|Host: localhost:9|Proxy-Authorization: Basic cHJveHk=||
server name localhost
GET /secret HTTP/1.1|Host: localhost:9|Authorization: Bearer origin||
CONNECT localhost:9 HTTP/1.1|Host: localhost:9|Proxy-Authorization: Basic cHJveHk=||"##
    );
}

/// The client and its TCP connector do not reference std.tls, so a program
/// that only speaks `http` links none of rustls or ring.
#[test]
fn plain_clients_do_not_link_the_tls_stack() {
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
    let program = |imports: &str, make: &str| {
        format!(
            r##"import std.http.client as hc
import std.time
{imports}
from std.sync import CancellationToken

def main() -> Result[(), Failure]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(1u32))?
    mut client = {make}?
    match client.get("http://127.0.0.1:9/", &deadline, &never):
        case Ok(_):
            pass
        case Err(error):
            print(error)?
    Ok(())
"##
        )
    };
    let plain = program("", "hc.client(hc.config()?)");
    let secure = program(
        "import std.http.https\nimport std.tls",
        "std.http.https.client(hc.config()?, std.tls.client_config(&std.tls.system_roots().unwrap()).unwrap())",
    );
    assert_eq!(symbols(&plain), 0);
    assert!(symbols(&secure) > 1);
}
