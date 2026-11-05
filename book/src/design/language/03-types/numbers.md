# Numeric types and literals

For a fixed inclusive integer interval, use [`Bounded[T, lo, hi]`](bounded.md).

Unsuffixed integers default to `i64`; decimal or exponent literals default to
`f64`. Width suffixes include `7u8`, `1f32`, `1.5f32`, and `1e-3f64`. Decimal forms
such as `.5` and `1.` are accepted. Literals must fit their selected width.

Integer literals may also be hexadecimal (`0x1F1E6`), octal (`0o755`), or
binary (`0b1010`). The prefix letter and hexadecimal digits may be either case.
These forms are always integers: they take the integer suffixes and range
checks of decimal literals, as in `0xFFu8` and `-0x80i8`, and have no fraction,
exponent, or float suffix, so `0x1f32` is the integer 7986. Any literal may
separate digits with single underscores, as in `1_000_000` and `0xffff_ffff`;
an underscore cannot lead, trail, repeat, or follow the prefix.

| Source | Diagnostic |
| --- | --- |
| `0x` | `` `0x` needs at least one hexadecimal digit `` |
| `0b102` | ``invalid digit `2` in a binary literal`` |
| `0b1f32` | ``invalid digit `f` in a binary literal`` |
| `0x1u7` | `invalid integer suffix` |
| `0x_FF` | `invalid integer separator` |
| `0x1FFu8` | `integer literal out of range for u8` |

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
mathematical value; one outside the target's range traps, unlike the default
recoverable integer arithmetic errors. This covers narrowing, a negative value
cast to an unsigned type, and an unsigned value above a signed type's maximum. A float is
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

