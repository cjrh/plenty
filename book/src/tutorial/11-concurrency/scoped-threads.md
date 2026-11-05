# Run scoped threads

Use `with spawn(...)` to run a function on a native thread. The current thread
continues until `join()` waits for the result:

```plenty
def squares(n: i64) -> Result[list[i64], AllocError | OverflowError[i64]]:
    [i * i for i in range(n)]

def main() -> Result[(), SpawnError[()] | AllocError | IoError | OverflowError[i64]]:
    with spawn(squares, 4).drop_job()? as first, spawn(squares, 3).drop_job()? as second:
        print(first.join()?)?
        print(second.join()?)?
    Ok(())
```
```output
[0, 1, 4, 9]
[0, 1, 4]
```

The `?` after `spawn(...)` handles failure to create the thread. The `?` after
`join()` handles the worker's own Result, here allocation failure or
arithmetic overflow while building its list. They are separate failures. `drop_job()` is explained in
[error types at thread start](#error-types-at-thread-start). Join consumes the task binding,
so it cannot be called twice.

Every started thread finishes before its `with` block exits. This includes
`return`, `?`, `break`, and `continue`. If you omit `join()`, exit waits and drops
the unused result, including any application error. Join when the result matters.
A task cannot escape its block or be stored for later.

Scopes cannot forcibly cancel workers. Keep their computations finite or give
them a shutdown protocol. Fatal traps terminate the process without unwinding.

## Error types at thread start

A failed start reports `SpawnError[F]`, which returns the unstarted job: here
the function and its arguments. `drop_job()` drops that job at the start, so the
error becomes `SpawnError[()]`, one member for every start. Keep the job when
you want to retry it, as [owned worker jobs](owned-worker-jobs.md) shows.
[Borrowed workers](borrowed-workers.md) report `SpawnError[()]` too.

Like any `?`, the start error widens into the enclosing Result's error sum, so
`main` lists `SpawnError[()]` beside the worker's `AllocError` and
`OverflowError[i64]`, and the printing `IoError`. A sum without the start error is rejected.

Use module-level [error aliases](../03-absence/anonymous-sums.md)
to keep longer signatures readable, as in [executor jobs](executors-and-futures.md). Each alias names that example's propagated
errors; it does not change widening through `?` or introduce an error wrapper.
