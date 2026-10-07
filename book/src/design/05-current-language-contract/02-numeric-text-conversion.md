# Numeric text conversion

Every integer type exposes `T.parse(text) -> Result[T, ParseError]`.
Parsing borrows its string, trims Unicode White_Space, accepts an optional ASCII
sign and decimal ASCII digits, and checks the target width. Empty or malformed
input returns `ParseError.Invalid`; a valid number outside the target range
returns `ParseError.OutOfRange`. Prefixes, underscores, and non-ASCII digits are
not accepted. Parsing and both error variants allocate nothing.

`f32.parse` and `f64.parse` use the same result/error contract, accepting ASCII
decimal/exponent forms and case-insensitive `inf`, `infinity`, and `nan` with an
optional sign. Conversion rounds directly to the requested width. Finite input
overflow returns `OutOfRange`; underflow may round to zero. Signed zero survives.
Numeric separators and hexadecimal forms are rejected. Parsing allocates nothing.

`str.from(number_or_bool) -> Result[str, AllocError]` renders a scalar through
a bounded stack buffer and one fallible output allocation. Integers use decimal;
floats use the same shortest round-trip representation as `print` (including
`-0.0`, `inf`, and `NaN`); booleans use `True`/`False`. Aggregate formatting and
format specifications remain deferred.
