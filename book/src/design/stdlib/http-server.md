# `std.http.server`

`import std.http.server` serves HTTP/1.1 over [`std.net`](net.md) TCP
connections: method and path routing, a fixed pool of worker threads fed by a
bounded admission queue, a deadline for each phase of a request, and
graceful shutdown. Framing comes from [`std.http`](http.md).
`import std.http.tls` serves HTTPS through [`std.tls`](tls.md). Both modules
are checked Plenty in `stdlib/std/http/` and add no runtime primitives. The tutorial lesson [Serve HTTP requests](../../tutorial/11-concurrency/http-server.md)
shows a complete server. [Qualification](http-qualification.md) records
which production gates the server passes, its measured baselines, and
deployment guidance; the server is production-ready.

## Serving

| Item | Meaning |
|---|---|
| `serve[R, E, H: Handler[TcpStream, R, E]](listener: TcpListener, router: &Router[R], handler: &H, config: &Config, stop: &CancellationToken)` | `Result[Report, ServerError]`: serve until `stop` is cancelled, then shut down |
| `std.http.tls.serve[R, E, H: Handler[TlsTransport, R, E]](listener, tls: ServerConfig, router, handler, config, stop)` | `serve` over TLS; see [HTTPS](#https) |
| `serve_with[T: Transport, O: Opener[T], R, E, H: Handler[T, R, E]](listener, opener: &O, router, handler, config, stop)` | `serve` with a transport that `opener` makes from each accepted `TcpStream` |
| `serve_connection[T: Transport, R, E, H: Handler[T, R, E]](stream: T, router, handler, config, stop, cancel: &CancellationToken)` | `Report`: serve every request on one connection, on the calling thread |
| `serve_connection_from[T, R, E, H](stream: T, peer: Option[SocketAddress], router, handler, config, stop, cancel)` | `serve_connection` that records the connection's peer address in each exchange |
| `protocol Handler[T, R, E]` | `def handle(self, route: &R, exchange: &mut Exchange[T]) -> Result[(), E]` |
| `protocol Transport` | `read_before`, `write_before` and `shutdown` with the signatures of `TcpStream`'s; `write_before` is generic in its `S: ByteSource` source |
| `reason(status: i64)` | `str`: the reason phrase for a common status, or `""` |

`E` is the handler's own error type: `HttpError`, or a sum such as
`AllocError | HttpError`. Calls infer it from `handle`, as they
infer `R` from the router; a generic function that passes a handler on
declares it too, as in `def run[E, H: Handler[TcpStream, Route, E]](handler: &H)`.
The server needs nothing from the error: it answers and counts every `Err`
alike (see [When a handler fails](#when-a-handler-fails)) and drops it on
the worker. A return type other than `Result[(), E]` does not satisfy
`Handler`:
``App does not satisfy std.http.server.Handler: signature of `handle` does not match``.

`serve` takes ownership of the listener and runs on the calling thread. It
starts `config.workers` scoped worker threads, then accepts connections on
its own thread and passes each to the queue once it has input (see
[Idle connections](#idle-connections)). Every worker shares one borrow of the router, the
handler and the config, so `handle` takes `self` and `route` by shared borrow
and cannot change them; shared state needs a synchronized type such as a
channel or token. A handler class and its `handle` method must be `pub`,
because `serve` calls them from another module, and so must the route type
its signature names. The route type and the handler must be usable on
worker threads, under the ordinary
[worker eligibility](../facilities/03-concurrency/index.md) check.

`serve` fails only before serving: `InvalidConfig` for a config outside the
bounds below, `Allocation` or `Thread` if a channel or worker cannot be made,
or `Net` if its selector or wake-up socket cannot be set up.
A worker that cannot start stops the workers already running. After that,
every problem is answered on the wire where possible, counted in the
`Report`, and logged to `config.events`; nothing aborts the server.

`serve_connection` serves one already-open stream. Any class with the three
`Transport` methods works, and an in-memory stream makes handlers testable
without sockets; its `shutdown` can do nothing. The server calls `shutdown`
only with `Shutdown.Write`, before a [lingering close](#lingering-close). Its exchanges have
no `peer`; `serve_connection_from` takes one, for a transport that knows its
peer or a test that plays one. `stop` and
`cancel` have the meanings described under [Shutdown](#shutdown).

## Routing

| Item | Meaning |
|---|---|
| `router[R]()` | `Result[Router[R], ServerError]`: an empty table whose routes carry values of type `R` |
| `router.add(method, pattern, route: R)` | `Result[(), ServerError]`: append a route |
| `router.find(method, path)` | `Result[Match, ServerError]`: the route for a request |
| `router.route(index)` | `&R`: the value of the route at `index`, in the order routes were added |
| `Match.Found(index, params: list[str])` | `index` is the matched route's; `params` holds the captures in pattern order, still percent-encoded |
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

Before routing, the server checks the target and `Host` with
[`std.http.request_target`](http.md#targets-and-host) and answers `400`
when they break RFC 9112 section 3.2: an HTTP/1.1 request needs exactly one
`Host`, no request may have two, and a `Host` value must be a host with an
optional port. An absolute-form target (`GET http://example.com/a HTTP/1.1`)
is routed on its path, and its authority replaces `Host`. The check lives in
`std.http` so any server built on it applies the same rules; the parser
itself stays permissive so clients and proxies can read any well-formed
message.

The server answers `404`, or `405` with an `Allow` field, without calling the
handler. A capture that does not percent-decode to UTF-8 gets `400`.

### Route values

Each route carries a value of the application's route type `R`, normally an
enum with one variant per route. The server passes the matched route's value
to `handle` as `route`, borrowed from the router, and the handler dispatches
with a `match`. Matching is exhaustive, so a variant the handler does not
answer is a compile error naming it (`non-exhaustive match; missing
Route.Hello(_)`), and a handler whose route type is not the router's is
rejected at the `serve` call (`conflicting types for R`).

```text
pub enum Route:
    Home
    Hello(Greeting)

pub class App:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), AllocError | HttpError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "home\n")?
            case Route.Hello(greeting):
                greeting.handle(exchange)?
        Ok(())

mut routes = std.http.server.router[Route]()?
routes.add("GET", "/", Route.Home)?
routes.add("GET", "/index.html", Route.Home)?
routes.add("GET", "/hello/{name}", Route.Hello(Greeting("hello, ")))?
```

A variant's payload can hold the route's own handler object and its state,
as `Hello` holds a `Greeting`; it is used through a shared borrow, like the
handler. Several patterns may carry equal values, so two paths can share one
variant; `DuplicateRoute` depends only on method and pattern shape. `router`
needs its type argument written out: `router[Route]()`.

The server does not generate the dispatch. A route enum cannot implement
`Handler` itself, because enums have no methods and protocols constrain only
classes, and a router of handler objects of different classes would need
existential protocol values, which Plenty does not have. One `match` in one
handler class keeps dispatch static and checked.

## Exchanges

The handler receives an `Exchange[T]` for one request.

| Field or method | Meaning |
|---|---|
| `request: RequestHead` | Method, target, version and fields as read |
| `authority: str` | `host[:port]` the request names: an absolute-form target's authority, else the `Host` value; empty only for HTTP/1.0 without `Host` |
| `path`, `query` | The target's path and the text after its first `?`, both still percent-encoded; an absolute-form target's scheme and authority are removed |
| `params: list[str]` | Captures, percent-decoded |
| `peer: Option[SocketAddress]` | The address the connection came from: a reverse proxy's, when there is one (see [`std.http.proxy`](http-proxy.md)) |
| `read_body(&mut buffer, start, stop)` | `Result[i64, HttpError]`: part of the request body; `0` from a non-empty range is the end, and an empty range returns `0` without reading |
| `read_all(&mut out)` | `Result[i64, HttpError]`: append the rest of the body, within `limits.max_body` |
| `send(status, content_type, &body)` | Whole response with `Content-Type` and `Content-Length` |
| `send_text(status, text)` | `send` with `text/plain; charset=utf-8` |
| `respond(head, &body)` | Whole response with the caller's head; `Content-Length` is added |
| `start(head)` | Begin a streamed response |
| `write(&source, start, stop)` | Send and flush part of a streamed body |
| `end()` | Finish a streamed response |
| `stopping()` | Shutdown has begun; a long response should end soon |
| `cancelled()` | The drain deadline has passed; reads and writes now fail |
| `status()`, `sent()` | Status sent (0 before), body bytes written (0 for `HEAD`) |
| `stop_token()`, `cancel_token()` | Shares of the tokens behind `stopping()` and `cancelled()`, for work that outlives a call |
| `switch_protocols(head)` | `Result[(T, Bytes), HttpError]`: answer `101` and take the connection; see [Switching protocols](#switching-protocols) |
| `detach(head)` | `Result[T, HttpError]`: answer with a close-delimited body and take the connection; see [Detaching a connection](#detaching-a-connection) |

The response methods return `Result[(), HttpError]`. A handler answers once.
`respond` refuses a head that declares `Content-Length` or
`Transfer-Encoding` (`ConflictingFraming`), and a body on `204` or `304`
(`LengthMismatch`). `start` streams exactly `Content-Length` bytes when the
head declares it; otherwise it adds chunked framing for HTTP/1.1 or closes
the connection after the body for HTTP/1.0. Writing past a declared length,
or ending short of it, is `LengthMismatch`; a `HEAD` response sends no body,
so it may end without writing. A streamed response the handler
leaves unended is ended after it returns.

The first body read sends `100 Continue` when the request asked for it. A
response sent before the body is read to its end closes the connection after
the response, since the next request cannot be found.

`exchange.send_text(200, &exchange.query)` is rejected: `send_text` borrows
the whole exchange exclusively while `exchange.query` is borrowed. Bind the
field first: `query = exchange.query` copies the `str`, sharing its storage.

### Switching protocols

`switch_protocols(head)` sends `head`, which must have status `101`, as
given, and hands the connection to the caller with any input that arrived
after the request head. The head carries the `Upgrade` and `Connection`
fields the new protocol needs. The request body must be empty or read to its
end (`Unfinished` otherwise), and the exchange must be unanswered (`Broken`
otherwise). The exchange then counts as answered; the server neither reuses
nor closes the connection, and the handler owns it until it returns or
drops it. The connection still occupies the worker for as long as the
handler runs. [`std.http.websocket`](websocket.md)'s `accept` is built on
it.

### Detaching a connection

`detach(head)` sends `head` with `Connection: close` added and hands the
connection to the caller, which writes a body that ends when it closes the
connection: a long-lived stream that the server's `response_timeout` would
otherwise cut off. The status must be 200 to 999 (`InvalidStatusLine`
otherwise) and allow a body (`LengthMismatch` for `204` and `304`), and the
head must not declare `Content-Length` or `Transfer-Encoding`
(`ConflictingFraming`). A `HEAD` request cannot detach (`Broken`), since its
response has no body; answer it with `start` and `end`. The rules for the
request body and the handler's ownership are those of `switch_protocols`;
input that arrived after the request head is dropped. A refused head leaves
the exchange unanswered. To watch for shutdown, take shares of
`stop_token()` and `cancel_token()` and pass them to the stream's waits.
[`std.http.sse`](sse.md)'s `accept` is built on it.

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
| `idle_connections` | Connections waiting on the accepting thread for a request, the rest of its head or the rest of a handshake, 0 to 1,000,000 | 256 |
| `handshake_timeout` | HTTPS only: finish the TLS handshake, from the accept | 10 s |
| `events` | `Option[ChannelSink]` for [`std.log`](log.md) events | `Nothing` |

Limits must be positive (`max_body` may be 0).

## Idle connections

Unless `idle_connections` is `0`, a worker does not wait for a client to
send a request. A connection waiting
for its first byte, for the next request after a keep-alive response, or
for the rest of a request head is parked on the accepting thread's
[`std.ready`](ready.md) selector instead of holding a worker. When the
first input arrives it joins the queue, and a worker reads what has
arrived. If that is not yet a whole head, the worker hands the connection
back with those bytes; the accepting thread then reads the rest as it
arrives and queues the connection once the head is complete. A worker
serves the request and, if the connection stays open with no next head
complete, returns it to be parked again. Ten keep-alive clients can
therefore share one worker, and clients that send heads slowly hold no
worker at all.

A head is complete for this purpose once its final empty line has arrived,
or once it is bound to fail: a line over `max_line`, a head over
`max_head`, a line ending in a bare LF, or a field line past
`max_headers`. A parked connection with a partial head holds those bytes,
at most `max_head` (64 KiB by default) and usually a 4 KiB buffer.
Parked connections close at `idle_timeout`, or answer `408` at
`head_timeout` once a head has started.

`idle_connections` caps the parked connections, including those with
partial heads, so it bounds how many slow clients the server waits for: a
new connection is refused when it is reached. A connection partway through
a head or a handshake that a worker hands back is parked even past the cap,
since it was admitted while there was room; at most
`queue + 2 * workers + 64` such connections can be outside the parking
slots at once. With `0`, nothing is parked: each connection goes to the queue at
once and a worker holds it until `idle_timeout`, reading each head as it
arrives, as a thread-per-connection server would.

## HTTPS

```text
import std.http.tls
import std.tls

tls = std.tls.server_config(&chain_pem, &key_pem)?
report = std.http.tls.serve(listener, tls, &table, &App(), &config, &stop)?
```

`std.http.tls.serve` takes a [`std.tls`](tls.md) `ServerConfig` and calls a
handler of type `Handler[TlsTransport, R, E]`; `TlsTransport.stream` is the
`TlsStream`. Each handshake must finish within `handshake_timeout` of the
accept. Workers advance it as the client's messages arrive, and between
them the connection waits parked, so a client that stalls mid-handshake
holds no worker. The deadline covers time spent parked and queued, so a
client that connects and stays silent is closed at that deadline, not at
`idle_timeout`. After the handshake, connections are served and parked like
plain ones: a TLS stream is only parked when it holds no decrypted input,
and a partial head is decrypted and gathered on the accepting thread. The
plain `serve` does not import `std.tls`.

`serve_with` accepts any `Opener[T]`, the protocol behind `std.http.tls`:

| Method | Meaning |
|---|---|
| `open(self, tcp: TcpStream)` | `Result[T, OpenError]`: the transport, made on a worker without waiting for the peer |
| `advance(self, stream: &mut T, deadline: &Deadline, cancel: &CancellationToken)` | `Result[(), OpenError]`: advance the opening handshake until it completes or `deadline` passes (`TimedOut`); with a deadline already passed, do only what is possible without waiting |
| `handshakes(self)` | `bool`: use `handshake_timeout` for new connections and close, not `503`, those refused |
| `watch(self, stream: &T, selector: &mut Selector, key: u64)` | `bool`: register a stream for what it waits for; called only after a read or `advance` that did not wait has timed out, so no input is buffered |
| `unwatch(self, stream: &T, selector: &mut Selector)` | Remove it |

A worker calls `advance` with an expired deadline and parks the connection
while it returns `TimedOut`; with `idle_connections` set to `0`, or once
shutdown has begun, it waits up to `handshake_timeout` instead. `OpenError`
is `TimedOut`, `Cancelled`, `Failed` (counted in `protocol_errors`) or
`Gone` (the client left or refused the handshake; not counted).

## Deadlines and faults

Every read and write waits under a deadline, so a slow or silent client
cannot hold a worker longer than the phase allows. With parking on, a
worker waits for neither a request head nor a handshake (see
[Idle connections](#idle-connections)).

| Situation | Server action |
|---|---|
| No request starts within `idle_timeout` | Close quietly |
| HTTPS: no handshake within `handshake_timeout` of the accept | Close; counted in `timeouts` |
| HTTPS: the client closes or sends a fatal alert during the handshake, for example because it rejects the certificate | Close; not counted |
| HTTPS: handshake fails otherwise, for example a plaintext request | Close; counted in `protocol_errors` |
| `idle_connections` connections already waiting | A new connection gets `503` with `Retry-After: 1` (HTTPS: is closed); counted in `rejected`. A served connection going idle is closed; one partway through a head or a handshake is parked anyway |
| Head not complete within `head_timeout` | `408`, close; counted in `timeouts` |
| Malformed head, or a line, head or header count over a limit | `400`, or `431` for size limits, `505` for an unsupported version, `501` for an unsupported transfer coding; close |
| Ambiguous framing (`Content-Length` with `Transfer-Encoding`) | `400`, close |
| Declared `Content-Length` over `max_body` | `413` before reading, close |
| HTTP/1.1 without `Host`, several `Host` fields, or an invalid `Host` value | `400`, close |
| Target neither origin-form (`/path`) nor `http`/`https` absolute-form | `400`, close |
| Part of a head arrives, then nothing | Parked without a worker; `408` from the accepting thread at `head_timeout`, close; counted in `timeouts` |
| Body read past `body_timeout`, or a write past its deadline | Close; counted in `timeouts` |
| Queue full | `503` with `Retry-After: 1` (HTTPS: close without a response), close; counted in `rejected` |
| `accept` fails, for example out of file handles | Count in `accept_errors`, pause 10 ms, keep accepting |

Responses written by the server itself carry `Connection: close`. Allocation
failure while serving closes that connection.

### Lingering close

Closing a socket while request bytes are still unread makes the kernel
reset the connection, and a client that has not yet read the answer can
lose it. So after a response that may leave request input unread, the
server ends its output instead (`Shutdown.Write`; over HTTPS, `close_notify`
first) and discards input until the client ends its own, 16 MiB have been
discarded, or 2 s (at most `write_timeout`) have passed, and then closes. The
client reads the whole answer and a clean end. The budget is large because a
reverse proxy keeps forwarding a refused body up to its own limit, and
answers `502` instead of the refusal if the connection resets under it. This applies to the `400`,
`408`, `413` and `431` refusals above, to a handler failure answered with a
body-read status, and to a handler response sent before the request body
was read to its end.

With parking on, the discarding happens on the accepting thread: the
connection is parked in a slot (and so counts toward `idle_connections`)
and closed when its input ends, its budget is spent, or its deadline
passes. With `idle_connections` `0`, under `serve_connection`, or once
shutdown has begun, the worker discards before taking its next connection.
A parked `408` from the accepting thread lingers the same way. Shutdown
closes lingering connections that are parked at once.

Bytes of a pipelined request that arrived with the previous one start that
request: its `head_timeout` runs from the end of the previous response, and
an end or stall inside it is answered (`400` or `408`) rather than closed
quietly. Blank lines alone do not start a request.

A head is parsed once it is complete (see
[Idle connections](#idle-connections)), so a malformed line followed by
nothing gets `408` at `head_timeout`, not `400` at once. `408` and `503`
answers written on the accepting thread get a write budget of
`write_timeout`, at most 100 ms.

Accepted connections have Nagle's algorithm turned off (`TCP_NODELAY`).
Responses are buffered and flushed whole, so this sends no extra small
segments, and a head followed by a body larger than the 4 KiB write buffer
is not held back for a delayed acknowledgement. Over HTTPS the server
also acknowledges the client's TLS `Finished` at once, so a client that
leaves Nagle's algorithm on sends its first request without waiting for a
delayed acknowledgement (see [`std.tls`](tls.md#trust-roots-and-configuration)).

## Shutdown

Cancelling `stop` begins shutdown:

1. `serve` stops accepting and closes the listener.
2. Connections already queued are still served. A connection waiting idle
   for its next request, or parked mid-handshake, closes at once; one whose
   request has started arriving is handed to a worker, which waits for the
   rest of its head, and served. Every response carries `Connection: close`.
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

To drain when a supervisor sends `SIGTERM` (or on Ctrl-C), run
[`std.signal.cancel_on`](signal.md#stopping-a-server) on a thread beside
`serve`: it cancels `stop` when the signal arrives. A second signal ends the
process at once.

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
  and a parked connection costs a list slot, room for one free-slot index,
  and a selector registration. Parking reuses a vacated slot in constant
  time; each wait still scans every slot for expired deadlines.
  A worker serves one connection at a time, from a complete head to the
  end of the response.
- `idle_connections` bounds the slow clients the server waits for: with
  that many partial heads parked, new connections get `503` until their
  `head_timeout` passes. Raise it, and the descriptor limit, for exposed
  servers; each parked partial head holds at most `max_head` bytes.
- HTTPS connections end without a TLS `close_notify`, except those closed
  [lingering](#lingering-close). Responses are
  length-delimited or chunked, so clients do not depend on it, except for a
  streamed response to HTTP/1.0, whose end is the close and so cannot be
  told from truncation.
- HTTPS has the [`std.tls` limits](tls.md#limits). The server speaks only
  HTTP/1.x, so its `ServerConfig` should offer no ALPN protocol but
  `http/1.1`.
- No `Date` header, compression, request trailers in the exchange, or
  HTTP/2. A connection taken by `switch_protocols` or `detach` holds its
  worker until the handler returns, so long-lived WebSockets and event
  streams count against `workers`. Forms, JSON, uploads, cookies and forwarded clients are in
  [`std.http.body`](http-body.md), [`std.http.cookie`](http-cookie.md)
  and [`std.http.proxy`](http-proxy.md); static files in
  [`std.http.files`](http-files.md).
- A `503` rejection reads only bytes that have already arrived. If more of
  the request arrives after the close, the client may see a reset instead.
- A [lingering close](#lingering-close) discards at most 16 MiB in at most
  2 s. A client still sending past that, such as a large upload refused
  with `413` without `Expect: 100-continue`, can still see a reset; past
  `idle_connections`, a refusal closes at once.
- Captures in `params` are decoded; `path` and `query` are not.
- `Host` and absolute-form authorities are checked against the URI
  grammar, not against names the server answers for. A handler that serves
  only some hosts compares `exchange.authority` itself.
- `OPTIONS *` and `CONNECT` targets get `400`.
- A trap in a handler ends the process; see
  [handler errors and traps](http-qualification.md#handler-errors-and-traps).
