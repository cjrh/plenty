# Identity, type facts and dataflow

[Parent proposal](../compiler-refactoring.md). These are experiments to evaluate,
not claims that every inspected lookup is a bottleneck.

## Function identity across phase boundaries

`src/frontend/functions.rs` already has `Symbol(u32)`, a name-to-symbol map
and indexed entries for signatures/declarations. Returned-field facts are keyed
by symbol. Do not propose implementing this table again.

The remaining boundary is visible in `src/op.rs`: `DefineFn(String, ...)`,
`Call(String)` and `TailCall(String)` retain function names. `check_with`
collects `Sigs = HashMap<&str, &FnSig>`. This table borrows names and
signatures; it does **not** copy every signature. `src/codegen/symbols.rs`
also builds a name map when snapshotting declared Cranelift functions, while
its function declarations are already indexed by Cranelift `FuncId`.

### Options to compare

1. Keep the current maps as control. Measure signature collection, call lookup
   and whole-program checking, rather than counting map sites.
2. Add a checker-local indexed view, retaining name-bearing operations. This
   isolates table construction/layout experiments, but calls still require name
   resolution unless translated once.
3. Carry a compiler-owned function ID through modern IR and use indexed
   signatures/call targets. Retain names for display, exports and external
   linkage. Adapt legacy stack-syntax lowering explicitly.
4. Compare interning all identifiers only if function IDs leave substantial
   measured name cost. This is a larger parser/AST/environment change, not a
   natural consequence of option 3.

Prefer a bounded checker experiment before changing the entire IR. For ID-based
IR, use a neutral identity module rather than exposing a frontend-private
`Symbol` as the backend contract.

IDs must identify the owning program/table. Never serialize a transient index
as a stable cache identity without deterministic remapping. Preserve generic
instance identity, function ordering, dump names, exported symbols and
cache-on/off object identity. Avoid a refactor that saves lookups but adds a
second full copy of every body.

## Nominal type access and fact caching

`src/nominal.rs::Types` owns mutex-protected class/enum slot vectors and a
name-keyed facts map. `Nominal` distinguishes local/stored definitions and
strong/weak table links. `get` returns an `Arc`; `try_get` upgrades a
slot and clones the finalized definition handle. Escaping stored fields promote
weak edges so recursive declarations do not create ownership cycles.

`src/type_facts.rs::Ty::facts` already handles simple types directly,
combines collection/box content facts, and caches complete class/enum facts.
`Nominal::define` clears table facts as definitions become available.
Incomplete recursive graphs must not acquire permanent final facts.

Explore these progressively:

- Attribute atomic clone/upgrade traffic, facts-map hashing and locking with
  sampling/counters. A lock in source is not evidence of contention.
- Compare indexed per-definition fact storage against the shared name map.
  Specify how finalizing any dependency invalidates prior results.
- Compare a build-then-freeze definition table, with borrowed read-only
  access during checking/emission. First identify on-demand specialization
  boundaries: freezing too early prevents legitimate instance construction.
- Consider broader type IDs only after these smaller experiments. Structural
  types, local builtin sums and escaped recursive definitions make a wholesale
  type arena substantially more complex than replacing an `Arc<Ty>`.

An epoch scheme could preserve invalidation with indexed caches; a finalized
immutable graph might permit one-time facts. Neither design is safe merely
because the field is called `OnceLock`. Prove that no definition consumed by
the cached fact can change within that phase.

Retain cycle release, weak-edge promotion, stable display names and complete
graph checks. Use `src/nominal/clone_tests.rs`, nominal unit tests,
`tests/test_recursive_data.rs`, `test_type_cycles.rs`,
`test_generic_data_safety.rs` and concurrent compiler checks.

## Ownership dataflow: conditional follow-up only

`src/ownership.rs::check_loans` skips loan-free bodies, borrows operations
in its CFG and reuses sorted `Live` buffers. Its liveness fixed point scans
reachable nodes until no set changes. Issue #303 already removed operation
cloning and per-round tree allocation; do not undo those improvements.

Only if loan-heavy workloads attribute significant time here, compare:

- a predecessor worklist that revisits nodes affected by changed successors;
- dense bitsets for functions with many dense loan IDs, against sorted sparse
  buffers for small/sparse sets;
- buffer reuse across functions without retaining excessive peak storage.

Measure nodes, edges, loans, rounds, set sizes and allocations. Full scans can
beat worklists on small functions; dense bitsets can waste memory. Any hybrid
threshold must come from workload evidence.

A traversal helper may reduce duplicated nested-block enumeration
(`in_body`, `has_loans`, codegen walkers), but passes differ in cleanup,
nested-function and control-flow semantics. Share only a precisely specified
traversal with tests for each operation family, not one universal visitor that
silently changes the graph.
