# Validation and adoption

[Parent proposal](../compiler-refactoring.md). These are acceptance criteria for
possible implementations, not checks claimed to have run for this assessment.

## Behavior-preserving changes

Each implementation should have a bounded responsibility, its own local PR and
a clear comparison against its base. Do not combine diagnostic migration,
function IDs, type storage and module persistence in one refactor.

For Rust changes, run formatting, strict workspace/all-target Clippy and the
full workspace/all-feature suite following
[the development workflow](../../development/workflow.md). Targeted tests help
development but do not replace the full handoff checks.

| Boundary | Existing coverage to retain / extend |
| --- | --- |
| Diagnostics and editor | `tests/test_lsp.rs`, diagnostic-specific files, generic instantiation and borrowing diagnostics |
| Function IDs / IR | `src/op.rs` unit tests, `tests/test_generics.rs`, `test_recursive_tail_errors.rs`, `test_compile_cache.rs`, `test_compiler_parallelism.rs` |
| Nominal facts | Nominal clone/cycle unit tests, `test_recursive_data.rs`, `test_type_cycles.rs`, `test_generic_data_safety.rs` |
| Ownership worklists | `src/ownership/graph_tests.rs`, `test_borrowing.rs`, `test_call_borrowing.rs`, `test_borrowed_match.rs` |
| Module replay | `test_modules.rs`, `test_compile_cache.rs`, module-edit harness and returned-projection dependency probe |
| Session state | Repeated library checks, editor recovery, independent concurrent sessions and unwind/nesting policy |

Add regression tests for the actual new boundary rather than only testing
through the CLI. Preserve error order across workers, object identity where
the existing contract requires it, source/trap attribution, runtime cleanup,
generic limits and invalid-program rejection.

## Measurement matrix

Follow [profiling](../../development/profiling.md) and
[module-edit measurements](../../development/module-edits.md). Use an isolated
target directory/cache directory so profiling builds do not replace another
worker's compiler or contaminate warm-cache comparisons.

Measure at least:

- small checks and larger generated programs across multiple sizes;
- ordinary and generic-heavy checking;
- std-heavy imports and a representative application;
- cache-off object emission, warm-function emission, unchanged-program hits,
  body edits, interface edits and generic edits;
- one worker/core and the intended multicore configuration.

Use release, feature-disabled compilers for final latency comparisons.
Use phase profiling for attribution, `profile-alloc` for allocation traffic,
and a sampling profiler for uninstrumented leaf costs. Inclusive rows cannot
be added as independent phase costs. Integration tests use a different
allocator path from compiler executables; do not interpret their wall time as
the CLI's allocation performance.

Record exact compiler commits, Rust/Cranelift versions, CPU/affinity,
allocator, flags, source size, cache state, warm-up policy and every timed
sample. Alternate baseline/candidate runs where possible. Report median,
dispersion, user/system CPU, peak RSS, allocation bytes/counts, instructions
and cache/TLB counters when available. Explain unavailable counters.
Keep linking separate from object emission.

## Decision rules

- A structural refactor can be adopted without a speedup if it measurably
  simplifies ownership/dependencies and preserves behavior without a material
  performance or memory regression. Review the resulting API, not only file
  size or line count.
- A performance change needs a repeatable end-to-end benefit beyond observed
  noise, not merely a smaller microbenchmark. Identify any small-program,
  multicore, memory, artifact-size or generated-code tradeoff explicitly.
- A module replay design needs correct invalidation on rejected as well as
  accepted inputs, plus net savings after replay costs.
- A session change needs defined lifetimes and ordinary destruction for
  embedding. Correct destruction does not require the allocator to return
  every freed page to the operating system immediately.
- A failed experiment is useful evidence. Keep its method/results and reject
  the complexity rather than weakening checks or moving the benchmark target.

No fixed percentage is promised. Agree on workload-specific acceptance and
acceptable regressions before implementing an invasive performance design.

## Documentation and handoff

Keep reference pages describing shipped behavior until implementation lands.
Then update the frontend/cache/library API references actually affected.
Update learner material only for observable changes; do not manufacture tutorial
changes for an internal representation refactor.

Run all three docs CI checks for book changes: source links, Python helper tests
and the complete versioned-site builder with the pinned mdBook. In each PR,
state what ran and what did not. Record delivered scope in completed-work
records and narrow the backlog, without marking a design proposal as a shipped
compiler optimization.
