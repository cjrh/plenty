# Share one HTTP client between workers

A [client](http-client.md) keeps finished connections in a pool for the next
request to the same server. Every client method borrows the client shared, so
jobs on an executor can use one client through
[`Shared`](shared-pool-state.md) handles and reuse each other's connections.

This program serves a loopback port from one thread and sends it requests
from three pool jobs that share a client:

```plenty
import std.http.client
import std.http.server
import std.net
import std.time
from std.concurrent import ThreadPoolExecutor, Future, SubmitError
from std.http.client import Client, ClientError, Plain, TcpConnector
from std.http.server import Exchange
from std.net import TcpListener, TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home

pub class Site:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        exchange.send_text(200, "home\n")?
        Ok(())

def serve(listener: TcpListener, stop: CancellationToken) -> Result[(), AllocError | std.http.server.ServerError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    drop(std.http.server.serve(listener, &routes, &Site(), &std.http.server.config(), &stop)?)
    Ok(())

def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(_):
            Err("submission failed")

def fetch_all(client: Shared[Client[Plain, TcpConnector]], url: str, count: i64) -> Result[i64, AllocError | ClientError | RangeError]:
    never = CancellationToken()?
    mut ok = 0
    for _ in range(count):
        deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
        if client.get(&url, &deadline, &never)?.status == 200:
            ok += 1
    Ok(ok)

def main() -> Result[(), Failure]:
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(serve, listener, stop.share())? as server:
        url = "http://127.0.0.1:".concat(&str.from(i64(port))?)?.concat("/")?
        client = Shared(std.http.client.client(std.http.client.config()?)?)?
        mut answered = 0
        with ThreadPoolExecutor(3, 8)? as pool:
            a = submitted(pool.submit(fetch_all, client.share(), url, 10))?
            b = submitted(pool.submit(fetch_all, client.share(), url, 10))?
            c = submitted(pool.submit(fetch_all, client.share(), url, 10))?
            answered = a.result()?? + b.result()?? + c.result()??
        stats = client.stats()
        print(answered, stats.opened + stats.reused, stats.opened <= 3)?
        stop.cancel()
    Ok(())
```
```output
30 30 True
```

`Shared(client(config)?)?` moves the client into one shared allocation;
`client.share()` gives each job its own owner, and `client.get` reaches the
client through the handle. Thirty requests went out, and every one either
opened a connection or reused one: `opened + reused` is 30. Three jobs ran at
once, so at most three connections were ever opened; a connection one job
parks is taken by whichever job asks next.

The client holds its pool lock only while taking a connection out or putting
one back. Connecting, sending, and waiting for a response happen with the lock
released, so a job stuck on a slow server does not delay the others. Each
request still has its own deadline and cancellation token.

`stats()` and `idle()` work through the handle too. The counts are exact however
many jobs run: here, `opened` is the number of connections now idle plus the
number counted as `discarded`. The pool limits `max_idle` and `max_idle_per_origin`
and the `idle_timeout` apply to the shared pool as a whole; see
[the client reference](../../design/stdlib/http-client.md#sharing-a-client-between-threads).
