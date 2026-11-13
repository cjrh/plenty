# Integer intervals

The checker gives every integer expression an **interval**: an inclusive range
`lo..hi` that contains every value the expression can have. An interval is a
proof, derived from the expression by the fixed rules on this page. There is no
solver and no heuristic: the same source always has the same intervals.

The checker uses an interval in three ways:

| Use | Rule |
| --- | --- |
| Arithmetic | An operation whose result interval lies inside its type cannot fail. It adds no error to the function's contract, and the generated code has no check for it. A [`checked:` block](#checked-blocks) keeps its error type. |
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
| The variable of `for name in range(...)` | Between the start and the stop: see [range loops](#range-loops) |
| An immutable local, parameter or loop variable where a comparison holds | Its interval met with the comparison's bound: see [comparisons](#comparisons) |
| Parenthesized `(e)`; unary `+e` | The interval of `e` |
| Every other expression | `T.MIN..T.MAX` |

"Every other expression" includes `mut` locals, pattern bindings, fields,
elements, calls, a `NonZero[T]` of a signed `T`, the variable of a loop over
anything other than a `range(...)` call, and a parameter where no comparison
narrows it.

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
handled by `checked:`, or a trap in `unchecked:`. `%` follows
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

## Range loops

The variable of a `for` loop written directly over a `range(...)` call has an
interval from the intervals of the call's arguments. The arguments are
evaluated once, before the first iteration, and the variable is immutable, so
the interval holds for every iteration.

| Loop | Variable interval |
| --- | --- |
| `for i in range(stop)` | `0..stop.hi - 1` |
| `for i in range(start, stop)` | `start.lo..stop.hi - 1` |
| `for i in range(start, stop, step)`, `step` a literal or module constant above zero | `start.lo..stop.hi - 1` |
| `for i in range(start, stop, step)`, `step` a literal or module constant below zero | `stop.lo + 1..start.hi` |
| Any other step, or an empty interval | `T.MIN..T.MAX` |

An empty interval means the loop body never runs. A name bound in the body
that shadows the variable is a new local with its own interval. A `range`
value stored in a variable and iterated later proves nothing, nor does
`enumerate`.

```plenty
const STEPS: i64 = 4
type Percent = Bounded[i64, 0, 100]

def main() -> Result[(), IoError]:
    for i in range(STEPS + 1):
        share = Percent.from(i * 25)
        print(share.get(), (STEPS - i) * 25)?
    Ok(())
```
```output
0 100
25 75
50 50
75 25
100 0
```

`i` is in `0..4`, so `i * 25` is in `0..100` and fits `Percent`, and
`STEPS - i` is in `0..4`.

## Comparisons

An integer comparison narrows an immutable local, a parameter or a loop
variable of an integer type where the comparison is known to hold. A loop over
a borrowed collection binds a borrow such as `&i64`, which is not narrowed. Such a binding cannot change
after it is bound, so a comparison that held once still holds. The binding is
an operand of `<`, `<=`, `>`, `>=` or `==`, on either side and of the
comparison's type; the other operand is any integer expression.

| Holds | Left operand `a` | Right operand `b` |
| --- | --- | --- |
| `a < b` | `T.MIN..b.hi - 1` | `a.lo + 1..T.MAX` |
| `a <= b` | `T.MIN..b.hi` | `a.lo..T.MAX` |
| `a > b` | `b.lo + 1..T.MAX` | `T.MIN..a.hi - 1` |
| `a >= b` | `b.lo..T.MAX` | `T.MIN..a.hi` |
| `a == b` | `b.lo..b.hi` | `a.lo..a.hi` |
| `a != b` | Nothing | Nothing |

The binding's interval becomes its current interval met with that bound. Two
bindings compared with each other both narrow. Where a condition is known
false, the opposite comparison holds: `a < b` false is `a >= b`, and `a == b`
false is `a != b`, which narrows nothing.

| Region | What holds |
| --- | --- |
| The suite of `if c:` or `elif c:` | `c` true, and each earlier `if` and `elif` condition false |
| The `else:` suite | Every `if` and `elif` condition false |
| The body of `while c:` | `c` true |
| The right operand of `a and b` | `a` true |
| The right operand of `a or b` | `a` false |
| The statements after an `if` statement in the same suite, when every suite but one ends in `return`, `break` or `continue`, or in an `if` or `match` whose every suite does | What holds in the one suite that continues; without an `else:`, that can be the implicit `else` |

A condition is true when `not c` is false, and `a and b` true means both
operands true; `a or b` false means both false. No other condition proves
anything: a call, a `bool` local or field, `a or b` true and `a and b` false
narrow nothing. A bound that excludes the binding's whole interval, as in
`if byte > 300u8:` on a `u8`, marks code that cannot run, and the binding
keeps its interval there.

```plenty
const LIMIT: i64 = 1000000

def area(width: i64, height: i64) -> i64:
    if width < 0 or width > LIMIT or height < 0 or height > LIMIT:
        return 0
    width * height

def upper(c: u8) -> u8:
    if c >= 97u8 and c <= 122u8:
        c - 32u8
    else:
        c

def main() -> Result[(), IoError]:
    print(area(3, 4), area(-1, 4), upper(104u8), upper(72u8))?
    Ok(())
```
```output
12 0 72 72
```

After the early `return`, `width` and `height` are in `0..1000000`, so the
product fits `i64` and `area` declares no error. In `upper`, `c` is in
`97..122`, so `c - 32u8` cannot wrap.

A `mut` local, a field, an element and a call result are never narrowed: each
can change, or be a different value, between the comparison and the
operation.

```plenty-error
def double(n: i64) -> i64:
    mut m = n
    if m < 1000:
        m * 2
    else:
        0

def main() -> ():
    double(3)
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

## Checked blocks

A [`checked:` block](numbers.md#checked-blocks) exists so that a function can
resolve the arithmetic errors of some operations in one place instead of
declaring them in its signature. That must keep working when an interval
proves one of those operations infallible, and for every instantiation of a
generic function. So the block's error type does not depend on interval
proofs: it is the error type the block's operators and operand types give by
the [operation table](numbers.md#checked-blocks) alone, including that
table's exceptions for a literal or constant divisor or count, a masked count
and a `NonZero` or `Positive` divisor.

```plenty
def sum(a: u8, b: u8) -> i64:
    result = checked:
        i64(a) + i64(b)
    match result:
        case Ok(value):
            value
        case Err(OverflowError(wrapped)):
            wrapped

def main() -> Result[(), IoError]:
    print(sum(200, 100))?
    Ok(())
```
```output
300
```

`result` is a `Result[i64, OverflowError[i64]]` although the addition cannot
overflow. The proof still removes the check from the generated code, so the
`Err` arm is never taken; an arm for an error that cannot occur is legal and
has no warning. A generic body is accepted alike for every instantiation:
`checked:` around `a % T(100) + T(100)` is a
`Result[T, OverflowError[T] | ZeroDivisionError]` for `i8`, where the
addition can overflow, and for `i16`, where it cannot.

A block with no arithmetic that the table lists as able to fail is still a
[compile error](numbers.md#checked-blocks). That rule does not depend on
intervals either: `checked:` around `i64(a) + i64(b)` is accepted, and around
`n // 2` it is rejected.

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
  a `Bounded`, `NonNegative` or `Positive` type to carry one outside a
  [comparison](#comparisons).
- `!=` narrows nothing, even where it excludes a bound.
- Match arms, `if` expressions and comprehension conditions do not narrow:
  `x * 2 if x < 1000 else 0` still needs `OverflowError`, and a `case`
  binding has the full range of its type.
- A `while` condition narrows its body only, not the statements after the
  loop.
- A generic function is checked for each instantiation, with the intervals of
  that integer type: `a % 100 + 100` cannot fail for `i16` and can for `i8`.
  A signature that must be valid for every instantiation declares the error;
  a [`checked:` block](#checked-blocks) has the same type for each.
- `len(x)` has an interval for built-in collections, strings and views. A
  class's own `len` method returns any `i64`.
