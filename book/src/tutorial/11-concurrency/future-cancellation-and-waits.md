# Cancel queued work and bound a future wait

`future.cancel()` returns `True` when it prevents a queued job from starting.
It cannot interrupt running work. A cancelled future reports
`FutureError.Cancelled` from `result()`. `future.done()` observes completion or
cancellation without consuming the handle. Explicit `pool.shutdown(True)` cancels
jobs still queued and waits for running jobs; ordinary scope exit drains all work.

For a future, `future.wait_timeout(milliseconds)` returns a boolean: true means
completed or cancelled. This only observes the future; call `result()` afterward
to take its result. A false return leaves the job running or queued.

## Jobs that send

Shutdown and `result()` both wait for a job, so a job that sends follows the
same [habits as a task](wait-habits.md). When you do not wait for its result
inside the block, guard the receiver as the last manager of the pool's `with`:

```plenty
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def drain(inbox: &Receiver[i64]) -> i64:
    mut total = 0
    while True:
        match inbox.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    total

def main() -> Result[(), ChannelError | IoError | PoolError | SubmitError[()]]:
    sender, receiver = channel[i64](2)?
    with ThreadPoolExecutor(1, 1)? as pool, receiver as inbox:
        pool.submit(produce, sender).drop_job()?
        print(drain(inbox))?
    Ok(())
```

```output
30
```

That guard drops the receiver at shutdown, which is too late for a `result()`
inside the block:

```plenty-error
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def drain(inbox: &Receiver[i64]) -> i64:
    mut total = 0
    while True:
        match inbox.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    total

def main() -> Result[(), ChannelError | FutureError | IoError | PoolError | SendError[i64] | SubmitError[()]]:
    sender, receiver = channel[i64](2)?
    with ThreadPoolExecutor(1, 1)? as pool, receiver as inbox:
        future = pool.submit(produce, sender).drop_job()?
        print(drain(inbox))?
        future.result()??
    Ok(())
```

```error
this result() waits for a job that holds `Sender[i64]` while `receiver` still holds `Receiver[i64]`
```

To wait for the result, end the receiver's own guard first:

```plenty
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def drain(inbox: &Receiver[i64]) -> i64:
    mut total = 0
    while True:
        match inbox.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    total

def main() -> Result[(), ChannelError | FutureError | IoError | PoolError | SendError[i64] | SubmitError[()]]:
    sender, receiver = channel[i64](2)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender).drop_job()?
        mut total = 0
        with receiver as inbox:
            total = drain(inbox)
        future.result()??
        print(total)?
    Ok(())
```

```output
30
```

A failed `submit(...)?` has submitted nothing, so it can come before the guard.
`done()` and `wait_timeout()` do not prove the channel protocol to the checker:
the result wait is rejected while a matching receiver lives, even after they
report completion.

A worker that may run indefinitely needs a cooperative shutdown protocol:
scope exit still waits after a timed wait returns.
