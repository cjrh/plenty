# Runtime-independent networking protocols

Status: **Proposed; not implemented as a general convention.** The three-layer
direction is agreed: codec, sans-I/O session, runtime driver. This proposal
specifies the intended contracts, not new APIs available today. Existing
[TLS sessions](../design/stdlib/tls.md#sessions) and the
[resumable HTTP decoder](../design/stdlib/http-decoder.md) demonstrate parts of
the design; they do not imply that all current networking modules already use it.
Remaining scope and priorities belong to B94 and B85 in the
[backlog](../backlog.md).

Companion pages describe the [session contract](sans-io-protocols/contract.md),
[runtime drivers](sans-io-protocols/drivers.md), and
[WebSocket validation and acceptance](sans-io-protocols/websocket.md).
All interface sketches on these pages are hypothetical, not executable Plenty
or final public declarations.

## Problem and decision

Plenty supports both sequential networking and supervised actor networking.
A protocol implementation should not need separate copies of its framing,
validation, exchange state, close rules and retransmission logic for each.
Generic socket wrappers help share code across TCP and TLS, but do not remove
the assumption that a protocol operation can wait on its caller's stack.

Adopt three layers for new stateful networking protocols:

1. **Codec:** parse and serialize protocol representations incrementally.
2. **Session:** own protocol state and advance it from explicit inputs without
   performing I/O or waiting.
3. **Driver:** connect a session to a runtime, transport and application API.

Share the protocol semantics, not one universal execution model. Sequential
callers retain convenient operations such as receiving a message; actor callers
receive events and submit commands without blocking the stage. Both drive the
same session implementation.

## Existing foundations and boundaries

| Existing component | What it establishes | What it does not establish |
| --- | --- | --- |
| [`std.tls.Session`](../design/stdlib/tls.md#sessions) | Socket-free TLS state, explicit ciphertext input/output, plaintext operations and EOF | A universal protocol interface or actor-independent configuration ownership |
| [`std.http.decoder`](../design/stdlib/http-decoder.md) | Incremental request framing, consumed-byte counts, explicit EOF and bounded feeds | A complete HTTP client/server session including response sequencing and lifecycle |
| [`std.actor.net`](../design/stdlib/actor-net.md) | Bounded socket buffers, readiness, timers, nonblocking connection setup | Application-protocol grammar or session rules |
| [`std.http.websocket`](../design/stdlib/websocket.md) | Implemented RFC 6455 behavior and a sequential API over generic transports | An actor WebSocket adapter or a public socket-free WebSocket session |

The actor HTTP server uses the resumable decoder. The worker HTTP server still
uses stream-oriented request reading; sharing parsing rules is not proof that
both servers share all protocol orchestration. Existing qualification remains
necessary during any migration.

## Responsibilities

### Codec

A codec recognizes or produces wire representations. It handles such matters
as lengths, field encodings, frame headers, masking and incremental UTF-8
validation where appropriate. It accepts partial input and preserves enough
state to resume; it must not require a complete peer-controlled frame to be
buffered before checking its advertised size.

A codec does not decide when a socket should be read, when an actor should run,
or whether an application may send a message in the current session state.
Keep codecs private unless a real independent consumer needs their API. Three
architectural layers do not require three public modules.

### Session

A session enforces protocol semantics across codec operations: legal message
sequences, negotiated settings, fragmentation, acknowledgements, control
responses, closure and protocol timers. It accepts application commands,
received data, explicit EOF and supplied time. It produces application events,
wire output, timer requirements and terminal status.

No session owns a socket, selector, actor context, thread, cancellation token or
runtime task. It neither waits nor obtains the current time. It can allocate
within documented limits; sans-I/O does not mean allocation-free or immutable.
State transitions happen through bounded, nonblocking calls.

### Driver

A driver owns transport progress, readiness registration, waiting, cancellation,
application delivery and runtime shutdown. It translates network EOF into
session EOF and runs protocol timers when requested. It retains partially sent
output and resumes it without loss or duplication.

Connection establishment, DNS and TLS configuration are driver/composition
concerns. Protocol-specific decisions such as an acknowledgement timeout or
WebSocket close response belong in the session. Application idle policies,
server admission and a caller's operation deadline belong outside it.

## Architectural dependency rules

The permitted direction is driver → session → codec. Codec/session code may
use ordinary data structures, byte operations and bounded computation, but
must not import transport, actor or blocking-wait facilities. Review actual
calls as well as imports: a callback or helper that secretly reads a socket
still violates the boundary.

Protocol layers compose through bytes and explicit state, not inheritance.
For example, a WebSocket session can sit above a TLS session, with a driver
moving plaintext and ciphertext between their bounded buffers. A simpler
blocking driver can instead use the existing TLS transport. This does not
require changing every layer to the same representation.

Do not add a universal `Protocol`, global event enum, runtime registry or
standard protocol executor initially. TCP byte streams and UDP datagrams have
different contracts. Multiplexed protocols may need stream identifiers,
connection-wide flow control and endpoint-level state. Share small helpers only
after independent drivers demonstrate identical mechanics.

## Public API direction

Keep existing convenience modules and behavior while extracting their protocol
logic. A proposed WebSocket session could live under
`std.http.websocket.session`; an actor adapter could live under
`std.actor.http.websocket`. These are candidate paths, not reserved names.
The codec may remain an implementation detail of the session module.

The session API should be useful to custom event loops and tests, not a public
hook exposed solely so another std module can compile. Its module reference
must document progress, ownership, allocation, errors and limits. Check exact
signatures against [stdlib conventions](../design/stdlib/conventions.md) before
implementation; regenerate the signature index only when public declarations
actually change.

An initial API can favor owned events and copying into caller-owned output
buffers. This is easier to specify under current borrowing and actor rules than
retained views or callbacks into a mutably borrowed session. A zero-copy API is
not a prerequisite; copies and memory costs must still be measured.

## Implementation policy

Implement small, straightforward protocol machinery in checked Plenty. Use an
established native library where substantial complexity or security expertise
justifies it, preferably through its sans-I/O boundary. TLS remains backed by
its existing security implementation; this proposal does not call for rewriting
TLS, certificate validation, cryptographic primitives or QUIC in Plenty.

A native wrapper must retain input/output bounds, allocator provenance, error
causes and actor cleanup guarantees. A library that performs hidden I/O is not
made sans-I/O by placing a different name around it. A library whose unbounded
allocation or global state cannot meet Plenty's contracts requires a separate
justification or must not be adopted.

## Alternatives

| Alternative | Useful for | Why it is not the general answer |
| --- | --- | --- |
| Generic `Reader`/`Writer` or transport | Sharing a sequential driver across TCP, TLS and memory | A blocked read or `WouldBlock` does not preserve an actor callback's execution position |
| Shared codecs only | Simple framing and stateless request formats | Stateful validation and automatic responses can still diverge between drivers |
| One dedicated protocol actor behind every API | Actor-native services | Sequential callers acquire an implicit runtime, mailbox and failure model |
| Worker offload of blocking implementations | Existing libraries without incremental interfaces | Waiting still consumes a worker; it is compatibility, not shared nonblocking protocol machinery |
| Coroutines or effects | Potential future sequential driver ergonomics | Requires suspension/ownership/cancellation machinery and does not remove backpressure requirements |
| One universal protocol interface | Potential common driver helpers | Prematurely forces stream, datagram, multiplexing and external-service differences into one shape |

The [blocking-work proposal](actor-blocking-work.md) remains complementary.
No async/await, stackful-task, effect, stored-reference or linearity feature is
required for this design.

## Scope and consequences

Use WebSocket to validate a complete shared session and both drivers. This is
not approval to rewrite HTTP, introduce every common protocol, or change the
actor scheduler. Existing modules migrate only with retained behavior and
protocol-specific evidence. A future DNS, MQTT, SMTP or QUIC design should use
the same separation while choosing its own session shape.

Benefits are a single location for protocol fixes, deterministic state-machine
tests, reusable custom-runtime integration and independent runtime evolution.
Costs are explicit state, more detailed progress contracts, bounded queues and
some runtime-specific driver code. Shared protocol code does not guarantee equal
throughput, equal memory use or identical application APIs.

## Design references

- [Sans-I/O guidance](https://github.com/brettcannon/sans-io/blob/main/how-to-sans-io.rst): separation of network protocol behavior from I/O mechanisms.
- [h11](https://h11.readthedocs.io/en/latest/): HTTP/1.1 as an I/O-independent event/state machine.
- [wsproto](https://python-hyper.org/projects/wsproto/en/stable/): a socket-free WebSocket implementation as a design precedent, not a dependency or proposed port.
- [Quinn protocol connections](https://docs.rs/quinn-proto/latest/quinn_proto/struct.Connection.html): explicit datagram, transmit and timer integration for a more complex protocol family.

These references inform the architecture; this documentation change imports no
external implementation or data.
