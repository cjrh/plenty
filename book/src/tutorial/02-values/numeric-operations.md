# Calculate with numbers

Arithmetic requires matching types. Integer failures are recoverable by default:
list their error types in the function's `Result` return, and use `Ok(...)`
for success. Operators themselves still produce ordinary numbers.

```plenty-error
def main() -> Result[(), IoError]:
    small = 7u8
    print(small + 1i64)?
    Ok(())
```

```error
cannot combine u8 with i64; convert with `i64(small)`
```

Choose a matching literal or explicitly convert a value:

```plenty
def main() -> Result[(), IoError]:
    small = 7u8
    print(small + 1u8)?
    print(i64(small) + 1)?
    Ok(())
```

```output
8
8
```

Neither addition can overflow, so `main` lists no arithmetic error: `small` is
bound once to `7`, and the compiler follows that value into both sums.
[Arithmetic that cannot fail](#arithmetic-that-cannot-fail) gives the rule.

`i64(small)` is safe because every `u8` value fits in an `i64`. When the target
cannot hold every value of the source, use `try_from`. It checks the range and
returns a `Result`: `Ok` with the converted value, or
`Err(RangeError.OutOfRange)` when the value does not fit. `RangeError` has only
this one variant; parsing text uses the separate `ParseError` type.

```plenty
def main() -> Result[(), RangeError | IoError | OverflowError[u8]]:
    wide = 200
    small = u8.try_from(wide)?
    print(small + 1u8)?
    Ok(())
```

```output
201
```

`?` takes the converted value or returns its error to the caller. The
`RangeError | IoError` error type preserves either a conversion error or a
printing error. `small` now comes from a conversion, so it can be any `u8`,
and `small + 1u8` adds `OverflowError[u8]`.
[Absence and failure](../03-absence/index.md) explains propagation
and how to recover from an error.

A cast to an integer type requires the value to fit. A literal that does not
fit is rejected when the program is compiled:

```plenty-error
def main() -> ():
    small = u8(300)
```

```error
300 is out of range for u8; u8.wrapping_from keeps the low bits
```

A value known only when the program runs is checked then. With `wide = 300`,
`u8(wide)` stops the program with `error: integer cast out of range`.
Casts retain this trapping behavior; arithmetic errors use typed returns. The same applies to a negative value cast to an
unsigned type, such as `u64(-1)`, and to an unsigned value above the signed
maximum, such as `i8(200u8)`. Cast when the value is known to fit, and use
`try_from` when it might not.

When discarding the high bits is the intent, say so with `wrapping_from`, which
never fails:

```plenty
def main() -> Result[(), IoError]:
    wide = 300
    negative = -1
    print(u8.wrapping_from(wide))?
    print(u64.wrapping_from(negative))?
    print(i8.wrapping_from(200u8))?
    Ok(())
```

```output
44
18446744073709551615
-56
```

Integer overflow returns `OverflowError[T]`, where `T` is the operand type.
An unsigned type has no negative values, so unary minus requires a signed
integer or a float:

```plenty-error
def main() -> Result[(), IoError]:
    small = 7u8
    print(-small)?
    Ok(())
```

```error
unary minus is not defined for unsigned type u8
```

Convert first: `-i16(small)` is `-7`.

The same rule applies to literals, including zero:

```plenty-error
def main() -> ():
    zero: u8 = -0
```

```error
unary minus is not defined for unsigned type u8
```

`+`, `-`, and `*` have their usual arithmetic precedence. `//` divides integers
and rounds down, including for negative values:

```plenty
def main() -> Result[(), IoError]:
    print(1 + 2 * 3)?
    print((1 + 2) * 3)?
    print(-7 // 3)?
    Ok(())
```

```output
7
9
-3
```

Integer division by zero returns `ZeroDivisionError`. Signed division also
returns `OverflowError[T]` for `T.MIN // -1`. A divisor whose value the
compiler knows rules out what it cannot cause: `n // 3` and `n % 3` can fail
with neither error, and `n // -1` only with overflow. An integer
[module constant](../07-modules/03-standard-math.md) proves the same as the
literal it holds. A divisor that can be any value, such as a parameter or a
`mut` variable, requires `ZeroDivisionError`, and for a signed type
`OverflowError[T]`, even when you know it is nonzero or not `-1`.
Use `/` for floating-point division.

Integers also support bitwise operations and shifts. Signed right shifts
preserve the sign; unsigned right shifts fill with zero. Left shifts discard
high bits.

```plenty
def main() -> Result[(), IoError]:
    bits = 5u8
    print(bits & 3)?
    print(bits | 2)?
    print(bits ^ 3)?
    print(~bits)?
    print(bits << 1)?
    print(bits >> 1)?
    print(255u8.wrapping_add(1))?
    print(0u8.wrapping_sub(1))?
    print(128u8.wrapping_mul(2))?
    Ok(())
```

```output
1
7
6
250
10
2
0
255
0
```

Use `wrapping_add`, `wrapping_sub`, or `wrapping_mul` when a calculation
deliberately wraps at the integer width, as in a hash or a random-number
generator. Ordinary arithmetic returns a typed overflow error. Parenthesize mixed
bitwise expressions when that makes the intended grouping easier to read.

A shift count must be nonnegative and smaller than the integer width. A literal
count outside that range, such as `bits << 8` on a `u8`, is rejected when the
program is compiled. A count held in a variable can be invalid, so the shift
can fail with `ShiftCountError[T]`, whose `count` field holds the rejected
count. It follows the same error rules as division by zero:

```plenty
def high_bits(value: u8, count: u8) -> Result[u8, ShiftCountError[u8]]:
    Ok(value >> count)

def show(value: u8, count: u8) -> Result[(), IoError]:
    match high_bits(value, count):
        case Ok(bits):
            print(bits)?
        case Err(ShiftCountError(rejected)):
            print("invalid shift count", rejected)?
    Ok(())

def main() -> Result[(), IoError]:
    show(200u8, 4u8)?
    show(200u8, 8u8)?
    Ok(())
```

```output
12
invalid shift count 8
```

Masking the count with a literal below the width, as in `value >> (count & 7u8)`,
keeps it in range, so that shift adds no error. As with divisors, an integer
module constant count or mask proves the same as its literal.
[Arithmetic that cannot fail](#arithmetic-that-cannot-fail) explains how the
compiler knows.

`rotate_left` and `rotate_right` move bits around the ends of an integer
instead of discarding them. The count has the integer's own type. No count
loses bits, so rotation never traps: a count of the full width leaves the value
unchanged.

```plenty
def main() -> Result[(), IoError]:
    bits = 129u8
    print(bits.rotate_left(1))?
    print(bits.rotate_right(1))?
    print(bits.rotate_left(8))?
    print(bits << 1)?
    Ok(())
```

```output
3
192
129
2
```

`pow` raises a number to a power, and each integer type names its limits as
`MIN` and `MAX`. As with the operators, the number and its exponent must have
one type, and the result has that type:

```plenty
def main() -> Result[(), IoError | OverflowError[i64]]:
    change = -7
    small = 200u8
    print(change.pow(2))?
    print((2).pow(10))?
    print(u8.MAX - small)?
    print(i64.MIN)?
    Ok(())
```

```output
49
1024
55
-9223372036854775808
```

There is no `**` operator. An integer literal receiver needs parentheses, as in
`(2).pow(10)`.

A result that does not fit returns the same typed overflow error as the
operators, and an integer `pow` always lists it. `u8.MAX - small` lists none:
both values are known. An integer power with a negative exponent still stops with
`negative integer exponent`; convert to a float for that.

## Recover from overflow

Declare the errors a calculation can produce and propagate them to its caller.
An arithmetic failure returns `Err` immediately; otherwise explicitly return
`Ok` with the successful value.

```plenty
def average(total: u8, count: u8) -> Result[u8, OverflowError[u8] | ZeroDivisionError]:
    rounded = total + count // 2
    Ok(rounded // count)

def main() -> Result[(), OverflowError[u8] | ZeroDivisionError | IoError]:
    print(average(200, 4)?)?
    print(average(250, 20).is_err())?
    print(average(250, 0).is_err())?
    Ok(())
```

```output
50
True
True
```

The error type lists what the function's operators can produce:
`OverflowError[u8]` for `+` on `u8`, and `ZeroDivisionError` for `//`.
Unsigned `//` cannot overflow, so it adds only `ZeroDivisionError`. `?` passes
either error to the caller, as with `try_from`. An `OverflowError` holds the
wrapped result in its `wrapped` field: `250 + 10` on a `u8` wraps to 4.
[Keep several error types](../03-absence/anonymous-sums.md) shows how to
handle each error instead of passing it on.
Arithmetic uses the function's error contract so formulas stay readable:
you do not need parentheses and `?` around each potentially failing operator.
The signature still tells callers which errors they must handle, without
exception handling or stack unwinding. Calls such as `try_from` keep their
explicit `Result` handling. See the
[arithmetic contract rationale](../../design/language/03-types/numbers.md#function-arithmetic-contracts).

For local recovery from arithmetic errors, use `checked:`. Its final expression becomes the
successful value of the block's `Result`; no explicit `Ok` is needed there:

```plenty
def increment_or_zero(value: i64) -> i64:
    result = checked:
        value + 1
    match result:
        case Ok(incremented):
            incremented
        case Err(_):
            0

def main() -> Result[(), IoError]:
    print(increment_or_zero(4), increment_or_zero(i64.MAX))?
    Ok(())
```
```output
5 0
```

Every named function and closure keeps its own declared error contract,
including when defined inside `checked:`. Calls returning `Result` still
need explicit handling.

Use `unchecked:` only where you can show the arithmetic cannot fail. It
yields an ordinary value, not a `Result`. A failure inside it terminates the
process, so do not use the suite to reject bad input that the caller should
be able to handle. Keep these suites small. See
[unchecked blocks](../../design/language/03-types/numbers.md#unchecked-blocks).
Integer [`std.math` helpers](../../design/stdlib/math.md) return ordinary
overflow results; their float variants remain infallible. Remaining std
implementations and parallel reductions still need the coordinated adoption
described in [B14](../../backlog.md). Calls never inherit a caller's mode.

## Arithmetic that cannot fail

The compiler gives each integer expression a range of possible values and
requires an error only where the result's range does not fit the type. The
ranges come from fixed rules, not from guesses:

- A literal, a module constant and `T.MIN` or `T.MAX` are one value.
- A variable bound once has the range of the expression it was bound to. A
  `mut` variable, a parameter and a call's result can be any value of the
  type.
- The variable of `for i in range(start, stop)` is between `start` and
  `stop - 1`.
- Inside `if n < 10:`, `n` is below 10 when `n` is a parameter or a variable
  bound once. A `mut` variable is never narrowed, because it can change.
- `x % 10` is in `0..9`, `x & 15` in `0..15`, and `i64(byte)` of a `u8` in
  `0..255`. `len(items)` is never negative.
- `+`, `-`, `*`, `//`, `%` and shifts combine the ranges of their operands.

```plenty
def mean(a: u8, b: u8) -> i64:
    (i64(a) + i64(b)) // 2

def check_digit(number: i64) -> i64:
    last = number % 10
    (last * 3 + 7) % 10

def main() -> Result[(), IoError]:
    print(mean(255, 255), check_digit(-13))?
    Ok(())
```
```output
255 8
```

`i64(a) + i64(b)` is at most 510, so `mean` returns a plain `i64`. In
`check_digit`, `last` is in `0..9`, so `last * 3 + 7` is in `7..34`. Written
with `u8` operands, `a + b` can exceed 255 and needs `OverflowError[u8]`:
the rule looks at ranges, never at what the values happen to be when the
program runs.

A comparison narrows a range only where it holds. Check the input once, at
the top of the function, and the rest of the body uses the proved range:

```plenty
const MAX_SIDE: i64 = 1000000

def area(width: i64, height: i64) -> i64:
    if width < 0 or width > MAX_SIDE or height < 0 or height > MAX_SIDE:
        return 0
    width * height

def main() -> Result[(), IoError]:
    print(area(3, 4), area(-1, 4))?
    Ok(())
```
```output
12 0
```

After the early `return`, both sides are in `0..1000000`, so the product
fits `i64`. Returning `0` for a bad side is a teaching simplification; an
application would usually return a `RangeError` the caller can see. `!=`
narrows nothing, and neither does a condition inside an `if` expression.
To carry a range across a call, in a parameter or field, give it a
[bounded type](../09-generics/value-parameters.md). A `checked:` block keeps
its error type when a range proves its arithmetic, so the block and the
`match` on its result stay valid. The
[interval reference](../../design/language/03-types/intervals.md) lists every
rule.

## Validate a divisor once

When a divisor comes from input, validate it with `NonZero.try_from` and pass
the resulting value to repeated divisions. A plain integer accepts that divisor
directly; the compiler omits its zero check.

```plenty
def per_group(total: i64, groups: NonZero[i64]) -> Result[i64, OverflowError[i64]]:
    Ok(total // groups)

def main() -> Result[(), IoError | RangeError | OverflowError[i64]]:
    count = 3
    groups = NonZero.try_from(count)?
    print(per_group(14, groups)?, 14 % groups)?
    Ok(())
```
```output
4 2
```

The quotient and remainder are ordinary integers and can be zero. Signed
`i64.MIN // -1` can still overflow, so the function's error type includes
`OverflowError[i64]`. A locally handled [checked block](#recover-from-overflow)
can instead keep this error out of the function signature.
See [nonzero integers](../../design/language/03-types/numbers.md#nonzero-integers)
for construction and arithmetic rules.
