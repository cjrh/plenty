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
| [Human-readable formatting and error reports](error-formatting.md) | [`std.report`](../../design/stdlib/report.md), [entry reports](../../design/toolchain/01-building/entry-report.md) | B71 extensions; builtin Failure removal is a separate migration |
