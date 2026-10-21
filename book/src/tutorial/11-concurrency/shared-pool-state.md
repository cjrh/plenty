# Share state with pool workers

Scoped threads can borrow an atomic or a mutex, but executor jobs must own their
inputs. `Shared[T]` gives a value several owners. Hand each job its own owner
with `share()`:

```plenty
from std.atomic import Atomic, Ordering
from std.sync import Mutex

class Stats:
    count: i64
    total: i64

def work(hits: Shared[Atomic[u64]], stats: Shared[Mutex[Stats]], value: i64) -> i64:
    for _ in range(1000):
        hits.fetch_add(1, Ordering.Relaxed)
        with stats.lock() as data:
            data.count = data.count + 1
            data.total = data.total + value
    value

type PoolJobError = FutureError | SubmitError[()] | PoolError

def main() -> Result[(), AllocError | IoError | PoolJobError]:
    hits = Shared[Atomic[u64]](Atomic(0))?
    stats = Shared[Mutex[Stats]](Mutex(Stats(0, 0)))?
    with ThreadPoolExecutor(2, 4)? as pool:
        first = pool.submit(work, hits.share(), stats.share(), 3).drop_job()?
        second = pool.submit(work, hits.share(), stats.share(), 4).drop_job()?
        print(first.result()? + second.result()?)?
    print(hits.load(Ordering.Acquire))?
    with stats.lock() as data:
        print(data.count)?
        print(data.total)?
    Ok(())
```
```output
7
2000
2000
7000
```

`Shared[T](value)` moves the value into one allocation, which can fail.
`share()` returns another owner of the same value without allocating. Each
owner is dropped like any other value; the last one to drop, on whichever
thread, runs the value's cleanup and frees it.

The handle lends its content: `hits.fetch_add(...)` and `stats.lock()` reach
the atomic and the mutex through it, and `&*hits` borrows the atomic itself.
The content is shared, never exclusive, so it changes only through an atomic
or a mutex:

```plenty-error
class Stats:
    count: i64

def main() -> Result[(), AllocError]:
    stats = Shared(Stats(0))?
    stats.count = 1
    Ok(())
```

```error
a Shared owner gives shared access only; mutate its content through an Atomic or a Mutex inside it
```

Handles cannot be compared, printed, or copied; read the content instead. A
value cannot hold a share of itself through a mutex, since it would never be
released: link back to it with an index into a list that owns the values.

The compiler's lock checks see through handles. Shutting a pool down while
holding a lock on a mutex one of its jobs can reach is rejected, as is waiting
for such a job's result. Wait after the lock block ends, as `main` reads
`stats` only after the pool's block. Nesting
a lock through a handle inside a lock on another `Mutex[T]` of the same type is
rejected too, because two handles can name one mutex. See [shared owners](../../design/facilities/01-builtins/shared.md)
for the full rules.
