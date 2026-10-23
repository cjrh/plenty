# `std.http.client`

`import std.http.client` sends HTTP/1.1 requests over pooled keep-alive
connections. Each call has a caller deadline and cancellation token and a
timeout for each phase. Redirects are bounded and drop credentials when the
origin changes. A proxy is used only when configured, and a request is
replayed only when that is safe. Request bodies are in memory or streamed
from a `std.io` `Reader`. Head parsing, writes and body framing use
[`std.http.typed`](http-typed.md), with shared [`std.http`](http.md) validation, and
sockets from [`std.net`](net.md); [`std.http.https`](#https) adds `https`
URLs through [`std.tls`](tls.md). Both modules are checked Plenty in
`stdlib/std/http/` and add no runtime primitives. The tutorial lessons
[Send HTTP requests](../../tutorial/11-concurrency/http-client.md),
[Share one HTTP client between workers](../../tutorial/11-concurrency/shared-http-client.md)
and [Send HTTPS requests](../../tutorial/11-concurrency/https-client.md) show
complete programs.

## Clients

| Item | Meaning |
|---|---|
| `client(config: Config)` | `Result[Client[Plain, TcpConnector], ClientError]`: plain `http` over TCP |
| `connector[T: Transport, C: Connector[T]](opener: C, config: Config)` | `Result[Client[T, C], ClientError]`: connections come from `opener`; give both type arguments |
| `client.send(request, &deadline, &cancel)` | `Result[Response[T], ClientError]`: the final response head, after redirects |
| `client.send_from(request, &mut body, &upload, &deadline, &cancel)` | `send` with the body read from `body`, a `std.io` `Reader`; see [Streamed bodies](#streamed-bodies) |
| `client.fetch(request, &deadline, &cancel)` | `Result[Fetched, ClientError]`: `send`, read the whole body, `finish` |
| `client.get(url: &str, &deadline, &cancel)` | `fetch` of a `GET` |
| `client.finish(response)` | Pool the connection if the body was read to its end and both sides allow reuse; otherwise close it |
| `client.close_idle()` | Close the connections pooled when it is called |
| `client.idle()` | `i64`: pooled connections |
| `client.stats()` | `Stats` |

Both constructors fail with `InvalidConfig` for a config outside the bounds
below. Every method borrows the client shared, so one client serves many
threads through [`Shared`](../facilities/01-builtins/shared.md) handles; see
[Sharing a client between threads](#sharing-a-client-between-threads).

`deadline` bounds the whole exchange, including later reads of the body of
the `Response` that `send` returns. `cancel` stops it: a wait in progress
ends with `Net(Cancelled)`. A request whose token is already cancelled, or
whose deadline has passed, fails with `Net(Cancelled)` or `Net(TimedOut)`
before it takes a connection, so it leaves pooled connections alone.

## Requests and responses

| Item | Meaning |
|---|---|
| `request(method: &str, url: &str)` | `Result[Request, ClientError]` with no fields and an empty body |
| `Request.method`, `url`, `headers: list[Header]`, `body: Bytes` | Public fields |
| `request.add(name, value)` | `Result[(), ClientError]`: append a field |
| `Response.status`, `head: ResponseHead`, `url`, `redirects` | Final status and head, the URL that answered, and redirects followed |
| `response.read_body(&mut buffer, start, stop)` | `Result[i64, ClientError]`: part of the body; `0` is the end |
| `response.read_all(&mut out)` | `Result[i64, ClientError]`: append the rest, within `limits.max_body` |
| `response.read_all_progress(&mut out)` | `Result[i64, Partial[ClientError]]`: append the rest and retain the count appended by this call if it fails |
| `response.read_typed(...)` | `Result[i64, StepError[ClientError]]`: read with the complete returned cause |
| `typed_response(response)`, `unwrap_response(stream)` | Owning `TypedResponse[T]` adapter for `std.io.typed.Reader[ClientError]`; unwrap before `client.finish` |
| `response.total()`, `response.trailers()` | Payload bytes successfully read, and borrowed trailer fields (complete after `is_done`) |
| `response.read(...)`, `take_failure()` | A `std.io` `Reader`; a client failure is `Io(InvalidInput)` and `take_failure` returns it, a short body is `UnexpectedEof` |
| `response.is_done()` | The whole body has been read |
| `Fetched.status`, `head`, `url`, `redirects`, `body: Bytes` | A whole response |

The client writes `Host` from the URL, and `Content-Length` when the body is
not empty or the method is `POST`, `PUT` or `PATCH`. An in-memory body is
sent with `Content-Length`. A request that sets
`Host`, `Content-Length`, `Transfer-Encoding`, `Connection`, `Keep-Alive`,
`Upgrade`, `TE`, `Trailer`, `Expect`, `Proxy-Authorization` or
`Proxy-Connection` fails with `InvalidRequest`, as do `CONNECT` and a method
that is not an HTTP token. These checks run before any connection opens.

URLs must be absolute `http` or `https` URLs with a host. Credentials in the
URL (`user:pass@`) are `InvalidUrl`; send an `Authorization` field instead.
The fragment is not sent. The TCP connector refuses an `https` URL with
`UnsupportedScheme`; use [`std.http.https`](#https).

Interim `1xx` responses are skipped, up to 16; `101 Switching Protocols` is
`UnexpectedInterim`. A response to `HEAD`, and a `204` or `304`, has no body.

Typed response reads preserve network, TLS, cancellation and framing causes
directly. `read_all_progress` counts bytes appended after the output's existing
prefix. It reserves output capacity before consuming payload; on failure the
prefix and appended bytes remain available, the connection closes, and
`total()` retains previously returned payload progress. The existing `read_all`
returns the same cause while omitting the count. `send` and `fetch` also retain
their established `ClientError` return types; use the lower-level typed HTTP
operations when per-head or per-write partial counts are needed.

The legacy `read` method still implements `std.io.Reader` and uses
`take_failure` for causes that `StreamError` cannot represent. Bridging that
method with `std.io.typed.reader` cannot recover the erased cause; use
`typed_response` instead. Internal head parsing, body decoding and CONNECT
tunnelling no longer store transport errors for later retrieval.

## Streamed bodies

`send_from` reads the request body from any `std.io` `Reader`, 16 KiB at a
time, and never holds more of it. The request's own `body` must be empty,
or the call fails with `InvalidRequest` before connecting.

| Item | Meaning |
|---|---|
| `sized(length: i64)` | `Upload` sent with `Content-Length: length` |
| `chunked()` | `Upload` sent with `Transfer-Encoding: chunked` and no trailers |
| `Upload.length: Option[i64]`, `expect_continue: bool` | Public fields; `expect_continue` is `False` from both constructors |

With `sized`, the reader must yield exactly `length` bytes: more, or an end
before `length`, is `WrongBodyLength` and the connection is closed. A
negative length is `InvalidRequest`. A reader failure is `Body(StreamError)`,
or `Allocation`. Explicit reader interruption is retried; negative or oversized
successful counts are rejected as `Body(InvalidProgress)`.

With `expect_continue`, the head carries `Expect: 100-continue` and the
client waits up to `config.continue_timeout` before sending the body:

| The server | The client |
|---|---|
| Sends `100 Continue` | Sends the body, then reads the final response |
| Sends nothing before `continue_timeout` | Sends the body anyway, as RFC 9110 allows |
| Sends a final response first, such as `417` or `401` | Returns it without sending the body; the connection is closed, not pooled |

A `417 Expectation Failed` is returned like any other status; the client
does not retry without the expectation, since the reader may not be
restartable.

## Connections and the pool

After `fetch`, or `finish` on a fully read response, the connection waits in
the pool for another request to the same scheme, host and port (and proxy).
The most recently used connection is taken first. A connection is closed
instead of pooled when:

- either side sent `Connection: close`, or the response is HTTP/1.0 without
  keep-alive, or its body ran to the end of the connection;
- the body was not read to its end, or reading it failed;
- bytes beyond the response are already buffered;
- the pool already holds `max_idle_per_origin` connections for that origin,
  or `max_idle` in all (the oldest is closed to make room).

Dropping a `Response` without `finish` closes its connection. Before reusing
a pooled connection the client checks, without waiting, that nothing has
arrived on it: a connection the server closed, or one with unexpected
bytes, is closed and another is used. A check that cancellation cuts short
puts the connection back. A connection idle longer than `idle_timeout` is
closed when the pool is next used.

## Sharing a client between threads

A client's pool is one value behind a `std.sync` mutex, and its counters are
atomics, so threads share a client by sharing the value:

```text
client = Shared(std.http.client.client(config)?)?
job = pool.submit(fetch_all, client.share(), url)?
```

Each job owns a handle and calls `get`, `send`, `finish` and the rest through
it; a connection one thread parks is reused by whichever thread asks next.
The lock is held only while a connection is taken out of the pool or put back
in: connecting, TLS, writing, waiting for a response and reading a body all
run with the lock released, as do closing any connection and the readiness
probe before reuse. A request to a slow origin therefore does not delay
requests to another through the same client. `close_idle` takes the
connections out one at a time, oldest first, and stops after as many as were
idle when it was called, so other threads parking connections cannot keep
it running.

`max_idle`, `max_idle_per_origin` and `idle_timeout` bound the shared pool as a
whole, and `Stats` counts every event exactly once. Every request sent,
counting replays and redirects, went on a connection counted in `opened` or in
`reused`, so `opened + reused` is the number of requests sent. Every
connection counted in `opened` is idle, held by a request or a `Response`, or
counted in `discarded` once closed, whatever closed it; so after all jobs
finish and every response has been finished or dropped, `opened` is `idle()`
plus `discarded`. `stats()` reads each counter without the lock, so a snapshot
taken while jobs run may show one counter ahead of another.

Sharing a client is subject to the `Shared` rules: the connector's type must
be able to cross threads. `TcpConnector` and `HttpsConnector`, whose TLS
configuration is immutable, qualify. A single-threaded client pays an
uncontended lock and unlock per checkout and return, and an atomic increment
per counted event; neither allocates.

## Replaying requests

A server may close an idle connection at the moment the client reuses it.
When a request on a reused connection fails before any byte of a response
arrives, by end of input, a reset, or a failed write, the client sends it
again on a new connection, at most `retries` times, but only if:

- the method is `GET`, `HEAD`, `OPTIONS`, `TRACE`, `PUT` or `DELETE`, and
- the body is the request's in-memory `body`, or a streamed body of which
  nothing has been read yet (for example, while waiting for `100 Continue`).

`POST`, `PATCH` and other methods are never replayed, since the server may
have acted on them. A failure on a new connection, or after response bytes,
is returned, and so is a timeout or cancellation.

## Redirects

A `301`, `302`, `303`, `307` or `308` response with a `Location` field is
followed, at most `redirects` times; one more is `TooManyRedirects`. With
`redirects = 0` the redirect response itself is returned.

| Status | Next request |
|---|---|
| `303` | `GET` without a body (`HEAD` stays `HEAD`) |
| `301`, `302` after `POST` | `GET` without a body |
| `301`, `302` otherwise, `307`, `308` | Same method and body |

When the body is dropped, so are `Content-*` fields. A redirect that keeps
the body is not followed for a streamed body, which cannot be read twice:
`send_from` returns the `301`, `302`, `307` or `308` response itself. `Location` may be
absolute, scheme-relative (`//host/path`), absolute-path or relative; it is
resolved against the current URL and `.` and `..` segments are removed.
A location that does not parse is `InvalidRedirect`, another scheme is
`UnsupportedScheme`, and a move from `https` to `http` is `InvalidRedirect`.

When the scheme, host or port changes, the next request and every later one
lose `Authorization`, `Cookie`, and every field named in `config.sensitive`.
They are not restored on a redirect back. The body of a redirect response is
read and discarded, up to 64 KiB, so the connection can be reused; a longer
body closes it.

## Proxies

| Item | Meaning |
|---|---|
| `proxy(url: &str)` | `Result[Proxy, ClientError]` for `http://host:port`, no path or credentials |
| `Proxy.endpoint`, `authorization: Option[str]` | Where to connect, and the `Proxy-Authorization` value |

With `config.proxy` set, every `http` request goes to the proxy with an
absolute-form target (`GET http://host/path HTTP/1.1`). An `https` request
opens a tunnel instead: the client sends `CONNECT host:port HTTP/1.1` to the
proxy, and after a `2xx` reply runs TLS to the origin through it, verifying
the origin's certificate, and sends an origin-form request. A non-`2xx`
reply is `TunnelRefused(status)`; its body is not read. `authorization` is
sent to the proxy as `Proxy-Authorization`, on `CONNECT` for a tunnel, and
never to an origin. Nothing is read from the environment: `HTTP_PROXY` and
`NO_PROXY` are ignored.

## Configuration

`config()` returns a `Config` with every field public:

| Field | Meaning | `config()` |
|---|---|---|
| `limits` | `std.http` `Limits` for response heads and bodies, including a proxy's `CONNECT` reply | `std.http.limits()` |
| `connect_timeout` | Resolve the host and connect | 10 s |
| `write_timeout` | Any one write | 10 s |
| `head_timeout` | From the end of the request to the end of the final response head | 30 s |
| `read_timeout` | Any one read | 30 s |
| `idle_timeout` | Keep a pooled connection | 30 s |
| `continue_timeout` | Wait for `100 Continue` before sending a streamed body anyway | 1 s |
| `max_idle` | Pooled connections in all, at least 0; 0 sends `Connection: close` | 16 |
| `max_idle_per_origin` | Pooled connections for one origin, at least 0 | 4 |
| `redirects` | Redirects to follow, 0 to 100 | 10 |
| `retries` | Replays of a safe request, 0 to 10 | 1 |
| `sensitive` | Further field names dropped across origins | empty |
| `proxy` | `Option[Proxy]` | `Nothing` |

Every wait ends at the earliest of the caller's deadline, its phase timeout
and its per-operation timeout.

## Transports

| Item | Meaning |
|---|---|
| `protocol Transport` | `read_before` and `write_before` with `TcpStream`'s parameters, returning `Result[i64, ClientError]`; `write_before` is generic in its `S: ByteSource` source |
| `protocol Connector[T]` | `open(self, endpoint: &Endpoint, deadline, cancel) -> Result[T, ClientError]`; the connector is borrowed shared, since threads sharing a client open connections at the same time |
| `Endpoint(scheme, host, port: u16, tunnel: Option[Tunnel])` | Where to connect: the origin, or the proxy; `host` is lowercase without brackets |
| `Tunnel(host, port: u16, authorization: Option[str], limits: Limits)` | The proxy to reach the origin through with `CONNECT`; `limits` bounds the proxy's reply head |
| `dial(&endpoint, &deadline, &cancel)` | `Result[TcpStream, ClientError]`: a TCP connection to the endpoint, through its tunnel if it has one |
| `Plain`, `TcpConnector` | A TCP connection; the connector refuses `https` and tunnels with `UnsupportedScheme` |

A connector decides how to reach an endpoint. `dial` resolves the host and
tries each address in turn. An in-memory connector makes code that uses a
client testable without sockets; one that hands out scripted connections
keeps them in a `Mutex`, since `open` borrows it shared. `connector` checks
both type arguments against the protocols where it is called, so a connector
whose `open` takes `self: &mut ...` is reported there; like any argument of a
bound in another module, a connector and a transport must be `pub` classes.

## HTTPS

`import std.http.https` supplies a connector that opens `http` endpoints over
TCP and `https` ones over [`std.tls`](tls.md). It is a separate module so
that a program using only `http` does not depend on the TLS stack.

| Item | Meaning |
|---|---|
| `client(config: Config, tls: ClientConfig)` | `Result[Client[Connection, HttpsConnector], ClientError]` |
| `system_client(config: Config)` | `client` with the operating system's trust roots |
| `connector(tls: ClientConfig)` | `HttpsConnector`, for `std.http.client.connector` |
| `secure_error(error: TlsError)` | The `ClientError` a TLS failure becomes |

Verification is always on: the server's certificate must chain to the roots
in `tls` and name the URL's host, a DNS name or IP address, which is also
sent as the server name (SNI). There is no switch to skip verification;
trust a private CA by building `tls` from its certificate. Client
certificates, protocol versions and revocation lists come from `tls`. If
`tls` offers ALPN protocols, a server that picks anything but `http/1.1` is
`Tls(Incompatible)`.

A pooled `https` connection is keyed by scheme, host and port like any
other, so it is reused only for the same server name. A redirect from
`https` to `http` is `InvalidRedirect`.

TLS failures map to `ClientError`:

| `TlsError` | `ClientError` |
|---|---|
| `Net(Stream(Allocation(a)))` | `Allocation(a)` |
| Any other `Net(e)` | `Net(e)` |
| `Truncated` | `Net(Stream(UnexpectedEof))`, so a closed pooled connection is replayed as a TCP one is |
| `Closed` | `Net(Stream(Io(Closed)))` |
| `Allocation(e)` | `Allocation(e)` |
| Any certificate, handshake or configuration failure | `Tls(TlsFailure)` with the same variant name |

## Errors

`ClientError` is `Net(NetError)` for connection failures, timeouts and
cancellation; `Http(HttpError)` for malformed or oversized responses, with
`Truncated` for a connection that closed early; `Url(UrlError)`;
`Allocation`; `Tls(TlsFailure)` for a TLS handshake or session failure;
`TunnelRefused(status)` for a proxy that refused `CONNECT`;
`Body(StreamError)` for a streamed body's reader failure; and
`InvalidConfig`, `InvalidUrl`, `UnsupportedScheme`, `InvalidRequest`,
`InvalidRedirect`, `TooManyRedirects`, `UnexpectedInterim` and
`WrongBodyLength`. `TlsFailure` has the variants `UntrustedCertificate`,
`ExpiredCertificate`, `WrongHost`, `InvalidCertificate`,
`RevokedCertificate`, `Rejected`, `Protocol`, `InvalidName`,
`InvalidConfiguration`, `NoTrustRoots`, `CertificateRequired` and
`Incompatible`; see [`TlsError`](tls.md). No variant carries a URL or field value, so printing an
error does not reveal credentials.

`Stats` counts, as `u64`: `opened` connections; `reused`, requests sent on a
pooled connection; `retried` replays; `redirected` hops; and `discarded`
connections, those closed for any reason: the response or server ended
reuse, the body was not read to its end, the exchange failed or was
replayed, the connection was stale, expired, over a pool limit or had unread
bytes buffered, or `close_idle` closed it. See
[Sharing a client between threads](#sharing-a-client-between-threads) for the
totals they keep.

## Limits

- No cookie jar. Read `Set-Cookie` fields from `response.head` and send a
  `Cookie` field yourself; the client drops it when the origin changes.
- No content decoding. The client does not send `Accept-Encoding`, so
  servers send identity bodies; a program that asks for `gzip` decodes the
  body itself with [`std.compress`](compress.md) under its own size limit.
- No HTTP/2, and no proxy for `https` other than an `http` proxy with
  `CONNECT`.
- Connecting tries addresses one after another, not in parallel.
