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
`cells[target]` releases `cells[source]` before `add` returns. `lock_now()`
produces only `WouldBlock`.

## Waiting for a condition

`with place.lock_when(ready, &deadline, &cancel)? as value:` takes the lock as
`lock()` does, then waits until `ready(value)` returns `True`, and runs the
block with the lock held. Use it when a thread must wait for another thread to
change the guarded value, such as a queue becoming non-empty or a phase
finishing. `ready` is called with `&T` and returns `bool`. Between checks the
waiter releases the lock and sleeps. Every unlock that follows a change wakes
the mutex's waiters, so the changing thread needs no notify call; each waiter
then relocks and checks its predicate again.

The wait ends with `std.sync.LockError.TimedOut` once the `std.time.Deadline`
passes, or `LockError.Cancelled` once the `CancellationToken` is cancelled; `?`
propagates the error after unlocking. Each round checks the token first and
then the predicate, so a predicate that holds when the deadline passes still
succeeds. A token cancelled during a sleep is seen within 10 ms. The deadline
is read once, when the manager starts. The place forms, `as` binding, and
unlock on every exit are those of `lock()`. `lock_when()` is valid only in this
propagated form.

```plenty
from std.sync import CancellationToken, Mutex, LockError
from std.time import Deadline, deadline_after, monotonic_now, seconds

class Jobs:
    done: i64

def finish(jobs: &Mutex[Jobs]) -> Result[(), OverflowError[i64]]:
    for _ in range(3):
        with jobs.lock() as j:
            j.done = j.done + 1
    Ok(())

def all_done(jobs: &Jobs) -> bool:
    jobs.done == 3

def main() -> Result[(), AllocError | IoError | LockError | RangeError | OverflowError[i64] | SpawnError[()]]:
    jobs = Mutex(Jobs(0))
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &seconds(5))?
    with spawn(finish, &jobs)? as worker:
        with jobs.lock_when(all_done, &deadline, &cancel)? as j:
            print(j.done)?
        worker.join()?
    Ok(())
```

```output
3
```

`ready` is the name of a function, a local anonymous function, or a reference
to either, optionally written `&ready`. The predicate runs while the lock is
held, so the compiler rejects one that can wait: one that locks a mutex, waits
on a channel, token, task, or pool, or calls a function whose body is not
visible, such as a `Callable` parameter or a boxed callable. Pass a named
function or an anonymous function instead; through a generic parameter, an
anonymous function is checked at each instantiation. A predicate is checked
the way [worker waits](executors.md) are, through its calls.

`lock_when()` is an error inside another lock block, or after a lock in the
same `with` header: the outer lock would stay held for the whole wait. A wait
inside a function called while a lock is held is not detected.

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

A lock taken by `lock_now()` or `lock_when()` is held like any other for these
rules.

Locking two mutexes in different orders on different threads can still
deadlock; take nested locks in one fixed order.

## No poisoning

A trap terminates the process without unwinding, so no program continues after
a holder fails mid-update. A Plenty mutex therefore has no poisoned state and
`lock()` has no error result.

## Not provided

There is no timed or cancellable wait for the lock itself (`lock_when()` waits
for the lock without a deadline, as `lock()` does), separate condition
variable or explicit notify, reentrant lock, read-write lock, or fairness
guarantee (backlog B56), and no unlocked access through
`&mut Mutex[T]` (Rust's `get_mut`); lock it instead. Executor and pool jobs own
their inputs and cannot borrow a mutex; give each job a share of a
`Shared[Mutex[T]]` instead.

## Lowering

A mutex holds a state word, a sequence word, and the value. The state word's
low two bits are 0 when unlocked, 1 when locked, and 2 when locked with
possible lock waiters; bit 2 (4) records `lock_when()` waiters. Lock is an
inline compare-exchange from 0 to 1; on failure the runtime marks the word
contended and sleeps on it with a Linux futex until it acquires the lock.
Unlock is an inline exchange to 0 that calls the runtime only if it replaced a
value above 1: it then wakes one lock waiter if the lock bits were 2, and, if
bit 2 was set, advances the sequence word and wakes every thread sleeping on
it. `lock_now()` is the same inline compare-exchange; a failure leaves the word
unchanged, and only a word holding just the waiter bit calls the runtime to
retry. A `lock_when()` round reads the sequence word under the lock, releases
the lock leaving only the waiter bit, and sleeps while the sequence is
unchanged, for at most 10 ms, before relocking. See the
[runtime mechanisms](../../toolchain/02-architecture/concurrency.md#mutexes).
