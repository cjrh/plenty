# Suspension, allocation and cleanup boundaries

Companion to the **unadopted** [async/await exploration](../async-await.md).
These are candidate constraints, not implemented Plenty contracts. The
[spike](evidence.md) exercises only the small subset explicitly identified there.

## Movable state without pinning

An inline frame can move between polls only if moving it cannot invalidate a
surviving reference. Scalars, owned heap handles and owned child frames fit that
rule when children obey it recursively. A short borrow during one poll is fine
if it ends before returning `Pending`. A pointer into an independently owned
heap buffer can stay valid when its handle moves, but owner/lifetime and
concurrent-access rules still need proof; a pointer into the frame cannot.

Start from the restrictions already used for
[generators](../../design/language/09-iteration/03-generators.md): no borrowed
parameters/captures, and no live loan across suspension. Extend the check through
awaited children and hidden operation state, not just syntactically obvious
local variables. A child awaiting a borrow into its parent's frame makes the
combined frame address-sensitive even if the parent source declares no named
reference. Compiler-created temporaries must be checked too.

```text
async def invalid() -> Result[(), IoError]:
    mut bytes = ...
    count = await read_into(&mut bytes)   # rejected under the movable design
    Ok(())
```

The straightforward alternative transfers owned state:

```text
async def receive(stream: Stream, buffer: Bytes) -> ReadOutcome:
    await read_owned(stream, buffer)

ReadOutcome = ReadOk(Stream, Bytes, Count)
            | ReadErr(Stream, Bytes, Cause)
```

These names are illustrative. An error may need to distinguish a reusable
stream from one whose framing is uncertain. Losing the stream on cancellation
may be safe locally but undesirable; an ownership-return cancellation API can
preserve it. Returning buffer/stream owners repeatedly is the chief ergonomic
cost, especially with nested protocol layers and multiple buffers. Ordinary
borrowed readers cannot automatically become async readers. The spike does not
validate this API or Plenty's generic ownership checker.

Reject frame-address escapes through FFI, callbacks, readiness registrations,
self-references and user-defined awaitables until they have checked contracts.
Use stable task identities or independently owned registration records for wake
notifications; never retain an inline frame pointer in the reactor. Deregister
before freeing the associated storage, including stale wakeups and concurrent
completion races. If an OS operation retains a buffer until completion, dropping
the frame cannot free that buffer merely because cancellation was requested.

A stable-storage alternative can hide address stability behind a consuming
owner API. That removes `Pin` from user spelling, but still requires non-moving
storage and lifetime/drop-order proofs inside the compiler/runtime. It also
changes fallible construction and task admission. Neither the spike nor existing
generator implementation proves that alternative sound.

## An allocation ledger, including failures

| Boundary | Candidate contract | Evidence / remaining work |
| --- | --- | --- |
| Inline coroutine creation | No new heap allocation; argument failures remain visible | Scalar captures demonstrated; owned captures and general layouts unimplemented |
| Nested await | Child frame stored inline; poll/resume bookkeeping does not allocate | Demonstrated, including nested state moved after suspension |
| Body allocation | Ordinary `Result` with `AllocError`, propagated explicitly | Spike uses a numeric status and tests injected failures; Plenty `Result` lowering unimplemented |
| Root admission | Reserve slot/result/wake storage before transferring owner; return owner on failure | One fallible root allocation and fixed two-slot queue demonstrated |
| Queue growth / saturation | Explicit fallible reservation or fixed capacity with a capacity error | Fixed capacity demonstrated; no dynamic growth |
| Wake/registration/timer storage | Reserve fallibly before registration; one owned cleanup path | Not implemented; cannot assume that waking or cloning a waker is free |
| Erased coroutine or result | Explicit fallible boxing/storage API | Not implemented; no implicit boxing fallback |
| Cancellation/drop | No allocator-dependent success path for mandatory synchronous release | Memory-only resources demonstrated; native handles and user cleanup unaudited |

The guarantee concerns coroutine machinery; caller expressions and user cleanup
retain their own effects. Compile-time layout computation and compiler memory
are outside the generated program's allocation contract. Inline frames can be
large even with no heap allocation. Set layout/nesting limits and diagnose
excess rather than silently box. A runtime stack limit is not a recoverable heap
allocation error. Recursive async calls need an explicit heap indirection or an
initial rejection, as recursive generator layouts already do.

## Cancellation is not asynchronous destruction

For the narrow candidate, dropping a suspended coroutine synchronously destroys
its live child and owned locals in reverse acquisition order. It does not resume
source statements, invoke arbitrary `await`, allocate an emergency cleanup task,
or promise that a remote peer rolled back work. The spike uses exactly this
rule. Scope cancellation can stop and destroy a suspended task on the executor
thread; cancellation cannot interrupt a currently executing poll or a blocking
foreign call. Long CPU work needs explicit cooperative checkpoints.

Only operations with a safe synchronous fallback can use that rule. Closing a
local socket may be an acceptable fallback to skipping a graceful protocol
shutdown; leaving a kernel operation referencing freed memory is never one.
The relevant property is not just a destructor returning `()`: it includes
in-flight registrations, external buffer ownership and the cost of unregistering.
A potentially blocking destructor also defeats bounded cancellation latency.
Do not generalize from the spike's `free()` to all existing Plenty values.

An explicit `await finish(owner)` can attempt graceful shutdown and return its
errors. The program still needs an admissible fallback if it is cancelled during
that await. Initially exclude awaiting inside `with`, async context managers,
async destructors and operations with mandatory suspending cleanup. This is a
restriction, not a promise that all cleanup becomes simple.

For a protocol requiring completion, a scope/runtime must retain and drive the
owner to a permitted terminal state, or return it to someone responsible. If
that can suspend, synchronous scope exit cannot provide a nonblocking completion
guarantee. Choices are explicit awaited closure, blocking exit with possible
deadlock, or transfer to an explicitly owned supervisor. Each needs failure and
shutdown rules; none belongs invisibly in `drop`.

This agrees with [linear integration](../linear-integration.md): affine
abandonment cannot carry a future linear obligation. `async with` alone supplies
a location for cleanup code, not an owner guaranteed to execute it. Initial async
frames should exclude non-discardable values if the separate linear proposal
lands. Cancellation shields can delay a request, but cannot prove termination,
repair allocation failure, or outlive a borrowed resource.

## Scope and error questions still open

A production scope needs rules for the first child error, siblings still
running, cancellation result ownership, unclaimed results and multiple errors.
Fail-fast cancellation can be useful, but aggregating errors must not introduce
infallible allocations. A bounded result array, first error plus explicitly
discardable later results, or a fallible collection step are distinct policies.

The experiment's task cancellation returns a separate `CANCELLED` status and
ordinary child errors keep their numeric code. In Plenty, `await coroutine`
should return the coroutine's declared output; awaiting a *task handle* may need
an additional scheduler cancellation result. Do not silently inject cancellation
into every function's error set or pretend graceful cancellation is just a
transport error. The spelling and exact ownership of those outcomes remain open.
