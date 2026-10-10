# Clean up owned values

Owned values clean up when their scope exits, including through `return`,
`break`, `continue`, and error propagation. A move transfers this
responsibility. Borrowing lends access while leaving cleanup with the owner.

Use `drop(value)` to release an owner early:

```plenty
def main() -> Result[(), Failure]:
    mut numbers = [1, 2, 3]?
    drop(numbers)
    numbers = [4]?
    print(numbers)?
    Ok(())
```

```output
[4]
```

The old contents are released and the consumed binding is unavailable until
reinitialized. Borrow checking prevents dropping an owner while a reference
still needs it. A borrow ending at its last use does not destroy the owner.

Strings may share immutable storage: dropping one does not invalidate another.
There is no tracing garbage collector. Fatal runtime traps terminate without
unwinding scopes. Later lessons explain how classes add custom cleanup and how
generators release their captured values.
