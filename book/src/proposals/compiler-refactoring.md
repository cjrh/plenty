# Rust compiler boundaries and measured performance

> **Status: Steps 1–6 implemented** (issues #691–#696). Replayable module
> lowering remains a design. No speedup is claimed. The
> [frontend reference](../design/toolchain/02-architecture/frontend.md) and
> [cache reference](../design/toolchain/02-architecture/cache.md) describe current
> behavior. Remaining scope and priority belong to B17/B19 in the
> [backlog](../backlog.md).

## Assessment and scope

The compiler's strongest properties are an independent typed-operation checker,
deterministic parallel emission and diagnostics, explicit ownership checks,
cycle-safe nominal metadata, and profiling-backed allocation decisions. Preserve
them; the aim is code that is easier to change, not a different architecture.

The main concern is coupling: parsing, generic expansion, local inference,
ownership provenance, editor recording and operation construction meet in a
large frontend with shared mutable tables, and diagnostics travel between
phases as rendered text. Improvements should make those dependencies explicit
and independently testable.

The source map was rechecked against local main `6477751e` (0.8.7). Step 6's
profile is recorded in [measured attribution](compiler-refactoring/performance.md#measured-attribution-issue-696);
no correctness audit was performed, and historical numbers below retain their
original scope. The runtime crate,
optional dataframe/linalg engines and standard-library adapters were not
audited. This is not an unsafe-code or security review. There is no proposed
learner-visible language change.

## Design map

| Question | Detailed design |
| --- | --- |
| How should lowering, syntax and ambient state be separated? | [Frontend and session boundaries](compiler-refactoring/frontend-and-sessions.md) |
| How should errors reach CLI, workers and editors? | [Structured diagnostics](compiler-refactoring/diagnostics.md) |
| Which hot representations merit experiments? | [Identity, type facts and dataflow](compiler-refactoring/performance.md) |
| What must exist before caching lowered imports? | [Replayable module results](compiler-refactoring/module-results.md) |
| How do we decide whether a change is worthwhile? | [Validation and adoption](compiler-refactoring/validation.md) |

These pages describe design dependencies and alternatives, not a second
schedule. The backlog owns priority, and the shared local issues register owns
defects, claims and PR state. Existing closed investigations remain closed
unless new evidence justifies a separately scoped follow-up.

## Implementation order

Each step is one local PR that preserves behavior unless it says otherwise.

1. **Structured diagnostics.** The editor renders errors to text and reparses
   that text for positions; the parallel checker flattens errors to strings.
   Replace both with one sendable diagnostic value. This fixes a
   representation, not an IR.
2. **Scoped ambient state.** Install the editor recorder through a guard that
   restores the previous state on return and unwind, and make teardown skipping
   a per-call option instead of a process-global flag.
3. **Split `src/op.rs` by responsibility.** It holds the IR definition, the
   historical stack-syntax compiler and the independent checker. Separate
   files give the checker an explicit boundary and keep the legacy compiler out
   of the IR module.
4. **Separate the syntax layer from lowering.** Tokens, the lexer, the syntax
   tree and the core parser make up about a third of `src/frontend.rs` and do
   not use lowering state. Moving them into child modules makes that
   independence checked by visibility rather than convention.
5. **Group lowering state.** Start with passive pipeline state on `Lower`,
   then borrow/provenance state if the first extraction shows a clear gain.
   The pipeline grouping did not show one, so borrow/provenance state stays
   on `Lower`.
6. **Attribution before optimization.** Function-name and nominal-type lookups
   are candidates only. Profile them and record the result; change the
   representation only on measured evidence. The
   [measured attribution](compiler-refactoring/performance.md#measured-attribution-issue-696)
   found neither material, so no representation changed.

Steps 1 and 2 both edit `src/frontend/ide.rs`; steps 1, 4 and 5 edit
`src/frontend.rs`. Land them in order rather than concurrently.

Replayable module lowering is a larger architectural opportunity, not a quick
cache patch. Imported source dependencies, generic instances and inferred
returned-reference facts must be reproducible before persistence is safe. It
stays a design until steps 1–5 land and a std-heavy application shows the need.

## Related plans

Several unimplemented plans meet these boundaries:

- B17 editor work (find references, document symbols, signature help and
  incremental module re-checking) consumes structured diagnostics and needs a
  recorder whose lifetime is explicit.
- B19 module caching needs the replay contract in
  [replayable module results](compiler-refactoring/module-results.md).
- B20's [GCC](codegen-gcc.md) and [Rust-source](rust-source-backend.md)
  backends consume checked operations; a separate checker module and, later,
  function IDs in the IR affect what a second backend must reimplement.
- [Language editions](language-editions.md) would need per-module options to
  flow through lowering, which explicit lowering state makes easier.

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
