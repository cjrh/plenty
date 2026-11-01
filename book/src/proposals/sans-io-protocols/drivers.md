# Blocking and actor protocol drivers

Companion to [Runtime-independent networking protocols](../sans-io-protocols.md).
Status: **Proposed architecture, not implemented adapters.** Drivers follow the
[session contract](contract.md). Work and priority live in the
[backlog](../../backlog.md).

## Shared mechanics, different execution

Both drivers must preserve the same input/output order and protocol state. They
need not expose identical application methods or share all control flow.
A sequential receive can wait; an actor receive reports an event and returns.
Connection establishment, transport ownership and application dispatch belong in
the driver, while frame validity and legal state transitions stay in the session.

Useful mechanics to extract after both implementations exist include cursor
management, bounded buffer transfer and a single bounded advancement step. Do
not hide waiting, actor contexts or runtime callbacks in those helpers. Keep
protocol-specific decisions out of both drivers even if copying them would be
shorter than extracting the correct session operation.

## Blocking driver

A sequential operation conceptually does the following:

```text
check caller cancellation and deadline before admitting a new command
submit the command, or remember its explicitly unaccepted suffix
repeat within a bounded advancement batch:
    deliver pending events relevant to the operation
    move session output into a bounded driver send buffer
    write pending transport output and retain any unsent suffix
    feed buffered input and retain any unconsumed suffix
    advance protocol time using supplied monotonic now
    retry admissible command progress
if operation is not complete:
    wait for useful transport readiness or the earliest relevant deadline
    perform bounded reads/writes, then resume advancement
```

This is an ordering sketch, not an instruction to drain every queue before
checking another. Balance both directions: bidirectional protocols can deadlock
if a driver insists on completing a large write before processing a peer's
control input. Conversely, do not read beyond bounded capacity merely because
input is ready.

Use existing deadline-aware networking and cancellation mechanisms rather than
polling or adding a hidden event runtime. The wait ends at the earliest of the
caller's deadline, the next protocol timer, and any driver shutdown deadline.
If protocol progress requires simultaneous interests, use the existing readiness
facilities rather than serial full-duration read and write waits. A generic
blocking transport remains useful inside this driver where its capabilities
actually suffice.

An operation completes according to its documented local semantics: command
admitted, bytes flushed, message received or close handshake finished. These
are different milestones. Output remaining after a receive operation must have
a specified owner and flush policy; automatic pongs cannot remain queued forever
because the application stopped calling receive.

Do not silently start a background thread to solve that problem. State whether
the API makes progress only during calls, offers a drive/flush operation, or
requires an explicit service loop for autonomous keepalive behavior. Preserve
existing sequential API guarantees during extraction.

## Cancellation and retry

Before admission, pre-cancellation leaves the session unchanged. Once any part
of an operation has affected the wire, a timeout/cancellation cannot generally
be retried as a fresh operation. The driver either exposes explicit resumable
progress or terminates the connection according to its existing contract.

For WebSocket, preserve the current rule that cancellation mid-frame closes
the connection rather than inviting an unsafe resend. Retaining internal state
makes correct resumption possible in principle; it does not change a public
method's cancellation promise without a separate compatibility decision.

An abort discards remaining output and releases the transport. A graceful close
is a separate bounded operation driven until its completion condition or a
specified deadline. Neither destructor cleanup nor actor trap cleanup may wait
for it.

## Actor driver

A connection actor constructs its session and buffers inside its own heap. It
owns the transport adapter, session, input cursor, output cursor and protocol
timer state. It reacts to:

| Trigger | Driver action |
| --- | --- |
| Network readiness | Run bounded transport progress, then bounded session advancement |
| Application message/command | Attempt admission without waiting and return an explicit result |
| Protocol timer | Advance with the current instant, ignore stale schedules, recompute deadline |
| Continuation | Resume already-runnable bounded work without requiring new socket readiness |
| Application capacity restored | Resume event delivery and any paused session/input progress |
| Stop/drain request | Reject new work, drive bounded protocol closure, then force abort at the drain deadline |

Readiness is evidence that an attempt may progress, not a promise that it will.
`WouldBlock` is normal. Enable writable interest only while transport output
actually waits; permanently watching a writable socket can spin the stage.
When input/event capacity is exhausted, pause the appropriate intake and retain
unconsumed input. Resume explicitly when capacity returns.

If a turn budget expires while buffered input or internal transitions remain,
arrange a continuation through a supported actor wake mechanism. Do not wait
for another readiness event: the data may already have left the kernel. Bound
continuation scheduling so one connection does not fill its own mailbox with
redundant wakeups. Timer/admission failure is a reported failure, not permission
to drop a required wake.

Handlers run to completion. The actor driver must not call blocking DNS, HTTP,
database, entropy acquisition or user callbacks with unspecified blocking
behavior from its protocol advancement path. Use existing nonblocking facilities
or an explicitly supported external-service integration; do not assume the
[proposed blocking-work pool](../actor-blocking-work.md) already exists.

An actor's bounded mailbox is not an automatic delivery guarantee. If command
submission or event forwarding encounters a full mailbox, preserve ownership
and report backpressure. Avoid forwarding every protocol event through another
mailbox when direct bounded delivery to the owning actor is sufficient.

## Layering over TLS

The straightforward choices are:

- Blocking WebSocket → existing TLS transport → TCP.
- Actor WebSocket → actor TLS → actor TCP.
- Custom runtime → WebSocket session → TLS session → caller-owned socket.

Every intermediate layer can retain partial input/output. Pump the composed
layers until an external dependency or work budget stops progress. Each layer
must identify pending work without assuming that lack of new socket input means
lack of plaintext or protocol events. Include plaintext, ciphertext and protocol
buffers in the connection's peak-memory accounting.

A TLS close notification is not a WebSocket close frame. Preserve the ordering
of WebSocket close, protocol flush, TLS close and socket release, with a bounded
fallback for peers that stop responding. TLS alerts and protocol close output
may still need draining after an error, but the driver must retain the original
cause and never wait indefinitely to deliver them.

## Datagram and multiplexed protocols

Do not adapt UDP by concatenating packets into a stream. A datagram session
receives one packet with source/destination metadata and produces packet output
with its destination and relevant metadata. Define atomic packet admission,
maximum datagram size, truncation handling and endpoint demultiplexing.

An endpoint may own routing state shared across several sessions. Multiplexed
sessions can require per-stream credit, connection credit and reset events.
Retransmission/congestion timers belong in that protocol implementation, not a
generic driver. These distinctions are reasons to reuse the architecture rather
than impose a universal session interface.

## Platform and failure claims

A socket-free session can be portable without making its driver portable.
Keep the existing actor platform limits explicit until native qualification
proves otherwise. Do not broaden supported targets based only on session tests.

Returned errors, contained actor traps and process-fatal runtime failures remain
different outcomes. Session extraction does not strengthen the actor sandbox.
Test resource recovery after actor heap exhaustion and ensure native session
state owns nothing in a discarded or unrelated heap. No hidden global protocol
service should turn a connection-local failure into shared corrupted state.
