# Quantities

`Quantity[T, D]` stores a numeric value in the coherent unit of a
[dimension](dimensions.md). `T` must be an integer, a float, or an integer domain
such as `Positive[i64]`. Quantity values copy. Put domains inside the quantity:
`Quantity[Positive[i64], Length]`, rather than `Positive[Quantity[i64, Length]]`.

```plenty
dimension Length("m")
dimension Time("s")
dimension Velocity = Length / Time

def main() -> Result[(), IoError]:
    metre = Quantity[f64, Length](1.0)
    kilometre = 1000.0 * metre
    distance = 2500.0 * metre
    elapsed = Quantity[f64, Time](100.0)
    speed: Quantity[f64, Velocity] = distance / elapsed
    print(distance.to(kilometre), speed.get())?
    Ok(())
```
```output
2.5 25.0
```

## Construction and conversion

`Quantity[T, D](value)` accepts `T`, with ordinary literal inference and domain
literal validation. `.get()` returns the stored `T`. The field is private.
There is no implicit conversion to or from a plain number, including for
`Quantity[T, One]`.

For floating storage, `q.to(unit)` returns `(q / unit).get()`. Both quantities
must have the same storage and dimension; the unit's value determines the
scale. For integer storage, `/` is unavailable, so use explicit `//` and `.get()`
when integer floor conversion is intended.

`Quantity[f32, D].from(q)` and `Quantity[f64, D].from(q)` convert a quantity
of the same dimension and plain numeric storage, with ordinary numeric cast
semantics. They neither change dimensions nor infer a scale.

`Quantity[Positive[T], D].try_from(n)` returns
`Result[Quantity[Positive[T], D], RangeError]`; the same operation supports
`NonNegative` and `NonZero`. It takes an integer of the domain's underlying
width and checks the domain exactly as its ordinary `try_from` does.

## Operators

The storage operation follows the existing [numeric rules](numbers.md), including
width checks, literal inference, domain restrictions, overflow and `checked:`.
The table's `R` is the storage operation's result type.

| Operation | Dimensions and result |
| --- | --- |
| `a + b`, `a - b` | Equal dimensions; `Quantity[R, D]` |
| `a * b` | `Quantity[R, A * B]` |
| `a / b`, `a // b` | `Quantity[R, A / B]`; `/` requires floats, `//` integers |
| `a % b` | Equal dimensions and integer storage; `Quantity[R, D]` |
| `q * k`, `k * q`, `q / k` | Numeric scalar; `Quantity[R, D]` |
| `k / q` | `Quantity[R, One / D]` |
| `+q`, `-q` | Same dimension, storage result `R` |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | Equal dimensions; `bool` |
| `+=`, `-=` | Equal dimensions and storage; result must fit the destination type |
| `*=`, `/=` | Scalar only; result must fit the destination type |

A scalar cannot be added to or compared with a quantity, even when it is zero.
Bitwise operators and other compound assignments are unavailable. Operands are
evaluated once in source order. Compound assignments preserve ordinary borrow
checks and evaluate a collection index once.

For `p: Quantity[Positive[i64], Length]`, `p + p` keeps positive storage,
`p * p` has positive storage and dimension `Pow[Length, 2]`, and `p - p` and
`-p` have plain `i64` storage while retaining `Length`. A plain integer quantity
accepts a `Positive` or `NonZero` quantity divisor for `//` and `%`, with the
same proof-based checks as the underlying integers. Domain dividends retain
the underlying domain's restrictions.

Inside `checked:`, failures remain `OverflowError[T]` and/or
`ZeroDivisionError`, as for the underlying arithmetic. Integer domains use
their underlying integer in `OverflowError`. Quantity construction and
rewrapping do not change arithmetic policy.

## Formatting

`print(q)` and `f"{q}"` show the coherent value followed by a space and the
coherent-unit symbol: `1.3 m/s`. Numeric format specs apply to the number,
including its padding; `f"{q:08.2f}"` gives `00001.30 m/s`. Integer domains
format as their underlying integers. Dimensionless quantities have no suffix.

Symbols follow normalized base order. Products use `*`, powers use `^`, and
multiple denominator factors are parenthesized: `kg*m/s^2` or `m/(kg*s)`.
The ordering is by nominal base identity, not by symbol. Distinct dimensions
with equal symbols remain distinct; their symbols do not cancel.

```plenty
dimension Length("m")
dimension Time("s")

def main() -> Result[(), IoError | AllocError]:
    speed = Quantity[f64, Length / Time](1.3)
    print(speed)?
    print(f"{speed:.2f}"?)?
    print(f"{speed!r}"?)?
    Ok(())
```
```output
1.3 m/s
1.30 m/s
Quantity[f64, Length * Pow[Time, -1]](1.3)
```

`str.repr(q)`, `!r` and quantities inside aggregates show
`Quantity[T, D](value)`, with the normalized dimension expression. `!a` escapes
non-ASCII characters in that debug spelling. `!s` converts the display spelling
to text first, so `f"{q!s:>12}"` pads the whole number-and-unit text.

Quantities support `display(out)` and `debug(out)` with a mutable
[`std.fmt.Formatter`](../../stdlib/fmt.md), including through `std.fmt.display`,
`debug`, `to_string` and `to_debug`. Direct writer formatting and numeric
f-string fields write the number and static symbols without allocating a
quantity-specific string; ordinary formatter limits and writer errors apply.
String-producing operations still allocate. Printing retains `IoError`, and
`str.repr` retains `AllocError`; quantities do not add user formatting errors.
Nested format specs and `c` retain their ordinary `FormatError` possibility.

## Representation and current limits

Dimensions add no runtime field. Quantities currently use the existing copy
class representation with one numeric field. Arithmetic and conversion do not
allocate, but the class calling convention and constructor bookkeeping still
remain in native code; identical instructions to plain-number programs are
not yet guaranteed.

Unit spellings live in static type metadata and compiled formatting calls, not
in each value. Quantity module constants and alias-aware diagnostic spelling
are not implemented. See the
[remaining units work](../../../backlog.md#units-of-measure).
