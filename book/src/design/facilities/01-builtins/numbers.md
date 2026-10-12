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

`T.try_from(value) -> Result[T, RangeError]` is defined for every integer type
`T` and takes one value of any integer type. It returns `Ok` with the same
mathematical value when `T` can represent it and `Err(RangeError.OutOfRange)`
otherwise. `RangeError` has just that one variant, so an exhaustive error match
needs one arm. Conversion allocates nothing and does not call the
runtime. Float and non-numeric arguments are compile-time errors, as is
`try_from` on a float type; float conversions use casts.

Parsing still returns `ParseError`, which distinguishes malformed text from
an out-of-range number. `?` requires identical error types unless the caller
deliberately erases details with `Failure`. A function that parses and then
narrows can explicitly match the conversion error and return
`Err(ParseError.OutOfRange)`; there is no implicit error conversion.

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
Float types provide `T.PI`, `T.E`, `T.TAU` (nearest to π, e, and 2π), `T.INF`
(positive infinity), and `T.NAN` (a quiet NaN). Each constant has type `T`, so
`u8.MAX - small` needs no cast. Float types have no `MIN` or `MAX`, and integers
have no float constants. Names are upper-case; unknown names are compile-time
errors listing the supported constants.

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
| `x.exp2()`, `x.expm1()` | 2 to the power `x`; e to the power `x` minus 1, accurate near zero |
| `x.log()` | Natural logarithm; `-inf` for zero, NaN for a negative `x` |
| `x.log2()`, `x.log10()`, `x.log1p()` | Base-2, base-10, and natural log of `1 + x` (accurate near zero) |
| `x.sin()`, `x.cos()`, `x.tan()` | Trigonometric functions of `x` radians |
| `x.asin()`, `x.acos()`, `x.atan()`, `y.atan2(x)` | Inverse trigonometry in radians; atan2 preserves quadrant and signed zeros |
| `x.sinh()`, `x.cosh()`, `x.tanh()` | Hyperbolic functions |
| `x.asinh()`, `x.acosh()`, `x.atanh()` | Inverse hyperbolic functions |
| `x.cbrt()` | Real cube root, including negative inputs |
| `x.floor()`, `x.ceil()`, `x.trunc()` | Whole value toward negative infinity, positive infinity, or zero; still a float |
| `x.round()` | Nearest whole value, choosing even at halfway values; preserves signed zero |
| `x.fabs()`, `x.copysign(y)` | Absolute magnitude; magnitude with the sign bit of `y` |
| `x.pow(y)` | `x` to the power `y` |
| `x.fmod(y)` | Remainder of `x / y` with the sign of `x`; NaN when `y` is zero |

An integer receiver is a compile-time error naming the float cast. `%` remains
integer-only; `fmod` is the float remainder. `floor` and `ceil` return floats,
and the cast `i64(x.floor())` produces the integer.

Each binary method takes one argument of the receiver's type. Unary methods
take no arguments. Generic `FloatType` functions can use every method.
[The imported `std.math` functions](../../stdlib/math.md) wrap these primitives with
recoverable domain and overflow errors and checked integer rounding.

## Formatting

`str.from(number_or_bool) -> Result[str, AllocError]` observes one scalar.
Integers use decimal; floats use shortest round-trip debug formatting, including
a decimal point for whole finite values, `-0.0`, `inf`, `-inf`, and `NaN`;
booleans use `True` and `False`. `str(value)` is not a cast; its diagnostic
names `str.from`.

Formatting uses bounded temporary storage and fallible output construction;
[small text results](text.md) require no heap allocation. Aggregate formatting
and format specifications are unsupported.
