# Proposals

Proposals record why a design was chosen and what was rejected. They do not
specify the language. The [reference](../design/index.md) defines current
behavior, and the [backlog](../backlog.md) is the only list of future work.
Where a proposal and the reference disagree, the reference is correct.

## Status

Each main proposal opens with a status note; companion evidence and example
pages identify their parent and whether their code is hypothetical. The note gives
the reference
pages that own the contract, what shipped differently from the text, and what
remains with its backlog ID.

| Status | Meaning |
| --- | --- |
| Implemented | The design shipped. The page keeps the rationale. |
| Partly implemented | Some of it shipped. The block says which parts. |
| Not implemented | Nothing shipped. The page is the design record for future work. |
| Investigation complete | An assessment reached a decision; it does not imply feature implementation. |

Below its status block a proposal keeps its original wording. It may say
"should" or "add" about things that now exist, and its code blocks may show
syntax that never shipped. Proposal code is not compiled by the test suite.

A change that implements or alters a proposed design updates that proposal's
status block in the same change. Move proposals whose agreed scope has shipped
into [completed/](completed/index.md), preserving their rationale and updating
links and the book navigation. Completion does not require implementing every
alternative or future extension discussed in the original text.

Partly implemented and not implemented proposals stay in this folder. Keep their
status blocks explicit about what shipped and what remains. Superseded or
abandoned proposals can be retained separately in `archived/` when needed; they
are not completed implementations. The reference and tutorial remain authoritative
for current behavior. Statuses were last scanned against local `main` at
`74fa81d8` on 2026-10-20, using the reference, implementation, and regression
coverage. This is a status review, not a fresh execution of every feature test.
HTTP acceptance, error composition and anonymous sums were reconciled separately
against local `main` at `4e40c4fc` on 2026-10-23 and moved to `completed/`.
Entry points and modules, with the executable-lessons decision, and the next
language phase review were reconciled against `161e1c63` on 2026-10-24 and
moved to `completed/`.

## Find a design by topic

