# Anonymous sums

`A | B` holds one owned value of either whole type. It works in ordinary
bindings, fields, arguments and returns, including `Result[T, A | B]` errors.
The compiler stores a tag and the largest active payload inline; wrapping and
widening allocate nothing. Payload construction retains its own costs.

Aliases, order, duplicates and nested sums normalize: `A | B`, `B | A` and
`(A | B) | A` are identical; `A | A` is just `A`. Distinct nominal classes and
enums remain distinct, and enum variants never become separate members.
There is no stable tag numbering or foreign layout contract.

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

This version does not infer residual sums after recovery, introduce empty
sums, forward member methods, or erase errors. See
[propagation](../08-errors/01-sums-and-propagation.md) for `?` and the temporary
`Failure` migration behavior.
