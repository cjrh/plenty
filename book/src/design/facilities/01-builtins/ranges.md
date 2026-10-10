# Ranges

`range(stop)`, `range(start, stop)`, and `range(start, stop, step)` return
`range[T]` directly. Stop is exclusive; omitted start is zero and omitted step
is one. A range is a small copyable value: construction, passing, returning,
standard-sum wrapping, and repeated iteration allocate nothing. Collecting its
elements into a collection is fallible.

`range[T](...)` selects any integer type. Start and stop have type `T`; step is
always signed `i64`, permitting descending unsigned ranges. Without explicit type
arguments, a typed bound or range annotation selects `T`; otherwise it defaults
to `i64`. Bounds must fit `T`, so an exclusive `u8` stop of 256 is rejected.

A zero step or length exceeding `i64` traps. Bounds and length handle the full
`u64` domain without intermediate overflow. `len`, `in`, `not in`, and `i64`
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