Integer arithmetic failures return `Err` from the enclosing function by
default. Its declared `Result` error type must include the possible errors;
successful arithmetic still produces ordinary integers. Use an explicit
[`checked:` block](#checked-blocks) for local recovery, or
[`unchecked:`](#unchecked-blocks) where the programmer guarantees that the
arithmetic cannot fail.
`//` floors signed quotients, including negative operands; `%` is integer-only. `/` accepts
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

## Function arithmetic contracts

```plenty
def scale(value: i64, factor: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value * factor)

def main() -> Result[(), IoError]:
    print(scale(3, 4).unwrap(), scale(i64.MAX, 2).is_err())?
    Ok(())
```
```output
12 True
```

Each named or anonymous function has its own boundary, independent of its
definition or call site. The declared error type must contain every escaping
arithmetic error; a plain or `Option` return cannot implicitly propagate them.
Explicit `Ok`, `Err`, returns and `?` keep their ordinary meaning. Operators
do not produce individual `Result` wrappers or implicitly unwrap calls.

The operation/error table under [checked blocks](#checked-blocks) also applies
to the function default. Error contracts follow operators and operand types,
not range analysis. Locally handled errors do not have to appear in the
function's error type. Generators cannot propagate function-level arithmetic
errors: handle a local checked result or explicitly opt out.

> Coordinated repository adoption under [B14](../../../backlog.md) remains in
> progress. Public contracts are explicit: do not assume that an arbitrary
> callback consumer accepts a Result-returning callback.

## Unchecked blocks

`unchecked:` states that integer arithmetic written in its lexical suite
does not overflow or divide by zero; that is the programmer's precondition,
not the compiler's. It yields the last expression's ordinary type, without wrapping or unwrapping a
`Result`. It is also allowed as a statement suite, including an empty
`pass` suite and suites whose paths exit. Locals remain block scoped and
reference-valued suites are not supported.

```plenty fragment
def scale_in_bounds(value: i64, factor: i64) -> i64:
    unchecked:
        value * factor
```

The `--unchecked MODE` build setting selects what a plain `unchecked:` suite
does with its preconditions. `unchecked trap:` always checks them:

| Suite | `--unchecked trap` (default) | `--unchecked assume` |
| --- | --- | --- |
| `unchecked:` | Check; trap on a violation | Omit the checks; a violation is undefined behavior |
| `unchecked trap:` | Check; trap on a violation | Check; trap on a violation |

A trap terminates the process; it never wraps or reaches an outer checked
boundary. In assume mode, overflow is not wrapping: a violation may produce a
silently wrong result or worse, and no crash is guaranteed. Use explicit
wrapping operations for modular arithmetic, and `unchecked trap:` or ordinary
validation when termination on bad input is part of the contract. The default
does not change with optimization or [profile](../../toolchain/01-building/packages.md#profiles).

```plenty fragment
def checked_index(row: i64, width: i64, column: i64) -> i64:
    unchecked trap:
        row * width + column
```

The preconditions are representable results of `+`, `-`, `*`, negation and
`pow`, nonzero divisors and representable quotients for `//` and `%`, valid
shift counts, and in-range integer conversions. `i64.MIN % -1` stays `0`, and
a negative `pow` exponent still traps in either mode. Floating-point
arithmetic, allocation, indexing and explicit wrapping, saturating and checked
methods keep their contracts.

Modes apply lexically. Nested `checked:` restores local recoverability; a
nested plain `unchecked:` follows the build setting, even inside
`unchecked trap:`; leaving any suite restores the outer mode. Named functions
and closures keep their own contracts wherever they are defined or called.
Calls still return their declared results, and `?`, `return`, `break` and
`continue` keep their ordinary targets.

`std` always checks its `unchecked:` suites, so the setting cannot change
library behavior before its preconditions are audited. The setting is part of
the [compile cache](../../toolchain/02-architecture/cache.md) identity; there
is no `plenty.toml` key yet. Cranelift has no no-overflow instruction flags,
so assume mode removes checks but cannot give the backend further overflow
facts.

`unchecked` and `trap` are contextual: only `unchecked:` or `unchecked trap:`
followed by a newline opens a suite; `unchecked: i64 = 0` remains a typed
binding. Suites support binding, assignment, return, function-tail and
match-arm-tail value positions.

## Checked blocks

A `checked:` block turns the integer failures written inside it into a value.
Its body is an indented suite whose last expression is the block's value `T`.
The block is an expression of type `Result[T, E]`:

```plenty fragment
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
| `//` on a signed `T` with a `T` or `NonZero[T]` divisor (`T.MIN // -1`) | `OverflowError[T]` |
| `//` and `%` with a plain integer divisor that may be zero | `ZeroDivisionError` |

`OverflowError[T]` has one field, `wrapped: T`, the exact result modulo
2<sup>N</sup> for an N-bit `T`. `ZeroDivisionError` has no fields. Both are
builtin classes; their names cannot be redefined. `T.MIN % -1` is `0`, inside
or outside a block. Shifts, casts, and float arithmetic are unchanged: they
neither add an error nor exit the block. A negative `pow` exponent still traps,
since it has no integer result. Float `a.div_checked(b)` returns
`Result[F, ZeroDivisionError]`, failing for either signed zero divisor; it
behaves as a one-operation block and may appear anywhere.

A [class pattern](../04-expressions/matching.md#class-patterns) names each
error and binds the wrapped result:

```plenty fragment
def halve_sum(a: i64, b: i64) -> i64:
    result = checked:
        (a + b) // 2
    match result:
        case Ok(half):
            half
        case Err(OverflowError(wrapped)):
            # A positive divisor rules out zero and MIN // -1.
            two: Positive[i64] = 2
            wrapped // two
        case Err(ZeroDivisionError()):
            0
```

The error type follows the operators, not their operand values. `n // 4`
inside a block still adds `ZeroDivisionError` and `OverflowError[i64]`; the
code generator omits the branches a constant divisor rules out. A declared
`Result` whose error includes every possible failure is kept, so a block can
be the body of a function that returns `Result[i64, OverflowError[i64] |
ParseError]`.

The rule is lexical within one function. Compound assignment such as
`x += 1`, comprehensions, and operators that
[`@newtype` classes](../04-expressions/operators.md#newtype-forwarding)
forward to their field report the field's arithmetic error types. The nearest
explicit arithmetic mode controls failure routing.

Functions and closures defined inside the block keep their own declared
contracts. There is no private callback error channel or checked-block lifetime
restriction; ordinary ownership and borrow restrictions still apply. A callee's
`Result` requires explicit handling, and `?` exits the function rather than
being caught by the block.

Integer [`std.math` reductions](../../stdlib/math.md) and serial pipeline
`sum()` return public overflow results; float reductions stay infallible.
[`sort_by_key`](../../facilities/01-builtins/lists.md) accepts ordinary
fallible key callbacks and returns their error. Other callback consumers
require their own explicitly fallible contracts. Do not assume a call inherits
local checking or that an infallible callback signature
can accept recoverable arithmetic.

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

Arithmetic uses plain values and a shared cold error exit at its nearest
recoverable boundary. A trapping `unchecked:` operation uses an inline trap
guard instead; assume mode emits the plain instruction. See the
[design note](../../../proposals/completed/checked-arithmetic.md).

## Nonzero integers

`NonZero[T]`, for any integer type `T`, holds a `T` that is not zero. Where
one is expected, so that the program cannot hold a zero, an integer literal
converts at compile time, including a negative one; `0` is a compile-time
error. Any other value is converted with `NonZero.try_from(value)`, which
returns `Result[NonZero[T], RangeError]` and fails with
`RangeError.OutOfRange` for zero. `T` is the value's type;
`NonZero[T].try_from(value)` names it and requires a `T`. `get()` reads the
integer back.

```plenty
def per_group(total: i64, groups: NonZero[i64]) -> Result[i64, OverflowError[i64]]:
    Ok(total // groups)

def main() -> Result[(), IoError | RangeError | OverflowError[i64]]:
    three: NonZero[i64] = 3
    print(per_group(12, three)?)?
    print(per_group(12, -4)?)?
    count = 5
    five = NonZero.try_from(count)?
    print(five * three, five + three, five > three)?
    match NonZero.try_from(count - 5):
        case Ok(n):
            print(n)?
        case Err(RangeError.OutOfRange):
            print("zero")?
    Ok(())
```

```output
4
-3
15 8 True
zero
```

A value has the same representation as its integer and allocates nothing.
The constructor is private: `NonZero(3)` is a compile-time error that names
the two conversions. `NonZero[T]` copies, compares with `==`, `!=`, `<`, `<=`,
`>`, and `>=`, prints and formats as its integer, and is a valid dictionary
key and set element.

Arithmetic keeps a result nonzero only where that follows from nonzero
operands. Unary `-` and `+`, and `*` between two `NonZero[T]` values, return
`NonZero[T]`; `*=` updates one. `+` and `-` between two values return `T`,
since their result can be zero. Overflow returns `Err` at the nearest
recoverable boundary, and violates the precondition of
[unchecked blocks](#unchecked-blocks), so a product cannot wrap to zero. Other
operators between two `NonZero` values, including `//`, `%`, bitwise operators,
and shifts, are compile-time errors that suggest `.get()`. An integer literal beside a `NonZero[T]` operand
is a `NonZero[T]` too: `step * 2` is a `NonZero[i64]`, and `step == 0` is a
compile-time error. Operands must otherwise have the same type; mixing
`NonZero[T]` with `T` is otherwise a type error whose hint shows `try_from`.

A plain `T` dividend accepts a `NonZero[T]` divisor: `total // groups` and
`total % groups` return `T`, and `//=` and `%=` update a `T` destination.
These forms omit the divide-by-zero check. Signed floor division still checks
`T.MIN // -1` for overflow; `T.MIN % -1` returns zero. In a `checked:` block,
signed division by `NonZero[T]` contributes only `OverflowError[T]`; modulo
and unsigned division contribute no failures. A block containing only
operations that cannot fail is rejected, as with other checked arithmetic.

A [range](../../facilities/01-builtins/ranges.md) step is `NonZero[i64]`.
`NonZero` is a builtin name and cannot be redefined.

## Positive and non-negative integers

`Positive[T]` holds an integer greater than zero; `NonNegative[T]` holds an
integer greater than or equal to zero. `T` must be `i8`, `i16`, `i32` or
`i64`. For unsigned integers use `NonZero[uN]` or plain `uN`, respectively.
Floats and other non-integer types are rejected without a replacement suggestion.
Both are builtin names and cannot be redefined.

An unsuffixed integer literal converts where the domain type is expected,
including parameters, defaults, fields, returns and collection elements.
An out-of-domain literal is a compile-time error. A
[module constant](../02-source/modules.md#constants) may have a domain type;
its value is checked the same way. A computed integer converts
with `Positive.try_from(value)` or `NonNegative.try_from(value)`, returning
`Result[Domain[T], RangeError]`; invalid values give `RangeError.OutOfRange`.
The qualified forms `Positive[T].try_from` and `NonNegative[T].try_from`
require exactly `T`. Constructors and fields are private; `.get()` reads `T`.
There is no implicit conversion between a domain and its integer or another
domain.

```plenty
def configured(value: i64) -> Result[Positive[i64], RangeError]:
    Positive.try_from(value)

def main() -> Result[(), IoError | RangeError | OverflowError[i64]]:
    size: Positive[i64] = 3
    limit: NonNegative[i64] = 0
    print(size + 2, size * 2, size - size, -size, limit)?
    stride = NonZero.from(size)
    print(list(range(0, 8, stride)).unwrap())?
    print(NonNegative.from(configured(4)?))?
    print(configured(0).is_err())?
    Ok(())
```
```output
5 6 0 -3 0
[0, 3, 6]
4
True
```

`+`, `*`, `+=` and `*=` preserve the domain; subtraction and unary `-`
return `T`, and unary `+` preserves it. Overflow returns `Err` by default
and violates the precondition of [unchecked blocks](#unchecked-blocks), so the
domain cannot be violated by wrapping. Comparisons accept the same domain type or a literal in its domain.
Other operators between domain values, including division, modulo, bitwise
operations and shifts, require `.get()`.

A plain `T` dividend accepts a `Positive[T]` divisor directly with `//`, `%`,
`//=` and `%=`. The result is `T`. These forms omit both the zero check and
the signed `MIN // -1` check, since a positive divisor can be neither zero
nor `-1`. They contribute no failures inside a `checked:` block. A
`NonNegative[T]` divisor still needs `.get()` or explicit conversion to a
nonzero domain, since zero is valid for it.

`NonZero.from(positive)` and `NonNegative.from(positive)` explicitly widen a
`Positive[T]`; they cannot fail or allocate. Qualified forms name `T`.
Conversion from a `NonNegative` to a `Positive` or `NonZero` uses
`try_from(value.get())`, since it may be zero.

Both domains have `T`'s representation, copy without allocation, print and
format as the integer, and work as dictionary keys and set elements. `len()`
continues to return `i64`; convert a computed length explicitly when a domain
is needed.
