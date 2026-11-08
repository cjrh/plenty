# Type argument inference

Calls supply all generic arguments, as in `identity[list[i64]](values)`, or infer all
from their arguments and expected result, as in `identity(values)`. Partial
explicit lists are unsupported. An expected instance of a generic class supplies
its constructor's arguments; see [generic data](data.md).

Inference structurally matches concrete arguments: `&T` against `&Message` learns
`T = Message`; `&dict[K, V]` against `&dict[str, u8]` learns both parameters.
Collections, ranges, generators, tuples, sums, callable signatures, and nominal
generic class/enum arguments participate. Repeated parameters must agree after
alias resolution. Nominal matching requires the same declaration as well as
matching arguments. [Value parameters](values.md) are inferred from nominal
arguments by the same structural match: `Bounded[T, lo, hi]` against
`Bounded[u8, 0, 100]` learns all three parameters.

A shared-reference parameter supplies ordinary implicit borrowing, so a
parameter `value: &T` can accept an owner directly. Mutable borrowing still needs
`&mut` or an existing exclusive reference. Arguments evaluate once in source
order.

## Expected results

A concrete expected result type is evidence too. It comes from an annotated
binding, a return type, a concrete parameter, a field, or an enclosing
`Some`, `Ok`, or `?`, and it applies to generic functions and generic methods
alike. The declared output is matched against it, as an argument is matched
against its parameter:

```plenty
def empty[T]() -> Result[list[T], AllocError]:
    []

def main() -> Result[(), AllocError | IoError]:
    values: list[u8] = empty()?
    print(values)?
    Ok(())
```
```output
[]
```

A `?` operand's expected success payload is the binding's type, so
`empty()?` learns `T = u8` from `list[u8]`. Only exact positions are
evidence. A value widens into an anonymous sum, so a parameter matched
against one learns nothing: `x: Option[u8] | str = none()` still needs
`none[u8]()`. A `?` operand's expected error is the enclosing function's error
type, so it is evidence only when that type is not a sum. With no expected type,
as in `x = none()`, the call needs explicit arguments.

Evidence from arguments, constraints, and the expected result must agree. A
disagreement is an error, never a choice between them: `x: u8 = same(n)` for
an `i64` `n` reports that `T` is `i64` from the arguments but `u8` from the
expected result.

## Literals

A generic parameter position whose type the expected result determines takes
that type as its context, as a concrete parameter does: `k: u8 = same(3)` and
`ys: list[u8] = repeat(7, 2)?` type their literals as `u8`, and
`id(empty()?)` passes the expectation on to the nested call. Any other generic
position supplies no expected type to its argument. Unsuffixed integers there
default to `i64` and floats to `f64`, independently of other arguments.
`add(1u8, 2u8)` works; `add(1u8, 2)` conflicts. `add[u8](1, 2)` guides both
literals. Nongeneric parameter positions retain
their ordinary contextual typing. Empty literals and incomplete sum constructors
may need an annotated binding or explicit arguments.

## Constraints and function values

Callable constraints contribute evidence from a known callback signature:
`F: Callable[[], T]` can determine `T` from the callback result. Parameterized
protocol bounds likewise contribute evidence from accessible methods on the known
class. Evidence from all inputs and constraints must agree. Inference does not
widen numbers or search for an implementation class.

A generic function becomes a concrete function value using `identity[u8]` or
an expected concrete `Callable` signature. A fully concrete callable constraint
can also select a generic named function passed directly by value. This contextual
selection does not change ordinary generic-call inference. `OnceCallable`
constraints accept consuming callbacks with the same inference rules. Constraint
signature shapes are checked at declaration; concrete ownership capabilities are
checked when the body is specialized. Polymorphic function values are unsupported.
