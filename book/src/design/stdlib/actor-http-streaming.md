# Streaming actor HTTP

[`std.actor.http.server`](actor-http-server.md) offers `serve_events` and
`serve_tls_events` for requests or responses that should progress in bounded
parts. They take the same listener, factory, configuration and stop token as
the complete-request entry points, and return `Result[Report, ServerError]`.
The TLS form additionally owns a `std.tls.ServerConfig`.

The factory creates one mutable `Handler[M, E]` per connection:

```text
def receive(self: &mut H, event: Event[M], response: &mut Response,
            context: &mut Context[Notice[M], ServerError]) -> Result[(), E]
```

## Events and stored state

| Event | Meaning |
|---|---|
| `Head(head, target)` | Validated request head and target |
| `Body(bytes)` | Owned decoded body bytes, bounded by the decoder's per-feed budget |
| `Trailers(fields)` | Validated chunked-body trailer fields |
| `End` | The complete request body has arrived |
| `Writable` | Response output progressed; offer previously unaccepted bytes again |
| `Message(value)` | A user `Notice.Message(value)` sent through the actor context |
| `Actor(mail)` | Unconsumed actor readiness, user timer, request/reply or child-exit mail |
| `Stopping` | One notification when this connection starts draining; finish pending work promptly |

Keep owned byte buffers, numeric state and actor-safe connection objects in
the handler. Text in a head or trailer event is for that callback; copy needed
text into `Bytes` before returning. Events are owned, so the handler decides
what body bytes to retain, consume or drop. Retained application data counts
against the connection's heap budget.

`Context` uses `Notice[M]` as its message type and `ServerError` as its actor
error type. Wrap user messages in `Notice.Message`. Other `Notice` variants
belong to server lifecycle/accounting and are not application commands.
Timer tags below zero are reserved by the server; use nonnegative tags for
application timers. `Event.Actor` preserves the original `Mail`, so existing
[`std.actor.net`](actor-net.md) and [`std.actor.tls`](actor-tls.md) state machines
can consume readiness and timers without reconstructing them.

For an outbound operation, retain a `Dialing` or `Connection` in the handler,
start it from a request callback, and pass its `Event.Actor(mail)` values to
the networking object's `handle` method with the supplied context. Watch
the relevant handle and keep application transitions bounded. A blocking
`std.http.client.fetch` inside the callback still blocks every actor.
Custom child spawning from a connection handler is not supported; the server's
spawn hook is for connection admission. Application timers share a bounded
pool and can return `TimerError.Capacity`.

## Writing responses

| Operation | Contract |
|---|---|
| `start(head)` | Validate and queue an owned response head; `Result[(), ServerError]` |
| `write(source: &[u8])` | Queue part of `source`; returns accepted byte count or `ServerError` |
| `finish()` | Complete framing; a declared length must match; `Result[(), ServerError]` |
| `respond(head, &body)` | Queue and finish one whole response; it must fit |
| `send(status, &content_type, &body)` | Whole response with an automatically encoded head |
| `send_text(status, &text)` | Whole UTF-8 plain-text response |
| `pending()` | Bytes waiting in the application queue, excluding transport/TLS buffers |
| `space()` | Payload space presently available, after framing reserve |
| `is_started()`, `is_done()`, `is_stopping()` | Whether framing began, finished, or shutdown was requested |
| `close_after()` | Disable reuse after this response |
| `shutdown_server()` | Start server-wide draining; this response gets the shutdown grace period |

All whole-response operations return `Result[(), ServerError]`. `start` and
`respond` consume their heads; a failed call does not return the head for retry.
Byte inputs are borrowed and accepted bytes are copied into the bounded queue.

A short successful `write` is back-pressure. Retain the suffix and resume it
on `Writable`; do not call `finish` until every intended byte was accepted.
Do not spin on a zero-byte write. Transport progress drives subsequent events,
and request decoding pauses when response capacity is exhausted. Application
state needs its own bound too: a callback must not keep appending to a separate
buffer while the response is blocked.

`start` accepts a `Content-Length` header, or selects chunked output for
HTTP/1.1 when the length is absent. HTTP/1.0 uses close-delimited output in that
case. Do not supply `Transfer-Encoding` yourself. `respond` supplies the
content length and rejects caller-supplied framing headers. Bodyless statuses
and HEAD obey their framing rules; HEAD consumes offered body bytes without
sending them. Invalid lengths and sequencing return HTTP errors.

A response can begin while request bytes are still arriving. The server keeps
delivering body events under back-pressure, but conservatively closes that
connection after the response rather than reusing it. Finishing a response
before request `End` abandons the unread body and uses bounded lingering close.
For reusable ordinary request/response traffic, finish reading the request
before starting the response.

The server bounds decoder advancement per actor turn. A handler must likewise
bound its own work; actor heap isolation does not impose a CPU-time budget.
Direct `101` upgrades and the worker server's `Exchange`-based WebSocket/SSE
adapters are not supported by this interface.
