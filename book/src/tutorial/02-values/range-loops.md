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
A third argument selects the step. A negative step counts down; zero is invalid.

The loop evaluates the range once. Each iteration binds `n` to the next
integer. Names declared in the body stay there; updates to an enclosing
`mut` binding persist. A loop completes with unit.

Creating and iterating a range do not allocate a list.
Ranges default to `i64`. Write `range[u8](8)` to choose another integer type.
Start and stop must fit that type, even though stop is excluded.
The step stays signed `i64`, so an unsigned range can count down.
