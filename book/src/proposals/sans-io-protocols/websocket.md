# WebSocket validation and acceptance

Companion to [Runtime-independent networking protocols](../sans-io-protocols.md).
Status: **Proposed validation scope, not implemented session or actor support.**
This page defines the evidence needed to validate the architecture. The
[backlog](../../backlog.md), B94 with B85, owns implementation sequencing and
priorities; the cases below are acceptance criteria, not a separate work queue.

## Why WebSocket

WebSocket is a useful first complete session because it combines incremental
framing with bidirectional commands, fragmentation, control responses, text
validation and a closing handshake. It exercises more than a passive decoder
without introducing QUIC's transport/security complexity. Plenty already has
an [implemented sequential WebSocket API](../../design/stdlib/websocket.md),
which supplies a behavioral baseline and a migration obligation.

The initial scope is existing RFC 6455 client/server behavior over TCP and TLS,
plus an actor integration. It excludes `permessage-deflate`, proxy tunnels,
HTTP/2 extended CONNECT, new scheduling models and automatic migration of all
HTTP machinery. No additional wire features are required to validate the
three-layer design.

## Codec responsibilities

The incremental codec handles frame headers, extended lengths, mask position
across input slices and payload encoding/decoding. Validate reserved bits,
opcodes, canonical length encodings and role-specific masking before accepting
an invalid frame. Check frame and message bounds before allocating based on
advertised lengths. Control frames remain at most 125 bytes and cannot be
fragmented.

The codec can emit bounded payload chunks rather than assemble a complete
message. Message limits are enforced across all fragments, including when the
application consumes chunks and no single retained buffer is large. UTF-8
validation carries state across network fragments and continuation frames;
validating each received byte slice independently is incorrect.

## Session responsibilities

The session has explicit client/server roles and state for fragmentation,
message size/text validation, automatic pong generation and closure. It rejects
continuations without an open fragmented message, overlapping data messages,
and data sends that are no longer legal after closure starts.

A conceptual input/output vocabulary is:

```text
Inputs: received bytes, send text/binary, ping, begin close,
        supplied entropy result, input EOF, supplied time, output flush milestone
Outputs: message chunks and boundaries, ping/pong notices, peer close,
         failure/termination status, wire bytes, entropy request, next deadline
```

This is a contract sketch, not a commitment to these exact events. The blocking
adapter can assemble chunks into its existing complete-message API. Actor
applications should be able to stream within bounds instead of requiring a
whole maximum-sized message in their heap. Do not expose a chunked text event
as independently valid `str` when a UTF-8 scalar can span chunks; use bytes and
message-level validation semantics.

Specify when incoming pings/pongs are observable, which replies are automatic,
and what happens when the application is not draining events. A bounded control
reserve must allow required replies without unbounded allocation. If continuing
input would exceed that reserve, stop consuming input or terminate explicitly.
No driver implements its own ping/pong or fragmentation policy.

## Entropy without hidden I/O

Clients need unpredictable handshake nonces and fresh unpredictable frame masks.
This must not become a hidden entropy read in the codec/session or a blocking
call on the actor stage. The driver/external-service boundary supplies securely
obtained nonce/mask material through a narrowly documented operation.

For each accepted outbound client frame, bind a fresh mask to that frame before
its header or payload can be emitted. If entropy is unavailable, leave the
command unaccepted or retain it in an explicitly bounded pending state. Retrying
must not duplicate a frame, reuse a previous frame's mask or consume unrelated
entropy results. Bound pending requests and reject late/duplicate identities.
Test providers may supply fixed material; production providers must use Plenty's
existing secure entropy facility.

Before shipping actor client support, demonstrate an actual nonblocking or
explicitly off-stage entropy path. This proposal does not claim that the current
entropy API already supplies one. If that requires runtime work, record and
review the concrete capability separately; do not weaken the sans-I/O boundary
or silently rely on the unimplemented general blocking-work pool.

## HTTP upgrade boundary

Opening-handshake validation and generation are protocol logic. Socket reading,
HTTP exchange ownership and actor dispatch are driver concerns. Reuse the same
handshake validators/builders from both drivers; they may be helpers separate
from the post-upgrade frame session, provided no protocol rules are duplicated.

Retain current checks for method/version/body, upgrade tokens, key/version,
accept value, subprotocol and unsupported extensions. Preserve existing server
refusal status and client error behavior unless a separate change explicitly
justifies it. Do not accept unsupported extensions merely because their frames
look superficially valid.

Upgrade transfers ownership of the transport **and every already-read byte**
beyond the HTTP head. Feed that suffix exactly once into the WebSocket session.
It may contain several frames or a partial frame, including when the HTTP
response and first frame arrive in one read. HTTP must not continue parsing
bytes after a successful transfer.

