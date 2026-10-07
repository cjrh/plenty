# 23. Cleanup and early drop

Owned values clean up automatically when their scope exits, including through
`return`, `break`, and `continue`. Moving a value transfers that responsibility.
A borrow never becomes responsible for destroying the original value.

Use `drop(value)` to release an owner earlier. The consumed binding becomes
unavailable; a mutable binding may then receive another value:

```plenty
def pending() -> Generator[i64]:
    print("started").unwrap()
    yield 1

def main() -> Result[(), Failure]:
    mut numbers = [1, 2, 3]?
    drop(numbers)
    numbers = [4]?
    print(numbers)?

    task = pending()
    drop(task)
    print("done")?
    Ok(())
```

```output
[4]
done
```

Dropping a generator cleans its captured values without resuming its body.
Borrow checking prevents dropping an owner while a reference still needs it.
A borrow can end at its last use; that does not implicitly destroy its owner.

Strings may share immutable storage. Dropping one string owner does not invalidate
another. There is no tracing garbage collector. Fatal runtime traps terminate
without unwinding scopes. Classes can provide custom cleanup with `__del__`, as
the next lesson shows.
