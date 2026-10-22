# Exploring async/await without exposed pinning

Status: **Not implemented; exploratory proposal with a standalone native
spike.** Async/await is not accepted or scheduled for Plenty. The user requested
this investigation while the language can still change freely, despite its
original deliberate exclusion of async/await. Keeping it out remains a valid
outcome. The [backlog](../backlog.md) records that boundary;
[concurrency](../design/facilities/03-concurrency/index.md) and
[generators](../design/language/09-iteration/03-generators.md) describe what works
today. Nothing here changes the tutorial or production compiler.

## The question and provisional answer

Could Python-shaped async syntax coexist with Plenty's explicit, fallible
allocation and deterministic ownership, without exposing `Pin` or requiring
asynchronous destructors? A restricted design looks possible: owned, concrete
movable frames; no surviving borrow across suspension; explicit task admission;
and synchronous cancellation only for operations whose pending work can be
safely abandoned. The [native spike](async-await/evidence.md) demonstrates those
mechanics for a tiny language. It does **not** demonstrate that the restrictions
are ergonomic enough for HTTP/TLS or that a production scheduler is affordable.

This could improve simplicity for Plenty's narrower contract, not improve on
Python or Rust in general. In particular, avoiding arbitrary borrows across
await gives up a major capability of Rust's design. Refusing implicit async
cleanup gives up automatic completion of some protocol obligations. Moving
those costs into adapters or a runtime does not make them disappear.

## What to borrow from Python and Rust

