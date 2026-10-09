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

## Borrow data for a worker

Borrow mutable inputs explicitly. Shared readers can coexist; an exclusive
borrow prevents the parent from accessing the same data until the block ends.
Omit `as` when you only need the worker's effects:

```plenty
def increment(value: &mut i64) -> ():
    *value = *value + 1

def main() -> Result[(), Failure]:
    mut count = 4
    with spawn(increment, &mut count)?:
        pass
    print(count)?
    Ok(())
```
```output
5
```

Reading or modifying `count` inside the block would be a conflicting borrow.
The borrow currently lasts until the block ends even if you explicitly join
earlier. Use a shorter block when you need access back sooner.

Reusable closures work too. Borrow the closure itself so that a failed start
leaves the job available to retry:

```plenty
def main() -> Result[(), Failure]:
    mut values = [1, 2]?
    mut job = def [&mut values]() -> Result[i64, AllocError]:
        values.append(3)?
        Ok(len(values))
    with spawn(&mut job)? as task:
        print(task.join()?)?
    print(values)?
    Ok(())
```
```output
3
[1, 2, 3]
```

For immutable captures, `spawn(&job)` allows shared access. Owned mutable inputs
must currently be borrowed or held in such a reusable job; `spawn` does not consume
them. The worker can return owned data normally.

To preserve creation error details, put the scope in a helper returning
`Result[T, ThreadError]` and match its result. `ThreadError.System(code)` contains
the native error code. Task bookkeeping itself needs no heap allocation, but
native thread stacks still require OS resources.

The compiler checks worker helpers, captured data, and destructors. Foreign
calls, File values, indirect function calls, and generators are not yet eligible.
Worker code can use checked `print`, but output from different threads may arrive
in any order. A worker must finish for its scope to exit; there is no forced
cancellation. Fatal traps terminate the process rather than unwinding scopes.
