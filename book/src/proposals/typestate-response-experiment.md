# Owned response experiment and consuming receivers

**Status: bounded experiment complete, 2026-10-26.** The source is in
`experiments/typestate/`; the compiler baseline is `344dfdbd`. This refreshes the
[typestate assessment](typestate-and-owned-transitions.md). It does not add
language syntax or change the HTTP server. Scheduling remains in [B72](../backlog.md).

## What the experiment establishes

Two public classes, `Unanswered` and `Writer`, hide their resource fields and
constructors. Consuming free functions implement these transitions:

```text
begin(Unanswered, length) -> Result[Writer, StartError]
write(Writer, count)      -> Result[Writer, WriteError]
finish(Writer)           -> Result[(), FinishError]
```

The wire is a deterministic counter with an injected failure position, not a
network transport. Negative lengths, oversized writes and incomplete finishes
return the still-valid owner in an error payload for retry. A partial write is
terminal: its error reports bytes accepted in that call and in total, and the
connection drops. An error must never offer a retry owner after effects that make
replaying the operation unsafe.

A freshly built debug compiler compiled and ran nine resource lifetimes. The
runner checked exactly one destructor invocation per resource, including legal
retries, cancellation, abandonment, an owner carried through `?` in a retry
error, and cleanup when a later function argument propagates an error. The
partial-write case accepted two bytes, then one of the requested three; it
reported `(1, 3)` and destroyed the connection before returning the error.

Thirteen compile-fail probes cover writing/finishing before begin, repeated
begin/finish, writing after finish, reusing the owner after a fallible write,
forging either state, reading private storage, copying a resource-bearing state,
consuming a borrowed writer, and the two current language restrictions discussed
below. These are reproducible assertions, not only a successful smoke check.

The cost is visible in the API: two public states and three error enums,
new writer bindings after successful writes, and explicit matching to recover
retry owners. One `Option` field in `Unanswered` and one `replace(..., Nothing)`
at `begin` avoid direct field extraction; `Writer` holds the resource directly.
That replacement needs no allocation
or dummy connection. Constructor privacy maintains the invariant that a live
public state contains a resource. No throughput, code-size, or allocation-count
benchmark was performed; the cleanup trace is not a performance measurement.

## Why this is not yet an HTTP server conversion

The actual `std.http.server.Handler.handle` receives `&mut Exchange[T]`. The
server retains the exchange for request draining, reports and connection reuse.
A handler cannot consume that borrowed owner. Fields such as request-body
state, cancellation tokens and the connection also belong to the same exchange.
An owned response API therefore needs a deliberate split of responsibilities,
or a different handler contract, not just renamed methods.

The model's recoverable incomplete finish is also deliberately different:
today `Exchange.end` marks a short sized response broken, and a transport error
during `write` leaves the response broken. The model does not prove that either
real failure can be retried. Real integration must preserve partial-progress
reporting, HEAD/chunked framing, cancellation and keep-alive behavior.

Keep ordinary writes borrowed if returning a writer for every write is too
verbose; then terminal transport failure still needs a runtime broken-state
check. Typestate is most promising at coarse begin/finish boundaries. Discarding
an unfinished writer remains legal: deterministic resource cleanup is not a
guarantee that a response completed. That stronger requirement belongs to
[linear obligations](linear-obligations.md).

## Consuming methods are a separate, moderate extension

There is no fundamental ownership obstacle to explicit `self: Class`. The
existing owned-argument path already transfers an entire value, rejects reuse
and conflicting borrows, and transfers its cleanup responsibility. The intended
model would be `value.finish()` behaving like `finish(value)`, with static
dispatch and no new allocation required by the call itself. This is an
implementation assessment, not a measured compiler patch.

It is more than relaxing one signature check. `src/frontend/classes.rs` checks
borrowed receivers and lowers ordinary calls by borrowing the base;
`src/frontend/enums.rs` and `src/frontend/protocols.rs` also constrain receivers.
Generic, named/default-argument and temporary-receiver paths need the same owned
call behavior. Some current paths assume a reference unconditionally.

A bounded implementation should preserve bare `self` as a shared borrow and
`self: &mut Class` as an exclusive borrow, adding explicit owned receivers for
ordinary methods. Lifecycle methods and operator hooks keep their existing
contracts. Owned parameters remain immutable bindings; a body can move `self`
into a mutable local. Protocol matching must distinguish owned and borrowed
requirements. An owned receiver must not silently move through a reference,
field or indexed element, or return a reference to its dying storage.

Tests must cover enums and generic classes/methods, temporary and named
receivers, protocol calls, named/default arguments, live loans, failure in a
later argument, conditional moves, and exactly-once destruction. Evaluation
order deserves explicit documentation: moving the receiver before evaluating
later arguments cannot reuse mutable borrowing's two-phase reservation to read
the moved owner again. Ordinary owned-function-call rules are the starting point.

## Field extraction requires a stronger ownership decision

Moving `self` as a whole does not permit returning `self.resource`. Today a
class's destructor runs with initialized fields, followed by automatic field
cleanup. General partial moves would need field-level availability and cleanup
rules across branches, loops, reassignment and early returns, plus a rule for
custom destructors that might inspect an extracted field. Initialization
tracking alone does not settle those later lifetime rules.

A narrower future candidate is consuming whole-object destructuring for classes
without a custom destructor: the object ceases to exist, each field transfers
or drops, and no partially moved object remains usable. Nested resource fields
would still need their ordinary cleanup. This could be easier to specify than
general partial moves, but neither is necessary for the tested `Option`/`replace`
pattern, and neither is implemented here.

The evidence supports exploring consuming receivers independently. It does not
justify a blanket HTTP/SQLite conversion or a general partial-move feature.
