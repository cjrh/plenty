# Run scoped threads

Use `with spawn(...)` to run a function on a native thread. The current thread
continues until `join()` waits for the result:

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

Scopes cannot forcibly cancel workers. Keep their computations finite or give
them a shutdown protocol. Fatal traps terminate the process without unwinding.

## Error types at thread start

The `Failure` signature above is a workaround for a current compiler limit.
Scoped `spawn(...)?` requires exactly `ThreadError` (or `SpawnError[F]` for an
owned job), or the erasing `Failure` type. It does not yet accept a sum containing
the start error, so examples combining a start with allocation or printing keep
`Failure`. Worker functions can still return precise errors, as `squares` does.
Ordinary calls and `?` outside this special start operation support error sums.
For a small scope, a helper returning `Result[T, ThreadError]` can preserve the
start error and let its caller compose it with other errors.
