# Numeric types and literals

Unsuffixed integers default to `i64`; decimal or exponent literals default to
`f64`. Width suffixes include `7u8`, `1f32`, `1.5f32`, and `1e-3f64`. Decimal forms
such as `.5` and `1.` are accepted. Literals must fit their selected width.

Annotations, parameters, returns, and typed arithmetic operands guide
unsuffixed literals within an expression. Already typed values and suffixed
literals must match exactly; no numeric widening occurs. Operands of different
numeric types are rejected with the cast that reconciles them, for example
``cannot combine i64 with f64; convert with `f64(n)` ``. Ordinary generic calls
have [argument inference rules](../07-generics/inference.md) that do not infer
one argument's literal width from another.

Explicit casts use numeric type names, for example `u8(200)`. A cast to an
integer type requires the target to represent the value. An integer keeps its
mathematical value; one outside the target's range is a runtime error, as
arithmetic overflow is. This covers narrowing, a negative value cast to an
unsigned type, and an unsigned value above a signed type's maximum. A float is
truncated toward zero first: `u8(255.9)` is `255` and `u8(-0.9)` is `0`. A float
whose truncated value is outside the target's range, an infinity, or NaN is the
same runtime error. The message is `integer cast out of range`.

A cast whose operand is a numeric literal is checked at compile time, so
`u8(300)` and `u8(300.7)` are compile-time errors. A cast between integer types
where the target represents every source value, such as `i64(small_u8)`, emits
no check. A failing check calls the runtime; a passing one does not, and no cast
allocates.

Casts to a float type never fail. Integer-to-float and narrowing float casts may
round, and an `f64` beyond `f32`'s range becomes an infinity. Widening `f32` to
`f64` is exact.

Two other forms convert between integer types.
[`T.try_from(value)`](../../facilities/01-builtins/numbers.md#checked-conversion)
returns a `Result` instead of stopping the program.
[`T.wrapping_from(value)`](../../facilities/01-builtins/numbers.md#wrapping-conversion)
keeps the low bits and never fails.

Unary minus requires a signed integer or a float; an unsigned operand is a
compile-time error. A sign written directly before a literal is part of the
literal, which must fit its type: `-3u8` is out of range, and `-0u8` is zero.

Integer arithmetic overflow and division by zero are runtime errors that
[report their source position](../08-errors/02-allocation-and-traps.md). `//` floors
signed quotients, including negative operands; `%` is integer-only. `/` accepts
matching-width floats. Float arithmetic follows IEEE behavior for rounding,
signed zero, infinities, NaN, and underflow; arithmetic overflow may yield infinity.

Integers support bitwise `&`, `|`, `^`, and unary `~`, and shifts `<<` and `>>`.
Operands must have the same integer type; a typed left operand guides an
unsuffixed count literal. Counts must be nonnegative and less than the integer
width; invalid counts trap before shifting. Left shifts discard high bits.
Signed right shifts extend the sign bit; unsigned right shifts fill with zero.
From lower to higher precedence: `|`, `^`, `&`, shifts, addition/subtraction,
multiplication/division/modulo, unary operators. Comparisons bind below `|`.
Prefix `&` and `&mut` still create borrows.

`value.wrapping_add(other)`, `wrapping_sub`, and `wrapping_mul` require matching
integer types and compute modulo two to the integer width. They allocate
nothing. Ordinary `+`, `-`, and `*` keep their overflow checks.

Comparisons require matching types; ordering accepts integers, floats, and
[strings](../../facilities/01-builtins/text.md). NaN is
unequal to every value, including itself, and all ordered comparisons with it are
false. These rules also apply to structural equality and membership. Floats cannot
be dictionary keys or set elements.
