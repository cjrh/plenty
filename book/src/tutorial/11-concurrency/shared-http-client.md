# Share one HTTP client between workers

A [client](http-client.md) keeps finished connections in a pool for the next
request to the same server. Every client method borrows the client shared, so
jobs on an executor can use one client through
[`Shared`](shared-pool-state.md) handles and reuse each other's connections.

This program serves a loopback port from one thread and sends it requests
from three pool jobs that share a client:

```plenty
import std.http
import std.http.client
import std.http.server
import std.net
import std.time
from std.concurrent import ThreadPoolExecutor, Future, SubmitError, PoolError, FutureError
from std.fmt import FormatError
from std.http.client import Client, ClientError, Plain, TcpConnector
from std.http.server import Exchange
from std.net import TcpListener, TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home

pub class Site:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), std.http.HttpError]:
        exchange.send_text(200, "home\n")?
        Ok(())

def serve(listener: TcpListener, stop: CancellationToken) -> Result[(), AllocError | std.http.server.ServerError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    drop(std.http.server.serve(listener, &routes, &Site(), &std.http.server.config(), &stop)?)
    Ok(())

def fetch_all(client: Shared[Client[Plain, TcpConnector]], url: str, count: i64) -> Result[i64, AllocError | ClientError | RangeError]:
    never = CancellationToken()?
    mut ok = 0
    for _ in range(count):
        deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
        if client.get(&url, &deadline, &never)?.status == 200:
            ok += 1
    Ok(ok)

type PoolJobError = PoolError | FutureError | SubmitError[()]
type ClientRunError = AllocError | IoError | ThreadError | RangeError | FormatError | ClientError | std.net.NetError | PoolJobError

def main() -> Result[(), ClientRunError]:
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(serve, listener, stop.share())? as server, stop.cancel_on_exit():
        url = f"http://127.0.0.1:{port}/"?
        client = Shared(std.http.client.client(std.http.client.config()?)?)?
        mut answered = 0
        with ThreadPoolExecutor(3, 8)? as pool:
            a = pool.submit(fetch_all, client.share(), url, 10).drop_job()?
            b = pool.submit(fetch_all, client.share(), url, 10).drop_job()?
            c = pool.submit(fetch_all, client.share(), url, 10).drop_job()?
            answered = a.result()?? + b.result()?? + c.result()??
        stats = client.stats()
        print(answered, stats.opened + stats.reused, stats.opened <= 3)?
        print(stats.opened == u64(client.idle()) + stats.discarded)?
    Ok(())
```
```output
30 30 True
True
```

`stop.cancel_on_exit()` stops the server however the block ends, so a failed
request cannot leave the block's join waiting for it.

`Shared(client(config)?)?` moves the client into one shared allocation;
`client.share()` gives each job its own owner, and `client.get` reaches the
client through the handle. Thirty requests went out, and every one either
opened a connection or reused one: `opened + reused` is 30. A job holds one
connection at a time and opens one only when none is idle, and a connection
one job parks is taken by whichever job asks next. Three jobs ran at once and
none of their connections closed, so at most three were opened.

The client holds its pool lock only while taking a connection out or putting
one back. Connecting, sending, and waiting for a response happen with the lock
released, so a job stuck on a slow server does not delay the others. Each
request still has its own deadline and cancellation token.

`stats()` and `idle()` work through the handle too. The counts are exact however
many jobs run. Every connection the client opened is idle, in use, or counted
as `discarded` once it has closed, whatever closed it; with no request or
response holding one, `opened` is `idle()` plus `discarded`, as the last line
shows. The pool limits `max_idle` and `max_idle_per_origin` and the
`idle_timeout` apply to the shared pool as a whole; see
[the client reference](../../design/stdlib/http-client.md#sharing-a-client-between-threads).
