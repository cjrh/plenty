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

## Newtype forwarding

A class annotated `@newtype` wraps exactly one field and forwards every
operator that the field's type supports, so a wrapper keeps its field's
arithmetic without one hook per operator:

```plenty
@newtype
class Metres:
    value: i64

def main() -> Result[(), IoError]:
    mut total = Metres(3) + Metres(4)
    total *= Metres(2)
    print(total, -total, total > Metres(10))?
    Ok(())
```
```output
Metres(value=14) Metres(value=-14) True
```

`a + b` on two `Metres` values lowers to `a.value + b.value` through the
ordinary operator rules for the field type, then wraps the result:

- A result of the field's type becomes the class: `Metres + Metres` is a
  `Metres`.
- A `Result[F, E]` result, where `F` is the field type, becomes
  `Result[Class, E]`. `@newtype class Name: text: str` gives
  `Name + Name -> Result[Name, AllocError]`, written `(a + b)?`.
- Any other result is returned as is, such as the `f64` from a field hook
  `__mul__(self, other: &Vec2) -> f64`.
- `<`, `<=`, `>`, and `>=` forward and return `bool`. `==` and `!=` stay
  structural, as for every class.
- Unary `+`, `-`, and `~` forward in the same way.
- `target += value` performs the field's own in-place operation,
  `target.value += value.value`, so it follows the field type's `+=`.

Forwarding happens only when no hook is selected and both operands have the
same `@newtype` class. Typing stays nominal: `Metres + i64`, `i64 + Metres`,
and `Metres + Feet` are errors unless a hook accepts them; there is no
implicit conversion. An explicit hook replaces forwarding for its operator
only: a `@newtype` class with `__add__` uses it for `+` and still forwards `-`.
Reflected hooks apply only to mixed operand types, so they never compete with
forwarding.

Each operand is evaluated once, left to right, and borrowed as a hook operand
would be; forwarding never moves or copies the wrapper. Overflow traps,
division rules, allocation failure, and other errors are those of the wrapped
operation. An operator the field type rejects is rejected with the field
type's diagnostic: a `list[i64]` wrapper has no `+`, and an `f64` wrapper has
no `%`. Forwarding allocates nothing beyond what the wrapped operation does.

Forwarded operations build the result through the class constructor, which is
why a `@newtype` class cannot define `__init__`. The declaration rules are in
[annotations](../03-types/classes.md#annotations).

## Boundaries

Equality remains structural, including inside collections. `__eq__` and
`__ne__` are ordinary methods, not hooks. Rich comparison hooks and the
proposed `__cmp__` ordering contract are not implemented; only
[newtype forwarding](#newtype-forwarding) provides ordering for a class. Nor are custom truthiness,
indexing, membership, assignment, dereference, `**`, or `@` operators.
`and`, `or`, and `not` keep their boolean and short-circuit rules. Numeric
operators retain their existing overflow checks and division rules. Inside a
[`checked:` block](../03-types/numbers.md#checked-blocks), operators a newtype
forwards to an integer field are checked like the field's own, and report
`OverflowError` of the field type; a user-defined hook is an ordinary call and
is not checked.


See the [operator tutorial](../../../tutorial/06-data/operators.md),
[assignment rules](assignment.md), and
[implementation and performance notes](operators-implementation.md).
