# Native concurrency contract

This proposal records the broader concurrency contract and its rationale.
[Scoped native threads](../design/32-scoped-native-threads.md) now implement the
initial checked worker boundary. The reference describes its precise limits;
future work and priorities live only in the [backlog](../backlog.md).

## Runtime audit

| Component | Current evidence | Consequence |
| --- | --- | --- |
| Type metadata | `src/codegen/metadata.rs` emits immutable `Type`/`Variant` graphs; aggregates retain static pointers | No `Rc<Type>` conversion is needed; loaded code and descriptors must stay resident |
| Shared storage | `plenty-runtime/src/memory.rs` now uses atomic counts, including immutable text | Count handoff is tested concurrently; mutable payload access still needs compiler eligibility checks |
| Destruction | The final release reuses the dead count as a thread-local queue link | Only the final releaser may enqueue; synchronize before repurposing the count |
| Inline frames | Generator cleanup uses the same final-release primitive; closures visit captured slots | Inline frames finish synchronously rather than entering the heap destruction queue |
| Allocators | Allocations retain an allocator identity; the built-in one uses the system allocator | Custom allocators need explicit transfer/deallocation permissions and lifetime guarantees |
| Files and foreign resources | File methods mutate state; foreign interfaces do not certify thread affinity | Exclude from initial cross-thread eligibility unless a contract proves transfer and drop are permitted |
| Runtime loading | Library leases release lookup ownership while code stays resident | Residency prevents dangling code, but grants no concurrent-call or affinity permission |
| Instrumentation | Allocation failure budgets are thread-local; live byte counters are atomic | Concurrent tests must keep failure injection local and check leak baselines after joining |

Use one atomic header contract initially, retaining the 16-byte ABI layout and
immortal-literal sentinel. Retain requires an existing live owner; it does not
publish payload writes. Release decrements with release ordering, and the final
releaser acquires before destruction. Reusing the count as a queue link is safe
only after that handoff, when no other owner remains. Keep queue operations
thread-local and preserve synchronous nested drops from user destructors. The
ordering follows the ownership-count argument in the Rustonomicon's
[clone](https://doc.rust-lang.org/nomicon/arc-mutex/arc-clone.html) and
[drop](https://doc.rust-lang.org/nomicon/arc-mutex/arc-drop.html) chapters.
Atomic counts do not make mutable payload access concurrent-safe.

## Compiler facts, not user-defined marker methods

Compute transfer and immutable-sharing eligibility structurally. Scalars and
immutable text are eligible once runtime sharing is safe. A moved collection
requires eligible contents and allocator; a shared collection additionally
requires that all reachable observations are safe under shared access. An
exclusive borrow transfers only within a scope that joins before that borrow
ends. Captures and suspended generator locals participate in the same check.

Do not infer a destructor's thread safety merely from its fields. A destructor
or named callback can call a foreign function with affinity requirements even
when it captures nothing. Eligibility needs a call-effect summary, an explicit
trusted foreign contract, or a conservative rejection. Recursive data analysis
must compute a bounded fixed point over nominal declarations, not recurse
indefinitely. Imports never change eligibility implicitly.

## Ownership at task boundaries

A scoped task can borrow its parent's data. Its scope joins every started task
before locals are reclaimed, including on `return`, `?`, and loop exits. Start
failure must preserve the unstarted job. The initial API borrows owners and
reusable closures, leaving the job with the caller on failure; a consuming
submission API would need to return unstarted ownership with its error.
Join transfers one result
to one consumer. A task returning `Result[T, E]` keeps that application error
distinct from failure to create a thread or allocate task storage.

Bounded channels own their queue storage and transfer values. A full queue may
block under an explicit operation; a nonblocking send reports full/closed with
the unsent value intact. Closing wakes waiters. Queued values drop exactly once
when no receiver remains. Queue allocation is fallible; send into reserved
capacity must not add an unreported allocation.

## Familiar executor behavior

Use Python's explicit executor construction, `submit`, result retrieval, ordered
`map`, and context-managed shutdown as the interface model. Python also offers a
buffer limit for mapped work and warns about workers waiting on work in the same
exhausted pool. [Python executor documentation](https://docs.python.org/3.14/library/concurrent.futures.html).

Plenty should require bounded submission storage and report submission failure
with ownership of the unqueued job. Exiting the executor joins running jobs and
reclaims unconsumed results; shutdown may cancel pending jobs but cannot kill
running ones. Worker code must not assume that waiting on another job in its
own pool makes progress. Thread count and queue capacity are explicit settings.
Result handles here are blocking task handles, not language async futures.

Parallel map returns results in input order; a deterministic fallible variant
reports the lowest failed input index after earlier work finishes. Cancellation
is cooperative. Dropping partial outputs and joining tasks cannot undo external
effects. Parallel reduction needs an explicit reassociation policy, especially
for floating point. Ordinary loops and comprehensions retain serial semantics.

Executor scheduling, adapters, and traversal belong in libraries where possible.
The compiler provides checked ownership/effect facts; the runtime provides
fallible thread creation, synchronization, and deterministic cleanup primitives.