Python separates a coroutine from a scheduled task: calling an async function
creates a coroutine, while awaiting or scheduling drives it. `async def` and
prefix `await` fit Plenty's surface syntax. Python also specifies separate
asynchronous context-manager operations; these are more than alternate spelling
for ordinary deterministic drop. [PEP 492](https://peps.python.org/pep-0492/)
describes that syntax and protocol separation.

Python task groups provide an attractive execution-ownership model: their exit
waits for child tasks, and failures initiate sibling cancellation. Cancellation
is delivered through `CancelledError`, can be suppressed, and needs careful
handling in cleanup. Task creation can also opt into eager execution, so Python
is not a mandate for one immutable scheduling rule. Plenty should specify lazy
construction and task admission itself. [Python task documentation](https://docs.python.org/3/library/asyncio-task.html)
is the comparison source, consulted 2026-10-22.

Rust's await machinery polls a child and suspends its parent on `Pending`;
its completion output can itself be a `Result`. That separation of suspension
from failure suits Plenty. [Rust's await reference](https://doc.rust-lang.org/reference/expressions/await-expr.html)
and [Future contract](https://doc.rust-lang.org/std/future/trait.Future.html)
also make readiness notification and the poll interface explicit.

Rust permits address-sensitive generated state, including a reference to
another field in the same future. Pinning protects the resulting address
invariant; it is not arbitrary syntax that can simply be deleted.
[The pinning documentation](https://doc.rust-lang.org/std/pin/) explains that
motivation. Rust's `AsyncDrop` remains an experimental nightly API in the
[documentation consulted](https://doc.rust-lang.org/nightly/std/future/trait.AsyncDrop.html).
This proposal avoids relying on it and makes no claim that Rust's cleanup
design is settled or inherently impossible to simplify.

## Candidate surface and evaluation order

The following is **proposed Plenty syntax**, not accepted source:

```text
async def fetch(request: Request) -> Result[Response, TransportError | AllocError]:
    response = (await exchange(request))?
    Ok(response)
```

`fetch(request)` evaluates arguments immediately and moves/copies them into a
concrete `Coroutine[Result[Response, ...]]`. It does not execute the body or
schedule anything. Frame construction is inline and adds no allocation; argument
expressions retain their own failures. `await` consumes a coroutine and produces
its declared result. It may finish immediately or suspend; it does not imply
another task, a thread, a failure, or a fairness yield. Parentheses deliberately
leave await/`?` precedence undecided. `?` propagates only the result error and
destroys active owned locals through the ordinary cleanup path.

Like today's `Generator[T]`, `Coroutine[T]` would retain a concrete producer
identity rather than secretly box different producers into one type. Returning
coroutines from factories, generic specialization, frame-size limits, recursive
async calls and containers need explicit contracts. Initially reject recursive
inline layouts and erased/dynamic coroutine storage. A coroutine value is affine:
it cannot be copied or awaited twice. Dropping an unstarted one destroys captures
without executing the body; this is a deliberate difference from Python's
unawaited-coroutine warning behavior.

`async` would be legal only on functions in the initial scope. `await` would be
legal only inside them, plus a library root runner that consumes a coroutine.
No implicit async `main`, async iteration, async closures, arbitrary awaitable
protocol, async generators or `async with` follows from adopting this core.
These restrictions keep a possible first implementation independently assessable.

## Scheduling is a separate choice

Direct await is sequential composition. To overlap work, an explicit bounded
scope admits multiple roots. A candidate API is
`scope.start(coroutine) -> Result[Task[T], StartError[Coroutine[T]]]`: capacity or
allocation failure returns the entire unstarted owner. The scope owns execution;
dropping a public handle must not orphan tasks. Results need their own owned,
allocation-audited storage and a policy when unclaimed. Prefer one result
consumer over implicit reference-counted shared results.

The minimal affine scope can synchronously cancel its remaining *abort-safe*
children at exit. A stronger scope promising asynchronous graceful completion
needs explicit awaited closure or retained responsibility elsewhere. It cannot
silently borrow an enclosing stack and let that stack return. The distinction
is developed in [ownership and cleanup](async-await/ownership.md).

Existing [`Future[T]`](../design/facilities/03-concurrency/futures.md) represents
an executor job's result and has different drop/cancellation rules. Do not
reinterpret it as a coroutine or make `.result()` secretly await. Blocking
code called from async remains blocking. A future adapter must submit work
fallibly, wake the async consumer safely, preserve result/error ownership, and
define cancellation when the worker continues. No such adapter exists here.

## Alternatives and the decision still needed

| Design | Benefit | Cost or unresolved boundary |
| --- | --- | --- |
| Keep current threads, bounded executors and readiness APIs | No second calling convention or async ecosystem; existing fallible admission | Many mostly idle connections may need manual state machines or many native stacks; measure a real workload |
| Owned movable async frames (spike) | Inline composition, no pin API or frame allocation per call | No stored loan across suspension; ownership-return I/O can be cumbersome |
| Runtime-owned stable frames | Potentially allow broader borrows without user-facing pin types | Fallible per-frame/arena reservation, lifetime proof, no relocation, fragmentation; address stability is internalized, not removed |
| Stackful tasks / goroutine-style syntax | Synchronous-looking call chains can suspend deeply | Fixed stacks consume reserved memory and can exhaust; growing/moving stacks must preserve pointers, FFI and allocation failures; blocking calls and task admission still need policies |
| General Rust-like borrowed futures | Broad zero-copy composition and explicit low-level control | Address-sensitive frame projection and poll contracts recreate the complexity we want to avoid |

Stackful tasks deserve independent investigation, including bounded fallible
stack creation, guard pages and overflow behavior, cross-thread movement,
foreign calls and cancellation. They are an alternative to syntax-directed
stackless transformation, not a consequence of it. Current OS workers already
provide one stackful baseline. Neither syntax alone solves protocol cleanup.

Before considering adoption, compare at least one bounded connection workload
and one file/worker workflow using existing APIs and each plausible prototype.
Measure allocations, reserved/live memory, throughput, latency, cancellation
latency, compile time and API friction. The spike establishes neither a
performance win nor a requirement for async. The main open decision is whether
owned-operation ergonomics justify a second execution model; a full borrowing
redesign should not be smuggled in as a small syntax convenience.
