//! `std.http.client` shared between threads through `Shared[Client[T, C]]`:
//! executor jobs reuse each other's keep-alive connections, the pool limits
//! and idle timeout hold under concurrency, `Stats` totals stay exact, and the
//! pool lock is never held across a request. Servers run on loopback ports.
mod support;
use support::assert_http_program as runs;
use support::check_file_error as rejected;

/// A loopback server on another thread, and jobs that send requests through
/// one shared client.
const SHARED: &str = r##"import std.http.client as hc
import std.bytes
import std.http
import std.http.server
import std.net
import std.time
from std.concurrent import ThreadPoolExecutor, Future, SubmitError
from std.http.server import Exchange
from std.net import TcpListener, TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home

pub class Site:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        exchange.send_text(200, "home\n")?
        Ok(())

def later() -> Result[std.time.Deadline, RangeError]:
    std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))

def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(_):
            Err("submission failed")

# `count` GETs of `url`; the sum of their statuses.
def fetches(client: Shared[hc.Client[hc.Plain, hc.TcpConnector]], url: str, count: i64) -> Result[i64, AllocError | hc.ClientError | RangeError]:
    never = CancellationToken()?
    mut total = 0
    for _ in range(count):
        total += client.get(&url, &later()?, &never)?.status
    Ok(total)

# `count` GETs of `url` as `send`, `read_all` and `finish`.
def streams(client: Shared[hc.Client[hc.Plain, hc.TcpConnector]], url: str, count: i64) -> Result[i64, AllocError | std.bytes.BytesError | hc.ClientError | RangeError]:
    never = CancellationToken()?
    mut total = 0
    for _ in range(count):
        mut response = client.send(hc.request("GET", &url)?, &later()?, &never)?
        mut body = std.bytes.empty()?
        response.read_all(&mut body)?
        total += response.status
        client.finish(response)
    Ok(total)

def line(client: &hc.Client[hc.Plain, hc.TcpConnector]) -> Result[str, AllocError]:
    s = client.stats()
    "opened=".concat(&str.from(s.opened)?)?.concat(" reused=")?.concat(&str.from(s.reused)?)?.concat(" retried=")?.concat(&str.from(s.retried)?)?.concat(" redirected=")?.concat(&str.from(s.redirected)?)?.concat(" discarded=")?.concat(&str.from(s.discarded)?)?.concat(" idle=")?.concat(&str.from(client.idle())?)

def serve(listener: TcpListener, stop: CancellationToken) -> Result[(), Failure]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    report = std.http.server.serve(listener, &routes, &Site(), &std.http.server.config(), &stop)?
    print("server saw", report.requests, "requests")?
    Ok(())

"##;

/// Jobs on two successive executors send 120 requests through one client.
/// Every connection one job opened serves the others: at most one connection
/// per concurrent job is ever opened, `opened + reused` is exactly the number
/// of requests, and every opened connection is either idle or counted as
/// discarded.
#[test]
fn executor_jobs_share_one_client_and_its_connections() {
    runs(
        SHARED,
        r##"
def main() -> Result[(), Failure]:
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(serve, listener, stop.share())? as server:
        url = "http://127.0.0.1:".concat(&str.from(i64(port))?)?.concat("/")?
        client = Shared(hc.client(hc.config()?)?)?
        mut sum = 0
        with ThreadPoolExecutor(4, 8)? as pool:
            a = submitted(pool.submit(fetches, client.share(), url, 20))?
            b = submitted(pool.submit(streams, client.share(), url, 20))?
            c = submitted(pool.submit(fetches, client.share(), url, 20))?
            sum = a.result()?? + b.result()?? + c.result()??
        first = client.stats()
        print("first batch:", sum, first.opened <= 3, first.opened + first.reused == 60)?
        with ThreadPoolExecutor(4, 8)? as pool:
            a = submitted(pool.submit(streams, client.share(), url, 20))?
            b = submitted(pool.submit(fetches, client.share(), url, 20))?
            c = submitted(pool.submit(fetches, client.share(), url, 20))?
            sum = a.result()?? + b.result()?? + c.result()??
        s = client.stats()
        print("second batch:", sum, s.opened <= 3, s.opened + s.reused == 120)?
        print("exact:", s.opened == u64(client.idle()) + s.discarded, s.retried, s.redirected)?
        stop.cancel()
    Ok(())
"##,
        r##"first batch: 12000 True True
second batch: 12000 True True
exact: True 0 0
server saw 120 requests"##,
    );
}

