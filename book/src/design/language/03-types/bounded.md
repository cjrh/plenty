# Bounded integers

`Bounded[T, lo, hi]` stores an integer `T` in the **inclusive** interval
`lo <= value <= hi`. `T` is any of the eight integer types. The two bounds are
[value arguments](../07-generics/values.md) of exactly that type. Bounds outside
`T` or a lower bound above the upper bound are compile errors, including when
an instance is used only in an alias, field or signature. Equal bounds are valid.

Use a type alias to name a domain:

```plenty
type Percent = Bounded[u8, 0, 100]

def set_percent(value: Percent = 50) -> u8:
    value.get()

def main() -> Result[(), IoError | RangeError]:
    literal: Percent = 25
    configured = 75u8
    checked = Percent.try_from(configured)?
    print(set_percent(literal), set_percent(100), checked.get())?
    print(Percent.try_from(101).is_err())?
    Ok(())
```
```output
25 100 75
True
```

## Construction and extraction

| Operation | Result |
| --- | --- |
| Integer literal where `Bounded[T, lo, hi]` is expected | Checked at compile time; no allocation or `Result` |
| `Bounded[T, lo, hi].try_from(value)` or an alias's `.try_from(value)` | `Result[Bounded[T, lo, hi], RangeError]`; `RangeError.OutOfRange` outside the interval |
| `Bounded[T, lo, hi].from(value)` or an alias's `.from(value)` | `Bounded[T, lo, hi]` with no run-time check; accepted only for a [proved range](#proved-ranges) |
| `value.get()` | The stored `T`, copied without allocation |

Expected literal contexts include annotated bindings, assignment, concrete
function parameters and returns, defaults, constructor fields and typed
collection elements. A suffixed literal must have exactly `T`. Plain computed
integers require `try_from`, even when the programmer knows they fit, unless
`from` accepts their [proved range](#proved-ranges). A
`try_from` input is evaluated once and may be `T` or a reference to `T`.
There is no implicit conversion between different bounded types or from a
bounded value to its stored integer. The constructor and field are private.

```plenty-error
def main() -> ():
    percent: Bounded[u8, 0, 100] = 101
```
```error
outside `Bounded[u8, 0, 100]`'s inclusive bounds
```

## Proved ranges

`from` converts a value whose [interval](intervals.md) lies inside `lo..hi`.
It has no run-time check and no `Result`. The interval comes from the
argument expression alone, by the
[interval rules](intervals.md#operators). The argument is an integer `T`, or a
`Bounded[T, a, b]` of the same `T`, whose interval is `a..b`.

| Argument | Interval |
| --- | --- |
| `x % c`, with `c` a literal or module constant above zero | `0..c - 1` |
| `x & c` or `c & x`, with `c` a literal or module constant of at least zero | `0..c` |
| `b.get() + 1`, with `b` a `Bounded[T, 0, 9]` | `1..10` |
| A `Bounded[T, a, b]` value of the same `T` | `a..b` |
| A parameter inside `if 0 <= n and n <= 9:` | `0..9`, by [comparisons](intervals.md#comparisons) |
| A `mut` local, field or call result of type `T`, or a parameter no comparison narrows | `T.MIN..T.MAX` |

An interval outside `lo..hi` is a compile error that names it. A `%` or `&`
implemented by a class's operator method proves nothing. There is no unchecked
conversion: a value the checker cannot prove goes through `try_from`.

```plenty
const MODULUS: i64 = 139968
const LARGEST: i64 = MODULUS - 1
type Seed = Bounded[i64, 0, LARGEST]

def advance(seed: Seed) -> Seed:
    Seed.from((seed.get() * 3877 + 29573) % MODULUS)

def main() -> Result[(), IoError]:
    wider = Bounded[i64, 0, 1000000].from(advance(42))
    next = Bounded[i64, 1, 1000001].from(wider.get() + 1)
    print(advance(42).get(), next.get(), Bounded[u8, 0, 15].from(0xabu8 & 0x0f).get())?
    Ok(())
```
```output
52439 52440 11
```

```plenty-error
def main() -> ():
    digit = Bounded[i64, 0, 9].from(1234 % 16)
```
```error
this value is in 0..15, which is not inside `Bounded[i64, 0, 9]`'s inclusive bounds 0..9
```

```plenty-error
def index(n: i64) -> Bounded[i64, 0, 9]:
    Bounded[i64, 0, 9].from(n)

def main() -> ():
    index(3)
```
```error
this value is in -9223372036854775808..9223372036854775807, which is not inside `Bounded[i64, 0, 9]`'s inclusive bounds 0..9; use `Bounded.try_from(value)?` to check it at run time
```

## Value behavior

A bounded integer is a copyable private single-field class. It stores only `T`;
the bounds are type metadata and add no fields. It follows ordinary class
borrowing, equality, hashing and cleanup behavior. Equality requires matching
bounded types. Default formatting shows the class and its value; `.get()` gives
the ordinary integer's formatting and ordering operations.

Arithmetic requires `.get()`, which returns an integer whose
[interval](intervals.md) is `lo..hi`. Arithmetic on it that stays inside `T`
cannot fail, so a function computing `seed.get() * 3877 + 29573` from a
`Bounded[i64, 0, 139967]` declares no `OverflowError`. The result is an
ordinary integer: convert it with `from` when its interval is
[proved](#proved-ranges), or check it with `try_from`.

`Positive[T]` and `NonNegative[T]` remain useful for their fixed lower bounds
and domain-preserving operations. Use `Bounded` when a ceiling or another lower
bound is part of the type. Existing standard-library limit signatures retain
their documented types and dynamic checks; the availability of `Bounded` does
not itself change them. Module constants may supply the bounds, but a module
constant cannot yet have a `Bounded` result type.

A bounded integer has no C ABI: see
[domain types and foreign code](numbers.md#domain-types-and-foreign-code).
