# Numeric types and literals

Unsuffixed integers default to `i64`; decimal or exponent literals default to
`f64`. Width suffixes include `7u8`, `1f32`, `1.5f32`, and `1e-3f64`. Decimal forms
such as `.5` and `1.` are accepted. Literals must fit their selected width.

Annotations, parameters, returns, and typed arithmetic operands guide
unsuffixed literals within an expression. Already typed values and suffixed
literals must match exactly; no numeric widening occurs. Operands of different
numeric types are rejected with the cast that reconciles them, for example
``cannot combine i64 with f64; convert with `f64(n)` ``. The suggested cast
never fails: an unsigned and a signed integer convert to the narrowest signed
type wider than the unsigned one, both operands if neither has that type
(`i8` with `u16` suggests `i32` for each). `u64` with a signed type has no such
type, so the message names no cast. Ordinary generic calls
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
allocates. A cast whose operand compiles to a constant, such as `i64(u16.MAX)`
or `u8(i64(200))` in a function body, emits the converted constant with no
conversion or check, as the literal `65535` would. Such an operand outside the
target's range, as in `u8(i64(300))`, is still the runtime error.

Casts to a float type never fail. Integer-to-float and narrowing float casts may
round, and an `f64` beyond `f32`'s range becomes an infinity. Widening `f32` to
`f64` is exact.

Two other forms convert between integer types.
[`T.try_from(value)`](../../facilities/01-builtins/numbers.md#checked-conversion)
returns a `Result` instead of stopping the program.
[`T.wrapping_from(value)`](../../facilities/01-builtins/numbers.md#wrapping-conversion)
keeps the low bits and never fails.

All three conversions take a number, not a reference. Borrowed iteration such
as `for b in &buffer` yields `&u8`, so `i64(b)` is a compile-time error that
names the fix: ``numeric casts require a number, not a reference; read it with
`*b` ``. A borrow written in the operand, as in `i64(&n)`, is reported with
``remove the `&` `` instead.

Unary minus requires a signed integer or a float; an unsigned operand is a
compile-time error, including a sign directly before a literal. Both `-3u8`
and `-0u8` report unsigned negation, as does `value: u8 = -0` when the literal
gets its type from context. Signed minimum literals such as `-128i8` remain
valid, and floating-point negative zero is preserved.

Integer arithmetic overflow and division by zero are runtime errors that
[report their source position](../08-errors/02-allocation-and-traps.md), unless
the arithmetic is written inside a [`checked:` block](#checked-blocks). `//` floors

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

`value.rotate_left(count)` and `value.rotate_right(count)` rotate the bits of
an integer: bits leaving one end enter at the other, so none are lost. The count
has the receiver's type, and a typed receiver guides an unsuffixed count literal.
Because no count loses bits, rotation never traps: the count is reduced modulo
the integer width, so rotating by the width is the identity and a negative
signed count rotates the other way. Signed values rotate their two's-complement
bit pattern. Rotation allocates nothing.
These built-ins are selected for integer receivers; classes may define methods
with the same names and their own signatures.

The [numeric facilities](../../facilities/01-builtins/numbers.md#limits-and-constants)
provide the type constants `T.MIN`, `T.MAX`, `T.PI`, and `T.E`; `pow` on every
numeric type; and the float methods `sqrt`, `exp`, `log`, `sin`, `cos`,
`floor`, `ceil`, and `fmod`. A two-operand method requires one type, as the
operators do, and an integer result outside its type is the same overflow
error.

Comparisons require matching types; ordering accepts integers, floats,
[strings](../../facilities/01-builtins/text.md), and [tuples](tuples.md) of those.
For scalar floats, NaN is
unequal to every value, including itself, and all ordered comparisons with it are
false. These rules also apply to structural equality and membership. Floats cannot
be dictionary keys or set elements.

## Checked blocks

A `checked:` block turns the integer failures written inside it into a value.
Its body is an indented suite whose last expression is the block's value `T`.
The block is an expression of type `Result[T, E]`:

```plenty
def mean(total: i64, count: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    checked:
        total * 2 // count
```

If no operation fails, the block's value is `Ok(T)`. The first failing
operation exits the block with `Err`, and the rest of the block does not run.
`E` is the [anonymous sum](anonymous-sums.md) of the errors that the block's
operators can produce:

| Operation inside the block | Error |
| --- | --- |
| `+`, `-`, `*`, unary `-` and `pow` on `T` | `OverflowError[T]` |
| `//` on a signed `T` (`T.MIN // -1`) | `OverflowError[T]` |
| `//` and `%` with a zero divisor | `ZeroDivisionError` |

`OverflowError[T]` has one field, `wrapped: T`, the exact result modulo
2<sup>N</sup> for an N-bit `T`. `ZeroDivisionError` has no fields. Both are
builtin classes; their names cannot be redefined. `T.MIN % -1` is `0`, inside
or outside a block. Shifts, casts, and float arithmetic are unchanged: they
neither add an error nor exit the block. A negative `pow` exponent still traps,
since it has no integer result. Float `a.div_checked(b)` returns
`Result[F, ZeroDivisionError]`, failing for either signed zero divisor; it
behaves as a one-operation block and may appear anywhere.

The error type follows the operators, not their operand values. `n // 4`
inside a block still adds `ZeroDivisionError` and `OverflowError[i64]`; the
code generator omits the branches a constant divisor rules out. A declared
`Result` whose error includes every possible failure is kept, so a block can
be the body of a function that returns `Result[i64, OverflowError[i64] |
ParseError]`.

The rule is lexical. Only arithmetic written in the block's own suite is
checked, including compound assignment such as `x += 1`, element updates
such as `items[i] += 1`, comprehensions, and operators that
[`@newtype` classes](../04-expressions/operators.md#newtype-forwarding) forward to their field, which
report `OverflowError` of the field type. Functions and closures called from
the block, and closures defined in it, keep trapping. Nested blocks catch at
the innermost block.

Control flow is otherwise unchanged. `?` inside the block exits the function,
and `return`, `break` and `continue` behave as they do outside it. A failure
releases the values and locals the block owns, as `?` does. It does not roll
back assignments the block already made to outer variables or elements, and an
assignment whose operation failed does not happen.

A block may appear wherever a [`match` block](../04-expressions/matching.md)
can produce a value: as the value of a binding or assignment, after `return`,
or as a function's or case's final expression. `checked` is not reserved: it
starts a block only when `:` and a newline follow it, so `checked: i64 = 0`
still declares a variable. A block whose value is discarded is rejected like
any unused `Result`. A block with no operation that can fail is a compile-time
error, as is one whose every path exits. A block cannot produce a reference.

Module constants are evaluated at compile time, where overflow is a
compile-time error. Arithmetic in a block is evaluated at run time even when
its operands are constants, so `checked:` around `i8.MAX + 1i8` produces
`Err(OverflowError[i8](wrapped=-128))`.

Arithmetic outside a block compiles exactly as before: plain types and an
inline check that traps. A block's operations compute the wrapped result and a
failure flag, then branch to a cold exit path; see the
[design note](../../../proposals/completed/checked-arithmetic.md).
