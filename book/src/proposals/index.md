# Proposals

Proposals record design rationale and rejected alternatives. The
[reference](../design/index.md) defines current behavior; the
[backlog](../backlog.md) tracks future work. If a proposal disagrees with the
reference, follow the reference.

## Status

Each main proposal opens with a status note linking to the governing reference,
identifying differences in what shipped, and naming remaining backlog items.
Companion pages identify their parent and whether their examples are hypothetical.

| Status | Meaning |
| --- | --- |
| Implemented | The design shipped. The page keeps the rationale. |
| Partly implemented | Some of it shipped. The block says which parts. |
| Not implemented | Nothing shipped. The page is the design record for future work. |
| Investigation complete | An assessment reached a decision; it does not imply feature implementation. |

Proposal text retains its original wording: it may describe shipped features as
future work or show syntax that never shipped. Proposal code is not compiled by
the test suite.

When a change implements or alters a design, update its status note. Move a
proposal to [completed/](completed/index.md) when its agreed scope ships or its
investigation concludes without an implementation commitment;
retain its rationale and update links and book navigation. Alternatives and
future extensions need not ship for the agreed scope to be complete. Keep
partly implemented and unimplemented proposals here; retain superseded or
abandoned designs separately in `archived/` when needed.

The status scan used reference, implementation, and regression coverage at
local `main` commit `74fa81d8` on 2026-10-20; it did not rerun every feature
test. Later reviews reconciled HTTP acceptance, error composition, and anonymous
sums at `4e40c4fc` on 2026-10-23, and entry points, modules, executable
lessons, and the next language phase at `161e1c63` on 2026-10-24.

## Find a design by topic

