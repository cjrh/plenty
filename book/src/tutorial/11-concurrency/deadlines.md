# Share one deadline across waits

A timeout bounds one wait. When a task makes several waits, give it a
`Deadline` from [`std.time`](../07-modules/04-standard-time.md): one total
budget that every wait draws from.

```plenty
import std.sync
from std.time import Deadline, monotonic_now, deadline_after, milliseconds

def collect(inbox: &Receiver[i64], deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    while True:
        match std.sync.recv_before(inbox, deadline, cancel):
            case Ok(value):
                total += value
            case Err(std.sync.WaitError.TimedOut):
                break
            case Err(std.sync.WaitError.Disconnected) | Err(std.sync.WaitError.Cancelled):
                break
    Ok(total)

def main() -> Result[(), AllocError | IoError | RangeError | SendError[i64] | OverflowError[i64]]:
    sender, receiver = channel[i64](4)?
    sender.send(1)?
    sender.send(2)?
    sender.send(3)?
    deadline = deadline_after(&monotonic_now(), &milliseconds(20))?
    cancel = CancellationToken()?
    print(collect(&receiver, &deadline, &cancel)?)?
    print(deadline.expired(&monotonic_now()))?
    print(deadline.remaining(&monotonic_now()).as_timeout_ms())?
    Ok(())
```

```output
6
True
0
```

`recv_before` shares one deadline and checks the token about every 10 ms,
so another thread's `cancel.cancel()` ends the wait with
`WaitError.Cancelled`. Prefer it to `recv_timeout_ms` in any loop that must
stop on request: a stop request cannot end a millisecond wait early. The loop makes four waits against one 20-millisecond budget instead of
granting each wait its own. `remaining` takes the current instant and returns
zero once the deadline has passed. A zero budget still receives a message that
is already queued, so the three sent values always arrive.

`as_timeout_ms()` is for the low-level `_timeout_ms` methods.
It converts a span to the `u64` milliseconds that
[timed waits](channel-timeouts.md) accept. It rounds up: a wait never returns
before its deadline, and a remainder below a millisecond does not turn the loop
into a zero-timeout spin.

## Wait for a change under a mutex

To wait until another thread changes a value guarded by a
[mutex](shared-mutex.md), lock it with `lock_when`. It takes a predicate over
the guarded value, the deadline, and a token, and runs the block once the
predicate holds:

```plenty
from std.sync import Mutex, LockError
from std.time import Deadline, deadline_after, milliseconds, monotonic_now, seconds

class Batch:
    loaded: i64

def load(batch: &Mutex[Batch]) -> Result[(), OverflowError[i64]]:
    for _ in range(4):
        with batch.lock() as b:
            b.loaded = b.loaded + 1
    Ok(())

def complete(batch: &Batch) -> bool:
    batch.loaded == 4

def main() -> Result[(), AllocError | IoError | RangeError | LockError | OverflowError[i64] | SpawnError[()]]:
    batch = Mutex(Batch(0))
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &seconds(5))?
    with spawn(load, &batch)? as loader:
        with batch.lock_when(complete, &deadline, &cancel)? as b:
            print(b.loaded)?
        loader.join()?
    soon = deadline_after(&monotonic_now(), &milliseconds(10))?
    match more(&batch, &soon, &cancel):
        case Err(LockError.TimedOut):
            print("no more")?
        case _ as other:
            other?
    Ok(())

def more(batch: &Mutex[Batch], deadline: &Deadline, cancel: &CancellationToken) -> Result[(), LockError | IoError]:
    with batch.lock_when(bigger, deadline, cancel)? as b:
        print(b.loaded)?
    Ok(())

def bigger(batch: &Batch) -> bool:
    batch.loaded > 4
```

```output
4
no more
```

The loader never notifies anyone: each unlock after a change wakes the
waiters, which relock and call the predicate again. A wait that outlives its
deadline fails with `LockError.TimedOut`, and one whose token is cancelled with
`LockError.Cancelled`; both unlock first. The predicate runs with the lock
held, so the compiler rejects one that can wait or lock another mutex, and
rejects `lock_when` inside another lock block. See
[waiting for a condition](../../design/facilities/03-concurrency/mutex.md#waiting-for-a-condition).

## Sleep that can be cancelled

`sleep` cannot be interrupted. To pause until either a span passes or someone
asks you to stop, wait on a
[cancellation token](cooperative-cancellation.md) with the span as the budget:

```plenty
from std.time import seconds

def pause(stop: &CancellationToken) -> bool:
    stop.wait_timeout_ms(seconds(3600).as_timeout_ms())

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    stop = CancellationToken()?
    with spawn(pause, &stop)? as task:
        stop.cancel()
        print(task.join())?
    Ok(())
```

```output
True
```

The worker borrows the token and asks for an hour. It returns as soon as
`cancel()` runs, even if the request comes before the wait starts. The result
is true when cancellation ended the wait and false when the span elapsed.

Deadlines use the monotonic clock, so changing the system time neither extends
nor shortens them. They limit waiting, not execution: a wait can return late on
a busy machine. See the [time reference](../../design/stdlib/time.md).
