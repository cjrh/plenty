# Numeric types and literals

Unsuffixed integers default to `i64`; decimal or exponent literals default to
`f64`. Width suffixes include `7u8`, `1f32`, `1.5f32`, and `1e-3f64`. Decimal forms
such as `.5` and `1.` are accepted. Literals must fit their selected width.

Annotations, parameters, returns, and typed arithmetic operands guide
unsuffixed literals within an expression. Already typed values and suffixed
literals must match exactly; no numeric widening occurs. Ordinary generic calls
have [argument inference rules](../07-generics/inference.md) that do not infer
one argument's literal width from another.

`T.try_from(value)` converts between integer types and
[checks the target's range](../../facilities/01-builtins/numbers.md#checked-conversion).

Explicit casts use numeric type names, for example `u8(257)`. Narrowing integers
discards high bits; widening sign-extends signed inputs and zero-extends unsigned
inputs. Same-width signed/unsigned casts reinterpret bits. Casts do not validate
range.

Float-to-integer casts truncate toward zero and saturate to the target range;
NaN becomes zero. Integer-to-float and narrowing float casts may round. Widening
`f32` to `f64` is exact.

Unary minus requires a signed integer or a float; an unsigned operand is a
compile-time error. A sign written directly before a literal is part of the
literal, which must fit its type: `-3u8` is out of range, and `-0u8` is zero.

Integer arithmetic overflow and division by zero are runtime errors. `//` floors
signed quotients, including negative operands; `%` is integer-only. `/` accepts
matching-width floats. Float arithmetic follows IEEE behavior for rounding,
signed zero, infinities, NaN, and underflow; arithmetic overflow may yield infinity.

Comparisons require matching types; ordering accepts integers, floats, and
[strings](../../facilities/01-builtins/text.md). NaN is
unequal to every value, including itself, and all ordered comparisons with it are
false. These rules also apply to structural equality and membership. Floats cannot
be dictionary keys or set elements.
