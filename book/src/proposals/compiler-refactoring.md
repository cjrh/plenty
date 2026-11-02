# Rust compiler boundaries and measured performance

> **Status: Not implemented.** This is a design proposal, not an approved
> compiler rewrite or a report of new speedups. The
> [frontend reference](../design/toolchain/02-architecture/frontend.md) and
> [cache reference](../design/toolchain/02-architecture/cache.md) describe current
> behavior. Remaining scope and priority belong to B17/B19 in the
> [backlog](../backlog.md).

## Assessment and scope

The compiler is thoughtfully engineered. Its strongest properties are an
independent typed-operation checker, deterministic parallel emission and
diagnostics, explicit ownership checks, cycle-safe nominal metadata, and
profiling-backed allocation decisions. Preserve these rather than replacing
them to obtain a more fashionable Rust architecture.

The main concern is coupling: parsing, generic expansion, local inference,
ownership provenance, editor recording and operation construction meet in a
large frontend with shared mutable tables. Improvements should make those
dependencies explicit and independently testable.

The source assessment was checked against local main `1c8e4113`. Earlier
discussion used `4327501e`; this proposal rechecked the relevant source
boundaries at the newer revision. No new compiler benchmark, correctness
audit, Rust lint run or implementation experiment was performed for this
proposal. Historical numbers below retain their original scope.

The runtime crate, optional dataframe/linalg engines and every standard-library
adapter were not audited. This is not an unsafe-code or security review.
There is no proposed learner-visible language change.

## Design map

| Question | Detailed design |
| --- | --- |
| How should lowering and ambient state be separated? | [Frontend and session boundaries](compiler-refactoring/frontend-and-sessions.md) |
| How should errors reach CLI, workers and editors? | [Structured diagnostics](compiler-refactoring/diagnostics.md) |
| Which hot representations merit experiments? | [Identity, type facts and dataflow](compiler-refactoring/performance.md) |
| What must exist before caching lowered imports? | [Replayable module results](compiler-refactoring/module-results.md) |
| How do we decide whether a change is worthwhile? | [Validation and adoption](compiler-refactoring/validation.md) |

These pages describe design dependencies and alternatives, not a second
schedule. The backlog owns priority, and the shared local issues register owns
defects, claims and PR state. Existing closed investigations remain closed
unless new evidence justifies a separately scoped follow-up.

## Recommended direction

Structured diagnostics are the best bounded starting point: the editor currently
renders errors to text and reparses that text for positions. Fixing the boundary
improves correctness of the representation without requiring an IR redesign.

Frontend decomposition is the most important structural work. Start with a
single cohesive state owner and behavior-preserving changes; do not move every
field at once. Session lifetime improvements can be introduced independently
where they remove ambient policy.

Remaining function-name and nominal-type lookups are performance **candidates**.
The frontend already interns function identities and caches type facts. Extend
what exists only after attribution shows a worthwhile cost.

Replayable module lowering is a larger architectural opportunity, not a quick
cache patch. Imported source dependencies, generic instances and inferred
returned-reference facts must be reproducible before persistence is safe.

## Existing work not to redo

The [performance delivery record](../completed-work/performance.md) documents:

- issue #302: shared token/type children, callable signatures and FxHash tables;
- issue #303: borrowed loan-graph operations and reusable liveness buffers;
- issue #305: large-workload slowdown attributed to cache/TLB misses rather
  than increasing instruction work per unit;
- issue #315: batched codegen and mimalloc addressing allocator contention;
- issue #321: selective CLI teardown skipping; bump allocation was rejected;
- issue #317: syntax-tree and separately compiled module experiments did not
  justify adopting those cache designs.

Avoid blanket clone removal, replacing `Arc` with `Rc`, a global arena
rewrite, more threads without contention measurements, or replacing Cranelift
without matched workload evidence. These are not prerequisites for the proposed
boundaries. Compiler latency, generated-program performance and Rust build time
are distinct outcomes; report them separately.
