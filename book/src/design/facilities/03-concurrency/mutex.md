# Mutexes

`std.sync.Mutex[T]` holds a value that several workers change through shared
references or [shared owners](../01-builtins/shared.md), one lock holder at a
time. Use it when several values must
change together to keep an invariant; a single independent counter or flag is
an [atomic](../../stdlib/atomic.md). The operations are listed in
[`std.sync`](../../stdlib/sync.md#mutext).

## Locking

`with place.lock() as value:` borrows the mutex at `place` shared, waits until
no other thread holds the lock, and binds `value: &mut T` for the block. `as`
may be omitted. The place is a name, a field, an element, or a dereference,
including a `&Mutex[T]` parameter, or a `Shared[Mutex[T]]` handle, which locks
the mutex through `*handle`; a temporary is rejected. `lock()` anywhere
other than a `with` manager is a compile-time error naming that form.

The lock is released on every exit from the block: its end, `return`, `?`,
`break`, and `continue`. Comma-separated managers nest, so
`with a.lock() as x, b.lock() as y:` takes `a` then `b` and releases `b` first.

`value` is an ordinary exclusive reference. Its loan belongs to the lock, not to
the mutex's owner, which stays shared-borrowed until the block ends: the mutex
cannot be moved, replaced, or consumed while locked. Like any local reference,
`value` cannot be stored, returned, captured by an anonymous function, or used
after the block.

`into_inner()` consumes an owned mutex and returns its value without locking.
Ownership proves that no thread holds or waits for the lock.

## Locking without waiting

`with place.lock_now()? as value:` takes the lock only if no thread holds it.
Otherwise it fails with `std.sync.LockError.WouldBlock` and `?` propagates the
error, so the block runs only while the lock is held. It does not wait or
record itself as a waiter. The place forms, binding, and unlock on every exit
are those of `lock()`. `lock_now()` is valid only in this propagated form;
`lock_now()` without `?`, or outside a `with` manager, is a compile-time error.
To handle a busy lock locally, put the block in a function and `match` its
result:

```plenty
from std.sync import Mutex, LockError

def add(cells: &list[Mutex[i64]], source: i64, target: i64) -> Result[(), LockError | OverflowError[i64]]:
    with cells[source].lock() as a, cells[target].lock_now()? as b:
        *b = *b + *a
    Ok(())

def report(result: Result[(), LockError | OverflowError[i64]]) -> Result[(), IoError | OverflowError[i64]]:
    match result:
        case Ok(_):
            print("added")?
        case Err(error):
            match error:
                case LockError.WouldBlock:
                    print("busy")?
                case overflow: OverflowError[i64]:
                    return Err(overflow)
                case _:
                    print("not acquired")?
    Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    cells = [Mutex(1), Mutex(2)]?
    report(add(&cells, 0, 1))?
    # Both indices name one mutex, which the outer lock holds.
    report(add(&cells, 1, 1))?
    Ok(())
```

```output
added
busy
```

A failure in a comma header unwinds the managers before it: above, a busy
`cells[target]` releases `cells[source]` before `add` returns. `LockError`
also declares `TimedOut` and `Cancelled` for planned deadline and cancellable
lock waits; `lock_now()` produces only `WouldBlock`.

## Waits the compiler rejects

A lock waits as long as another holder keeps it. The compiler rejects these
patterns that would wait forever:

- **Relocking in the block.** Locking a place that an enclosing lock block in
  the same function already holds is an error. Places are compared as written:
  `m` and `*m` match. This includes `lock_now()`, which would always fail.
- **Nesting locks that could alias.** Locking a `Mutex[T]` inside a lock block
  that holds another `Mutex[T]` of the same `T` is an error when either place
  is reached through a reference, a `Shared` handle, or a computed index
  (`cells[i]`). Such places can name one mutex (`r = &a`, `b = a.share()`,
  `i == j`), and only types are compared. Two owned places built from local
  names, fields, and constant indices, such as `a` and `b` or `cells[0]` and
  `cells[1]`, are distinct mutexes and may nest. Otherwise wrap one value in a
  distinct class, or take the inner lock with `lock_now()`: it cannot wait, so
  it may nest under any lock and fails with `WouldBlock` when both places name
  one mutex. A blocking `lock()` inside a `lock_now()` block is checked as
  under `lock()`. A relock inside a called function is not detected and
  deadlocks at run time.
- **Joining under a lock.** A scoped task cannot be joined, by `task.join()` or
  by leaving its `with` block, while the joining function holds a lock on a
  `Mutex[T]` and the worker can reach a `Mutex[T]` of the same `T`: as an
  argument or capture, a field or element of one, or behind a reference. The
  worker could be waiting for that lock. Mutexes are matched by type, like the
  [channel rule](channels.md#joins-and-live-endpoints). Start the task outside
  the lock: `with spawn(...)?:` around `with m.lock():` unlocks before joining.
  A join inside a function called while the lock is held is not detected and
  deadlocks at run time.
- **Waiting for a pool job under a lock.** A job submitted to a
  [pool](executors.md) records the `Mutex[T]` types it can reach, including
  through `Shared` handles. `future.result()` on its future, and a shutdown of
  any pool after it was submitted (`shutdown()`, `drop`, reassignment, or the
  end of the pool's scope), is an error while the function holds a lock on a
  `Mutex[T]` of a recorded `T`. A future
  is traced like the
  [channel rule](futures.md#result-waits-and-live-receivers) traces it, through
  local bindings and calls that return it.
- **Yielding under a lock.** `yield` inside a lock block is an error: a
  suspended generator would keep the lock indefinitely.

A lock taken by `lock_now()` is held like any other for these rules.

Locking two mutexes in different orders on different threads can still
deadlock; take nested locks in one fixed order.

## No poisoning

A trap terminates the process without unwinding, so no program continues after
a holder fails mid-update. A Plenty mutex therefore has no poisoned state and
`lock()` has no error result.

## Not provided

There is no timed or cancellable lock wait, reentrant lock, condition variable,
read-write lock, or fairness guarantee (backlog B56), and no unlocked access through
`&mut Mutex[T]` (Rust's `get_mut`); lock it instead. Executor and pool jobs own
their inputs and cannot borrow a mutex; give each job a share of a
`Shared[Mutex[T]]` instead.

## Lowering

The state word is 0 when unlocked, 1 when locked, and 2 when locked with
possible waiters. Lock is an inline compare-exchange from 0 to 1; on failure the
runtime exchanges in 2 and sleeps on the word with a Linux futex until an
exchange finds 0. Unlock is an inline exchange to 0 that calls the runtime to
wake one waiter only if it replaced 2. `lock_now()` is the same inline
compare-exchange with no slow path: a failure leaves the word unchanged. See the
[runtime mechanisms](../../toolchain/02-architecture/concurrency.md#mutexes).
