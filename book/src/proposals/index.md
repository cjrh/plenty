# Proposals

Proposals record why a design was chosen and what was rejected. They do not
specify the language. The [reference](../design/index.md) defines current
behavior, and the [backlog](../backlog.md) is the only list of future work.
Where a proposal and the reference disagree, the reference is correct.

## Status

Every proposal opens with a status block. It gives one status, the reference
pages that own the contract, what shipped differently from the text, and what
remains with its backlog ID.

| Status | Meaning |
| --- | --- |
| Implemented | The design shipped. The page keeps the rationale. |
| Partly implemented | Some of it shipped. The block says which parts. |
| Not implemented | Nothing shipped. The page is the design record for future work. |

Below its status block a proposal keeps its original wording. It may say
"should" or "add" about things that now exist, and its code blocks may show
syntax that never shipped. Proposal code is not compiled by the test suite.

A change that implements or alters a proposed design updates that proposal's
status block in the same change. A proposal that is superseded or abandoned, or
that only records completed work, is deleted; Git history keeps it. Statuses
were last scanned against merged implementation work on 2026-10-17.

## Implemented

| Proposal | Reference | Remaining |
| --- | --- | --- |
| [Runtime allocation audit](allocation-audit.md) | [Allocation and traps](../design/language/08-errors/02-allocation-and-traps.md) | B13 |
| [References, ownership, and explicit copying](references-and-copying.md) | [Ownership](../design/language/05-ownership/index.md) | B11 |
| [One immutable `str` with explicit lengths](strings.md) | [Text](../design/facilities/01-builtins/text.md) | B12, B27 |
| [Sum types and enums](sum-types.md) | [Enums](../design/language/03-types/enums.md), [matching](../design/language/04-expressions/matching.md) | B19, B33 |
| [Generics, structural protocols, context managers, and error propagation](protocols-generics-and-context-managers.md) | [Generics](../design/language/07-generics/index.md), [resource scopes](../design/language/08-errors/03-resource-scopes.md) | B11, B14, B29 |
| [Native owned generators](generators.md) | [Generators](../design/language/09-iteration/03-generators.md) | Unscheduled |
| [Allocation-free generator frames and ranges](allocation-free-generators.md) | [Generators](../design/language/09-iteration/03-generators.md), [ranges](../design/facilities/01-builtins/ranges.md) | B12 |
| [Callable surface review](callable-simplicity.md) | [Functions](../design/language/06-functions/index.md) | B32 |
| [Stored closure environments](stored-closures.md) | [Consuming and stored closures](../design/language/06-functions/04-consuming-and-stored-closures.md) | B11, B32 |
| [Native concurrency contract](native-concurrency.md) | [Concurrency](../design/facilities/03-concurrency/index.md) | B13, B24, B28 |
| [Bounded channel implementation](bounded-channels.md) | [Channels](../design/facilities/03-concurrency/channels.md) | Deferred B22 extensions |
| [Named arguments and fresh defaults](named-arguments-and-fresh-defaults.md) | [Signatures and calls](../design/language/06-functions/01-signatures-and-calls.md) | B36 refinements; unpacking remains separate |

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
| [Keyword arguments, defaults, and static unpacking](keyword-arguments-and-unpacking.md) | Named calls and fresh defaults under the narrower design above | B36: splats and signature markers remain separate candidates |
| [Explicit error composition](error-composition.md) | [Result mapping](../design/stdlib/error.md); isolated typed-stream experiment | B71: production stream composition and broader framing |
| [Bundled HTTP acceptance](standard-http.md) | [Client](../design/stdlib/http-client.md), [server](../design/stdlib/http-server.md), TLS and deployment helpers | B66 qualification |

## Not implemented

| Proposal | Backlog |
| --- | --- |
| [Finite error sets](error-sets.md) | B71 |
| [Typestate and owned transitions](typestate-and-owned-transitions.md) — assessment and experiments using existing facilities | B72 |
| [GCC backend (`plenty_codegen_gcc`)](codegen-gcc.md) | B20 |
