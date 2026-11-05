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
| `value.get()` | The stored `T`, copied without allocation |

Expected literal contexts include annotated bindings, assignment, concrete
function parameters and returns, defaults, constructor fields and typed
collection elements. A suffixed literal must have exactly `T`. Plain computed
integers require `try_from`, even when the programmer knows they fit. A
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

## Value behavior

A bounded integer is a copyable private single-field class. It stores only `T`;
the bounds are type metadata and add no fields. It follows ordinary class
borrowing, equality, hashing and cleanup behavior. Equality requires matching
bounded types. Default formatting shows the class and its value; `.get()` gives
the ordinary integer's formatting and ordering operations.

Arithmetic requires `.get()`. The compiler does not derive new intervals for
arithmetic or widen intervals automatically. Check the result again with
`try_from` if it must re-enter a bounded domain. Arithmetic on the extracted
integer retains ordinary overflow/error contracts.

`Positive[T]` and `NonNegative[T]` remain useful for their fixed lower bounds
and domain-preserving operations. Use `Bounded` when a ceiling or another lower
bound is part of the type. Existing standard-library limit signatures retain
their documented types and dynamic checks; the availability of `Bounded` does
not itself change them. Module constants may supply the bounds, but a module
constant cannot yet have a `Bounded` result type.
