# Session progress, ownership and resource contract

Companion to [Runtime-independent networking protocols](../sans-io-protocols.md).
Status: **Proposed contract, not implemented APIs.** The notation below describes
operations, not final Plenty declarations. Priorities remain in the
[backlog](../../backlog.md).

## Inputs and outputs

A stream session needs these conceptual operations. A concrete protocol may
combine them when doing so leaves every progress and ownership rule explicit.

| Operation | Input | Observable result |
| --- | --- | --- |
| Construct | Role, configuration, limits and initial protocol data | Valid session or a named configuration/allocation failure |
| Feed | Read-only byte view | Consumed-byte count, optional event and reason advancement stopped |
| Submit | Application command | Accepted command, partial byte acceptance, or refusal preserving ownership |
| Take output | Caller-owned mutable byte buffer and range | Count of wire bytes transferred out of the session |
| Advance time | Supplied monotonic instant | Bounded timer transitions, possibly producing events/output |
| Finish input | Explicit stream EOF | End-of-input transition or truncation failure |
| Inspect | No external effects | Pending work, next protocol deadline and lifecycle state |

Read-only bytes use `&[u8]`; owned buffers use `Bytes`; caller output uses
`&mut Bytes` and a half-open `start, stop` range. Counts and capacities use
`i64`. Time uses `Instant`, `Duration` and `Deadline`, never integer milliseconds.
No nonwaiting session operation takes a cancellation token merely to resemble
a blocking API.

An event-oriented session may offer a separate event-draining operation rather
than return one event from `feed`. Either arrangement must bound the number and
size of events retained internally. Returning an unbounded list of all events
from one input is not acceptable.

## Progress must be unambiguous

A feed reports a count in `0..source.len()`. The caller advances by precisely
that count and retains the remainder unchanged. A short count does not imply
EOF, and a call can consume bytes without producing an application event.
An empty feed may expose already-pending work but never announces EOF.

Distinguish at least these stop conditions, whether through a result enum or
separate documented queries:

- **Needs input:** no internal work can advance without more network data.
- **Needs output space:** pending output must be transferred before advancement.
- **Needs event delivery:** the application must take a pending event.
- **Needs external result:** a named operation is awaiting a supplied result.
- **Budget exhausted:** work remains runnable; arrange a continuation.
- **Terminal:** no future protocol work is possible.

A pending event can be progress even when the consumed count is zero. Conversely,
`0` consumed with no event and no change in pending work is not permission to
spin. A driver must wait for the identified dependency or yield after exhausting
its budget. If multiple dependencies exist, report enough information to choose
interests without repeatedly probing unchanged state.

Do not automatically reset a completed exchange if bytes for another exchange
remain in the source. A protocol must specify when the application can admit the
next exchange; HTTP pipelining is an example where this affects response order.

## Input ownership and persistent state

`feed` borrows only for that call. Before returning, the session either consumes
bytes into owned, bounded state or leaves them for the caller. It never retains
a pointer into a caller buffer. Header parsing must not keep a view alive while
a driver compacts, grows or reuses that buffer.

Initially favor owned byte events. An event moved out belongs to its recipient;
the session cannot mutate it later. Ownership does not mean unrestricted actor
storage: an owned event containing shared `str` or another actor-ineligible type
may be usable only during a callback. The concrete reference must distinguish
transient events from persistable events and explain any conversion required.

