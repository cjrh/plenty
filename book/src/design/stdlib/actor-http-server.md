# `std.actor.http.server`

HTTP/1.1 with one supervised actor per connection, over plain TCP or TLS.
The server uses the [actor networking](actor-net.md), [actor TLS](actor-tls.md)
and [resumable HTTP decoder](http-decoder.md) modules. Application callbacks
run on one thread. A contained trap discards the connection's heap and closes
its tracked sockets; admission and unrelated connections continue.

The [qualification record](actor-http-qualification.md) separates tested
behavior from outstanding production-readiness gates. Qualification currently
targets x86-64 GNU/Linux. HTTP/2 and HTTP/3 are outside this implementation.

## Choosing a server

Both HTTP servers remain supported, with different execution contracts.

| Choice | `std.http.server` | `std.actor.http.server` |
|---|---|---|
| Ordinary handlers | Sequential `Exchange` operations, shared handler | Complete bounded `Request`, mutable handler per connection |
| Streaming | Sequential reads and writes on workers | Events, partial writes and explicit resumable state |
| Scheduling | Bounded worker pool; idle heads wait off workers | One nonblocking stage; callbacks run to completion |
| Contained failures | Returned errors; runtime traps end the process | Returned errors, handler traps and actor heap exhaustion |
| Existing helpers | Router, uploads, files, WebSockets and SSE | HTTP framing and response primitives; application dispatch |

