# Typestate and owned transitions

Status: design exploration of existing facilities and limits, not a new
language feature. Scheduling belongs in [B72](../backlog.md).

The separate [linear-obligations proposal](linear-obligations.md) (issue #149)
addresses non-abandonment of an owner, including retry errors. Typestate can
remain affine when abandoning a state is harmless. See the
[integration assessment](linear-integration.md) for a protocol example combining
state transitions with obligations, worker effects, and execution responsibility.

## What Plenty can express now

Typestate makes an operation available only for an object in a particular
state type. Distinct nominal classes plus consuming free functions express a
basic form today: `prepare(draft: Draft) -> Ready` moves the draft, and
`send(ready: Ready)` cannot accept an unprepared draft. The ownership checker
prevents reuse of the consumed binding. Private fields/constructors and module
visibility must prevent callers fabricating the supposedly validated state.

A small `Draft` to `Ready` probe type-checked with the available local debug
compiler during this assessment. This was a smoke check of that binary, not a
fresh build or an exhaustive typestate conformance test. The design basis is
the current [ownership](../design/language/05-ownership/index.md),
[classes](../design/language/03-types/classes.md), and
[function](../design/language/06-functions/01-signatures-and-calls.md) contracts.

Ordinary methods currently borrow their receiver; do not assume Rust-style
consuming `self` methods or specialized method sets on `Connection[Open]` versus
`Connection[Closed]`. Generic classes exist, but a marker parameter alone does
not prove state-specific method availability. Distinct classes and consuming
free functions are the conservative starting point.

Moving an entire owner is supported; extracting affine class fields is not.
Transitions that rebuild a wrapper around an owned connection may need an enum
whose payload can be consumed, an `Option` plus `replace`, or API restructuring.
That friction is relevant to whether a typestate design is pleasant, not a
reason to weaken initialization guarantees indiscriminately.

The HTTP client now demonstrates the API-restructuring option (issue #134,
commit `2772825`). `run` returns the response head and an `Incoming` body owner
as tuple components. `fetch` can consume the body and move the head into its
result without constructing a fallible placeholder; `send` assembles the public
`Response` only when needed. `attempt` likewise carries its head separately.
This removes the observed placeholder work using existing tuple moves, without
adding partial class moves or compile-time response states.

## Actual stdlib use

| Area at baseline `bef41b1` | Existing approach | Assessment |
| --- | --- | --- |
| `http.client.Response`, `Client.finish` | Response owns the connection; `finish` consumes it | Ownership expresses a lifecycle boundary, but body completion is checked at runtime |
| `http.server.Exchange.start/write/end` | One class with integer state, remaining length and flags | Not compile-time typestate; invalid order returns an error |
| `http.BodyReader` | Runtime state machine over input-dependent chunk framing | Runtime states are natural; enums can improve internal invariants |
| `json.Decoder` | Runtime token/Unicode/container states | Arbitrary input cannot reasonably become a compile-time sequence of state types |
| `json.Builder.Frame` | Payload enum for array/object accumulation | Good state representation, distinct from public typestate |
| `db.sqlite.Transaction.commit/rollback` | Mutable methods; a busy commit leaves the transaction retryable | Resource ownership with dynamic lifecycle, not consuming state transitions |

The stdlib therefore uses ownership more broadly than typestate. An enum
eliminating an invalid combination of internal fields and a public API rejecting
an invalid operation sequence at compile time are complementary techniques.

## Useful experiments and tradeoffs

For server responses, compare an API where beginning a response consumes an
unanswered handle and returns a writer, and ending consumes that writer. This
can rule out writing before start or ending twice. Runtime checks still enforce
declared content length, I/O failures, and cancellation. Separate size/chunked
writers only if they simplify callers rather than multiplying types and generic
parameters. Existing handlers borrow `Exchange`, so a consuming design also
requires deciding how request-body access and handler ownership interact.

For builders, use distinct stages only for meaningful required choices.
Optional configuration normally suits a single options class with validation.
An explicit constructor establishing JSON decoder invariants is preferable to
a 28-stage builder mirroring fields.

Failure ownership is part of every transition's contract. A failed consuming
transition can destroy the resource, or return the still-valid owner alongside
the error for retry. SQLite's busy-commit behavior demonstrates why blindly
changing a mutable method to a consuming function can lose useful behavior.
Cancellation and partial external effects likewise need explicit outcomes.

Avoid placing every connection phase in a distinct public type if callers must
immediately put those types back into an enum for queues, pools, or runtime
routing. Compare end-to-end handler code, not just the impressive compile-time
rejection of one small example.

## Validation before adoption

For each experiment, demonstrate one useful compile-time rejection, legal
retry after failure, preservation of partial-progress reporting, and exactly-once
cleanup. Check that public construction cannot forge states. Measure new public
types, caller branching, placeholder replacements, and code size. Keep runtime
enums for input-driven parsers; prototype typestate at coarse ownership
boundaries. No blanket stdlib conversion is proposed.
