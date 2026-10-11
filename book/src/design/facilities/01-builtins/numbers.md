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

## Formatting

`str.from(number_or_bool) -> Result[str, AllocError]` observes one scalar.
Integers use decimal; floats use shortest round-trip debug formatting, including
a decimal point for whole finite values, `-0.0`, `inf`, `-inf`, and `NaN`;
booleans use `True` and `False`. `str(value)` is not a cast; its diagnostic
names `str.from`.

Formatting uses bounded temporary storage and fallible output construction;
[small text results](text.md) require no heap allocation. Aggregate formatting
and format specifications are unsupported.