/// `max_idle_per_origin`, `max_idle` and `max_idle = 0` bound the pool while
/// four jobs return connections at once, and the totals stay exact: every
/// request opened or reused a connection, and a pooled connection is idle or
/// was counted as discarded.
#[test]
fn pool_limits_hold_under_concurrency() {
    runs(
        SHARED,
        r##"
def bounded(url: &str, max_idle: i64, per_origin: i64) -> Result[(), Failure]:
    mut config = hc.config()?
    config.max_idle = max_idle
    config.max_idle_per_origin = per_origin
    client = Shared(hc.client(config)?)?
    mut sum = 0
    with ThreadPoolExecutor(4, 8)? as pool:
        a = submitted(pool.submit(fetches, client.share(), *url, 25))?
        b = submitted(pool.submit(fetches, client.share(), *url, 25))?
        c = submitted(pool.submit(streams, client.share(), *url, 25))?
        d = submitted(pool.submit(streams, client.share(), *url, 25))?
        sum = a.result()?? + b.result()?? + c.result()?? + d.result()??
    s = client.stats()
    mut limit = max_idle
    if per_origin < limit:
        limit = per_origin
    print(sum, "idle<=limit:", client.idle() <= limit, "exact:", s.opened + s.reused == 100)?
    if max_idle == 0:
        # `Connection: close` responses are not pooled and not counted as
        # discarded.
        print("no reuse:", s.opened == 100 and s.reused == 0 and client.idle() == 0)?
    else:
        print("accounted:", s.opened == u64(client.idle()) + s.discarded)?
    client.close_idle()
    print("closed:", client.idle())?
    Ok(())

def main() -> Result[(), Failure]:
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(serve, listener, stop.share())? as server:
        url = "http://127.0.0.1:".concat(&str.from(i64(port))?)?.concat("/")?
        bounded(&url, 16, 2)?
        bounded(&url, 1, 4)?
        bounded(&url, 0, 4)?
        stop.cancel()
    Ok(())
"##,
        r##"20000 idle<=limit: True exact: True
accounted: True
closed: 0
20000 idle<=limit: True exact: True
accounted: True
closed: 0
20000 idle<=limit: True exact: True
no reuse: True
closed: 0
server saw 300 requests"##,
    );
}

/// A connection idle longer than `idle_timeout` is closed by whichever
/// thread uses the pool next.
#[test]
fn expired_connections_are_closed_by_the_next_user_of_the_pool() {
    runs(
        SHARED,
        r##"
def main() -> Result[(), Failure]:
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(serve, listener, stop.share())? as server:
        url = "http://127.0.0.1:".concat(&str.from(i64(port))?)?.concat("/")?
        mut config = hc.config()?
        config.idle_timeout = std.time.milliseconds(50u32)
        client = Shared(hc.client(config)?)?
        never = CancellationToken()?
        print(client.get(&url, &later()?, &never)?.status)?
        print(line(&*client)?)?
        std.time.sleep(&std.time.milliseconds(150u32))
        with ThreadPoolExecutor(1, 1)? as pool:
            print(submitted(pool.submit(fetches, client.share(), url, 1))?.result()??)?
        print(line(&*client)?)?
        stop.cancel()
    Ok(())
"##,
        r##"200
opened=1 reused=0 retried=0 redirected=0 discarded=0 idle=1
200
opened=2 reused=0 retried=0 redirected=0 discarded=1 idle=1
server saw 2 requests"##,
    );
}

