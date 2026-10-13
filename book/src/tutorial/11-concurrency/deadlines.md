# Share one deadline across waits

A timeout bounds one wait. When a task makes several waits, give it a
`Deadline` from [`std.time`](../07-modules/04-standard-time.md): one total
budget that every wait draws from.

```plenty
from std.time import Deadline, monotonic_now, deadline_after, milliseconds

def collect(inbox: &Receiver[i64], deadline: &Deadline) -> i64:
    mut total = 0
    while True:
        budget = deadline.remaining(&monotonic_now()).as_timeout_ms()
        match inbox.recv_timeout(budget):
            case Ok(value):
                total += value
            case Err(RecvTimeoutError.TimedOut):
                break
            case Err(RecvTimeoutError.Disconnected):
                break
    total

def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](4)?
    sender.send(1)?
    sender.send(2)?
    sender.send(3)?
    deadline = deadline_after(&monotonic_now(), &milliseconds(20))?
    print(collect(&receiver, &deadline))?
    print(deadline.expired(&monotonic_now()))?
    print(deadline.remaining(&monotonic_now()).as_timeout_ms())?
    Ok(())
```

```output
6
True
0
```

The loop makes four waits against one 20-millisecond budget instead of
granting each wait its own. `remaining` takes the current instant and returns
zero once the deadline has passed. A zero budget still receives a message that
is already queued, so the three sent values always arrive.

`as_timeout_ms()` converts a span to the `u64` milliseconds that
[timed waits](channel-timeouts.md) accept. It rounds up: a wait never returns
before its deadline, and a remainder below a millisecond does not turn the loop
into a zero-timeout spin.

## Sleep that can be cancelled

`sleep` cannot be interrupted. To pause until either a span passes or someone
asks you to stop, wait on a
[cancellation token](cooperative-cancellation.md) with the span as the budget:

```plenty
from std.time import seconds

def pause(stop: CancellationToken, ready: Sender[i64]) -> bool:
    ready.send(1).unwrap()
    stop.wait_timeout(seconds(3600).as_timeout_ms())

def main() -> Result[(), Failure]:
    stop = CancellationToken()?
    ready, started = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool, started as inbox:
        future = pool.submit(pause, stop.share(), ready)?
        inbox.recv()?
        stop.cancel()
        print(future.result()?)?
    Ok(())
```

```output
True
```

The worker asks for an hour and returns as soon as `cancel()` runs. The result
is true when cancellation ended the wait and false when the span elapsed.

Deadlines use the monotonic clock, so changing the system time neither extends
nor shortens them. They limit waiting, not execution: a wait can return late on
a busy machine. See the [time reference](../../design/stdlib/time.md).
