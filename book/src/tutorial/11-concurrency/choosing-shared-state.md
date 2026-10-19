# Choose how workers share state

Prefer giving each worker what it needs and taking results back. Reach for shared
mutable state only when workers must see each other's changes while running.

| Need | Use |
|---|---|
| Inputs a worker reads or owns | Shared borrows, or move the values in |
| Results to collect | `task.join()`, or send them on a [channel](channels-and-shutdown.md) |
| One independent counter or flag | An [atomic](atomic-counters.md) |
| Several values that must change together | A [mutex](shared-mutex.md) |
| State that executor jobs share | A [`Shared` handle](shared-pool-state.md) to an atomic or a mutex |
| A request to stop | A [cancellation token](cooperative-cancellation.md) |

Here each worker sums its own slice of the work and returns the result, so
nothing is shared and nothing needs a lock:

```plenty
def total(start: i64, end: i64) -> i64:
    mut sum = 0
    for value in range(start, end):
        sum = sum + value
    sum

def main() -> Result[(), Failure]:
    with spawn(total, 0, 500)? as low, spawn(total, 500, 1000)? as high:
        print(low.join() + high.join())?
    Ok(())
```
```output
499500
```

An atomic is the cheapest shared update, but each operation is separate: two
atomics updated one after the other can be observed half-done. A mutex makes a
whole block of updates appear at once, at the cost of making other threads wait.

Atomics and mutexes are shared by borrowing, so they work with scoped threads.
Executor jobs must own their inputs, so they cannot borrow either one. Give each
job its own owner of a `Shared[Atomic[T]]` or `Shared[Mutex[T]]` instead, or
send jobs their data and collect their results.
