# An HTTP server on supervised actors

Status: **Implementation and qualification in progress (B85).** This proposal
records the intended second server; it does not declare the actor server
production-ready. The existing [worker server](../design/stdlib/http-server.md)
remains supported independently. The [backlog](../backlog.md) owns remaining work.

## Two supported servers

`std.http.server` keeps sequential handlers, bounded workers and its existing
qualification. `std.actor.http.server` uses one supervised actor per connection,
with bounded heaps, mailboxes and socket buffers. A contained handler trap loses
that connection while unrelated connections and admission remain available.
Applications choose the execution and failure model that suits their work.

The actor server should make ordinary bounded request/response handlers simple.
A separate event handler interface handles streaming input/output, timer wakes,
actor messages and shutdown without blocking the stage. Request heads are
owned transient callback inputs; persistent application state obeys the existing
actor heap rules. Factory and connection initialization run in the child heap.
No compiler heap-boundary relaxation is part of this design.

## Protocol and lifecycle

Share HTTP line, field, target and framing rules with `std.http`. A resumable
byte-feed decoder retains no shared strings across actor callbacks and consumes
a bounded amount of input per turn. Partial bodies and output resume on readiness;
waiting does not occupy a native worker. Plain TCP and TLS use the existing actor
network layers. Bounds cover admission, headers, bodies, output, actor heap use,
requests per connection, idle time, handshake, request phases and writes.

Shutdown stops admission, finishes bounded active responses during a grace
period, closes idle connections and then forcibly retires remaining connection
actors. A stop signal must not bypass the drain phase. Refusals preserve the
existing bounded lingering-close policy where possible.

The first supported implementation uses one stage/thread and the existing
1024-live-heap process ceiling. Handlers run to completion and cannot safely
perform blocking operations or unbounded CPU work. Stack overflow and internal
runtime panics remain process-fatal; actor containment is not a sandbox. Shared
foreign state and TLS configuration ownership need explicit repeated-failure
checks. HTTP/2, HTTP/3 and multi-thread stages are separate scope.

## Qualification and comparison

Production readiness requires retained parser corpus parity (including
fragmentation, malformed framing, pipelining and trailers), real TCP/TLS clients,
slow heads/bodies/readers/handshakes, bounded admission, repeated handler traps
and allocation exhaustion, descriptor/heap baselines, graceful shutdown under
load, reverse proxies, fuzzing that treats an unexpected contained parser trap
as a finding, and documented platform limits.

Run the worker and actor servers on the same machine, compiler revision,
requests, response sizes, concurrency and CPU allocations. Publish both an
equal-core comparison and each model's normal configuration where useful.
Record throughput, latency, memory, failures and failure isolation. Simplicity,
speed and resilience differences are hypotheses to measure, not promises inferred
from the concurrency model. The benchmark must stay below each implementation's
configured connection limit.
