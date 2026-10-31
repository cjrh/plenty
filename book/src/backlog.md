# Backlog

This is the single maintained list of **remaining** language, compiler, runtime,
standard-library and tooling work. Entries are candidates, not promises about
syntax or delivery dates. Row order is a catalogue, not a priority list.

Delivered milestones and retired entries live in [Completed work](completed-work.md).
A B ID can appear there for its delivered subset and here for its remaining
extensions; IDs are never renumbered or reused. The
[implementation status](design/toolchain/02-architecture/support.md) and
[reference](design/index.md) describe what works today. [Proposals](proposals/index.md)
record rationale and alternatives, not another work queue.

## Current position

B34's bounded cleanup/review, B71's typed HTTP acceptance batch, B76's initial
dataframe datatype expansion, the initial B39–B70 library slices and B17 package
tooling have landed. Their delivery records are in [Completed work](completed-work.md).
Preserve their installed-toolchain, CLI/data, JSON-service, mutual-TLS and
HTTP qualification checks during further work.

Issue severity takes precedence over the batches below. Defects, reproductions,
claims and implementation/review status belong in the shared `issues.db`.
Nothing on this page assigns ownership or approves an API without a scoped
agreement and acceptance checks.

### Next batches

- **B76 — analytical workloads:** decimal columns should follow a separate
  `std.decimal` design and implementation. Use the pinned Polars engine's
  Parquet capabilities; fixed-size arrays, dictionary semantics, nested casts
  and broader list operations need separate scope. Use the
  [daily sales application](tutorial/special-purpose/dataframe-application.md)
  and `examples/sales_report/` as evidence: truly large or rejection-heavy
  workloads should guide multi-output streaming. Temporal/grouped as-of
  matching and time-bucketed/rolling aggregations need a separate workload
  before expanding the API.
- **B23/B76 — measured SIMD extensions:** packed views, wider shapes, broader
  predicates, maps/folds, runtime ISA selection and parallel scans need bounded
  legality/performance evidence. Account for code placement using the retained
  [placement research](proposals/columnar-list/placement-research.md).
- **B71 — additive error ergonomics:** keep the `compile_bench` precise-error
  template migration deferred while comparisons depend on its unchanged
  workload. Floating-point report formatting and other conveniences are
  independent candidates, not prerequisites for typed HTTP composition.

Further library breadth, B11 stored-reference extensions and B72
state/linear-obligation experiments remain candidates requiring a concrete use
case and an agreed contract. This sequence preserves the existing batch scope;
it does not schedule all catalogue entries.

### Deliberate deferrals

- **B19 — module caching and frontend scale:** serial efficiency, streaming
  lowering/emission and per-module caching need measurements and a focused
  batch. The [module-cache investigation](development/module-cache-investigation.md)
  rejected separately compiled modules and stored syntax trees. A lowered-module
  cache needs imported interfaces and invalidation by body-derived facts;
  its broad-import benchmark offered about 30 ms of frontend work in a 44 ms
  body-edit build. Revisit for frontend-bound std-heavy applications, using
  [module-edit measurements](development/module-edits.md) as the baseline.
  Remote caching waits on a local per-module contract.
