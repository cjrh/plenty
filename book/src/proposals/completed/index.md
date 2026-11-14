# Completed proposals

These proposals have shipped within their agreed scope, or completed the
experiment or investigation identified in their entry, including explicitly
rejected designs. They preserve design rationale and alternatives; the [reference](../../design/index.md) and
[tutorial](../../tutorial/index.md) describe current behavior. Further extensions
listed below do not make the completed scope pending again. The
[backlog](../../backlog.md) remains the only work queue.

See the [proposal index](../index.md) for partially implemented and unimplemented
designs and the policy for moving completed proposals.

| Proposal | Reference | Further work |
| --- | --- | --- |
| [Derived serialization for classes and enums](derived-serialization.md) | [`std.serial`](../../design/stdlib/serial.md), [derivation](../../design/stdlib/serial-derive.md), [typed JSON](../../design/stdlib/json-codecs.md), [typed TOML](../../design/stdlib/toml-codecs.md) | B77 / issue #781 delivered; reflection, macros and other formats are separate designs |
| [User-defined dictionary keys and set members](user-type-hashing.md) | [Collections](../../design/facilities/01-builtins/collections.md), [`KeyType`](../../design/language/07-generics/index.md#keytype) | B74 structural-key policy delivered; optional hash caching/key projections remain separate candidates |
| [Checked arithmetic by default](checked-by-default.md) | [Function arithmetic contracts](../../design/language/03-types/numbers.md#function-arithmetic-contracts), [bounded validation](../../completed-work/checked-default-validation.md) | B14: unchecked configuration/audit and further numeric/error conveniences; no universal performance or ABI claim |
| [Dataframes backed by Polars](polars-dataframes.md) | [`std.dataframe`](../../design/stdlib/dataframe.md), [engine deployment](../../design/stdlib/dataframe-engine.md) | B76 extensions; completed scope is the optional engine foundation, not every Polars API or a replacement for `clist` |
| [Boxed protocol objects](boxed-protocols.md) | [Protocol objects](../../design/language/07-generics/boxed-protocols.md) | Further protocol capabilities remain separate from the delivered erased-owner contract |
| [Positive and NonNegative integer domain types](domain-types.md) | [Numbers](../../design/language/03-types/numbers.md#positive-and-non-negative-integers), [std conventions](../../design/stdlib/conventions.md#invalid-arguments) | B96 `Bounded`; issue #686 |
| [Multiple versions of transitive dependencies](multiversion-dependencies.md) | [Packages, identity, updates and inspection](../../design/toolchain/01-building/packages.md#dependencies) | B17: registry/range selection, shared per-user cache and other package tooling remain separate |
| [Single-file scripts](single-file-scripts.md) | [Script runner](../../design/toolchain/01-building/scripts.md) | B17/B18: wider executable-cache coverage |
| [Library-owned operations and backend hooks](library-owned-operations.md) — investigation complete; no language change | [Protocols](../../design/language/07-generics/protocols.md), [visibility](../../design/language/02-source/visibility.md) | B29; future default/final methods and private-hook grants belong to [Protocol method bodies](../protocol-method-bodies.md) |
| [Fluent iterator pipelines](fluent-iterators.md) | [Pipelines](../../design/language/09-iteration/06-pipelines.md) | B75 extensions; related B12, B22, B24 |
| [Owned response experiment](typestate-response-experiment.md) | [Field moves](../../design/language/05-ownership/01-bindings-moves-and-copies.md#moving-fields) | B72: real HTTP integration remains unexplored |
| [Flat class layout](flat-class-layout.md) | [Value representations](../../design/toolchain/02-architecture/values.md), [memory](../../design/toolchain/02-architecture/memory.md) | B19 performance extensions; related B35 |
| [Runtime allocation audit](allocation-audit.md) | [Allocation and traps](../../design/language/08-errors/02-allocation-and-traps.md) | B13 |
| [References, ownership, and explicit copying](references-and-copying.md) | [Ownership](../../design/language/05-ownership/index.md) | B11 |
| [One immutable `str` with explicit lengths](strings.md) | [Text](../../design/facilities/01-builtins/text.md) | B12, B27 |
| [Sum types and enums](sum-types.md) | [Enums](../../design/language/03-types/enums.md), [matching](../../design/language/04-expressions/matching.md) | B19, B33 |
| [Generics, structural protocols, context managers, and error propagation](protocols-generics-and-context-managers.md) | [Generics](../../design/language/07-generics/index.md), [resource scopes](../../design/language/08-errors/03-resource-scopes.md) | B11, B14, B29 |
| [Native owned generators](generators.md) | [Generators](../../design/language/09-iteration/03-generators.md) | Unscheduled |
| [Allocation-free generator frames and ranges](allocation-free-generators.md) | [Generators](../../design/language/09-iteration/03-generators.md), [ranges](../../design/facilities/01-builtins/ranges.md) | B12 |
| [Callable surface review](callable-simplicity.md) | [Functions](../../design/language/06-functions/index.md) | B32 |
| [Stored closure environments](stored-closures.md) | [Consuming and stored closures](../../design/language/06-functions/04-consuming-and-stored-closures.md) | B11, B32 |
| [Native concurrency contract](native-concurrency.md) | [Concurrency](../../design/facilities/03-concurrency/index.md) | B13, B24, B28 |
| [Bounded channel implementation](bounded-channels.md) | [Channels](../../design/facilities/03-concurrency/channels.md) | Deferred B22 extensions |
| [Named arguments and fresh defaults](named-arguments-and-fresh-defaults.md) | [Signatures and calls](../../design/language/06-functions/01-signatures-and-calls.md) | B36 refinements; unpacking remains separate |
| [Human-readable formatting and error reports](error-formatting.md) | [`std.report`](../../design/stdlib/report.md), [entry reports](../../design/toolchain/01-building/entry-report.md) | B71 extensions; `Failure` remains a deliberate last resort |
| [Explicit error composition](error-composition.md), with [experiment and production evidence](error-composition/streams-and-evidence.md) | [`std.error`](../../design/stdlib/error.md), [`std.io.typed`](../../design/stdlib/io-typed.md), [`std.http.typed`](../../design/stdlib/http-typed.md), [HTTP client](../../design/stdlib/http-client.md) | B71 additive extensions and deferred benchmark-template migration |
| [Finite error sets as anonymous sums](error-sets.md) | [Anonymous sums](../../design/language/03-types/anonymous-sums.md), [propagation](../../design/language/08-errors/01-sums-and-propagation.md) | B14/B71: guards, residual-set inference and empty sums |
| [Bundled HTTP acceptance](standard-http.md) | [HTTP qualification](../../design/stdlib/http-qualification.md), [client](../../design/stdlib/http-client.md), [server](../../design/stdlib/http-server.md) | Preserve B66 gates; additional platforms and HTTP/2 or HTTP/3 are separate scope |
| [Explicit applications, modules, and executable lessons](entrypoints-modules-and-tutorials.md) | [Entry point](../../design/language/02-source/entrypoint.md), [modules](../../design/language/02-source/modules.md), [runnable documentation](../../development/workflow.md#runnable-documentation) | B17: re-exports and module caching; literate lesson sources not adopted |
| [Plenty's next language phase](next-language-phase.md), a design review | Per topic in its outcome table | B13, B23, and the extensions it lists |
| [Python slice syntax](slice-syntax.md), with its [implementation plan](slice-syntax/plan.md) | [Slices](../../design/language/04-expressions/slices.md) | B39: borrowed views `&xs[a:b]` (issue #581); issue #583 |
| [Checked arithmetic blocks](checked-arithmetic.md) | [Numbers](../../design/language/03-types/numbers.md#checked-blocks) | Checked shifts and casts unscheduled; class patterns landed with issue #558 |
| [Rust compiler boundaries and measured performance](compiler-refactoring.md) — steps 1–6 and legacy-syntax removal | [Frontend](../../design/toolchain/02-architecture/frontend.md), [cache](../../design/toolchain/02-architecture/cache.md) | B17 compilation session; B19 module replay and frontend candidates |

## Completed investigations

These records conclude an assessment, not delivery of every possible feature it discusses.

| Assessment | Outcome | Further work |
| --- | --- | --- |
| [Owned structures and checked node identities](arenas.md), with the [arena prototype](arenas/prototype.md) | Existing-language patterns and prototype assessed; no `std.arena` API delivered | B73 identity/access qualification and B11 broader borrowing |
| [Typestate and owned transitions](typestate-and-owned-transitions.md) | Existing owned transitions and their limits assessed; complemented by the [owned response experiment](typestate-response-experiment.md) | B72 state/ownership ergonomics; [linear obligations](../linear-obligations.md) remain proposed |
| [Thread-local reference counts](thread-local-reference-counts.md) | Stage 1 measured and rejected without a cycle gain; local PR #944 closed unmerged, stage 2 not recommended | Revisit only with new platform/workload evidence; no non-atomic count mode shipped |

## Rejected proposals

These designs were closed without further implementation planned. Their
experiments and design discussion remain historical records.

| Proposal | Decision | Related work |
| --- | --- | --- |
| [Compiling Plenty to Rust source](rust-source-backend.md), with its semantic contract, probe evidence and checked-emitter measurements | Rejected at the user's direction on 2026-11-14; no further implementation planned | B20 retains the GCC assessment; [Rust dependency integration](../rust-crate-dependencies.md) remains B17 |
