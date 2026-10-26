# User-defined arithmetic operators

Classes and user-defined enums can implement arithmetic with Python-style
special methods. The compiler selects the method statically and emits an ordinary
direct call. There is no runtime lookup, boxing, or allocation for dispatch.

| Operator | Left method | Reflected method | In-place method |
| --- | --- | --- | --- |
| `+` | `__add__` | `__radd__` | `__iadd__` |
| `-` | `__sub__` | `__rsub__` | `__isub__` |
| `*` | `__mul__` | `__rmul__` | `__imul__` |
| `/` | `__truediv__` | `__rtruediv__` | `__itruediv__` |
| `//` | `__floordiv__` | `__rfloordiv__` | `__ifloordiv__` |
| `%` | `__mod__` | `__rmod__` | `__imod__` |
| `&` | `__and__` | `__rand__` | `__iand__` |
| `\|` | `__or__` | `__ror__` | `__ior__` |
| `^` | `__xor__` | `__rxor__` | `__ixor__` |
| `<<` | `__lshift__` | `__rlshift__` | `__ilshift__` |
| `>>` | `__rshift__` | `__rrshift__` | `__irshift__` |

Unary `+`, `-`, and `~` call `__pos__`, `__neg__`, and `__invert__`.
Precedence and associativity are unchanged.

## Signatures and resolution

A value-producing hook has a shared `self`, exactly one other parameter for
binary operators, and no other parameters for unary operators. The other
operand is a shared reference or copyable value. Its type can differ from the
receiver's. The result is an owned value of any supported return type, including
`Result`; unit and reference results are rejected. Hooks cannot declare method
type parameters, parameter defaults, or yield. Generic classes and ordinary
generic functions can use operators after specialization.

For `a + b`, the compiler selects `a.__add__` if declared; otherwise it selects
`b.__radd__` if declared. A selected hook's signature must match: a type error
does not cause fallback. There is no dynamic `NotImplemented` protocol, subtype
priority, or implicit conversion. A reflected method is allowed for operands
of the same type when the left method is absent. Ordinary member visibility
applies: exported APIs need `pub` hooks.

Both operands are evaluated exactly once, left to right, even for a reflected
call. Shared operands stay borrowed through the call. Temporary owners survive
the full expression and are destroyed normally; no implicit deep copy occurs.
Methods can still be called explicitly.

An operation that allocates should return `Result`, just as a named method does.
Write `(a + b)?` to propagate failure: `?` binds more tightly than `+`.
The operator syntax does not automatically propagate, unwrap, or discard errors.

## In-place operations

`target += value` calls `target.__iadd__(value)`; the other in-place names follow
the table. Its signature requires `self: &mut T`, one shared or copyable operand,
and `-> ()`. It mutates the existing destination and does not replace it with a
returned owner. The target must be a writable binding, field, collection element,
or exclusive reference. Its indices are evaluated once before the right side.
The ordinary mutable-method receiver reservation prevents invalidating that place
while evaluating arguments.

There is no fallback from `+=` to `+`. For an operation that creates a new value
or can fail, use explicit assignment (`a = (a + b)?`) or a named fallible method.
This keeps allocation and error handling visible. An in-place hook can perform
explicitly handled allocation inside its body; dispatch itself allocates nothing.

## Boundaries

Equality remains structural, including inside collections. `__eq__` and
`__ne__` are ordinary methods, not hooks. Rich comparisons and the proposed
`__cmp__` ordering contract are not implemented. Nor are custom truthiness,
indexing, membership, assignment, dereference, `**`, or `@` operators.
`and`, `or`, and `not` keep their boolean and short-circuit rules. Numeric
operators retain their existing overflow checks and division rules.

See the [operator tutorial](../../../tutorial/06-data/operators.md),
[assignment rules](assignment.md), and
[implementation and performance notes](operators-implementation.md).
