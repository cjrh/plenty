# Stop work cooperatively

Use a cancellation token to ask a running worker to stop. Share the token
explicitly; all handles observe the same request.

```plenty
def work(stop: CancellationToken, ready: Sender[i64]) -> Result[i64, SendError[i64]]:
    ready.send(1)?
    stop.wait()
    # Release resources and return through ordinary control flow.
    Ok(42)

type CancellableWorkError = SendError[i64] | FutureError | RecvError | SubmitError[()] | ChannelError | PoolError

def main() -> Result[(), AllocError | IoError | CancellableWorkError]:
    stop = CancellationToken()?
    ready, started = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(work, stop.share(), ready).drop_job()?
        with started as inbox:
            inbox.recv()?
        stop.cancel()
        print(future.result()??)?
    print(stop.is_cancelled())?
    Ok(())
```

```output
42
True
```

The worker reports that it started, then waits for the request. The receiver's
own guard ends before `future.result()` waits, and it also covers the `?` on
`recv()`, as in [jobs that send](future-cancellation-and-waits.md#jobs-that-send).
Without the guard, an exit from the pool's block would reach shutdown with
`started` alive, and Plenty rejects that:

```plenty-error
def report(ready: Sender[i64]) -> Result[(), SendError[i64]]:
    ready.send(1)

def main() -> Result[(), ChannelError | PoolError | RecvError | SubmitError[()]]:
    ready, started = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        pool.submit(report, ready).drop_job()?
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