Construct session buffers in the connection actor's heap, not outside it and
then move them into a child. Immutable configuration, foreign handles and secret
material need the same provenance review as
[actor TLS](../../design/stdlib/actor-tls.md#memory). Sans-I/O is not an exemption
from actor heap rules.

## Command admission and retry safety

An application command is not accepted until the session reserves the resources
needed to retain it or has made the documented partial progress. Backpressure is
normal flow control, not a malformed-peer error.

Prefer borrowing command payloads when copying/encoding during the call. For a
partial byte-oriented submission, return the accepted count; the caller retains
the unsent suffix. For an ownership-taking command that can be refused before
use, return the original command with the refusal cause, following std's
[returned-input convention](../../design/stdlib/conventions.md#returning-moved-inputs).
Do not silently discard a moved message because the output queue is full.

Admission must have one specified outcome on allocation failure: either the
command is entirely unaccepted, or an explicit progress value identifies the
accepted prefix. Never return a bare failure after accepting an unknown amount
and invite the caller to retry the entire message. Session mutation that can
fail midway needs transactional preparation or a documented terminal failure.

For complete-message operations, accepting means local ownership/encoding
responsibility, not successful peer delivery. Drivers must not invent delivery
acknowledgements from successful socket writes.

## Wire output and partial writes

Separate output transfer from transport delivery:

1. `take` copies/moves a bounded prefix into a driver-owned buffer and removes
   exactly that prefix from the session's pending output.
2. The driver owns those bytes until the transport accepts them or the
   connection terminates. A short socket write advances only its send cursor.
3. The driver drains that cursor before sending later bytes on the same stream.

A temporary unwritable socket does not permit calling `take` again into the
same storage and overwriting unsent data. A future borrowed-output/commit API
could avoid a copy but needs its own borrow and failure contract; it is not
required here.

A session cannot infer that the peer has received a close frame just because
`take` returned its bytes. Where protocol state needs to distinguish queued
output from output accepted by the transport, the driver must provide an
explicit ordered flush milestone. No public API may claim graceful closure
solely because the session's internal output queue is empty.

## Time and external dependencies

Supply monotonic `now` explicitly for time-sensitive transitions. The session
returns an optional next protocol deadline; a timer notification means
"advance using this instant", not "assume the old deadline is still current".
Recompute the next deadline after every state change. Stale timer wakes must be
harmless, and decreasing instants must produce a named invalid-time error
rather than move protocol time backwards. Tests use supplied fake time.

Caller operation deadlines and cancellation remain in the driver. A keepalive,
retransmission or close-handshake timeout belongs in the session; a server-wide
drain deadline can force termination earlier without pretending the protocol
completed gracefully.

Do not obtain entropy, resolve names, read credentials or invoke arbitrary
application code inside a session transition. Supply data up front or request
an explicit bounded external result. A request carries a checked identity so a
late or duplicate result cannot satisfy a different operation. The requesting
session specifies whether it can do other work while waiting. External failure
preserves its cause and follows a documented retry/terminal policy.

Security-sensitive randomness must still come from the existing secure entropy
facility. Supplying entropy explicitly makes tests reproducible; it does not
justify a predictable seed or random stream in production.

## Resource bounds and fairness

Each session documents separate caps for:

- incomplete headers, frame payloads and assembled messages;
- queued application commands and undelivered events, in bytes and count;
- protocol output, including automatic control traffic;
- codec scratch space and retained metadata;
- active exchanges, streams or external requests where applicable.

The total bound includes codec state, session queues, driver buffers and
transport/TLS buffers. Moving bytes between layers must not double-count them
as free capacity or hide multiple full-sized copies. Document peak memory, not
just the session's most visible buffer. Validate configurations before use;
never silently clamp a bad size or overflow a combined-capacity calculation.

Check advertised lengths before allocating or converting to a machine index.
Queue saturation pauses progress; an oversized peer object produces a named
limit error. Allocation failures are preserved in the module's domain error as
`Allocation(AllocError)`.

A bounded input byte count is insufficient by itself. Bound parser scanning,
event emission, internal transitions and output production per call; bound the
number of calls and bytes copied per actor turn. Work left after a budget
requires a scheduled continuation, not a wait for network readiness that might
never recur. Limits must also cover many tiny control frames and fragmented
messages, not just large payloads.

Reserve bounded space for mandatory control output where the protocol requires
it. Define what happens if that reserve is exhausted: stop input, reject new
application output or terminate according to the protocol. Never let a full
application queue cause unbounded automatic replies or deadlock a required close
response. For stream protocols, preserve wire order: a control response cannot
be inserted into a frame already partly emitted.

## EOF, errors and final states

Distinguish no bytes available, read-side EOF, orderly protocol closure, protocol
failure, transport failure and administrative abort. A transport half-close does
not automatically close the opposite direction; the protocol determines whether
remaining output is useful. Dropping state performs resource cleanup only and
must not wait to send a graceful close.

A session's native error describes protocol/configuration/allocation/external
input failures, not fictitious network failures. The driver preserves actual
transport causes in its own error mapping. Keep existing public error variants
where compatibility requires them; a new pure-session API need not inherit
network variants that it cannot produce.

After a fatal parse error, do not resume from an untrusted frame boundary. A
session may enter a bounded **failing** state to emit a protocol alert/close;
the original error remains available while that output drains. It then becomes
terminal. Terminal means no further output, event or timeout can advance it;
failed-with-output-pending is not terminal. Document which inspection/draining
operations remain permitted after failure and reject other operations with a
named state error.
