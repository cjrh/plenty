# Anonymous sums

`A | B` holds one owned value of either whole type. It works in ordinary
bindings, fields, arguments and returns, including `Result[T, A | B]` errors.
The compiler stores a tag and the largest active payload inline; wrapping and
widening allocate nothing. Payload construction retains its own costs.

Aliases, order, duplicates and nested sums normalize: `A | B`, `B | A` and
`(A | B) | A` are identical; `A | A` is just `A`. Distinct nominal classes and
enums remain distinct, and enum variants never become separate members.
There is no stable tag numbering or foreign layout contract.

Structural formatting (`print`, `str.repr`, and `std.fmt.represent`) shows the
active member directly, without an anonymous wrapper. For example, a
`Result[(), IoError | ThreadError]` error can be represented as
`Result[(), IoError | ThreadError].Err(ThreadError.System(11))`. Type names use
the normalized `A | B` spelling. The structural fallback for an error returned
by `main` follows the same rule; named enums keep their usual qualified variants.

A closure signature such as `OnceClosure[[], i64]` names no concrete
environment, so it is a member only in a function's declared return type,
directly or inside a member such as `SpawnError[OnceClosure[[], i64]]`. The
body's returned or propagated closure resolves it; see
[closure return inference](../06-functions/04-consuming-and-stored-closures.md).

`|` binds less tightly than references and type applications. Use `&(A | B)`
to borrow a sum. `&A | B` attempts to store a reference as a member and is
rejected, as are other members forbidden by owned storage rules. Callable
brackets delimit their types: `Callable[[A | B], C | D]`. Parentheses without
a comma group a type; `(A,)` is a singleton tuple and `()` is unit.

An expected type permits lossless injection of a member or widening to a
superset. This applies to annotated bindings, assignments, arguments,
constructors and returns. `Result[T, E]` can widen to `Result[T, F]` when
`F` contains every member of `E`; its success type must remain identical.
Other generic containers and references do not gain covariance. Conversions
evaluate their source once, move owned payloads and invoke no user conversion.
Public return types and binding types remain declared, never silently widened.
A constructor, tuple or collection literal takes its context from the one
member it can build, so `[1.0, 2]?` passed as `list[f64] | Unset` checks its
elements as `f64`, and `[]?` or `{}?` needs no further annotation. Several
viable members, such as `list[f64] | list[i64]`, are ambiguous and rejected;
bind the value with a member type first.

Match a whole member with `case name: MemberType:`. An ordinary wildcard
matches any remaining value. Each member must be covered, and duplicate or
unreachable arms are rejected. Match an enum payload's constructors in a
nested match. A member annotation must name a single normalized type; pattern
`|` still means alternatives and does not change into type union syntax.
Typed member patterns are supported at the case position, not inside other
patterns. Case guards remain unsupported; test payloads inside the arm.

An owned match moves noncopyable payloads. Matching `&value` or `&mut value`
borrows the active member with the same permission and lifetime. Only the
active payload is initialized or destroyed. Copyability and thread transfer
require every alternative to qualify. If generic substitution collapses
`T | A` to `A`, `case value: A:` binds that ordinary value; two equivalent
member arms are still duplicates. Generic inference rejects ambiguous member
substitutions; supply explicit type arguments in that case.

For reporting, a sum satisfies
[`std.report.Formattable`](../../stdlib/report.md) when every normalized member
satisfies that protocol. Formatting dispatch borrows the active member without
allocation or consuming the sum. A missing or mismatched method on any member
rejects the whole bound, regardless of which member is active.

This version does not infer residual sums after recovery, introduce empty
sums, forward arbitrary member methods or protocols, or erase errors. See
[propagation](../08-errors/01-sums-and-propagation.md) for `?` and builtin
`Failure`, the last-resort error that erases its cause.

The [anonymous-sum proposal](../../../proposals/completed/error-sets.md) records the
design rationale and migration boundary. The [composition overview](../../../proposals/completed/error-composition.md)
distinguishes sums from deliberate error mapping and typed stream APIs.