| Topic | Start here | Related decisions |
| --- | --- | --- |
| Rust compiler architecture and performance | [Compiler boundaries](completed/compiler-refactoring.md) | [Diagnostics](completed/compiler-refactoring/diagnostics.md), [frontend/session state](completed/compiler-refactoring/frontend-and-sessions.md), [identity and facts](completed/compiler-refactoring/performance.md), [module replay](completed/compiler-refactoring/module-results.md) |
| Runtime-independent networking protocols | [Codec, session and driver architecture](sans-io-protocols.md) | [Session contract](sans-io-protocols/contract.md), [runtime integration](sans-io-protocols/drivers.md), [WebSocket acceptance](sans-io-protocols/websocket.md) |
| Executable scripts and embedded metadata | [Single-file scripts](completed/single-file-scripts.md) | [Modules and entry points](completed/entrypoints-modules-and-tutorials.md), [current packages](../design/toolchain/01-building/packages.md) |
| Mixed-edition packages and language evolution | [Package language editions](language-editions.md) | [Current packages](../design/toolchain/01-building/packages.md), [script metadata](completed/single-file-scripts.md) |
| Protocol bodies, defaults, `final`, and private hooks | [Protocol method bodies](protocol-method-bodies.md) | [Agreed rules](protocol-method-bodies/final-and-visibility.md), [short programs](protocol-method-bodies/examples.md), [existing wrappers and capabilities](completed/library-owned-operations.md) |
| Heterogeneous protocol objects | [Boxed protocols](completed/boxed-protocols.md) | [Explicit ownership and method contract](completed/boxed-protocols/contract.md), [implementation design](completed/boxed-protocols/implementation.md), [research evidence](completed/boxed-protocols/evidence.md) |
| Error composition | [Composition overview](completed/error-composition.md) | [Anonymous sums and the `Failure` policy](completed/error-sets.md), [completed reporting](completed/error-formatting.md), [typed-stream evidence](completed/error-composition/streams-and-evidence.md) |
| Ownership and state | [Typestate](typestate-and-owned-transitions.md) | [Linear obligations](linear-obligations.md), [resolution scopes](linear-scope-resolution.md), [integration boundaries](linear-integration.md) |
| Optional and fallible borrows | [Returned enums that carry references](returned-reference-enums.md) | [Returned references](../design/language/05-ownership/04-returned-references.md), [borrowed matching](../design/language/05-ownership/06-borrowed-matching.md) |
| Linked structures and graphs | [Owned structures and checked identities](arenas.md) | [Recursive representation](recursive-data.md), [arena prototype](arenas/prototype.md) |
| Collections | [Ordering](user-type-ordering.md) | [Stable sorting algorithm](timsort.md), [hashing and equality](user-type-hashing.md), [slice syntax](completed/slice-syntax.md) |
| Compile-time sizes and fixed inline arrays | [Value generic parameters](value-generics.md) | [Fixed capacity and inline storage](memory-parallelism-and-simd.md#fixed-capacity-and-inline-storage), [portable `std.simd` API](std-simd/api.md) |
| Typed data formats | [Derived serialization](derived-serialization.md) | [`std.json`](../design/stdlib/json.md), [`std.toml`](../design/stdlib/toml.md), [method annotations](protocol-method-bodies/final-and-visibility.md#method-annotations) |
| Native platforms and libraries | [Cross-platform assessment](cross-platform.md) | [FFI](ffi-and-dynamic-libraries.md), [export contracts](ffi-export-contracts.md), [GCC backend](codegen-gcc.md) |
| Allocation and parallel execution | [Memory, parallelism, and SIMD](memory-parallelism-and-simd.md) | [Allocation audit](completed/allocation-audit.md), [shipped concurrency](completed/native-concurrency.md) |
| Cooperative suspension and owned tasks | [Async/await exploration](async-await.md), [Go-style scoped tasks](stackful-tasks.md) | [Async ownership and cleanup](async-await/ownership.md), [async spike evidence](async-await/evidence.md), [stackful runtime choices](stackful-tasks/runtime.md), [stackful spike evidence](stackful-tasks/evidence.md), [linear execution responsibility](linear-integration.md) |
| Iteration and adapter chains | [Fluent iterator pipelines](completed/fluent-iterators.md) | [Shipped generators](completed/generators.md), [parallel execution](memory-parallelism-and-simd.md) |
| Portable vector operations and automatic SIMD | [Portable `std.simd`](std-simd.md) | [API contract](std-simd/api.md), [pipeline recognition](std-simd/automatic.md), [backend evidence](std-simd/evidence.md) |
| Columnar record storage | [Columnar lists](columnar-list.md) | [Ownership and API](columnar-list/contract.md), [chunks and deletion](columnar-list/chunks.md), [layout evidence](columnar-list/evidence.md), [Plenty implementation spike](columnar-list/implementation-spike.md) |
| Analytical dataframes | [Polars embedding](polars-dataframes.md) | [Native interface and measured costs](polars-dataframes/evidence.md) |
| Plotting and data visualisation | [std.plot](std-plot.md) | [Examples beside ggplot2 and plotnine](std-plot/examples.md) |
| Scientific numerics: optimization, integration, ODEs, special functions | [Numerics core](numerics.md) | [API sketch](numerics/api.md), [crate evaluation](numerics/crates.md), [`std.linalg`](../design/stdlib/linalg.md), [`std.stat`](../design/stdlib/stat.md) |
| Exact decimal and financial arithmetic | [std.decimal](decimal.md) | [Polars embedding](polars-dataframes.md), [ordering](user-type-ordering.md), [dictionary keys](user-type-hashing.md), [checked arithmetic](completed/checked-arithmetic.md) |
| Physical units and dimensional analysis | [Units of measure](units-of-measure.md) | [precedents](units-of-measure/precedents.md), [spike evidence](units-of-measure/evidence.md), [domain types](completed/domain-types.md), [value generics](value-generics.md) |
| Language consistency: copying, formatting, views, iteration, errors, callbacks | [Six rules for a consistent core](consistency-rules.md) | [Fluent iterator pipelines](completed/fluent-iterators.md), [user type hashing](user-type-hashing.md), [error composition](completed/error-composition.md) |

Companion examples and evidence do not imply additional implementation commitments.

## Completed

See [Completed proposals](completed/index.md) for shipped designs and their
reference pages. Further extensions remain tracked in the backlog.

## Partly implemented

| Proposal | Shipped | Remaining |
| --- | --- | --- |
| [Recursive owned data](recursive-data.md) | [Recursive storage](../design/language/03-types/recursive-data.md) through `Box[T]` | B11, B33 |
| [Typed ranges, generic calls, and expression-local inference](typed-ranges-and-expression-inference.md) | [Typed ranges](../design/facilities/01-builtins/ranges.md), contextual literals, explicit type arguments | B12, B16, B32 |
| [C interop and dynamic libraries](ffi-and-dynamic-libraries.md) | [Imports, linking, exports, and resident loading](../design/facilities/04-interop/index.md) | B18, B27, B28 |
| [Generated library contracts and C headers](ffi-export-contracts.md) | [Exports](../design/facilities/04-interop/exports.md), [contracts](../design/facilities/04-interop/contracts.md) | B27, B28 |
| [Fallible memory, parallel execution, and SIMD](memory-parallelism-and-simd.md) | [Fallible allocation](../design/language/08-errors/02-allocation-and-traps.md), [concurrency](../design/facilities/03-concurrency/index.md), [initial SIMD](../completed-work/performance.md#b23-simd) | B13 allocator control; B23 SIMD extensions; B24 automatic parallelization; B83 fixed arrays |
| [Standard library scope](standard-library-scope.md) | [Embedded library](../design/stdlib/index.md), including data formats, TLS, HTTP, SQLite, signals and testing | Remaining B38–B70 scope |
| [Keyword arguments, defaults, and static unpacking](keyword-arguments-and-unpacking.md) | [Named calls and fresh defaults](completed/named-arguments-and-fresh-defaults.md) under the narrower design | B36: splats and signature markers remain separate candidates |
| [Units of measure](units-of-measure.md) | [Dimensions](../design/language/03-types/dimensions.md) and [`Quantity[T, D]`](../design/language/03-types/quantities.md), erased to their storage type (issue #701) | B97: `std.units`, sharing bodies that differ only in dimension |

## Not implemented

| Proposal | Backlog |
| --- | --- |
| [Runtime-independent networking protocols](sans-io-protocols.md) — agreed codec/session/driver direction; complete shared WebSocket session and both drivers remain proposed | B94, B85 |
| [Actor budgets and retained stacks](actor-budget-scheduling.md) — test-only heap/trap probe; compiler checkpoints and stage ownership integration remain unimplemented | B85 |
| [Blocking work for actors](actor-blocking-work.md) — bounded native workers, independent job ownership and event-based completion | B85 |
| [Async/await exploration](async-await.md) — owned movable frames, fallible admission and synchronous cancellation; isolated spike only | Unscheduled exploration; async implementation remains out of scope pending an adoption decision |
| [Cosmopolitan and Actually Portable Executables](cosmopolitan.md) — bounded compatibility experiment before any runtime/platform commitment | B18, B17 |
| [Package language editions](language-editions.md) — exploratory compatibility mechanism; no editions implemented | B17 |
| [Value generic parameters](value-generics.md) — accepted design for `const N` and inline arrays | B83; related B13, B23 |
| [Using Rust crates in Plenty](rust-crate-dependencies.md) — three prototypes favor generated C adapters over a Cargo/rustc graph; production integration remains | B17, related B20 |
| [Compiling Plenty to Rust source](rust-source-backend.md) — checked-IR scalar emitter, native companion round-trip and Rust build-cost measurements; ownership probes remain hand-lowered, no production backend | B20, related B17 |
| [Linear obligations](linear-obligations.md), including [aggregate and error rules](linear-obligations/values-and-errors.md) — opt-in non-abandonment | B72 (issue #149) |
| [Synchronous resolution scopes](linear-scope-resolution.md) — consuming scope policy; depends on linear obligations | B72 (issue #149) |
| [Linearity, typestate, and execution responsibility](linear-integration.md) — shared boundaries and deferred worker/async contracts | B72 (issue #149) |
| [GCC backend (`plenty_codegen_gcc`)](codegen-gcc.md) | B20 |
| [Portable `std.simd` and automatic pipeline SIMD](std-simd.md) — small portable API, bounded safe transformations, and backend spike evidence | B23; related B12, B19, B24, B75 |
| [Columnar lists](columnar-list.md) — explicit field-column storage, a bounded object facade, and native layout/ownership spikes | B76; related B11, B12, B23, B75 |
| [User-defined ordering](user-type-ordering.md) — one three-way `__cmp__` hook | B37 |
| [Timsort for stable, adaptive sorting](timsort.md) — fallible workspace, stability, and compatibility choices | B37 (issue #331) |
| [User-defined dictionary keys and set members](user-type-hashing.md) — derived structural hashing and a `KeyType` bound | Structural class/enum/anonymous-sum keys, including custom cleanup, plus `KeyType`; copying operations require duplicable keys |
| [Derived serialization for classes and enums](derived-serialization.md) — `@serial` derives hooks against a JSON-shaped `std.serial` data model shared by JSON and TOML | None yet; candidate under B51/B38 (issue #471) |
| [Returned enums that carry references](returned-reference-enums.md) — `Option[&T]` and `Result[&T, E]` returned under the first-reference-parameter rule, held in locals, never stored | None yet; extends B11 (issue #516) |
| [Go-style scoped tasks](stackful-tasks.md), with [runtime choices](stackful-tasks/runtime.md) and [native spike evidence](stackful-tasks/evidence.md) — exploratory fixed-stack alternative; no lightweight scheduler adopted | B22 investigation; implementation uncommitted |
| [Arbitrary-precision integers](bigint.md) — Plenty limbs with an inline small value; malachite rejected on allocation failure and LGPL | B38 (issue #481) |
| [Faster linking](faster-linking.md) — measured link costs; caching the executable, running the linker directly and reusing the extracted runtime archive; object-format changes do not pay | B19; related B17 |
| [Checked arithmetic by default](checked-by-default.md) — function-level typed errors, explicit `unchecked:` with a validated no-failure precondition and retained local `checked:` recovery; core/repository adoption and bounded validation complete; local review and merge pending | B14 |
| [Exact decimal arithmetic](decimal.md) — allocation-free 38-digit `Decimal` in Plenty, scale kept with numeric equality as in Python, exact operators under the integer trap and `checked:` rules, `NonZero[Decimal]` divisors, explicit rounding | None yet; related B76, B37, B74 |
| [Numerics core](numerics.md) — `std.numeric` in Plenty over `std.linalg`; a scoped SciPy subset in three milestones; `basin` and `numeris` not adopted | B38 (replaces issues #492, #493) |
| [Six rules for a consistent core](consistency-rules.md) — one copy rule, one formatting path, owner-to-view conversion, adapters on iterators only, error types that name only what can fail, every callable form as a callback | None yet; proposes B88–B93; related B12, B49, B84 (issues #614–#636) |

## Completed investigations

| Assessment | Decision / related backlog |
| --- | --- |
| [Native macOS, Windows, and AArch64 support](cross-platform.md) | Native ports need target-aware builds, ABI validation and OS/stdlib contracts before release packaging. B18, with B17/B12 dependencies. |
| [Protocol method bodies](protocol-method-bodies.md) | Core default/final dispatch and member-specific grants agreed; definition-time contract checking required. No automatic method injection. Not implemented or scheduled (B29). |
| [Owned structures and checked node identities](arenas.md) | Tested patterns and arena prototype; production identity/access contract remains B73, broader borrowing B11. |
| [Typestate and owned transitions](typestate-and-owned-transitions.md) | Existing owned transitions and their limits; linear non-abandonment is a separate proposed extension (B72). |
