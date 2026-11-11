# Integer intervals

The checker gives every integer expression an **interval**: an inclusive range
`lo..hi` that contains every value the expression can have. An interval is a
proof, derived from the expression by the fixed rules on this page. There is no
solver and no heuristic: the same source always has the same intervals.

The checker uses an interval in three ways:

| Use | Rule |
| --- | --- |
| Arithmetic | An operation whose result interval lies inside its type cannot fail. It adds no error to the function's contract or to a `checked:` block, and the generated code has no check for it. |
| Division | `//` and `%` whose divisor interval excludes zero add no `ZeroDivisionError`. `//` adds no `OverflowError[T]` unless the dividend can be `T.MIN` and the divisor can be `-1`. |
| Conversion | [`Bounded[T, lo, hi].from(e)`](bounded.md#proved-ranges) accepts `e` exactly when its interval lies inside `lo..hi`. An integer cast `T(e)` whose operand interval lies inside `T` has no run-time check. |

Anything the rules do not prove keeps its check and its error. There is no
annotation that asserts an interval.

```plenty
const MODULUS: i64 = 139968
type Seed = Bounded[i64, 0, 139967]

def advance(seed: Seed) -> Seed:
    Seed.from((seed.get() * 3877 + 29573) % MODULUS)

def mean(a: u8, b: u8) -> i64:
    (i64(a) + i64(b)) // 2

def main() -> Result[(), IoError]:
    print(advance(42).get(), mean(255, 255))?
    Ok(())
```
```output
52439 255
```

`seed.get()` is in `0..139967`, the product in `0..542652059`, the sum in
`29573..542681632`, and the remainder in `0..139967`. Every step fits `i64`,
so `advance` declares no error, and the remainder fits `Seed`.

## Sources

| Expression of integer type `T` | Interval |
| --- | --- |
| Integer literal; integer [module constant](../02-source/modules.md#constants) | Its value |
| `T.MIN`, `T.MAX`, where `T` is an integer type, an alias of one, or a type parameter instantiated with one | Its value |
| `b.get()` on a `Bounded[T, lo, hi]` | `lo..hi` |
| `n.get()` on a `NonNegative[T]` | `0..T.MAX` |
| `n.get()` on a `Positive[T]`, or on a `NonZero[T]` with unsigned `T` | `1..T.MAX` |
| `len(x)` of a built-in collection, string or view | `0..i64.MAX` |
| An immutable local bound by `name = e` or `name: T = e` | The interval of `e` |
| Parenthesized `(e)`; unary `+e` | The interval of `e` |
| Every other expression | `T.MIN..T.MAX` |

"Every other expression" includes parameters, `mut` locals, loop variables,
pattern bindings, fields, elements, calls, and a `NonZero[T]` of a signed
`T`. A comparison in an `if` or `while` does not
narrow an interval.

A source is decided by the resolved type or callee, never by spelling.
`len(x)` on a class, enum or protocol value calls its own `len` method and
has the full range of `i64`, as does `get()` on a class. A parameter or local
that shadows a module constant is not that constant.

## Operators

`a` and `b` are operands of one integer type `T`, with intervals `a.lo..a.hi`
and `b.lo..b.hi`.

| Expression | Result interval | Cannot fail when |
| --- | --- | --- |
| `a + b` | `a.lo + b.lo..a.hi + b.hi` | The interval is inside `T` |
| `a - b` | `a.lo - b.hi..a.hi - b.lo` | The interval is inside `T` |
| `a * b` | Least to greatest of the four products of the operands' bounds | The interval is inside `T` |
| `-a` | `-a.hi..-a.lo` | The interval is inside `T` |
| `a // b` | Least to greatest floored quotient of the operands' bounds, taking `b` below and above zero separately | `b` excludes `0`, and `a` excludes `T.MIN` or `b` excludes `-1` |
| `a % b`, `b` above zero | `0..b.hi - 1`; `0..min(a.hi, b.hi - 1)` when `a.lo >= 0` | `b` excludes `0` |
| `a % b`, `b` below zero | `b.lo + 1..0`; `max(a.lo, b.lo + 1)..0` when `a.hi <= 0` | `b` excludes `0` |
| `a & b` | `0..` the smaller upper bound of the operands that are at least zero; no rule when both can be negative | Always |
| `a >> b` | Least to greatest floored `a // 2**b` over the bounds | `b` is inside `0..N - 1` for an `N`-bit `T` |
| `a << b` | Least to greatest `a * 2**b` over the bounds when that is inside `T`; otherwise `T.MIN..T.MAX` | `b` is inside `0..N - 1` |
| `T(e)`, `e` an integer of another type | The interval of `e` | It is inside `T` |
| `\|`, `^`, `~`, `pow`, and every method | `T.MIN..T.MAX` | Their own rules |

A divisor interval that spans zero uses both remainder rows. The result of an
operation that can fail is `T.MIN..T.MAX`, whether the failure is returned,
handled by `checked:`, or assumed away in `unchecked:`. `%` follows
[floored division](numbers.md): the remainder has the divisor's sign.

Interval bounds are computed in a wider integer than any `T`, and a bound that
does not fit it proves nothing, so the computation itself cannot wrap.

```plenty
def digit(n: i64) -> i64:
    last = n % 10
    last * last + 1

def shifted(bits: u32, count: u32) -> u32:
    bits >> (count & 31)

def middle(items: &list[i64]) -> i64:
    len(items) // 2

def main() -> Result[(), AllocError | IoError]:
    items = [1, 2, 3]?
    print(digit(-3), shifted(256u32, 36u32), middle(&items))?
    Ok(())
```
```output
50 16 1
```

`last` is immutable, so its interval `0..9` reaches the next line. A `mut`
local has none:

```plenty-error
def total(n: i64) -> i64:
    mut last = n % 10
    last * last + 1

def main() -> ():
    total(3)
```
```error
arithmetic can fail with OverflowError[i64]
```

## Declared errors

A function may declare an error that its body cannot return: a signature is a
contract with callers, not a description of the body. Arithmetic that an
interval proves infallible therefore never makes a declared `OverflowError`,
`ZeroDivisionError` or `ShiftCountError` wrong. Removing the error from the
signature is always allowed, and callers then stop handling it.

A `checked:` block is different: a block with no arithmetic that can fail is a
[compile error](numbers.md#checked-blocks), so a block whose arithmetic is all
proved must be removed. Proofs also narrow a block's error type: its
`Result` lists only the errors of the operations that were not proved.

```plenty-error
def sum(a: u8, b: u8) -> i64:
    result = checked:
        i64(a) + i64(b)
    result.unwrap_or(0)

def main() -> ():
    sum(1, 2)
```
```error
this `checked:` block contains no arithmetic that can fail
```

The same expression in a generic function can therefore be valid for one
instantiation and rejected for another: `checked:` around
`a % T(100) + T(100)` has an `OverflowError[i8]` to handle for `i8` and
nothing to check for `i16`.

Both rules can reject a program that compiled before intervals existed: a
`match` arm for an error the block can no longer produce, and a block that
is now fully proved.

## Generated code

The checker's verdict is the only thing that removes an error from a contract,
and it is passed to code generation with the operation: an operation the
checker proved is lowered without its check, and one it did not prove keeps
the check. The two cannot disagree, because code generation does not repeat
the interval computation.

The [operation validator](../../toolchain/02-architecture/emission.md#value-bounds)
recomputes a proof from its operands where their intervals are visible in the
operation sequence, and otherwise checks only that the claimed interval fits
the type.

Code generation also derives [value bounds](../../toolchain/02-architecture/emission.md#value-bounds)
of its own, for example for a loop counter or a local assigned a remainder in
a loop. Those remove instructions only. Where they prove more than the rules
above, the check disappears from the generated code and the error stays in the
contract.

## Limits

- Intervals are local to one function body. A call's result has the full range
  of its type, whatever the callee returns.
- Only immutable locals carry an interval between statements. Parameters need
  a `Bounded`, `NonNegative` or `Positive` type to carry one.
- Conditions do not narrow: after `if n < 10:`, `n` still has its full range.
- A generic function is checked for each instantiation, with the intervals of
  that integer type: `a % 100 + 100` cannot fail for `i16` and can for `i8`.
- `len(x)` has an interval for built-in collections, strings and views. A
  class's own `len` method returns any `i64`.
