# Share mutable state with a mutex

An atomic keeps one value consistent. When several values must change together,
put them in a `Mutex` from `std.sync`. Only one thread at a time can hold its
lock, so no worker sees `count` updated without `total`:

```plenty
from std.sync import Mutex

class Stats:
    count: i64
    total: i64

def record(stats: &Mutex[Stats], value: i64, rounds: i64) -> Result[(), OverflowError[i64]]:
    for _ in range(rounds):
        with stats.lock() as data:
            count = data.count + 1
            total = data.total + value
            data.count = count
            data.total = total
    Ok(())

def main() -> Result[(), SpawnError[()] | IoError | OverflowError[i64]]:
    stats = Mutex[Stats](Stats(0, 0))
    with spawn(record, &stats, 3, 10000)? as first, spawn(record, &stats, 4, 10000)? as second:
        first.join()?
        second.join()?
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
end, `return`, `?`, `break`, or `continue`. Both new fields are computed before
either is stored, so arithmetic failure cannot leave a half-updated pair. Workers share `&stats` without `mut`,
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

def bump(counter: &Mutex[i64]) -> Result[(), OverflowError[i64]]:
    with counter.lock() as value:
        *value = *value + 1
    Ok(())

def main() -> Result[(), SpawnError[()]]:
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

def bump(counter: &Mutex[i64]) -> Result[(), OverflowError[i64]]:
    with counter.lock() as value:
        *value = *value + 1
    Ok(())

def main() -> Result[(), SpawnError[()] | IoError | OverflowError[i64]]:
    counter = Mutex(0)
    with spawn(bump, &counter)? as task:
        with counter.lock() as value:
            *value = *value + 10
        task.join()?
    print(counter.into_inner())?
    Ok(())
```
```output
11
```

Nesting a lock inside the block of another mutex of the same type is also
rejected when either one is reached through a reference or a computed index:
two references can name one mutex. Two separately owned mutexes, such as two
local names, may nest. When two indices may name one mutex, take the inner lock
with `lock_now()?`. It never waits: it fails with `LockError.WouldBlock` when
the mutex is held, including by the outer block, and leaving through `?`
unlocks the outer lock too:

```plenty
from std.sync import Mutex, LockError

def transfer(accounts: &list[Mutex[i64]], source: i64, target: i64) -> Result[(), LockError | OverflowError[i64]]:
    with accounts[source].lock() as paid, accounts[target].lock_now()? as received:
        *paid = *paid - 5
        *received = *received + 5
    Ok(())

def attempt(accounts: &list[Mutex[i64]], source: i64, target: i64) -> Result[(), IoError | OverflowError[i64]]:
    match transfer(accounts, source, target):
        case Ok(_):
            print("moved")?
        case Err(error):
            match error:
                case LockError.WouldBlock:
                    print("busy")?
                case overflow: OverflowError[i64]:
                    return Err(overflow)
                case _:
                    print("not moved")?
    Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    accounts = [Mutex(20), Mutex(20)]?
    attempt(&accounts, 0, 1)?
    attempt(&accounts, 1, 1)?
    with accounts[1].lock() as balance:
        print(*balance)?
    Ok(())
```
```output
moved
busy
25
```

`WouldBlock` also reports another thread holding `target`, even briefly. When
other threads contend for these locks, compare the indices first and lock once
when they are equal, or retry the transfer.

The compiler cannot see a relock inside a called
function, a join inside a function called while the lock is held, or two
threads taking two locks in opposite orders. Keep lock blocks short and take
nested locks in one fixed order. See
[`std.sync`](../../design/stdlib/sync.md) and
[mutexes](../../design/facilities/03-concurrency/mutex.md) for the full rules.