Choose using the application's failure model, I/O needs and measured workload.
The [comparison harness](actor-http-qualification.md#performance-comparison)
does not assume that either implementation is faster. The actor server does not
make blocking database or HTTP-client operations nonblocking.

## Complete requests

Start with the runnable [actor HTTP lesson](../../tutorial/11-concurrency/actor-http-server.md).
The simple entry points are:

| Entry point | Contract |
|---|---|
| `serve(listener, factory, &config, &stop)` | Serve plain TCP until shutdown; returns `Result[Report, ServerError]` |
| `serve_tls(listener, tls, factory, &config, &stop)` | The same lifecycle using an owned `std.tls.ServerConfig` |
| `config()` | Default configuration, without starting a server |

The server takes ownership of the listener and factory, and of a supplied TLS
configuration. A failed setup releases these resources; they are not returned
for retry. This differs from a queue's failed submission: `serve` owns the
server lifecycle, including cleanup after an error. Configuration and the stop
token are borrowed for the call.

`factory` is a named function returning `Result[H, FE]`. The server calls it
inside each connection's heap. `H` implements `RequestHandler[E]`:

```text
def handle(self: &mut H, request: Request, response: &mut Response) -> Result[(), E]
```

The handler persists across requests on that connection. It is not shared
across connections. Factory and handler error types may differ. Its stored
fields must obey the [actor heap boundary](actor.md#the-spawn-hook):
use owned `Bytes` for persistent text, not `str`, shared owners or locks.
Closures with captured state do not cross that boundary. Build connection
state in the factory instead of allocating it outside the actor heap.

`Request` owns `head: RequestHead`, `target: RequestTarget`, `body: Bytes` and
`trailers: list[Header]`. These values are temporary callback inputs. The
complete request body is buffered up to `config.limits.max_body`, subject also
to the actor's heap limit. Route using the validated target and method; the
worker server's `Exchange` and stored `Router` are not actor state.

Use `Response.send_text`, `send`, or `respond` for a whole bounded response.
It must fit in `pending_output` together with its encoded head and framing
reserve. Larger bodies use the [streaming interface](actor-http-streaming.md).
Returning without a response produces `500`; an unfinished response is
finished before the callback completes, which can fail on a length mismatch.

## Configuration and bounds

| Field | Default | Meaning |
|---|---:|---|
| `capacity` | 256 | At most this many connection actors; accepted range 1–1000 |
| `heap_limit` | 4 MiB | Budget per connection; accepted range 256 KiB–64 MiB |
| `input_buffer`, `output_buffer` | 4096 each | TCP ciphertext or plaintext transport buffers; at least 64 bytes |
| `pending_output` | 65536 | Application response queue, including its head; at least 512 bytes |
| `limits` | `std.http.limits()` | Maximum line, head, header count and decoded request body |
| `idle_timeout` | 5 s | Waiting for a request |
| `head_timeout` | 10 s | Receiving a request head |
| `body_timeout` | 30 s | Receiving its body |
| `write_timeout` | 10 s | Stalled transport output |
| `response_timeout` | 60 s | Completing a response |
| `handshake_timeout` | 10 s | Establishing TLS |
| `drain_timeout` | 5 s | Grace period before forced stage shutdown |
| `max_requests` | 1000 | Maximum requests per connection; at least one |

Zero phase durations disable those phase timers. A zero drain duration instead
requests immediate teardown. The independent heap, buffer and protocol limits
all apply: increasing `max_body` alone does not increase available memory.
The runtime has at most 1024 live actor heaps per process, including the server's
acceptor and any other stages. Capacity is not a promise that admission can
always reserve another heap or descriptor.

At capacity or when admission cannot reserve a child, the server closes a newly
accepted connection. It does not promise an HTTP `503` response for overload.
HTTP parse refusals use bounded lingering close so unread request bytes do not
immediately turn a completed response into a TCP reset.

## Errors, reports and shutdown

`ServerError` distinguishes invalid configuration and loss of the acceptor,
and preserves allocation, byte-buffer, HTTP, network, TLS, timer, selector,
actor-start and time-range causes in named variants. Application failures are
classified as `Handler`; application error values are not sent to the peer.
Report application-specific diagnostics before returning them when needed,
using an operation that respects the nonblocking callback contract.

A handler `Err` produces a generic `500` when a response has not started.
After output has started, an error closes the connection rather than adding a
second response. A trap closes the connection without a recoverable HTTP
answer. Isolated construction covers failures before the handler starts too.
It does not retry a partially handled request.

`Report.actors` is the underlying actor `Stats`; it includes the acceptor and
admitted construction attempts. `requests` counts completed responses,
`rejected` counts admission refusals, and `protocol_errors` counts reported
parse failures. A selector or setup failure returns `Err` instead of a report.

Cancel the external stop token, or call `Response.shutdown_server()` from an
authorized application route, to start draining. Admission stops, idle
connections close, and active responses get the configured grace period.
The external driver then calls [stage shutdown](actor.md#actors-and-stages),
even if application mailboxes or timers are full. Early error returns also
drop the stage and reclaim its live heaps. Use [`std.signal.cancel_on`](signal.md#stopping-a-server)
outside the stage to turn process signals into cancellation.

## Reliability boundary

Callbacks and destructors are not preempted. Blocking I/O or an infinite loop
can stall the whole stage, including its shutdown checks. Stack overflow and
internal runtime panics remain process-fatal. Traps do not roll back external
effects or run user destructors; foreign-library allocations and process-wide
locks have the [actor runtime's limits](actor.md#limits).

TLS trap cleanup also retains a reference to its immutable configuration;
see [TLS memory ownership](actor-tls.md#memory). Repeated failure with one
configuration and repeated configuration replacement are different workloads.
WebSocket upgrades, dedicated actor SSE helpers, blocking-work offload and
multithreaded stages are not provided by this module.

## Scheduler adapter

`Node[F, H]` is an opaque actor-dispatch adapter with private state. Its public
`started`, `receive` and `spawn` methods implement the lifecycle expected by
[`std.actor`](actor.md#actors-and-stages), using `Context[Notice[M], ServerError]`
and returning `ServerError`. `spawn` returns an optional child node. These
methods advance the server's admission or connection state; applications use
the `serve` entry points instead of constructing or driving nodes directly.
The type and lifecycle methods are public because the existing generic actor
dispatcher checks their visibility across module boundaries. This is a
visibility exception, not a new raw runtime hook.
