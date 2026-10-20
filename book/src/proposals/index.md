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

## Find a design by topic

| Topic | Start here | Related decisions |
| --- | --- | --- |
| Protocol bodies, defaults, `final`, and private hooks | [Protocol method bodies](protocol-method-bodies.md) | [Agreed rules](protocol-method-bodies/final-and-visibility.md), [short programs](protocol-method-bodies/examples.md), [existing wrappers and capabilities](library-owned-operations.md) |
| Error composition | [Composition overview](error-composition.md) | [Anonymous sums and Failure migration](error-sets.md), [completed reporting](completed/error-formatting.md), [typed-stream evidence](error-composition/streams-and-evidence.md) |
| Ownership and state | [Typestate](typestate-and-owned-transitions.md) | [Linear obligations](linear-obligations.md), [resolution scopes](linear-scope-resolution.md), [integration boundaries](linear-integration.md) |
| Linked structures and graphs | [Owned structures and checked identities](arenas.md) | [Recursive representation](recursive-data.md), [arena prototype](arenas/prototype.md) |
| Collections | [Ordering](user-type-ordering.md) | [Stable sorting algorithm](timsort.md), [hashing and equality](user-type-hashing.md) |
| Native platforms and libraries | [Cross-platform assessment](cross-platform.md) | [FFI](ffi-and-dynamic-libraries.md), [export contracts](ffi-export-contracts.md), [GCC backend](codegen-gcc.md) |
| Allocation and parallel execution | [Memory, parallelism, and SIMD](memory-parallelism-and-simd.md) | [Allocation audit](completed/allocation-audit.md), [shipped concurrency](completed/native-concurrency.md) |

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
| [Explicit applications, modules, and executable lessons](entrypoints-modules-and-tutorials.md) | [Entry point](../design/language/02-source/entrypoint.md), [modules](../design/language/02-source/modules.md), executed Markdown fences | B17, B21 |
| [C interop and dynamic libraries](ffi-and-dynamic-libraries.md) | [Imports, linking, exports, and resident loading](../design/facilities/04-interop/index.md) | B18, B27, B28 |
| [Generated library contracts and C headers](ffi-export-contracts.md) | [Exports](../design/facilities/04-interop/exports.md), [contracts](../design/facilities/04-interop/contracts.md) | B27, B28 |
| [Fallible memory, parallel execution, and SIMD](memory-parallelism-and-simd.md) | [Fallible allocation](../design/language/08-errors/02-allocation-and-traps.md), [concurrency](../design/facilities/03-concurrency/index.md) | B13, B23, B24 |
| [Plenty's next language phase](next-language-phase.md) | Nine of eleven recommendations, at least in part | B13, B23, and others |
| [Standard library scope](standard-library-scope.md) | [Embedded library](../design/stdlib/index.md), including data formats, TLS, HTTP, SQLite, signals and testing | Remaining B38–B70 scope |
| [Keyword arguments, defaults, and static unpacking](keyword-arguments-and-unpacking.md) | [Named calls and fresh defaults](completed/named-arguments-and-fresh-defaults.md) under the narrower design | B36: splats and signature markers remain separate candidates |
| [Explicit error composition](error-composition.md) | [Result mapping](../design/stdlib/error.md), anonymous sums, reporting, [typed I/O](../design/stdlib/io-typed.md) and [typed HTTP bodies](../design/stdlib/http-typed.md) | B71: Failure removal and broader production stream composition |
| [Finite error sets as anonymous sums](error-sets.md) | [Normalization, injection, widening, and typed-member matching](../design/language/03-types/anonymous-sums.md) | B14/B71: builtin Failure migration; residual-set inference and guards remain deferred |
| [Bundled HTTP acceptance](standard-http.md) | [Client](../design/stdlib/http-client.md), [server](../design/stdlib/http-server.md), TLS and deployment helpers | B66 qualification |

## Not implemented

| Proposal | Backlog |
| --- | --- |
| [Linear obligations](linear-obligations.md), including [aggregate and error rules](linear-obligations/values-and-errors.md) — opt-in non-abandonment | B72 (issue #149) |
| [Synchronous resolution scopes](linear-scope-resolution.md) — consuming scope policy; depends on linear obligations | B72 (issue #149) |
| [Linearity, typestate, and execution responsibility](linear-integration.md) — shared boundaries and deferred worker/async contracts | B72 (issue #149) |
| [GCC backend (`plenty_codegen_gcc`)](codegen-gcc.md) | B20 |
| [User-defined ordering](user-type-ordering.md) — one three-way `__cmp__` hook | B37 |
| [Timsort for stable, adaptive sorting](timsort.md) — fallible workspace, stability, and compatibility choices | B37 (issue #331) |
| [User-defined dictionary keys and set members](user-type-hashing.md) — derived structural hashing and a `KeyType` bound | None yet |

## Completed investigations

| Assessment | Decision / related backlog |
| --- | --- |
| [Native macOS, Windows, and AArch64 support](cross-platform.md) | Native ports need target-aware builds, ABI validation and OS/stdlib contracts before release packaging. B18, with B17/B12 dependencies. |
| [Library-owned operations and backend hooks](library-owned-operations.md) | Existing composition and optional capabilities; new protocol access deferred. B29 (issue #145). |
| [Protocol method bodies](protocol-method-bodies.md) | Core default/final dispatch and member-specific grants agreed; definition-time contract checking required. No automatic method injection. Not implemented or scheduled (B29). |
| [Owned structures and checked node identities](arenas.md) | Tested patterns and arena prototype; production identity/access contract remains B73, broader borrowing B11. |
| [Typestate and owned transitions](typestate-and-owned-transitions.md) | Existing owned transitions and their limits; linear non-abandonment is a separate proposed extension (B72). |
