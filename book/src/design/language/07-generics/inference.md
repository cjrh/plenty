# Type argument inference

Calls supply all generic arguments, as in `identity[list[i64]](values)`, or infer all
from inputs, as in `identity(values)`. Partial explicit lists are unsupported.
A generic function's expected result type does not select its arguments, so its
output-only or unused parameters need an explicit list. A generic class
constructor is the exception: an expected instance of the class supplies its
arguments; see [generic data](data.md).

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

A generic parameter position supplies no expected type to its argument.
Unsuffixed integers default to `i64` and floats to `f64`, independently of other
arguments or the result context. `add(1u8, 2u8)` works; `add(1u8, 2)` conflicts.
`add[u8](1, 2)` guides both literals. Nongeneric parameter positions retain their
ordinary contextual typing. Empty literals and incomplete sum constructors may
need an annotated binding or explicit arguments.

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
