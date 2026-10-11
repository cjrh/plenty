# Numbers

The integer types are `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, and `u64`;
the floating-point types are `f32` and `f64`. Literal typing, arithmetic,
comparisons, and casts follow the [numeric language rules](../../language/03-types/numbers.md).
There is no builtin `int` type or implicit numeric widening.

## Parsing

`T.parse(text: str) -> Result[T, ParseError]` observes its input and allocates
nothing. Parsing trims Unicode White_Space. `ParseError.Invalid` means malformed
input; `OutOfRange` means a valid number outside the target's range. Both errors
are allocation-free.

Integer parsing accepts an optional ASCII sign and
decimal ASCII digits, and checks the requested width. Empty input, prefixes,
underscores, and non-ASCII digits are invalid.

Float parsing accepts ASCII decimal/exponent forms and case-insensitive `inf`,
`infinity`, and `nan`, with an optional sign. It rounds directly to the requested
width. Finite overflow returns `OutOfRange`; underflow may round to zero. Signed
zero survives. Separators and hexadecimal forms are invalid.

## Checked conversion

`T.try_from(value) -> Result[T, ParseError]` is defined for every integer type
`T` and takes one value of any integer type. It returns `Ok` with the same
mathematical value when `T` can represent it and `Err(ParseError.OutOfRange)`
otherwise, the error `T.parse` reports for a number outside `T`'s range. It
never returns `ParseError.Invalid`, allocates nothing, and does not call the
runtime. Float and non-numeric arguments are compile-time errors, as is
`try_from` on a float type; float conversions use casts.

## Wrapping conversion

`T.wrapping_from(value) -> T` is defined for every integer type `T` and takes
one value of any integer type. It keeps the low bits of the value's
two's-complement representation, sign-extending a narrower signed value first,
so the result is the value modulo 2 to the power of `T`'s width:
`u8.wrapping_from(300)` is `44`, `u64.wrapping_from(-1)` is
`18446744073709551615`, and `i8.wrapping_from(200u8)` is `-56`. It never fails,
allocates nothing, and does not call the runtime. Float and non-numeric
arguments are compile-time errors, as is `wrapping_from` on a float type.

The bare cast `T(value)` is the form that
[requires the value to fit](../../language/03-types/numbers.md).

## Limits and constants

`T.MIN` and `T.MAX` are the smallest and largest values of an integer type `T`.
`T.PI` and `T.E` are the values of a float type `T` nearest to π and e. Each is
a value of type `T`, so `u8.MAX - small` needs no cast. A float type has no
`MIN` or `MAX`, and an integer type has no `PI` or `E`. The names are
upper-case; any other name after a numeric type is a compile-time error that
lists these four.

## Absolute value, minimum, and maximum

`abs(x)` takes one number and returns its magnitude in the same type. The
minimum of a signed integer type has no magnitude in that type, so
`abs(i64.MIN)` is the `integer overflow` runtime error that `-i64.MIN` is. On an
unsigned integer `abs` returns its operand.

`min(a, b)` and `max(a, b)` take exactly two numbers of one type and return one
of them. Operands of different numeric types are rejected with the cast that
reconciles them, as arithmetic operands are. Calls nest for more values, as in
`min(a, min(b, c))`; a collection needs a loop. For floats, a NaN operand gives
NaN, and `-0.0` is below `0.0`.

## Sum

`sum(xs)` takes one list of numbers and returns the total in the element type.
It starts from zero and adds the elements in order, so an empty list gives `0`
or `0.0`. Integer addition is checked at every step: a partial total outside
the element type is the `integer overflow` runtime error even when the full
total would fit. Float addition follows IEEE 754. `sum` observes its list, as
`len` does, and allocates nothing. Other collections and ranges, and lists of
other element types, are compile-time errors; add those in a loop.

`abs`, `min`, `max`, and `sum` are built-in names: a top-level declaration
cannot take one, and a local binding can.

## Powers

`base.pow(exponent)` is defined on every numeric type and takes one exponent of
the receiver's type. There is no `**` operator.

For integers the result is exact. A result outside the type is the
`integer overflow` runtime error, and a negative exponent is the runtime error
`negative integer exponent`. `x.pow(0)` is `1` for every `x`, including zero.
Convert to a float for a fractional or negative power.

For floats `pow` is the platform math library's `pow`: `(2.0).pow(0.5)` is the
square root of two, and a negative base with a fractional exponent gives NaN.

An integer literal receiver needs parentheses, as in `(2).pow(10)`, because
`2.` begins a float literal. A suffixed literal does not: `2u8.pow(7)`.

## Float functions

These methods are defined on `f32` and `f64` and return the receiver's type.
None fails or allocates; results outside a function's domain are NaN or an
infinity, following IEEE 754 and the platform math library.

| Method | Result |
|---|---|
| `x.sqrt()` | Square root; NaN for a negative `x` |
| `x.exp()` | e to the power `x` |
| `x.log()` | Natural logarithm; `-inf` for zero, NaN for a negative `x` |
| `x.sin()`, `x.cos()` | Sine and cosine of `x` radians |
| `x.floor()`, `x.ceil()` | Nearest whole value at or below, at or above `x`; still a float |
| `x.pow(y)` | `x` to the power `y` |
| `x.fmod(y)` | Remainder of `x / y` with the sign of `x`; NaN when `y` is zero |

An integer receiver is a compile-time error naming the float cast. `%` remains
integer-only; `fmod` is the float remainder. `floor` and `ceil` return floats,
and the cast `i64(x.floor())` produces the integer.

## Formatting

`str.from(number_or_bool) -> Result[str, AllocError]` observes one scalar.
Integers use decimal; floats use shortest round-trip debug formatting, including
a decimal point for whole finite values, `-0.0`, `inf`, `-inf`, and `NaN`;
booleans use `True` and `False`. `str(value)` is not a cast; its diagnostic
names `str.from`.

Formatting uses bounded temporary storage and fallible output construction;
[small text results](text.md) require no heap allocation. Aggregate formatting
and format specifications are unsupported.
