# Stop work cooperatively

Use a cancellation token to ask a running worker to stop. Share the token
explicitly; all handles observe the same request.

```plenty
def work(stop: CancellationToken, ready: Sender[i64]) -> i64:
    ready.send(1).unwrap()
    stop.wait()
    # Release resources and return through ordinary control flow.
    42

def main() -> Result[(), Failure]:
    stop = CancellationToken()?
    with ThreadPoolExecutor(1, 1)? as pool:
        ready, started = channel[i64](1)?
        future = pool.submit(work, stop.share(), ready)?
        started.recv()?
        stop.cancel()
        drop(started)
        print(future.result()?)?
    print(stop.is_cancelled())?
    Ok(())
```

```output
42
True
```

The receiver drops before `future.result()` waits. The channel is created inside
the pool's scope, so an earlier error also drops its receiver before shutdown.
A receiver guard that only ends with the pool would still be alive at the wait.
Shutdown also waits for submitted jobs, and a job waiting to send is released
only when the last receiver drops. Plenty rejects a shutdown reached while a
receiver for a submitted job's sender is still alive:

```plenty-error
def report(ready: Sender[i64]) -> ():
    ready.send(1).unwrap()

def main() -> Result[(), Failure]:
    ready, started = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(report, ready)?
        started.recv()?
    Ok(())
```

```error
leaving its scope shuts down this pool after a job that holds `Sender[i64]` was submitted, while `started` still holds `Receiver[i64]`
```

As with [task joins](channels-and-shutdown.md), the compiler compares message
types rather than counting messages, so it asks for the guard even though this
job's one send always fits the queue.

Check `stop.is_cancelled()` between work units and return an appropriate result.
Cancellation does not interrupt instructions or skip cleanup.

`cancel()` is safe to repeat. Dropping a token handle does not cancel work.
`wait_timeout(milliseconds)` waits for a request and returns whether it arrived.
A token does not wake an ordinary `recv()` call. Use a shutdown message or a
[timed receive](channel-timeouts.md) when a worker must periodically check for
cancellation. Automatic joining still waits for that worker to return.

In a receive loop, check `is_cancelled()` between bounded receives. Avoid a
zero-timeout retry loop that consumes a CPU while idle.
