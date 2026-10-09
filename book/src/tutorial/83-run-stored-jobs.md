# Run stored jobs

Stored reusable callbacks can run on scoped threads. Borrow them explicitly, just
as you would borrow a local closure. Distinct tuple slots have separate loans.

```plenty
def counter(total: i64) -> Closure[[], i64]:
    def [mut total]() -> i64:
        total = total + 1
        total

def main() -> Result[(), Failure]:
    mut jobs = (counter(10), counter(100))?
    with spawn(&mut jobs[0])? as first, spawn(&mut jobs[1])? as second:
        print(first.join())?
        print(second.join())?
    print(jobs[0]())?
    Ok(())
```

```output
11
101
12
```

The main thread prints in a defined order by joining each result in that order.
The two worker bodies may run concurrently. A failed start leaves its stored
callback owned by the caller, and any earlier successful start is joined.

Workers can also build an owned callback and transfer it back through `join()`.
This does not allocate an extra environment object.

```plenty
def make(offset: i64) -> Closure[[i64], i64]:
    def [offset](n: i64) -> i64:
        offset + n

def main() -> Result[(), Failure]:
    with spawn(make, 10)? as task:
        callback = task.join()
        print(callback(3))?
    Ok(())
```

```output
13
```

List and dictionary entries can also supply a borrowed job, but their loans
protect the whole collection. Even after `join()`, wait until the `with` block
ends before resizing or replacing that collection. The compiler checks captures,
callback bodies, and destructors for thread eligibility; putting a callback in a
container does not bypass those checks.
