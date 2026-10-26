# Owned response experiment and consuming receivers

**Status: experiment complete; consuming methods implemented.** The source is
in `experiments/typestate/`. The original model used consuming free functions
against baseline `344dfdbd`; it now exercises explicit owned receivers. This
refreshes the [typestate assessment](typestate-and-owned-transitions.md).
The HTTP server is unchanged and individual field moves remain out of scope.
Scheduling remains in [B72](../backlog.md).

## What the experiment establishes

Two public classes, `Unanswered` and `Writer`, hide their resource fields and
constructors. Consuming methods implement these transitions:

```text
Unanswered.begin(self: Unanswered, length) -> Result[Writer, StartError]
Writer.write(self: Writer, count)          -> Result[Writer, WriteError]
Writer.finish(self: Writer)                -> Result[(), FinishError]
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

Twelve compile-fail probes cover writing/finishing before begin, repeated
begin/finish, writing after finish, reusing the owner after a fallible write,
forging either state, reading private storage, copying a resource-bearing state,
consuming a borrowed writer, and the continuing restriction on field extraction.
Compiler integration tests separately exercise owned receiver lowering, generics,
protocols, generators and cleanup. These are reproducible assertions, not only
a successful smoke check.

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

## Consuming methods use existing ownership

Explicit `self: Class` reuses the owned-argument path: it transfers an entire
value, rejects reuse and conflicting borrows, and transfers cleanup
responsibility. `value.finish()` behaves like `finish(value)`, with static
dispatch and no new allocation required by the call itself. A compiler test
compares optimized assembly for equivalent method and free-function calls.

`src/frontend/classes.rs`, `src/frontend/enums.rs` and
`src/frontend/protocols.rs` admit owned receivers. Ordinary and generic method
calls forward them as values; named/default-argument calls use the existing
wrappers. Temporary borrowed views cannot be turned into owners by dispatch.

Bare `self` remains a shared borrow and `self: &mut Class` an exclusive borrow.
Lifecycle methods and operator hooks keep their existing contracts. Owned
parameters remain immutable bindings; a body can move `self` into a mutable
local. Protocol matching distinguishes owned and borrowed requirements. An owned
receiver cannot silently move through an affine reference, class field or indexed
element, or return a reference to its dying storage. Copyable enums keep ordinary
copy semantics rather than becoming affine solely because a method takes a value.

Tests cover enums and generic classes/methods, temporary and named receivers,
protocol calls, named/default arguments, live loans, failure in a later argument,
conditional moves and exactly-once destruction. Receiver evaluation precedes
later arguments; mutable borrowing's two-phase reservation does not allow reading
the moved owner again. These are ordinary owned-function-call rules.

The first stdlib adoption is `std.text.Builder.finish()`. It consumes the builder
on success or allocation failure; `to_text()` remains a borrowed snapshot for
reuse and retry. `std.text.join` and `wrap` now finish their builders explicitly.
Both finalization paths still copy output bytes; no zero-copy improvement is
claimed. SQLite and HTTP lifecycle contracts are not converted.

## Individual field moves remain deferred

Moving `self` as a whole does not permit returning `self.resource`. Today a
class's destructor runs with initialized fields, followed by automatic field
cleanup. General partial moves would need field-level availability and cleanup
rules across branches, loops, reassignment and early returns, plus a rule for
custom destructors that might inspect an extracted field. Initialization
tracking alone does not settle those later lifetime rules.

The tested `Option`/`replace` pattern remains sufficient for this experiment.
Neither partial field moves nor whole-object destructuring is included in the
consuming-method change.
