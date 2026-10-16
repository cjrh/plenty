# `std.http.server`

`import std.http.server` serves HTTP/1.1 over [`std.net`](net.md) TCP
connections: method and path routing, a fixed pool of worker threads fed by a
bounded admission queue, a deadline for each phase of a request, and
graceful shutdown. Framing comes from [`std.http`](http.md). The module is
checked Plenty in `stdlib/std/http/server.plenty` and adds no runtime
primitives. The tutorial lesson [Serve HTTP requests](../../tutorial/11-concurrency/http-server.md)
shows a complete server.

## Serving

| Item | Meaning |
|---|---|
| `serve[H: Handler[TcpStream]](listener: TcpListener, router: &Router, handler: &H, config: &Config, stop: &CancellationToken)` | `Result[Report, ServerError]`: serve until `stop` is cancelled, then shut down |
| `serve_connection[T: Transport, H: Handler[T]](stream: T, router, handler, config, stop, cancel: &CancellationToken)` | `Report`: serve every request on one connection, on the calling thread |
| `protocol Handler[T]` | `def handle(self, exchange: &mut Exchange[T]) -> Result[(), Failure]` |
| `protocol Transport` | `read_before` and `write_before` with the signatures of `TcpStream`'s |
| `reason(status: i64)` | `str`: the reason phrase for a common status, or `""` |

`serve` takes ownership of the listener and runs on the calling thread. It
starts `config.workers` scoped worker threads, then accepts connections and
passes each to the queue. Every worker shares one borrow of the router, the
handler and the config, so `handle` takes `self` by shared borrow and cannot
change the handler; shared state needs a synchronized type such as a channel
or token. A handler class and its `handle` method must be `pub`, because
`serve` calls them from another module.

`serve` fails only before serving: `InvalidConfig` for a config outside the
bounds below, `Allocation` or `Thread` if a channel or worker cannot be made.
A worker that cannot start stops the workers already running. After that,
every problem is answered on the wire where possible, counted in the
`Report`, and logged to `config.events`; nothing aborts the server.

`serve_connection` serves one already-open stream. Any class with the two
`Transport` methods works, which is how a TLS session will be served, and an
in-memory stream makes handlers testable without sockets. `stop` and
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
| `events` | `Option[ChannelSink]` for [`std.log`](log.md) events | `Nothing` |

Limits must be positive (`max_body` may be 0).

## Deadlines and faults

Every read and write waits under a deadline, so a slow or silent client
cannot hold a worker longer than the phase allows.

| Situation | Server action |
|---|---|
| No request starts within `idle_timeout` | Close quietly |
| Head not complete within `head_timeout` | `408`, close; counted in `timeouts` |
| Malformed head, or a line, head or header count over a limit | `400`, or `431` for size limits, `505` for an unsupported version, `501` for an unsupported transfer coding; close |
| Ambiguous framing (`Content-Length` with `Transfer-Encoding`) | `400`, close |
| Declared `Content-Length` over `max_body` | `413` before reading, close |
| Target not starting with `/` | `400`, close |
| Body read past `body_timeout`, or a write past its deadline | Close; counted in `timeouts` |
| Queue full | `503` with `Retry-After: 1`, close; counted in `rejected` |
| `accept` fails, for example out of file handles | Count in `accept_errors`, pause 10 ms, keep accepting |

Responses written by the server itself carry `Connection: close`. Allocation
failure while serving closes that connection.

## Shutdown

Cancelling `stop` begins shutdown:

1. `serve` stops accepting and closes the listener.
2. Connections already queued are still served. A connection waiting idle
   for its next request closes at once; a request already arriving is read
   and served. Every response carries `Connection: close`.
3. Handlers see `stopping()` and should finish long responses.
4. At `drain_timeout`, `serve` cancels the second token. Every further read
   or write fails with `Cancelled`, `cancelled()` is true, and the
   connection closes; counted in `cancelled`.
5. `serve` joins every worker and returns the merged `Report`.

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
`timed out`, `cancelled`, `queue full` and `accept failed`, most with a
`status` field. An event the sink cannot take is dropped and counted in
`dropped_events`; logging never blocks serving.

## Limits

- Each worker serves one connection at a time, so an idle keep-alive
  connection occupies a worker until `idle_timeout`. Size `workers` and
  `idle_timeout` together; readiness-based parking of idle connections is
  later work.
- Only plain TCP. TLS is served through `serve_connection` once
  [`std.tls`](../../backlog.md) lands; `serve` over a TLS listener is later
  work.
- No `Date` header, compression, static file serving, request trailers in
  the exchange, or HTTP/2.
- A `503` rejection reads only bytes that have already arrived. If more of
  the request arrives after the close, the client may see a reset instead.
- Captures in `params` are decoded; `path` and `query` are not.
