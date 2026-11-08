# `std.sync`

Exports `CancellationToken`, the existing
[cooperative cancellation facility](../facilities/03-concurrency/cancellation.md),
`Mutex[T]` and its `LockError`. The imported and prelude `CancellationToken` types are
identical; no wrapper, allocation, or new effect permission is introduced by
importing. `Mutex` is not a prelude name: each use needs an import, through any
item or module alias. Supported target: x86_64 Linux GNU.

`CancellationToken() -> Result[CancellationToken, AllocError]` creates an owned
shared state in one fallible allocation.

| Method | Result | Effect |
|---|---|---|
| `share()` | `CancellationToken` | Explicitly retain the same state |
| `cancel()` | `()` | Permanently request cancellation and wake token waiters |
| `is_cancelled()` | `bool` | Observe the request |
| `wait()` | `()` | Wait until cancellation is requested |
| `wait_timeout_ms(timeout_ms: u64)` | `bool` | Observe cancellation within the budget |
| `cancel_on_exit()` | `with` guard | Cancel on every exit from the `with` block; see [cancel on every exit](../facilities/03-concurrency/cancellation.md#cancel-on-every-exit) |

Methods borrow the handle, allocate nothing, and copy no captured data. Share
and observation have constant bookkeeping; cancellation also wakes waiters.
Lock contention and scheduling have no bounded latency. Tokens are safe to move
or explicitly share into eligible workers; borrowed handles retain ordinary
loans. Dropping the last handle releases the state but does not request stopping.
Cancellation is idempotent and does not reset.

A token does not interrupt instructions, cancel a future, or wake a channel
wait. Workers must return cooperatively, using bounded waits where needed.
[`std.time`](time.md) supplies deadline arithmetic and timeout conversion.

## Typed deadline waits

Use these waits for channels, futures and tokens in application code, and
always in a loop that must stop on request: a worker, a long poll, or a server
that drains on a signal. They borrow the handles, allocate nothing, and preserve the deadline across
wakeups. `deadline` and `cancel` are the last two parameters.

| Operation | Result |
|---|---|
| `recv_before(&receiver, &deadline, &cancel)` | `Result[T, WaitError]` |
| `send_before(&sender, value, &deadline, &cancel)` | `Result[(), SendBeforeError[T]]` |
| `select_recv_before(&first, &second, &deadline, &cancel)` | `Result[Selected[A, B], WaitError]` |
| `future_before(&future, &deadline, &cancel)` | `Result[(), WaitError]` |
| `cancelled_before(&token, &deadline, &cancel)` | `Result[(), WaitError]` |

`WaitError` has `TimedOut`, `Cancelled` and `Disconnected` (channels only).
Every `SendBeforeError[T]` variant (`TimedOut`, `Cancelled`, `Disconnected`)
returns the original unsent value, without copying it. Cancellation visible
before an attempt wins even over readiness. Otherwise an already-ready value
or disconnection wins over an expired deadline. Selection keeps the builtin's
first-receiver priority. Future observation leaves its result owned by the
future and does not cancel its worker. Token observation leaves its state intact.

Channel and future primitives cannot wake on a separate token, so these wrappers
sleep in slices of at most 10 ms, rechecking cancellation between slices. This
is a polling interval, not a hard latency guarantee under scheduler or lock
contention. Positive fractional milliseconds round up when passed to the native
wait, so a final slice can overrun by less than 1 ms. No deadline is restarted.

Low-level millisecond methods have explicit unit names: `send_timeout_ms`,
`recv_timeout_ms`, `wait_timeout_ms` on futures and tokens, and
`select_recv_timeout_ms`. The original `_timeout` spellings remain compatibility
aliases with the same millisecond units. They
[take no token](../facilities/03-concurrency/timed-selection.md): only a
token's own `wait_timeout_ms` observes a `CancellationToken`, and no token wakes
a channel, selection or future wait, so a stop request waits for the whole
budget. Do not pass a long
budget to a channel, selection or future primitive where shutdown must be
prompt; the [signal lesson](../../tutorial/11-concurrency/stop-on-signal.md#stop-a-long-wait-on-a-signal)
stops a long `select_recv_before` on `SIGTERM` instead.

## `Mutex[T]`

A value that workers update through shared references or
[`Shared[Mutex[T]]`](../facilities/01-builtins/shared.md) handles, one lock
holder at a time. See [mutexes](../facilities/03-concurrency/mutex.md) for the rules
and the deadlocks the compiler rejects.

```plenty
from std.sync import Mutex

class Stats:
    count: i64
    total: i64

def record(stats: &Mutex[Stats], value: i64) -> Result[(), OverflowError[i64]]:
    with stats.lock() as data:
        data.count = data.count + 1
        data.total = data.total + value
    Ok(())

def main() -> Result[(), SpawnError[()] | IoError | OverflowError[i64]]:
    stats = Mutex[Stats](Stats(0, 0))
    with spawn(record, &stats, 3)? as first, spawn(record, &stats, 4)? as second:
        first.join()?
        second.join()?
    final = stats.into_inner()
    print(final.total)?
    Ok(())
```
```output
7
```

The explicit joins handle each worker's arithmetic Result; merely leaving a
spawn scope waits for completion but does not propagate the worker's error.
An arithmetic failure unlocks the mutex on exit, retaining earlier mutations.

| Operation | Result | Effect |
|---|---|---|
| `Mutex[T](value)`, `Mutex(value)` | `Mutex[T]` | Store `value` unlocked, inline |
| `with mutex.lock() as value:` | binds `value: &mut T` | Wait for the lock, hold it for the block |
| `with mutex.lock_now()? as value:` | binds `value: &mut T`, or propagates `Err(LockError.WouldBlock)` | Take the lock only if it is free, hold it for the block |
| `mutex.into_inner()` | `T` | Consume an owned mutex and return its value |

`T` is any type a generic class field can hold: no references, generators, or
borrowed environments. A mutex is affine, with no copy, equality, or
formatting. It is stored inline (a 32-bit state word beside `T`); construction,
locking, and unlocking allocate nothing. An uncontended lock and unlock are
inline atomic instructions; a contended one waits in the kernel.

`lock()` is valid only as a `with` manager, and `lock_now()` only as a
propagated one. Both borrow the mutex shared, so no `mut` is needed, and every
exit from the block unlocks. `lock_now()` never waits and allocates nothing; a
failure leaves the lock to its holder. See
[locking without waiting](../facilities/03-concurrency/mutex.md#locking-without-waiting).
There is no timed or cancellable lock wait, poisoning, reentrant locking, or
fairness guarantee.

`enum LockError: WouldBlock | TimedOut | Cancelled` reports a lock that was not
acquired. Only `WouldBlock` is produced; `TimedOut` and `Cancelled` are for
planned deadline and cancellable lock waits (backlog B56). The variants match
[`std.fs.lock.LockError`](fs/lock.md); import one under an alias when a module
uses both.
`into_inner()` needs no lock: owning the mutex proves no other thread can hold
it.

Atomic integers and booleans are in [`std.atomic`](atomic.md). The shared
owner `Shared[T]` is a [prelude type](../facilities/01-builtins/shared.md).
Semaphores, condition variables, and read-write locks are not exported.

## Acknowledgements

- `Mutex` uses mutex 2 from Ulrich Drepper's ["Futexes Are Tricky"](https://www.akkadia.org/drepper/futex.pdf).
- Uses the Rust [`libc`](https://github.com/rust-lang/libc) crate for the target-specific Linux futex syscall number (MIT OR Apache-2.0).
