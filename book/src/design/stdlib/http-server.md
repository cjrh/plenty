# `std.http.server`

`import std.http.server` serves HTTP/1.1 over [`std.net`](net.md) TCP
connections: method and path routing, a fixed pool of worker threads fed by a
bounded admission queue, a deadline for each phase of a request, and
graceful shutdown. Framing comes from [`std.http`](http.md).
`import std.http.tls` serves HTTPS through [`std.tls`](tls.md). Both modules
are checked Plenty in `stdlib/std/http/` and add no runtime primitives. The tutorial lesson [Serve HTTP requests](../../tutorial/11-concurrency/http-server.md)
shows a complete server.

## Serving

| Item | Meaning |
|---|---|
| `serve[H: Handler[TcpStream]](listener: TcpListener, router: &Router, handler: &H, config: &Config, stop: &CancellationToken)` | `Result[Report, ServerError]`: serve until `stop` is cancelled, then shut down |
| `std.http.tls.serve[H: Handler[TlsTransport]](listener, tls: ServerConfig, router, handler, config, stop)` | `serve` over TLS; see [HTTPS](#https) |
| `serve_with[T: Transport, O: Opener[T], H: Handler[T]](listener, opener: &O, router, handler, config, stop)` | `serve` with a transport that `opener` makes from each accepted `TcpStream` |
| `serve_connection[T: Transport, H: Handler[T]](stream: T, router, handler, config, stop, cancel: &CancellationToken)` | `Report`: serve every request on one connection, on the calling thread |
| `serve_connection_from[T, H](stream: T, peer: Option[SocketAddress], router, handler, config, stop, cancel)` | `serve_connection` that records the connection's peer address in each exchange |
| `protocol Handler[T]` | `def handle(self, exchange: &mut Exchange[T]) -> Result[(), Failure]` |
| `protocol Transport` | `read_before` and `write_before` with the signatures of `TcpStream`'s |
| `reason(status: i64)` | `str`: the reason phrase for a common status, or `""` |

`serve` takes ownership of the listener and runs on the calling thread. It
starts `config.workers` scoped worker threads, then accepts connections on
its own thread and passes each to the queue once it has input (see
[Idle connections](#idle-connections)). Every worker shares one borrow of the router, the
handler and the config, so `handle` takes `self` by shared borrow and cannot
change the handler; shared state needs a synchronized type such as a channel
or token. A handler class and its `handle` method must be `pub`, because
`serve` calls them from another module.

`serve` fails only before serving: `InvalidConfig` for a config outside the
bounds below, `Allocation` or `Thread` if a channel or worker cannot be made,
or `Net` if its selector or wake-up socket cannot be set up.
A worker that cannot start stops the workers already running. After that,
every problem is answered on the wire where possible, counted in the
`Report`, and logged to `config.events`; nothing aborts the server.

`serve_connection` serves one already-open stream. Any class with the two
`Transport` methods works, and an in-memory stream makes handlers testable without sockets. Its exchanges have
no `peer`; `serve_connection_from` takes one, for a transport that knows its
peer or a test that plays one. `stop` and
`cancel` have the meanings described under [Shutdown](#shutdown).

## Routing

| Item | Meaning |
|---|---|
| `router()` | `Result[Router, ServerError]`: an empty table |
| `router.add(method, pattern, key: i64)` | `Result[(), ServerError]`: append a route |
| `router.find(method, path)` | `Result[Match, ServerError]`: the route for a request |
| `Match.Found(key, params: list[str])` | `params` holds the captures in pattern order, still percent-encoded |
| `Match.NotFound` | No pattern matches the path |
| `Match.MethodNotAllowed(allow: str)` | Patterns match but not for this method; `allow` lists the methods, comma-separated |

A pattern starts with `/`. Each segment is literal text, `{name}`, which
captures one nonempty segment, or, last only, `{name...}`, which captures the
rest of the path, possibly empty, slashes included. Names are letters,
digits and `_`. `method` is a method name or `*` for any method. A bad
method or pattern fails with `InvalidPattern`, and a route with the same
method and the same shape as an earlier one with `DuplicateRoute`.

Routes are tried in the order they were added; the first match wins, so add
specific routes before general ones. A `GET` route also answers `HEAD`, with
the body left out. Matching compares the path exactly: no case folding, no
trailing-slash redirect and no `..` normalization. The query string is not
part of the path.

The server answers `404`, or `405` with an `Allow` field, without calling the
handler. A capture that does not percent-decode to UTF-8 gets `400`.

## Exchanges

The handler receives an `Exchange[T]` for one request.

| Field or method | Meaning |
|---|---|
| `request: RequestHead` | Method, target, version and fields as read |
| `path`, `query` | The target split at the first `?`; `query` is raw |
| `route: i64` | The key of the matched route |
| `params: list[str]` | Captures, percent-decoded |
| `peer: Option[SocketAddress]` | The address the connection came from: a reverse proxy's, when there is one (see [`std.http.proxy`](http-proxy.md)) |
| `read_body(&mut buffer, start, stop)` | `Result[i64, HttpError]`: part of the request body; `0` is the end |
| `read_all(&mut out)` | `Result[i64, HttpError]`: append the rest of the body, within `limits.max_body` |
| `send(status, content_type, &body)` | Whole response with `Content-Type` and `Content-Length` |
| `send_text(status, text)` | `send` with `text/plain; charset=utf-8` |
| `respond(head, &body)` | Whole response with the caller's head; `Content-Length` is added |
| `start(head)` | Begin a streamed response |
| `write(&source, start, stop)` | Send and flush part of a streamed body |
| `end()` | Finish a streamed response |
| `stopping()` | Shutdown has begun; a long response should end soon |
| `cancelled()` | The drain deadline has passed; reads and writes now fail |
| `status()`, `sent()` | Status sent (0 before), body bytes written |

The response methods return `Result[(), HttpError]`. A handler answers once.
`respond` refuses a head that declares `Content-Length` or
`Transfer-Encoding` (`ConflictingFraming`), and a body on `204` or `304`
(`LengthMismatch`). `start` streams exactly `Content-Length` bytes when the
head declares it; otherwise it adds chunked framing for HTTP/1.1 or closes
the connection after the body for HTTP/1.0. Writing past a declared length,
or ending short of it, is `LengthMismatch`. A streamed response the handler
leaves unended is ended after it returns.

The first body read sends `100 Continue` when the request asked for it. A
response sent before the body is read to its end closes the connection after
the response, since the next request cannot be found.

`exchange.send_text(200, &exchange.query)` is rejected: `send_text` borrows
the whole exchange exclusively while `exchange.query` is borrowed. Copy the
field first (`query = exchange.query.concat("")?`).

### When a handler fails

| Handler outcome | Response | Counted as |
|---|---|---|
| `Ok` after answering | As sent | `requests` |
| `Ok` without answering | `500` | `handler_errors` |
| `Err` before answering, after a body read failed | `413` (too large), `408` (timed out) or `400` | `protocol_errors` |
| `Err` before answering, otherwise | `500`, connection closed | `handler_errors` |
| `Err` after a streamed response started | Connection closed mid-body | `handler_errors` |

## Configuration

`config()` returns a `Config` with every field public:

| Field | Meaning | `config()` |
|---|---|---|
| `workers` | Worker threads, 1 to 1024 | 4 |
| `queue` | Accepted connections waiting for a worker, at least 1 | 16 |
| `limits` | `std.http` `Limits` for every request | `std.http.limits()` |
| `idle_timeout` | Wait for the first byte of the next request | 5 s |
| `head_timeout` | Read a request head, from its first byte | 10 s |
| `body_timeout` | Read a request body, from the end of its head | 30 s |
| `write_timeout` | Any one write | 10 s |
| `response_timeout` | Whole response, from the end of the request head | 60 s |
| `drain_timeout` | Shutdown grace for active connections | 5 s |
| `max_requests` | Requests on one connection before it closes, at least 1 | 1000 |
| `idle_connections` | Connections waiting for input on the accepting thread, 0 to 1,000,000 | 256 |
| `handshake_timeout` | HTTPS only: finish the TLS handshake, from the accept | 10 s |
| `events` | `Option[ChannelSink]` for [`std.log`](log.md) events | `Nothing` |

Limits must be positive (`max_body` may be 0).

## Idle connections

A connection waiting for its first byte, or for the next request after a
keep-alive response, is parked on the accepting thread's
[`std.ready`](ready.md) selector instead of holding a worker. When input
arrives it joins the queue; a worker serves its request and, if the
connection stays open with nothing more buffered, returns it to be parked
again. Parked connections still close at `idle_timeout`. Ten keep-alive
clients can therefore share one worker.

`idle_connections` caps the parked connections. With `0`, nothing is
parked: each connection goes to the queue at once and a worker holds it
until `idle_timeout`, as a thread-per-connection server would.

## HTTPS

```text
import std.http.tls
import std.tls

tls = std.tls.server_config(&chain_pem, &key_pem)?
report = std.http.tls.serve(listener, tls, &table, &App(), &config, &stop)?
```

`std.http.tls.serve` takes a [`std.tls`](tls.md) `ServerConfig` and calls a
handler of type `Handler[TlsTransport]`; `TlsTransport.stream` is the
`TlsStream`. A worker runs each handshake, which must finish within
`handshake_timeout` of the accept. The deadline covers time spent parked
and queued, so a client that connects and stays silent is closed at that
deadline, not at `idle_timeout`. After the handshake, connections are
served and parked like plain ones: a TLS stream is only parked when it holds
no decrypted input. The plain `serve` does not import `std.tls`.

`serve_with` accepts any `Opener[T]`, the protocol behind `std.http.tls`:

| Method | Meaning |
|---|---|
| `open(self, tcp: TcpStream, deadline: &Deadline, cancel: &CancellationToken)` | `Result[T, OpenError]`: the transport, made on a worker |
| `handshakes(self)` | `bool`: use `handshake_timeout` for new connections and close, not `503`, those refused |
| `watch(self, stream: &T, selector: &mut Selector, key: u64)` | `bool`: register a stream with no buffered input until it has more |
| `unwatch(self, stream: &T, selector: &mut Selector)` | Remove it |

`OpenError` is `TimedOut`, `Cancelled`, `Failed` (counted in
`protocol_errors`) or `Gone` (the client left; not counted).

## Deadlines and faults

Every read and write waits under a deadline, so a slow or silent client
cannot hold a worker longer than the phase allows.

| Situation | Server action |
|---|---|
| No request starts within `idle_timeout` | Close quietly |
| HTTPS: no handshake within `handshake_timeout` of the accept | Close; counted in `timeouts` |
| HTTPS: handshake fails, for example a plaintext request | Close; counted in `protocol_errors` |
| `idle_connections` connections already waiting | A new connection gets `503` with `Retry-After: 1` (HTTPS: is closed); counted in `rejected`. A served connection going idle is closed |
| Head not complete within `head_timeout` | `408`, close; counted in `timeouts` |
| Malformed head, or a line, head or header count over a limit | `400`, or `431` for size limits, `505` for an unsupported version, `501` for an unsupported transfer coding; close |
| Ambiguous framing (`Content-Length` with `Transfer-Encoding`) | `400`, close |
| Declared `Content-Length` over `max_body` | `413` before reading, close |
| Target not starting with `/` | `400`, close |
| Body read past `body_timeout`, or a write past its deadline | Close; counted in `timeouts` |
| Queue full | `503` with `Retry-After: 1` (HTTPS: close without a response), close; counted in `rejected` |
| `accept` fails, for example out of file handles | Count in `accept_errors`, pause 10 ms, keep accepting |

Responses written by the server itself carry `Connection: close`. Allocation
failure while serving closes that connection.

## Shutdown

Cancelling `stop` begins shutdown:

1. `serve` stops accepting and closes the listener.
2. Connections already queued are still served. A connection waiting idle
   for its next request closes at once, whether parked or on a worker; one
   whose request has started arriving is handed to a worker and served. Every response carries `Connection: close`.
3. Handlers see `stopping()` and should finish long responses.
4. At `drain_timeout`, `serve` cancels the second token. Every further read
   or write fails with `Cancelled`, `cancelled()` is true, and the
   connection closes; counted in `cancelled`.
5. `serve` joins every worker and returns the merged `Report`. Every
   accepted connection is closed by then. A client may still be reacting
   to that close: `serve` returning says nothing about the client's
   progress, so join a client task before relying on its output.

Cancellation is cooperative. A handler that loops without reading, writing
or checking `cancelled()` keeps its worker, and `serve` waits for it.

## Reports and events

`Report` counts, as `u64`: `connections`, `rejected`, `requests`,
`handler_errors`, `protocol_errors`, `timeouts`, `cancelled`,
`accept_errors` and `dropped_events`.

With `events` set, each answered request emits an `Info` event `request`
with fields `method`, `target`, `status`, `bytes` (body bytes sent) and
`micros` (from head to end of response). Faults emit `Warn` events: `bad
request`, `bad request body`, `handler failed`, `handler sent no response`,
`timed out`, `cancelled`, `queue full`, `too many connections`, `handshake
timed out`, `handshake failed` and `accept failed`, most with a
`status` field. An event the sink cannot take is dropped and counted in
`dropped_events`; logging never blocks serving.

## Limits

- The accepting thread waits on one selector of at most 64 events per wait,
  and a parked connection costs a list slot and a selector registration.
  A worker serves one connection at a time, from its first byte to the end
  of the response.
- HTTPS connections end without a TLS `close_notify`. Responses are
  length-delimited or chunked, so clients do not depend on it, except for a
  streamed response to HTTP/1.0, whose end is the close and so cannot be
  told from truncation.
- HTTPS has the [`std.tls` limits](tls.md#limits). The server speaks only
  HTTP/1.x, so its `ServerConfig` should offer no ALPN protocol but
  `http/1.1`.
- No `Date` header, compression, request trailers in the exchange, or
  HTTP/2. Forms, JSON, uploads, cookies and forwarded clients are in
  [`std.http.body`](http-body.md), [`std.http.cookie`](http-cookie.md)
  and [`std.http.proxy`](http-proxy.md); static files in
  [`std.http.files`](http-files.md).
- A `503` rejection reads only bytes that have already arrived. If more of
  the request arrives after the close, the client may see a reset instead.
- Captures in `params` are decoded; `path` and `query` are not.
