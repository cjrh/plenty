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
matches any remaining value. A member annotation names a single normalized
type; pattern `|` means alternatives, not a type union. Typed member patterns
are supported at the case position, not inside other patterns.

An enum variant pattern also selects its member directly, including inside
`Err` or another payload. The compiler tests the member before testing its
variant, so equal variant names in different enums remain distinct:

```plenty
def parse(text: &str) -> Result[i64, AllocError | ParseError]:
    Ok(i64.parse(text)?)

def main() -> Result[(), IoError]:
    match parse("x"):
        case Ok(value):
            print(value)?
        case Err(ParseError.Invalid):
            print("not a number")?
        case Err(ParseError.OutOfRange):
            print("outside i64 range")?
        case Err(_):
            print("out of memory")?
    Ok(())
```
```output
not a number
```

Every member and each enum member's variants must be covered, whether by
variant patterns, a whole-member pattern, or a wildcard. Duplicate and
unreachable cases are rejected; covering one variant does not cover its
siblings. Payload literals still need a fallback for the other payload values.

The matched value supplies generic arguments, as in ordinary enum patterns.
If a qualifier names several members, such as `Choice[i64] | Choice[str]`,
`Choice.Value(value)` is ambiguous even when only one payload type would fit
its subpattern. Use a whole-member pattern with an explicit type and match
that value's variants inside the arm. Case guards remain unsupported.

An owned match moves noncopyable payloads. Matching `&value` or `&mut value`
borrows the active member with the same permission and lifetime. Only the
active payload is initialized or destroyed. Copyability and thread transfer
require every alternative to qualify. If generic substitution collapses
`T | A` to `A`, `case value: A:` binds that ordinary value; two equivalent
member arms are still duplicates. Generic inference rejects ambiguous member
substitutions; supply explicit type arguments in that case.

For reporting, a sum satisfies
[`std.fmt.Formattable`](../../stdlib/report.md) when every normalized member
satisfies that protocol. Formatting dispatch borrows the active member without
allocation or consuming the sum. A missing or mismatched method on any member
rejects the whole bound, regardless of which member is active.

This version does not infer residual sums after recovery (a
[residual member pattern](#residual-members) states one explicitly), introduce
empty sums, forward arbitrary member methods or protocols, or erase errors. See
[propagation](../08-errors/01-sums-and-propagation.md) for `?` and builtin
`Failure`, the last-resort error that erases its cause.

The [anonymous-sum proposal](../../../proposals/completed/error-sets.md) records the
design rationale and migration boundary. The [composition overview](../../../proposals/completed/error-composition.md)
distinguishes sums from deliberate error mapping and typed stream APIs.

## Residual members

After cases that handle some members, `case rest: B | C:` matches every
remaining value and binds it as the smaller sum `B | C`. Use it to recover
from some errors and forward the others:

```plenty
enum A:
    Bad
enum B:
    Bad
enum C:
    Worse
    Bad

def narrow(error: A | B | C) -> Result[(), B | C]:
    match error:
        case handled: A:
            Ok(())
        case rest: B | C:
            Err(rest)

def main() -> Result[(), AllocError | IoError]:
    print(str.repr(&narrow(A.Bad))?)?
    print(str.repr(&narrow(C.Bad))?)?
    Ok(())
```
```output
Result[(), B | C].Ok(())
Result[(), B | C].Err(C.Bad)
```

The written members must be exactly those the earlier cases leave; otherwise
the diagnostic names the correct set. A member is handled only when earlier
cases cover all of its values, by a whole-member pattern or by covering every
variant (and payload). Matching `case C.Worse:` alone leaves `C` in the
residual. A member type written in the case must belong to the matched sum.

The members normalize like any sum. When one member remains, the ordinary
`case rest: B:` is the residual case; `B | C` written for a single remaining
`B` is rejected. In generic code, substitution can collapse a written
`T | C` to `C`, which then matches as an ordinary member pattern. The check
runs on each instance, so the same case rejects an instance whose `T` is a
member that earlier cases already handle. A residual
case with nothing left is an unreachable case; there are no empty sums.
Written first, a residual naming every member binds the whole value.

A residual case appears only at the case position of a matched sum: not inside
`Err(...)` or another payload, and not as a `|` alternative. Match the payload
with a nested `match`, as the [tutorial](../../../tutorial/03-absence/anonymous-sums.md#recover-from-one-error-and-forward-the-rest)
does. An owned match moves the active payload into the smaller sum's layout
without allocating; only that payload is later destroyed. A borrowed match
(`match &value`) is rejected when the residual is smaller than the matched
sum, because the smaller sum has a different tag layout and a reference cannot
be narrowed in place. Bind `_ as rest` to borrow the whole sum instead.

## Collection keys

A sum satisfies `KeyType` when every member is a [supported key type](../../facilities/01-builtins/collections.md).
Use `set[i64 | str]` or `dict[i64 | str, V]` directly. Hashing and equality
include the active member type and its structural value: `1`, `1i8`, `True`
and `"1"` stay distinct in a sum that admits them. Normalized aliases and
member order do not change identity. Keys with an unsupported member are
rejected even if that member is never inserted.

Insertion and lookup use the ordinary expected-type injection rules. Observed member values can be injected for lookup without moving them; their
source remains borrowed through the operation. This does not make reference
types covariant. Sums containing move-only classes keep their ownership rules, and
stored keys are only exposed through shared borrows. Anonymous sums use the
hash-table representation, including sums with finite members.