| Topic | Start here | Related decisions |
| --- | --- | --- |
| Executable scripts and embedded metadata | [Single-file scripts](single-file-scripts.md) | [Modules and entry points](completed/entrypoints-modules-and-tutorials.md), [current packages](../design/toolchain/01-building/packages.md) |
| Mixed-edition packages and language evolution | [Package language editions](language-editions.md) | [Current packages](../design/toolchain/01-building/packages.md), [script metadata](single-file-scripts.md) |
| Protocol bodies, defaults, `final`, and private hooks | [Protocol method bodies](protocol-method-bodies.md) | [Agreed rules](protocol-method-bodies/final-and-visibility.md), [short programs](protocol-method-bodies/examples.md), [existing wrappers and capabilities](library-owned-operations.md) |
| Error composition | [Composition overview](completed/error-composition.md) | [Anonymous sums and the `Failure` policy](completed/error-sets.md), [completed reporting](completed/error-formatting.md), [typed-stream evidence](completed/error-composition/streams-and-evidence.md) |
| Ownership and state | [Typestate](typestate-and-owned-transitions.md) | [Linear obligations](linear-obligations.md), [resolution scopes](linear-scope-resolution.md), [integration boundaries](linear-integration.md) |
| Linked structures and graphs | [Owned structures and checked identities](arenas.md) | [Recursive representation](recursive-data.md), [arena prototype](arenas/prototype.md) |
| Collections | [Ordering](user-type-ordering.md) | [Stable sorting algorithm](timsort.md), [hashing and equality](user-type-hashing.md) |
| Typed data formats | [Derived serialization](derived-serialization.md) | [`std.json`](../design/stdlib/json.md), [`std.toml`](../design/stdlib/toml.md), [method annotations](protocol-method-bodies/final-and-visibility.md#method-annotations) |
| Native platforms and libraries | [Cross-platform assessment](cross-platform.md) | [FFI](ffi-and-dynamic-libraries.md), [export contracts](ffi-export-contracts.md), [GCC backend](codegen-gcc.md) |
| Allocation and parallel execution | [Memory, parallelism, and SIMD](memory-parallelism-and-simd.md) | [Allocation audit](completed/allocation-audit.md), [shipped concurrency](completed/native-concurrency.md) |
| Cooperative suspension and owned tasks | [Async/await exploration](async-await.md), [Go-style scoped tasks](stackful-tasks.md) | [Async ownership and cleanup](async-await/ownership.md), [async spike evidence](async-await/evidence.md), [stackful runtime choices](stackful-tasks/runtime.md), [stackful spike evidence](stackful-tasks/evidence.md), [linear execution responsibility](linear-integration.md) |
| Iteration and adapter chains | [Fluent iterator pipelines](fluent-iterators.md) | [Shipped generators](completed/generators.md), [parallel execution](memory-parallelism-and-simd.md) |
| Portable vector operations and automatic SIMD | [Portable `std.simd`](std-simd.md) | [API contract](std-simd/api.md), [pipeline recognition](std-simd/automatic.md), [backend evidence](std-simd/evidence.md) |
| Columnar record storage | [Columnar lists](columnar-list.md) | [Ownership and API](columnar-list/contract.md), [chunks and deletion](columnar-list/chunks.md), [layout evidence](columnar-list/evidence.md), [Plenty implementation spike](columnar-list/implementation-spike.md) |
| Analytical dataframes | [Polars embedding](polars-dataframes.md) | [Native interface and measured costs](polars-dataframes/evidence.md) |

These families keep distinct contracts separate: a sorting algorithm does not
define user equality, protocol defaults do not imply private access, and error
formatting does not determine recovery. Companion pages hold examples or
evidence; they are not additional implementation commitments.

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
| [Fallible memory, parallel execution, and SIMD](memory-parallelism-and-simd.md) | [Fallible allocation](../design/language/08-errors/02-allocation-and-traps.md), [concurrency](../design/facilities/03-concurrency/index.md) | B13, B23, B24 |
| [Standard library scope](standard-library-scope.md) | [Embedded library](../design/stdlib/index.md), including data formats, TLS, HTTP, SQLite, signals and testing | Remaining B38–B70 scope |
| [Keyword arguments, defaults, and static unpacking](keyword-arguments-and-unpacking.md) | [Named calls and fresh defaults](completed/named-arguments-and-fresh-defaults.md) under the narrower design | B36: splats and signature markers remain separate candidates |
| [Fluent iterator pipelines](fluent-iterators.md) | [Serial and composed pipelines](../design/language/09-iteration/06-pipelines.md) with `.generator()` conversion; replaces the removed `std.iter` | B75 lending and `.par(&pool)` stages; relates to B12, B22, B24 |

## Not implemented

| Proposal | Backlog |
| --- | --- |
| [Async/await exploration](async-await.md) — owned movable frames, fallible admission and synchronous cancellation; isolated spike only | Unscheduled exploration; async implementation remains out of scope pending an adoption decision |
| [Single-file scripts with embedded manifests](single-file-scripts.md) — `plenty script`, isolated resolution and cached builds | B17 |
| [Package language editions](language-editions.md) — exploratory compatibility mechanism; no editions implemented | B17 |
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
| [User-defined dictionary keys and set members](user-type-hashing.md) — derived structural hashing and a `KeyType` bound | None yet |
| [Derived serialization for classes and enums](derived-serialization.md) — `@serial` derives hooks against a JSON-shaped `std.serial` data model shared by JSON and TOML | None yet; candidate under B51/B38 (issue #471) |
| [Go-style scoped tasks](stackful-tasks.md), with [runtime choices](stackful-tasks/runtime.md) and [native spike evidence](stackful-tasks/evidence.md) — exploratory fixed-stack alternative; no lightweight scheduler adopted | B22 investigation; implementation uncommitted |

## Completed investigations

| Assessment | Decision / related backlog |
| --- | --- |
| [Native macOS, Windows, and AArch64 support](cross-platform.md) | Native ports need target-aware builds, ABI validation and OS/stdlib contracts before release packaging. B18, with B17/B12 dependencies. |
| [Library-owned operations and backend hooks](library-owned-operations.md) | Existing composition and optional capabilities; new protocol access deferred. B29 (issue #145). |
| [Protocol method bodies](protocol-method-bodies.md) | Core default/final dispatch and member-specific grants agreed; definition-time contract checking required. No automatic method injection. Not implemented or scheduled (B29). |
| [Owned structures and checked node identities](arenas.md) | Tested patterns and arena prototype; production identity/access contract remains B73, broader borrowing B11. |
| [Typestate and owned transitions](typestate-and-owned-transitions.md) | Existing owned transitions and their limits; linear non-abandonment is a separate proposed extension (B72). |
