# Borrow in a closure

Use `&` in a capture list to keep ownership outside the closure:

```plenty
def main() -> Result[(), Failure]:
    mut values = [4]?
    read = def [&values]() -> i64:
        values[0]
    print(read())?
    moved = read
    print(moved())?
    values.append(5)?
    print(values)?
    Ok(())
```
```output
4
4
[4, 5]
```

The list remains owned by `values`. Its shared borrow follows the closure through
the move into `moved`, and ends after the last call. Changing the list before a
later call would be rejected. Borrowing closures cannot be returned from the
function that creates them or stored in collections or sum types. Other functions
can use them through reference parameters:

```plenty
def invoke(f: &Closure[[], i64]) -> i64:
    f()

def main() -> Result[(), Failure]:
    value = 7
    read = def [&value]() -> i64:
        value
    print(invoke(read))?
    Ok(())
```
```output
7
```

The `&Closure` parameter borrows `read` implicitly; `invoke(&read)` is also valid.
The capture remains borrowed throughout `invoke`, including while its other
arguments are being evaluated. A closure can also explicitly borrow another
closure; the same rule protects all of their captured owners.

Use `&mut` to modify an existing binding rather than moving it into private state:

```plenty
def main() -> Result[(), Failure]:
    mut count = 0
    mut increment = def [&mut count]() -> ():
        count = count + 1
    increment()
    increment()
    print(count)?
    Ok(())
```
```output
2
```

Both the source and closure bindings need `mut`. The original count becomes
available again after the last closure call. Reading it before another call would
conflict with the exclusive capture.
