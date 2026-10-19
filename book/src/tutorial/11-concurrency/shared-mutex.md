# Share mutable state with a mutex

An atomic keeps one value consistent. When several values must change together,
put them in a `Mutex` from `std.sync`. Only one thread at a time can hold its
lock, so no worker sees `count` updated without `total`:

```plenty
from std.sync import Mutex

class Stats:
    count: i64
    total: i64

def record(stats: &Mutex[Stats], value: i64, rounds: i64) -> ():
    for _ in range(rounds):
        with stats.lock() as data:
            data.count = data.count + 1
            data.total = data.total + value

def main() -> Result[(), Failure]:
    stats = Mutex[Stats](Stats(0, 0))
    with spawn(record, &stats, 3, 10000)?, spawn(record, &stats, 4, 10000)?:
        pass
    final = stats.into_inner()
    print(final.count)?
    print(final.total)?
    Ok(())
```
```output
20000
70000
```

`stats.lock()` waits until no other thread holds the lock, then binds `data`, a
`&mut Stats`, for the block. Leaving the block unlocks, whether by reaching its
end, `return`, `?`, `break`, or `continue`. Workers share `&stats` without `mut`,
because the lock, not the borrow, makes the access exclusive. After the workers
finish, `into_inner()` takes the value back without locking. Nothing here
allocates.

`lock()` works only as a `with` manager, so a lock cannot be left held by
mistake. `data` belongs to the block: it cannot be stored, returned, or used
after it.

Waiting under a lock for something that needs the same lock never finishes. The
compiler rejects the waits it can see. Relocking a mutex inside its own block is
one; joining a worker that can lock the same kind of mutex is another:

```plenty-error
from std.sync import Mutex

def bump(counter: &Mutex[i64]) -> ():
    with counter.lock() as value:
        *value = *value + 1

def main() -> Result[(), Failure]:
    counter = Mutex(0)
    with counter.lock() as value:
        with spawn(bump, &counter)?:
            pass
    Ok(())
```

```error
leaving this `with` block joins a task whose worker can lock a `Mutex[i64]` while this function holds a lock on a `Mutex[i64]`
note: the lock is acquired here; end the lock's `with` block before this one, or start the task outside it
```

Start the task first, then lock inside its block: the lock's block ends, and
unlocks, before the task is joined.

```plenty
from std.sync import Mutex

def bump(counter: &Mutex[i64]) -> ():
    with counter.lock() as value:
        *value = *value + 1

def main() -> Result[(), Failure]:
    counter = Mutex(0)
    with spawn(bump, &counter)?:
        with counter.lock() as value:
            *value = *value + 10
    print(counter.into_inner())?
    Ok(())
```
```output
11
```

The compiler cannot see a relock through another reference or inside a called
function, or two threads taking two locks in opposite orders. Keep lock blocks
short and take nested locks in one fixed order. See
[`std.sync`](../../design/stdlib/sync.md) and
[mutexes](../../design/facilities/03-concurrency/mutex.md) for the full rules.
