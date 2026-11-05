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

def main() -> Result[(), IoError | OverflowError[i64]]:
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
would be; forwarding never moves or copies the wrapper. Overflow errors,
division rules, allocation failure, and other errors are those of the wrapped
operation. An operator the field type rejects is rejected with the field
type's diagnostic: a `list[i64]` wrapper has no `+`, and an `f64` wrapper has
no `%`. Forwarding allocates nothing beyond what the wrapped operation does.

Forwarded operations build the result through the class constructor, which is
why a `@newtype` class cannot define `__init__`. The declaration rules are in
[annotations](../03-types/classes.md#annotations).

## Equality, ordering and hashing

Equality, ordering and hashing never call user code:

- `==` and `!=` compare fields structurally, including inside collections.
- `<`, `<=`, `>` and `>=` on a class come only from
  [newtype forwarding](#newtype-forwarding).
- Dictionary keys and set elements use built-in structural hashing. Classes
  are not keys yet; named enums with supported key payloads are accepted.

A class, enum or protocol that declares `__eq__`, `__ne__`, `__lt__`,
`__le__`, `__gt__`, `__ge__`, `__cmp__`, `__hash__` or `__key__` is rejected,
so a Python-style hook cannot compile and then be ignored. Rename a method
that is not meant as a hook, or compare explicitly:

```plenty
class Money:
    cents: i64

    def same_amount(self, other: &Money) -> bool:
        self.cents == other.cents

def main() -> Result[(), IoError]:
    print(Money(1) == Money(2), Money(1).same_amount(&Money(1)))?
    Ok(())
```
```output
False True
```

User-defined ordering through one `__cmp__` hook is planned under
[B37](../../../backlog.md), and user-defined keys under
[B74](../../../backlog.md); see the [ordering](../../../proposals/user-type-ordering.md)
and [hashing](../../../proposals/user-type-hashing.md) proposals.

## Reserved method names

A method named `__name__`, with two underscores on each side, must be a hook
the compiler calls: an operator hook in the tables above, or, on a class,
`__init__`, `__del__`, `__enter__` or `__exit__`. A class, enum or protocol that declares any
other such name is rejected, so a Python special method cannot compile and
then be ignored. The names stay free for hooks the language may add. Where
Plenty has its own form, the diagnostic names it:

| Python method | Plenty form |
| --- | --- |
| `__str__`, `__repr__`, `__format__` | [`display` and `debug`](../../stdlib/fmt.md#static-user-contracts-and-aggregates) methods |
| `__pow__` | a `pow` method, called as `base.pow(exponent)` |
| `__iter__`, `__next__` | a [generator](../09-iteration/03-generators.md) method |
| `__len__`, `__contains__` | an ordinary shared `len` method for `len(value)`; explicit search methods for membership |
| `__bool__` | a predicate method such as `is_empty` |
| `__getitem__`, `__setitem__` | named methods such as `get` and `set` |

[`len(value)`](../../facilities/01-builtins/collections.md) calls an ordinary
shared `len(self) -> i64` method on classes and enums. Membership through `in`
and `not in` still uses only intrinsic operations; user-container membership
remains under [B49](../../../backlog.md). A name with underscores on one side
only, such as `__size`, is an ordinary method.

## Boundaries

Custom truthiness, indexing, membership, assignment, dereference, `**`, and
`@` operators are not implemented; their Python hooks are
[reserved](#reserved-method-names).
`and`, `or`, and `not` keep their boolean and short-circuit rules. Numeric
operators retain their overflow checks and division rules. Operators a newtype
forwards to an integer field report `OverflowError` of the field type through
the [function's declared Result](../03-types/numbers.md#function-arithmetic-contracts)
by default, through a local [`checked:` block](../03-types/numbers.md#checked-blocks),
or as a violated precondition in `unchecked:`, just like the field's own
operators. A user-defined
hook is an ordinary call with its own declared contract, independent of the
caller's mode; a fallible hook still needs explicit Result handling.


See the [operator tutorial](../../../tutorial/06-data/operators.md),
[assignment rules](assignment.md), and
[implementation and performance notes](operators-implementation.md).
