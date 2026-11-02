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

## Measured attribution (issue #696)

Sampled on 2026-11-02 at the head of the step 1–5 branches, with a release
build using frame pointers, `perf record -e cpu-clock` on eight physical cores
and the compiled-function cache off. Shares are of all samples, by self time.

| Family | Generated check | Std-heavy check |
| --- | --- | --- |
| `collect_sigs` (building the name-keyed map) | 1.5% | 0.7% |
| Lookups in that map | 0.1% | <0.1% |
| `Nominal` handle clone, `get` and drop | 5.6% | 5.6% |
| Type facts, including cached nominal facts | 0.1% | 0.2% |
| Mutex/futex/`Once` | 0.2% | 0.1% |
| `Ty` clone and drop | 4.1% | 4.3% |
| mimalloc | 9.3% | 13.2% |

The generated check is `compile_bench 1000 20 --workload check --no-warmup`
(39,003 lines; median 138 ms per check with frame pointers). The std-heavy check
is 40 `plenty --check` runs of a program importing `std.http.server`,
`std.http.client`, `std.db.sqlite`, `std.json`, `std.csv`, `std.toml`,
`std.markdown`, `std.xml`, `std.url`, `std.regex`, `std.time` and `std.text`
(about 100 ms each).

Conclusions:

- **Function identity:** the signature map costs at most about 1.7% of a check, almost
  all of it the walk that collects signatures rather than hashing or lookup.
  Function IDs in the IR (options 2–4) would not pay for their migration.
  Revisit only if a workload attributes a material share to `collect_sigs` or
  call lookup.
- **Nominal facts and locking:** fact computation and locks are below 0.3%.
  Indexed fact storage, epochs and a frozen table are not justified by cost.
- **Nominal handles:** clone/`get`/drop traffic is about 5.6%, spread across
  many callers: `Ty::clone` in lowering, generator measurement and validation. Together with `Ty` clone/drop and allocation it
  is a cost of the type representation, not of the table. A change here is the
  broader type-ID or clone-reduction work this page already defers, and it needs
  its own design and matched timings.

The `memcmp`, `memmove` and allocator shares are larger than any family above.
`memcmp` is mostly under `ownership::check` (1.7% of the std-heavy run); it was
not investigated further here.

## Ownership dataflow: conditional follow-up only

`Checker::loans` in `src/ownership.rs` skips loan-free bodies, borrows operations
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
