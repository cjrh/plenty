# Ranges

`range(stop)`, `range(start, stop)`, and `range(start, stop, step)` return
`range[T]` directly. Stop is exclusive; omitted start is zero and omitted step
is one. A range is a small copyable value: construction, passing, returning,
standard-sum wrapping, and repeated iteration allocate nothing. Iterating a
stored range is a [counted loop](../../language/09-iteration/01-iterables.md),
like a direct `range(...)` source. Collecting its elements into a collection is
fallible.

`range[T](...)` selects any integer type. Start and stop have type `T`; step is
always [`NonZero[i64]`](../../language/03-types/numbers.md#nonzero-integers),
permitting descending unsigned ranges. Without explicit type arguments, a typed
bound or range annotation selects `T`; otherwise it defaults to `i64`. Bounds
must fit `T`, so an exclusive `u8` stop of 256 is rejected.

A step literal such as `-2` converts at compile time, and `0` is a
compile-time error. A computed step is converted first, which handles zero as
an error value:

```plenty
def evens(stop: i64, stride: i64) -> Result[list[i64], AllocError | RangeError]:
    step = NonZero.try_from(stride)?
    Ok(list(range(0, stop, step))?)

def main() -> Result[(), AllocError | IoError | RangeError]:
    print(evens(9, 2)?)?
    print(list(range(5, 0, -2))?)?
    Ok(())
```

```output
[0, 2, 4, 6, 8]
[5, 3, 1]
```

Passing an `i64` step is a compile-time error: ``range step must be
`NonZero[i64]`, not `i64`; write a nonzero literal, or convert a value with
`NonZero.try_from(step)?` ``. A range therefore never has a zero step. A length
exceeding `i64` traps. Bounds and length handle the full `u64` domain without
intermediate overflow. `len`, `in`, `not in`, and `i64`
indexing are supported; negative indices count from the end and missing indices
trap. Membership is constant-time.

In a directly written comprehension range, a numeric output annotation can guide
the loop variable through direct arithmetic in the output expression:

```text
squares: list[u8] = [n * n for n in range(8)]?
```

This context does not flow backward through arbitrary calls, filters, stored
ranges, or explicit range arguments. Use `range[u8](...)` at those boundaries.
Arithmetic retains its [fixed-width rules](../../language/03-types/index.md).
