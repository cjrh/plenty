# Count with atomics

A shared borrow lets several workers read the same data, but not change it. An
atomic from `std.atomic` is the exception: its methods update it through a
shared reference, one indivisible step at a time:

```plenty
from std.atomic import Atomic, Ordering

def count(hits: &Atomic[u64], rounds: i64) -> ():
    for _ in range(rounds):
        hits.fetch_add(1, Ordering.Relaxed)

def main() -> Result[(), Failure]:
    hits = Atomic[u64](0)
    with spawn(count, &hits, 100000)?, spawn(count, &hits, 100000)?:
        pass
    print(hits.load(Ordering.Acquire))?
    Ok(())
```
```output
200000
```

`Atomic[u64](0)` stores the counter inline in `main`'s frame; nothing is
allocated. `fetch_add` returns the value it replaced. Like `+`, it stops the
program on overflow, before storing anything.

Every operation names an `Ordering`. `Relaxed` only makes the update itself
indivisible. `Release` on a store and `Acquire` on a load make the writes before
the store visible to the thread that loads it. Use `SeqCst` when unsure. The
compiler rejects orderings an operation cannot use:

```plenty-error
from std.atomic import Atomic, Ordering

def main() -> ():
    ready = Atomic[bool](False)
    ready.store(True, Ordering.Acquire)
```

```error
store does not accept Ordering.Acquire; use Ordering.Relaxed, Ordering.Release, or Ordering.SeqCst
```

A class can hold atomics as fields, so one borrow shares several of them. Atomics
are not values to print or compare, so read each one with `load`:

```plenty
from std.atomic import Atomic, Ordering

class Stats:
    seen: Atomic[u32]
    largest: Atomic[i64]

def record(stats: &Stats, start: i64) -> ():
    for value in range(start, start + 1000):
        stats.seen.fetch_add(1, Ordering.Relaxed)
        stats.largest.fetch_max(value, Ordering.Relaxed)

def main() -> Result[(), Failure]:
    stats = Stats(seen=Atomic(0), largest=Atomic(i64.MIN))
    with spawn(record, &stats, 0)?, spawn(record, &stats, 5000)?:
        pass
    print(stats.seen.load(Ordering.Acquire))?
    print(stats.largest.load(Ordering.Acquire))?
    Ok(())
```
```output
2000
5999
```

`compare_exchange` replaces a value only if it still equals the one you expect,
and returns `Err` with the actual value otherwise. See
[`std.atomic`](../../design/stdlib/atomic.md) for every method.
