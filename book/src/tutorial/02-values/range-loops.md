# Repeat with a range

A `for` loop repeats its body for each integer in a range:

```plenty
def main() -> Result[(), IoError]:
    mut total = 0
    for n in range(1, 5):
        total = total + n
    print(total)?
    for n in range(5, 0, -2):
        print(n)?
    Ok(())
```

```output
10
5
3
1
```

`range(stop)` starts at zero. `range(start, stop)` selects the start.
The stop is always excluded: `range(1, 5)` yields 1, 2, 3, and 4.
A third argument selects the step. A negative step counts down. A zero step
would never reach the stop, so the step has type
[`NonZero[i64]`](../../design/language/03-types/numbers.md#nonzero-integers):
an integer that cannot be zero. A literal step converts, and `range(5, 0, 0)`
is a compile-time error. Convert a step computed while the program runs with
`NonZero.try_from`, which works like
[`u8.try_from`](numeric-operations.md) and returns
`Err(RangeError.OutOfRange)` for zero:

```plenty
def main() -> Result[(), RangeError | IoError]:
    stride = 3
    step = NonZero.try_from(stride)?
    for n in range(0, 10, step):
        print(n)?
    Ok(())
```

```output
0
3
6
9
```

Passing `stride` directly is a compile-time error whose message shows this
conversion. `step.get()` reads the integer back.

The loop evaluates the range once. Each iteration binds `n` to the next
integer. Names declared in the body stay there; updates to an enclosing
`mut` binding persist. A loop completes with unit.

Creating and iterating a range do not allocate a list.
Ranges default to `i64`. Write `range[u8](8)` to choose another integer type.
Start and stop must fit that type, even though stop is excluded.
The step stays a signed `NonZero[i64]`, so an unsigned range can count down.
