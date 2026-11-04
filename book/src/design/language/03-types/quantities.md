# Quantities

`Quantity[T, D]` stores a numeric value in the coherent unit of a
[dimension](dimensions.md). `T` must be an integer, a float, or an integer domain
such as `Positive[i64]`. Quantity values copy. Put domains inside the quantity:
`Quantity[Positive[i64], Length]`, rather than `Positive[Quantity[i64, Length]]`.
[`std.units`](../../stdlib/units.md) provides the SI dimensions, units and
constants; the examples here declare their own dimensions to show the
underlying rules.

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

## Module constants

Declare units and derived values with `const` or `pub const`. Use a concrete
`Quantity[T, D]` annotation; dimension aliases may appear in `D`. Initializers
can use the Quantity constructor, other constants (including imports and forward
references), numeric casts of extracted values, quantity operators, `.get()`,
and floating `.to(unit)`. The compiler evaluates them before generating code.

```plenty
dimension Length("m")
dimension Time("s")
const metre: Quantity[f64, Length] = Quantity[f64, Length](1.0)
const kilometre: Quantity[f64, Length] = 1000.0 * metre
const minute: Quantity[f64, Time] = 60.0 * Quantity[f64, Time](1.0)
const speed: Quantity[f64, Length / Time] = kilometre / minute
assert kilometre.to(metre) == 1000.0

def main() -> Result[(), IoError | AllocError]:
    print(kilometre)?
    print(f"{speed:.2f}"?)?
    Ok(())
```
```output
1000.0 m
16.67 m/s
```

Storage widths, dimensions and integer-domain results follow ordinary quantity
arithmetic. A domain constructor accepts a valid literal or a value already in
that domain. `p - p` still erases the storage domain, so a positive quantity
cannot be declared from that subtraction. `.get()` on a domain-backed quantity
returns the domain; call `.get().get()` to extract its plain integer.

Integer overflow and division by zero are compile errors. Conditional and
short-circuit expressions check both branches but evaluate only the selected
branch. A dimension mismatch in a skipped branch is still an error. Constant
cycles are rejected. User function calls, fallible conversions and formatting
are not constant expressions; for a float storage conversion, write
`Quantity[f32, D](f32(q.get()))`. See
[module constants](../02-source/modules.md#constants) for the common rules.

## Formatting

`print(q)` and `f"{q}"` show the coherent value followed by a space and the
coherent-unit symbol: `1.3 m/s`. Numeric format specs apply to the number,
including its padding; `f"{q:08.2f}"` gives `00001.30 m/s`. Integer domains
format as their underlying integers. Dimensionless quantities have no suffix.

Products use `*`, powers use `^`, and multiple denominator factors are
parenthesized: `kg*m/s^2` or `m/(kg*s)`. Symbols appear in the order their
base dimensions are declared within a module, with modules ordered by name.
[`std.units`](../../stdlib/units.md) declares its bases in SI order, so a
newton prints as `kg*m/s^2`. The order is presentation only: distinct
dimensions with equal symbols remain distinct, and their symbols do not
cancel.

```plenty
dimension Mass("kg")
dimension Length("m")
dimension Time("s")

def main() -> Result[(), IoError | AllocError]:
    speed = Quantity[f64, Length / Time](1.3)
    print(speed)?
    print(f"{speed:.2f}"?)?
    print(f"{speed!r}"?)?
    momentum = Quantity[f64, Mass](2.0) * speed
    print(momentum)?
    print(f"{momentum!r}"?)?
    Ok(())
```
```output
1.3 m/s
1.30 m/s
Quantity[f64, Length * Pow[Time, -1]](1.3)
2.6 kg*m/s
Quantity[f64, Length * Mass * Pow[Time, -1]](2.6)
```

`str.repr(q)`, `!r` and quantities inside aggregates show
`Quantity[T, D](value)`, with the normalized dimension expression. That
expression orders bases by qualified name, as dimension identity does, so it
can differ from the symbol order. `!a` escapes
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

`Quantity[T, D]` uses `T`'s native value and parameter/result representation.
Dimensions add no runtime field, tag, or arithmetic operation. Construction and
`get` retag the coherent numeric value without a constructor call; float `to`
lowers to ordinary storage division. Integer-domain storage keeps the domain's
existing representation and arithmetic contracts. Ordinary classes keep their
existing ABI.

Collections, aggregates, references, closures and generators use the same
storage packing as their numeric payloads. Arithmetic and conversion do not
allocate. The [release instruction probe](../04-expressions/operators-implementation.md#quantity-erasure)
compares addition, multiplication and unit conversion with plain-number code.
This does not promise that every surrounding function call or temporary is
optimized away. Generic instances that differ only in dimension
[share one body](../../toolchain/02-architecture/emission.md#identical-code-folding)
unless they format a value or build a collection, whose type descriptor names
the dimension.

Unit spellings live in static type metadata and compiled formatting calls, not
in each value. A single quantity carries no type pointer: the compiler knows
its dimension at every use and embeds the symbol in the formatting code. A
list already holds one pointer to its type descriptor, whatever its element
type, because the runtime's shared printing, sorting, hashing and cleanup code
reads the element type through it. A `list[Quantity[f64, D]]` stores plain
8-byte `f64` rows; only its descriptor, and through it the unit symbol, differs
from a `list[f64]`. Descriptors are static read-only data, so neither lists nor
values allocate type information. [`std.units.wrap_list` and
`strip_list`](../../stdlib/units.md#lists) therefore convert between the two
list types by rewriting one header word, leaving every element untouched. See
the [remaining units work](../../../backlog.md#units-of-measure).

Type and dimension errors include
[scoped alias notes](dimensions.md#diagnostic-alias-notes), while debug formatting
keeps normalized dimensions.
