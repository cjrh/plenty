# Run scoped threads

Use `with spawn(...)` to run a function on a native thread while the current
thread continues. `join()` waits and returns the answer. Here both calculations
can run concurrently, and the parent prints their results in a fixed order:

```plenty
def squares(n: i64) -> Result[list[i64], AllocError]:
    [i * i for i in range(n)]

def main() -> Result[(), Failure]:
    with spawn(squares, 4)? as first, spawn(squares, 3)? as second:
        print(first.join()?)?
        print(second.join()?)?
    Ok(())
```
```output
[0, 1, 4, 9]
[0, 1, 4]
```

The `?` after `spawn(...)` handles failure to create the thread. The `?` after
`join()` handles the worker's own Result, here an allocation failure while
building its list. They are separate failures. Join consumes the task binding,
so it cannot be called twice.

Every started thread finishes before its `with` block exits. This includes
`return`, `?`, `break`, and `continue`. If you omit `join()`, exit waits and drops
the unused result, including any application error. Join when the result matters.
A task cannot escape its block or be stored for later.


A scope waits for every worker to return; it cannot forcibly cancel one. Keep
worker computations finite, or give them a shutdown protocol. The channel and
cancellation lessons below show how waiting workers can finish normally.
Fatal traps terminate the process rather than unwinding scopes.