/// The pool lock is not held while a request waits: a job stuck on a server
/// that never answers does not stop another job's requests through the same
/// client, which finish and then cancel the stuck one. Were the lock held
/// across the exchange, the second job could never take a connection and the
/// program would hang.
#[test]
fn a_stalled_origin_does_not_hold_up_other_requests() {
    runs(
        SHARED,
        r##"
# Accepts connections and never answers until `stop` is cancelled.
def silent(listener: TcpListener, stop: CancellationToken) -> Result[(), Failure]:
    mut held = list[TcpStream].new()?
    while not stop.is_cancelled():
        match listener.accept(&later()?, &stop):
            case Ok(stream):
                held.append(stream)?
            case Err(_):
                pass
    Ok(())

def stuck(client: Shared[hc.Client[hc.Plain, hc.TcpConnector]], url: str, halt: CancellationToken) -> Result[str, AllocError | RangeError]:
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(30u32))?
    match client.get(&url, &deadline, &halt):
        case Ok(_):
            Ok("answered")
        case Err(error):
            match error:
                case hc.ClientError.Net(std.net.NetError.Cancelled):
                    Ok("cancelled")
                case _:
                    Ok("failed")

def then_halt(client: Shared[hc.Client[hc.Plain, hc.TcpConnector]], url: str, halt: CancellationToken) -> Result[i64, AllocError | hc.ClientError | RangeError]:
    total = fetches(client, url, 20)?
    halt.cancel()
    Ok(total)

def main() -> Result[(), Failure]:
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    quiet = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    quiet_url = "http://127.0.0.1:".concat(&str.from(i64(quiet.local_address()?.port))?)?.concat("/")?
    stop = CancellationToken()?
    with spawn(serve, listener, stop.share())? as server, spawn(silent, quiet, stop.share())? as sink:
        url = "http://127.0.0.1:".concat(&str.from(i64(port))?)?.concat("/")?
        client = Shared(hc.client(hc.config()?)?)?
        halt = CancellationToken()?
        with ThreadPoolExecutor(2, 2)? as pool:
            waiting = submitted(pool.submit(stuck, client.share(), quiet_url, halt.share()))?
            busy = submitted(pool.submit(then_halt, client.share(), url, halt.share()))?
            print(busy.result()??, waiting.result()??)?
        print(line(&*client)?)?
        stop.cancel()
    Ok(())
"##,
        r##"4000 cancelled
opened=2 reused=19 retried=0 redirected=0 discarded=0 idle=1
server saw 20 requests"##,
    );
}

/// An `https` client shares like a plain one: its connector holds only a TLS
/// configuration, which may be used from any worker. A connector that holds
/// something that cannot cross threads is rejected when the client is shared.
#[test]
fn https_clients_share_and_ineligible_connectors_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(
        &path,
        r##"import std.http.client as hc
import std.http.https
import std.tls

def main() -> Result[(), Failure]:
    roots = std.tls.system_roots()?
    client = Shared(std.http.https.client(hc.config()?, std.tls.client_config(&roots)?)?)?
    other = client.share()
    print(client.idle(), other.idle())?
    Ok(())
"##,
    )
    .unwrap();
    plenty::check_file(&path, None).unwrap_or_else(|error| panic!("{error}"));
    let error = rejected(
        r##"import std.http.client as hc
from std.concurrent import ThreadPoolExecutor
from std.http.client import ClientError, Endpoint, Plain
from std.sync import CancellationToken
from std.time import Deadline

class Lazy:
    pool: ThreadPoolExecutor

    pub def open(self, endpoint: &Endpoint, deadline: &Deadline, cancel: &CancellationToken) -> Result[Plain, ClientError]:
        Err(ClientError.UnsupportedScheme)

def main() -> Result[(), Failure]:
    client = Shared(hc.connector[Plain, Lazy](Lazy(ThreadPoolExecutor(1, 1)?), hc.config()?)?)?
    Ok(())
"##,
    );
    assert!(error.contains("cannot hold ThreadPoolExecutor"), "{error}");
}
