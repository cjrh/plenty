# Borrow local data for a worker

Borrow mutable inputs explicitly. Shared readers can coexist; an exclusive
borrow prevents the parent from accessing the same data until the block ends.
Omit `as` when you only need the worker's effects:

```plenty
def increment(value: &mut i64) -> ():
    *value = *value + 1

def main() -> Result[(), ThreadError | IoError]:
    mut count = 4
    with spawn(increment, &mut count)?:
        pass
    print(count)?
    Ok(())
```
```output
5
```

The borrow lasts until the block ends, even after an explicit join. Reading or
modifying `count` inside the block conflicts with it; use a shorter block when
you need access back sooner.

Reusable closures work too. Borrow the closure itself so that a failed start
leaves the job available to retry:

```plenty
def main() -> Result[(), ThreadError | IoError | AllocError]:
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

For immutable captures, `spawn(&job)` allows shared access. Scoped work may
borrow or own its inputs, so a named worker can also receive an owned list:

```plenty
def total(values: list[i64]) -> i64:
    mut result = 0
    for value in values:
        result = result + value
    result

def main() -> Result[(), SpawnError[()] | IoError | AllocError]:
    values = [2, 3, 4]?
    with spawn(total, values).drop_job()? as task:
        print(task.join())?
    Ok(())
```
```output
9
```

`values` moves and cannot be used again. Because every argument is owned, the
start reports `SpawnError`, like the [scoped threads](scoped-threads.md) lesson's
starts: without `drop_job()` a failed start would return `values` inside the
error. A start that borrows reports `ThreadError` instead, because an error
cannot yet hold a borrow. If such a start fails, its moved arguments drop on the
parent thread.

To preserve creation error details, put the scope in a helper and match its
result. `ThreadError.System(code)` and `SpawnError.Unavailable(job, code)` both
contain the native error code.

The compiler checks worker helpers, captured data, and destructors. Foreign
calls, File values, indirect function calls, and generators are not yet eligible.
Worker code can use checked `print`, but output from different threads may
arrive in any order.