- **B22 — broader concurrency:** richer mapping/effects, streaming results,
  rendezvous, larger or send-side selection remain deferred. Existing
  [channel](design/facilities/03-concurrency/channels.md#what-the-rule-does-not-decide)
  and [executor](design/facilities/03-concurrency/executors.md#shutdown-and-live-receivers)
  checks do not establish deadlock freedom.
- Automatic parallelization (B24), optional backends (B20), stored-reference
  relationships (B11), broader callback erasure (B32) and automatic deep
  recursive operations (B33) require their own contracts and acceptance checks.

### Scope of follow-on work

**B22 — task models.** The [stackful-task investigation](proposals/stackful-tasks.md)
and [async/await exploration](proposals/async-await.md) do not commit Plenty to
either implementation. Before adoption, compare the same bounded TCP/HTTP
workload: I/O cancellation/registration lifetimes, allocation and failed-start
preservation, stack budgets, suspension/loan checks, memory and latency. Fixed
stacks trade pinning/relocation for reservations and fatal unchecked exhaustion.
Retaining native concurrency remains an option. Prefer library APIs over new
syntax; require bounded queues/backpressure, failed-send ownership, joins on
all exits, cooperative cancellation and partial-result cleanup. Scoped generic
tasks need not wait for heterogeneous callback storage in B32.

**B32 — additional callback representations.** Evaluate registry/event-handler
use cases and specify invocation, move/drop, allocation failure and capture
lifetimes before adding erased modes. Escaping borrowed captures depend on B11;
C ABI callbacks and foreign-library lifetimes remain B28.

## Work items

### Language, compiler and tooling

| ID | Remaining work | Scope / dependency |
| --- | --- | --- |
| B11 | Broader borrowing | Stored/returned references and views; returning a borrow chosen among several reference parameters; symbolic-index collection loans; field-precise loans through `mut` reference bindings. Optional/fallible references: phase 2 (`mut` locals and reference-carrying parameters), nested patterns through the carried reference, `match` expressions producing these values, `dict.get_ref` and `list.get_ref`/`get_mut`. Preserve owner/variant stability and temporary lifetimes; measure precision against compile-time cost. [Proposal](proposals/returned-reference-enums.md); [delivered subset](completed-work/language-and-tooling.md#b11-borrowing). |
| B12 | Practical I/O and iteration | Reusable iterator/view protocols, stored/returned slice and substring views (B11), portable path semantics (B18). For substrings: `str` and std text APIs accepting `&substr`, more view methods and cached scalar counts. Further adapters need use cases; pipeline-specific extensions are B75. [Delivered subset](completed-work/language-and-tooling.md#b12-io-and-iteration). |
| B13 | Public allocator control | Global/default and per-container selection, allocator state/lifetimes and buffer provenance. Consider bounded/inline-capacity storage separately. [Memory design](proposals/memory-parallelism-and-simd.md). |
| B14 | Typed error composition | Additional Option/Result conveniences, typed composition of parsing/conversion errors, checked shifts and casts. Preserve precise errors and use `Failure` only as a last resort. Human-readable reporting extensions are B71. [Delivered subset](completed-work/language-and-tooling.md#b14-errors). |
| B16 | Inference and value conveniences | Expected-result inference, partial explicit type arguments, contextual literals across generic arguments; `NonZero[T]` module constants; saturating conversion; standalone unit bindings/parameters/storage and nested unpacking. Require concrete use cases/contracts, not automatic extensions of today's inference. Parsing/conversion error composition is B14. [Delivered subset](completed-work/language-and-tooling.md#b16-values-and-syntax). |
| B17 | Modules, tooling and packaging | Shared per-user Git cache; semver selection over tags; per-module incremental objects (B19); re-exports and import-cycle policy. LSP: find references, document symbols, signature help, `case` pattern variants and incremental module re-checking. Preserve deterministic lookup, standalone compilation and [reproducibility](design/toolchain/01-building/linking.md). Further targets need runtime/linker/artifact tests (B18); std breadth is B38. Exploratory workflows are listed under [tooling investigations](#tooling-investigations). [Delivered subset](completed-work/language-and-tooling.md#b17-tooling-and-packaging). |
| B18 | Windows and additional targets | Native GNU/Linux AArch64 compiler/runtime implementation is present alongside x86-64: baseline ARMv8-A code generation, target-aware native dependencies, Linux bindings, UDF traps, stack handling and AAPCS64 actor recovery. Qualification includes host SIGILL coexistence (issue #588), first actor-heap ownership (issue #589), and native libFuzzer discovery (issue #591). [Development and emulated qualification](development/linux-aarch64.md) records the reproducible harness; B17 owns artifact delivery. Native ARM CI passes the compiler/runtime suites, both optional engines and Docker proxy tests. Remaining admission gates: dedicated native hardware concurrency/signal stress, minimum glibc/kernel and page-size qualification, and clean release/engine smoke tests. Packaged linalg integration and tutorial tests pass under the ARM guest; that local artifact does not qualify the release libc floor. The updated [cross-platform assessment](proposals/cross-platform.md), [interface/source map](proposals/cross-platform/boundaries.md) and [toolchain experiments](proposals/cross-platform/experiments.md) guide ARM macOS and x86-64 Windows; Windows ARM64 remains deferred. Changing the linker alone does not add cross-compilation. The [Cosmopolitan/APE proposal](proposals/cosmopolitan.md) recommends a bounded x86-64 compatibility experiment and identical-artifact non-Linux qualification before deciding on a restricted profile or broader port. |
| B19 | Measured codegen and storage | See the [remaining performance work](#remaining-performance-work); completed measurements and optimizations are in the [performance record](completed-work/performance.md). |
| B20 | Optional backends | Evaluate the [GCC proposal](proposals/codegen-gcc.md) before adopting another backend/common IR. The [Rust-source investigation](proposals/rust-source-backend.md) includes a [checked scalar emitter](proposals/rust-source-backend/checked-emitter.md); ownership-bearing APIs remain hand-lowered probes and need emitted semantic parity. Cranelift stays the production backend. Rust dependencies are B17. |
| B22 | Native concurrency extensions | User-facing scoped pool jobs borrowing the submitting scope; broader iterator/closure mapping, streaming results, richer certified effects/types, rendezvous and broader selection. Mutable, one-shot, borrowed, generator-containing and indirect map callbacks need separate scope. Preserve fallible submission and deterministic shutdown. Task-model gates are under [follow-on scope](#scope-of-follow-on-work). [Delivered subset](completed-work/language-and-tooling.md#b22-native-concurrency). |
| B23 | SIMD extensions | Packed storage/views, broader vector shapes, further predicates, total maps/modular folds, runtime ISA selection and parallel scans. Require separate evidence; hide alignment/tail/target bookkeeping, preserve checked arithmetic and ordered floating folds, and do not treat generic lists as packed arrays. [Spikes/backend gates](proposals/std-simd/evidence.md); [delivered subset](completed-work/performance.md#b23-simd). |
| B24 | Automatic parallelization | Deferred beyond B34. Require evidence that effects, cleanup, allocation failures and result ordering remain correct; a thread-count setting alone is insufficient. |
| B27 | Expanded C representations | Export text/byte buffers and views, error sums of payload-free enums, payload-carrying errors, recoverable returned ownership and explicit class-method adapters. Keep allocator provenance, output initialization and error-path transfer visible. [Owned export contract](design/facilities/04-interop/owned-exports.md). |
| B28 | Library loading/lifecycle | Generate loaders from extracted contracts without original export source; improve third-party C binding tooling and error detail. Unloading, retained/foreign-thread callbacks, C record layout and header-assisted bindings need separate contracts. [Current resident/thread-bound loaders](design/facilities/04-interop/loading.md). |
| B29 | Further generic constraints | Protocol bounds on data, generic methods satisfying protocols, combined bounds/composition, generic aliases and enum constructor inference. Check bodies against declared bounds so argument-independent errors stay at definitions; preserve explicit lookup and bounded specialization. [Library-owned operations](proposals/library-owned-operations.md) favors composition/capabilities; protocol access grants remain deferred. [Protocol bodies](proposals/protocol-method-bodies.md) records agreed default/final dispatch, member-specific grants and definition-time contract checking, not automatic injection; no implementation scheduled. [Delivered subset](completed-work/language-and-tooling.md#b29-generic-checking). |
| B32 | Broader callback storage | Mutable/consuming/borrowed/worker-bound erased modes and generator-containing environments in heap owners need their own contracts. Escaping borrowed storage depends on B11; see [follow-on scope](#scope-of-follow-on-work). [Delivered subset](completed-work/language-and-tooling.md#b32-callback-storage). |
| B33 | Deep recursive operations | Iterative fallible copy/format work storage and an explicit bounded-memory contract for structural equality/membership, without hidden allocation failure. Preserve cleanup, shared-string ownership and float/NaN behavior. [Rationale](proposals/recursive-data.md). |
| B36 | Named-call extensions | Finer protocol receiver tracking, deliberate builtin argument names and structured initialization for large records. Splats, variadics and `/`/`*` markers remain in the [unpacking proposal](proposals/keyword-arguments-and-unpacking.md). [Delivered defaults/named calls](completed-work/language-and-tooling.md#b36-named-arguments-and-defaults). |
| B37 | Ordering extensions | Cached/fallible sort keys, stable sorting and user-defined ordering without hidden allocation. The [ordering proposal](proposals/user-type-ordering.md) recommends `__cmp__`/`Order` for comparisons/equality and runtime sorting, not dictionary keys. Evaluate [Timsort/Powersort](proposals/timsort.md) with fallible scratch space; naming/migration remain decisions. Use measured runtime-sort bounds (issue #331). [Delivered subset](completed-work/language-and-tooling.md#b37-ordering). |
| B38 | Standard-library breadth | See [library follow-ups](#standard-library-follow-ups) and [ergonomics](#stdlib-driven-language-ergonomics). Further breadth requires a use case and scoped contract. Prefer Plenty unless native complexity/security justifies a wrapper; use the [engine criteria](design/toolchain/01-building/engines.md) for large native libraries. [Delivered modules](completed-work/libraries.md#b38-library-breadth). |

### Tooling investigations

These belong to B17 and are not scheduled implementation commitments:

- [`plenty script`](proposals/single-file-scripts.md): embedded manifests,
  isolated dependencies and cached single-file builds. Settle headers and
  lock/storage rules before implementation.
- [Language editions](proposals/language-editions.md): wait for a concrete
  incompatible change; prove mixed-edition compilation with shared types/runtime
  before edition or migration tooling.
- [Rust crate dependencies](proposals/rust-crate-dependencies.md): three prototypes
  compare Rust ABI, generated C adapters and Rust-source lowering. The next gate
  is real Plenty project integration with checked foreign owners and a coherent
  runtime build; generated C adapters are the proposal's recommendation.
- [Local semantic search](development/semantic-search.md): improve chunk boundaries
  and measure retrieval quality before deciding on committed index snapshots.
  Prefer Plenty for the interface and record language defects in `issues.db`.

### Remaining performance work

B19 is evidence-driven; these are candidates, not an approved optimization batch.
Use [profiling](development/profiling.md), [faster-linking evidence](proposals/faster-linking.md)
and the [completed measurements](completed-work/performance.md) as baselines.

- **Generated code:** retain/release for text operands computed by calls or loaded
  in another block; dispatcher copies of sums with inline-record payloads;
  code size and compilation/link latency. Revisit match density/size policies
  only with application/CPU evidence. An opt-in CPU target such as x86-64-v2
  requires a decision against B17's fixed-baseline reproducibility contract.
- **Layout/cleanup:** natural record-field packing beyond 16-byte slots, compact
  sum tags/niches, relocation/release plans for records holding ranges/sums/other
  managed fields, records over 192 bytes and tuple release plans. Measure inline
  copies beyond current runtime-copy paths; owned temporaries and nonscalar list
  rows still use runtime helpers. [Flat-layout rationale](proposals/flat-class-layout.md).
- **Serial frontend efficiency:** module-scope name hashing and `op::check`'s
  signature map, builtin-sum interning and nominal-handle refcounts. Whole-program
  table/cache misses still increase per-function cost at scale (issue #57);
  interned names or per-function batch processing need measurements. Further
  type-representation changes follow the completed issue #302 investigations.
- **Peak memory:** lower and emit one function at a time instead of retaining
  full syntax/operation sets at phase boundaries (issue #54). IR construction's
  measured 7% of emission CPU limits scaling only above about fourteen cores.
- **Caching/lowering:** imported-interface lowering with explicit outputs and
  serialized operations/types; body-derived invalidation and per-function
  granularity for one-module programs (issue #317). Separately evaluate lowering
  only reachable `std` functions. A trusted opt-in remote cache follows a local
  per-module contract, not before it. See [deferrals](#deliberate-deferrals).
- **Upstream gates:** regalloc2's quadratic block-parameter handling needs an
  upstream report/fix before removing the 2,048-argument fallback. Reopen issue
  #425's deferred arithmetic-check work only when Cranelift changes unfused
  overflow branches, frame setup or callee-save spills; do not treat closed
  investigations as active claims.

### Stdlib-driven language ergonomics

| ID | Remaining work | Scope / dependency |
| --- | --- | --- |
| B71 | Error presentation | Floating-point report formatting (issue #334), benchmark-template migration after active comparisons, and separately scoped conveniences such as field-level redaction (for example URL userinfo, beyond shipped class/enum redaction). Case guards, residual-set inference, empty sums, arbitrary protocol forwarding and dynamic error objects remain deferred. [Delivered composition/HTTP work](completed-work/libraries.md#b71-error-composition-and-http). |
| B72 | State/ownership ergonomics | Explore [typestate](proposals/typestate-and-owned-transitions.md) and runtime-state enums. [Linear obligations](proposals/linear-obligations.md), [resolution scopes](proposals/linear-scope-resolution.md) and [integration boundaries](proposals/linear-integration.md) do not approve implementation or blanket std conversion. Real HTTP integration of the [owned-response experiment](proposals/typestate-response-experiment.md) remains unexplored. Field moves through `Box`/`Shared`, per-element tuple tracking, partial destruction for `__del__` classes and whole-object destructuring need contracts. [Delivered subset](completed-work/language-and-tooling.md#b72-state-and-ownership). |
| B73 | Arenas/linked data | Before `std.arena`, replace caller namespaces with a safe arena-identity contract covering copy/concurrency/exhaustion, review callbacks/movement cost, and qualify deletion/reuse, bad handles, affine cleanup, OOM preservation and tree/graph consumers. Keep linked lists/trees as examples until a workload justifies an API. Direct disjoint loans/worklists relate to B11; no intrusive-pointer extension implied. [Assessment/prototype](proposals/arenas.md). |
| B74 | User-defined keys | [Structural hashing proposal](proposals/user-type-hashing.md): tuples/classes/enums with key-type fields, built-in `KeyType`, no user `__hash__`/`__eq__`. Stages: copyable enums, `KeyType`, classes. [Existing hash/traversal work](completed-work/performance.md#b74-collection-hashing). |
| B75 | Pipeline extensions | Early stop for parallel `any`/`all` and integer range sources beyond `range[i64]`. Broader reusable iterator/view relationships are B12/B11. All four original stages are [delivered](completed-work/language-and-tooling.md#b75-fluent-pipelines); they are not outstanding tasks. |
| B76 | Columnar extensions | Decimal design first; temporal/grouped/floating as-of keys, time-bucketed/rolling aggregations, fixed-size arrays, dictionary semantics, nested casts and broader list operations need workload evidence. Validate use cases before static-schema/zero-copy machinery. For `clist`: row views/method adaptation, field references, affine fields, chunks, statistics, tombstones/vacuum and interchange. Broader SIMD/parallel scans need legality/performance evidence (B23); borrowing B11/B12, pipelines B75. [Delivered dataframe/clist subsets](completed-work/libraries.md#b76-columnar-data). |
| B77 | Derived serialization | Accepted [design](proposals/derived-serialization.md), not implementation: JSON-shaped `std.serial`; hooks/compiler-expanded entry points; class `@serial`; TOML; docs. `SerialError[E]` retains format error/field path. Measure compile time/throughput before derive. Relates to B29/B51; reuse the closed annotation mechanism. |
| B84 | Generic copy classes | A copyable bound for generic `@copy` classes. Class dictionary keys stay B74. [Delivered non-generic subset](completed-work/language-and-tooling.md#b84-copy-classes). |
| B85 | Actor extensions | `std.actor.httpserver` over `std.actor.net`; multi-thread stages; sliding-window/all-for-one restart, parent restart notices and an actor-specific `&mut self` spawn diagnostic. macOS heaps wait on B18. [Delivered actors/network/TLS](completed-work/libraries.md#b85-actors). |
| B80 | Plotting | Finish the [accepted core milestone](proposals/std-plot.md): remaining scales, smoothing, broader facet controls and font-measured layout. Then broader geoms/PNG, dates, reflected records and a separate dataframe bridge. [Delivered subset](completed-work/libraries.md#b80-plotting). |
| B83 | Value generics/arrays | Accepted [design](proposals/value-generics.md), not shipped: `const N`, inline `array[T, N]`, allocation-free literals at generic parameters, per-instance assertions; then `BoundedList` (B13) and measured internal SIMD rewrite (B23). Derived sizes in signatures remain deferred. |

### Standard library follow-ups

Only outstanding extensions are listed here. Initial delivery records, issue/PR
references and reference links live in [Completed library work](completed-work/libraries.md).
B IDs keep their original scope; a closed initial issue does not approve a
follow-up. All candidates still require scope and acceptance checks.

| ID | Area | Remaining candidates |
| --- | --- | --- |
| B38 | Additional breadth | `std.bigint` (issue #481): Plenty implementation per the [bigint proposal](proposals/bigint.md); needs `u64` widening-multiply and add-with-carry builtins. XML namespace URI resolution and tree/query APIs; recursive filesystem watches/automatic reconciliation; accurate summation/norms, float decomposition/adjacent values and special functions; constant function calls and `bytes` concatenation. Linalg conditioning, least squares and sparse storage need separate scope. Image: GIF/LZW/frames, baseline/progressive JPEG/encoding and WebP/VP8/VP8L/RIFF, then 16-bit buffers, streaming decode, metadata, resampling filters and the measured 2–6× Pillow speed gap; implement in Plenty under the same `Limits`. Broader statistics need numerical contracts; sketches need mergeable quantiles/frequent items, stronger tail accuracy and workload-led xor/fuse tuning. System: cgroup accounting, CPU utilization sampling and filesystem free space. |
| B39 | Bytes | Stored/returned views (B11); `ByteSource` support in HTTP server `Exchange.write`/`respond`/`send` and crypto updates. |
| B40 | Binary streams | Unscheduled nonblocking descriptors, vectored I/O, peeking and erased streams. |
| B41 | Error context | Native-cause OS description text (B14/B71). |
| B42 | Time/deadlines | Unscheduled `Duration`/`Deadline` overloads of timed waits. |
| B43 | Processes | Deadline waits via pidfd/cancellation, file/child redirection, stderr merging, dropped-child reaping policy and process groups. |
| B46 | Logs | JSON-lines sink, sampling/rate limits, per-module thresholds and span context. |
| B47 | Text/Unicode | Grapheme-aware `std.text.wrap` and broader classification; keep the external `unicode_text` package's Unicode version aligned with `std.unicode`. |
| B48 | Regex | Lazy DFA/one-pass acceleration and literal prefilters; full Unicode case folding, script and general-category properties. |
| B49 | Collections/iterators | User iterator protocol (B12, needs B11 stored references); further collections need use cases. |
| B50 | Random/secrets | Jump/split streams, distributions/float ranges, weighted choice/element sampling, scratch-free secret integers and a zero-on-drop buffer. |
| B51 | JSON | Bulk scans outside string runs, pretty-printing, JSON Lines and derived codecs (B77). |
| B52 | TOML | Independent conformance checks (`toml-test`/reference crate); evaluate TOML 1.1. |
| B53 | CSV | Lenient parsing/recovery, header mapping/typed columns, non-UTF-8 encodings, multibyte delimiters and throughput benchmarks. |
| B54 | Encodings | Bulk native codec/packing throughput, streaming codecs, float packing and varints. |
| B55 | Filesystem | Passwd fallback, `XDG_RUNTIME_DIR` and `XDG_*_DIRS` search lists. |
| B56 | Concurrency primitives | Condition variables, rwlocks and semaphores only with a concrete use case. |
| B60 | URLs | Relative resolution, normalization/equivalence, IDNA and form encoding. |
| B63 | HTTP client | WebSocket `permessage-deflate` and proxy tunnels; cookie jars/content decoding were declined, not scheduled. |
| B64 | HTTP server | SSE client reader; waking SSE streams from channel messages rather than polling. |
| B65 | HTTP helpers | Static-file `Range` requests and directory indexes. |
| B66 | HTTP qualification | Generated route dispatch without user `match` waits on enum methods or existential protocol values; preserve existing qualification suites. |
| B67 | Crypto | P-384/JWT ES384 and SHA-384 HMAC/JWT HS384; password KDFs need separate use-case/API review. |
| B68 | Compression | Gzip header access, zstd and trailing-zero tolerance; archives remain B69. |
| B69 | Archives | Policy-controlled link creation, mode/time restoration and streaming ZIP writes with data descriptors. |
| B70 | SQLite | Runtime allocator integration for accounting/failure injection, user-defined functions and backup API. |
| B78 | Diffs | Line-first pass for char/word diffs and diffs in `std.test` failures (B57). Performance baselines belong to B19. |
| B79 | Markdown | GFM tables, strikethrough/task lists and parser throughput beyond the recorded ~1 MB/s. |
| B82 | Graphs | Weak components, Prim/Floyd–Warshall, flow/matching, compaction, non-`f64` weights and `()` type arguments. |

## Reconciled completed work

The delivery records and retired entries moved to [Completed work](completed-work.md).
This heading remains as a link destination; completed-only B IDs no longer
appear as work to select here.

## Keeping this list useful

- Add, prioritize or split future work here. Keep IDs stable and link design
  detail rather than copying queues into proposals, tutorials or reference pages.
- Settle a candidate's scope and acceptance checks before implementation. Table
  order does not approve APIs or require every extension.
- Before syntax/builtins, state the user problem, why ordinary library code is
  insufficient, and ownership, allocation, compilation and teaching costs.
- On completion, move the delivered scope to [Completed work](completed-work.md),
  update reference/status/runnable lessons, and leave only genuine outstanding
  extensions here with a link to their delivery record. Do not mark an entire
  mixed item done because its initial slice shipped.
- Issues, claims and review state stay in `issues.db`; reference pages describe
  implemented behavior. Use the [development workflow](development/workflow.md)
  for ongoing testing, documentation and performance practices.
- Historical proposals may contain old syntax/dependencies; introductory notices
  must say so. Future priorities belong here, not on completed-work pages.

A JIT and an interpreter remain out of scope, not waiting at the bottom of this
backlog. Async/await remains unadopted and unscheduled under B22's investigation gates.

## B18 — macOS and Windows implementation sequence

The [portability design](proposals/cross-platform.md) records rationale and
[toolchain evidence](proposals/cross-platform/experiments.md); this section owns
the work sequence. Target ARM64 macOS and x86-64 Windows, preserving both Linux
architectures. Use isolated child worktrees based on the portability branch,
with component ownership agreed before concurrent edits. No local Mac/Windows
machine is assumed; provision CI execution before treating either port as tested.

| Work package | Dependency / owned components | Completion evidence |
| --- | --- | --- |
| B18.1 — backend and toolchain decisions | Standalone probes; pinned Cranelift, Zig/cargo-zigbuild, C/Rust adapters; compare Windows MSVC and GNU | Real Mach-O/COFF output and native CI execution of mixed ABI/Tail/large-frame fixtures; record SDK/CRT/deployment choice and unwind strategy. Cross-link success alone is insufficient. |
| B18.2 — target and host separation | B18.1 findings; `toolchain`, frontend facts, cache/runtime identity | One target description, explicit build-host/compiler-host/output-target identities; incompatible combinations rejected; existing Linux suites pass. |
| B18.3 — object and linking interfaces | B18.2; object metadata/exports/source/trap/coverage records, drivers/distribution | ELF implementation preserved; Mach-O/COFF metadata, retention, exports and relocation round trips pass; no GNU options leak into other drivers. |
| B18.4 — dependency and runtime builds | B18.1–2; `build.rs`, native sources, test hooks, engine helper | Both runtime modes and native dependencies match target; Cargo and embedded builds agree; choose direct-build versus orchestrated Cargo using a measured prototype; no recursive Cargo lock contention. |
| B18.5 — runtime core | B18.3–4; memory, errors, entropy, threads, mutex/parking, signals/exceptions, actor recovery | Minimal native Plenty program, C callbacks, fallible allocation, secure randomness, stack protection and recovery tests pass on each OS; no success stubs or hidden allocation regressions. |
| B18.6 — OS contracts and services | Design decisions begin alongside B18.1; implementations depend on B18.5; paths/resources/files/process/readiness/watch | Lossless names, full process status, rooted filesystem security and resource-aware cancellation tests; explicit watch/shutdown capabilities; separate compatible portable contracts from Linux-only APIs. |
| B18.7 — library parity | B18.6; networking/DNS/TLS/HTTP, SQLite/KV, crypto/compression, dataframe/linalg | Real native execution, including both optional engines and CPU fallbacks; count skipped/early-return opt-ins separately; publish exact capability gaps before any preview. |
| B18.8 — developer workflows | B18.3–6; package CLI/cache/LSP/fuzz/export tools and tests | Extracted installation handles spaces/Unicode, literal arguments, replacement races, diagnostics and library hosts; explicitly report unqualified fuzz/sanitizer paths. |
| B18.9 — release admission | B18.7–8; native CI, packaging, notices, minimum-version audits, signing policy | Offline clean-machine compilation with documented prerequisites; tested target/engine artifacts, baseline OS/SDK/CRT evidence and signing/unsigned-preview policy. Preserve Linux ARM's outstanding hardware/release gates. |

Current evidence: the [native CI smoke suite](proposals/cross-platform/experiments.md#native-cranelift-ci-smoke-tests)
passes on macOS ARM64, Windows x86-64 MSVC and Linux x86-64. The first B18.2 increment centralizes
target facts and compiler-host/runtime validation; it does not implement new
object formats or OS services. This is a partial
B18.1 gate: large frames, unwind, Rust boundaries and runtime dependencies still
need qualification. Both new OS targets are experimental; neither is release-qualified. The shared Apple archive-command
fix addresses issue #600; native build-tool tests run before the OS port build
and on both Linux architectures. The first B18.5 increment separates native
mutex waiting and thread handles, hides cancellation wake storage, and adds
Darwin signal-context and stack-handler support. Standalone native synchronization
tests run before the complete compiler build. Windows exception dispatch and per-thread stack guarantees are implemented with
standalone subprocess tests that pass on the native Windows runner. Native OS
entropy fixes dictionary initialization on non-Linux hosts (issue #604); Windows
no-replace bundle publication and portable graph fixtures address issues #605
and #606. The initial B18.3 increment enables experimental Mach-O/COFF
application output, native C conventions and linker syntax, with framed Mach-O
interface metadata. Native qualification now passes full compiler/runtime builds,
unit tests, platform-boundary checks and generated-program callback/result probes
on macOS ARM64 and Windows x86-64. Linux x86-64 and ARM64 CI also passes all
integration shards, both optional engines and Docker proxy tests. Full new-OS
standard-library parity and release admission remain gates; these results do not
complete B18.5–9.

B18.1 and B18.6 design research can run together; B18.3 and B18.4 can proceed in
separate worktrees after the common description settles. Keep independent OS
implementations separate after their contracts are agreed. A failing native
probe changes the design before target admission. User-facing cross-compilation,
Intel macOS and Windows ARM64 remain separate decisions, not implied outcomes.