For actor HTTP, define a supported ownership/lifecycle handoff before exposing
upgrades. The current actor server lists upgrades as remaining work; constructing
a frame session alone does not implement that integration. Specify how watches,
timers, shutdown ownership and connection accounting transfer, including failure
between sending `101` and completing the handoff.

## Closing and compatibility

Track local close queued/flushed, peer close received, transport EOF and timeout
as distinct facts. Echo/response behavior, close-code validation and reason
validation must follow the existing contract and RFC. A forced abort is not a
successful close handshake. Simultaneous close must not generate endless
responses, and a close timeout must not discard the original failure cause.

Keep `std.http.websocket` and `std.http.wss` source-compatible where practical.
Preserve bounded frame/message behavior, refusal/rejection distinctions,
cancellation behavior, server stop tokens, worker ownership and conditional TLS
linking. Current whole-message buffering or exclusive borrowing may remain in
the convenience driver even if the session supports finer-grained progress.
Do not use extraction as an excuse to change these promises silently.

Reference/tutorial updates ship with the implementation: explain choosing the
blocking or actor driver, show recommended error handling and shutdown, and
include runnable examples for both. The session reference must explain custom
runtime integration, progress and memory bounds. The proposal itself is not a
substitute for those implemented-behavior pages.

## Deterministic session evidence

Run the same wire corpus without sockets, threads, real clocks or production
entropy. Include:

- Every split point for representative headers, extended lengths, masks,
  control payloads, UTF-8 sequences and close frames; one-byte and coalesced feeds.
- Fragmented text/binary messages with interleaved controls, invalid opcodes,
  bad masks, invalid UTF-8, invalid lengths and limits exceeded across fragments.
- Empty feeds, EOF at each incomplete boundary, repeated inspection after
  terminal states and rejected operations after failure/close.
- Zero/small output capacity, partial takes, partial command admission,
  saturated event/output queues and exactly-once retry behavior.
- Stale/duplicate external results, unavailable entropy, stale timer wakes,
  decreasing time, simultaneous close and a peer that never completes close.
- Allocation failure at each fallible growth/admission point, preserving
  ownership and reporting known progress or terminal failure.

Property tests vary byte segmentation, output capacity and legal pump scheduling
while preserving input order. Equivalent schedules must have equivalent message
and terminal semantics; chunk boundaries and batching may differ. Assert that
reported consumption stays in range, queue bounds hold, output order is stable,
and no no-progress state spins. Replayable traces contain inputs, commands,
supplied time and test entropy, not secrets from production sessions.

Fuzz codec/session transitions and external-result ordering. Treat an unexpected
trap, hang, bound violation or consumption inconsistency as a finding. Mutating
only complete frames misses incremental-state defects. Independent RFC vectors
and another implementation are needed as checks: both drivers can agree because
they share the same bug.

## Driver and composition evidence

Use one scripted peer corpus against both drivers and compare normalized
application events, wire validity and terminal outcomes. Cover real TCP/TLS,
partial writes, slow readers, read/write interdependence, EOF/half-close,
cancellation, close deadlines and both upgrade directions. Include HTTP heads
coalesced with WebSocket bytes and TLS records split at unrelated boundaries.

Actor tests must demonstrate bounded work per turn, progress without fresh
network readiness after a budget yield, output/event backpressure recovery,
correct timer rearming, full-mailbox handling, graceful/forced shutdown and
unrelated-connection progress under load. Repeat handler traps and heap
exhaustion; compare descriptor, live-heap and native-allocation baselines after
cleanup. Verify transport ownership is recovered or closed exactly once on every
upgrade-construction failure.

Retain existing HTTP, WebSocket, WSS and actor networking/TLS regressions.
Measure throughput, latency, CPU and peak memory with matched payloads, connection
counts, limits, TLS configuration and CPU allocation. Include idle connections,
large/fragmented messages and control-heavy traffic. Report measured copy and
buffering costs; do not claim an improvement from architecture alone. Actor
platform support requires real platform evidence, not just portable session tests.

## Acceptance and boundaries

The convention is validated when one complete WebSocket implementation supplies
both blocking and actor drivers, with protocol rules only in the shared
codec/session/handshake helpers; all documented progress/resource contracts have
tests; existing convenience behavior is retained or explicitly reviewed; and
reference, signatures and executable tutorials match the implementation.

The first extraction can be reviewed independently of the actor handoff, but it
must not be called complete cross-runtime support until actor upgrades, client
entropy and lifecycle integration are demonstrated. Any unresolved gate remains
visible in the backlog and PR evidence. No production-readiness, zero-copy or
cross-platform claim follows automatically.

## Protocol specification

- [RFC 6455](https://www.rfc-editor.org/rfc/rfc6455): opening handshake, framing,
  masking, control frames, UTF-8 and closing rules. No RFC code or data is imported
  by this proposal.
