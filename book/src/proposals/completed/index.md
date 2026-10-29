# Completed proposals

These proposals have shipped within their agreed scope. They preserve design
rationale and alternatives; the [reference](../../design/index.md) and
[tutorial](../../tutorial/index.md) describe current behavior. Further extensions
listed below do not make the completed scope pending again. The
[backlog](../../backlog.md) remains the only work queue.

See the [proposal index](../index.md) for partially implemented and unimplemented
designs and the policy for moving completed proposals.

| Proposal | Reference | Further work |
| --- | --- | --- |
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
| [Checked arithmetic blocks](checked-arithmetic.md) | [Numbers](../../design/language/03-types/numbers.md#checked-blocks) | Checked shifts and casts unscheduled; class patterns landed with issue #558 |
