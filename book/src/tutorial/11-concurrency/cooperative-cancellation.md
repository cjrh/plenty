# Stop work cooperatively

Use a cancellation token to ask a running worker to stop. Share the token
explicitly; all handles observe the same request.

```plenty
def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(SubmitError.Full(_)):
            Err("submission queue is full")
        case Err(SubmitError.Shutdown(_)):
            Err("executor has shut down")
        case Err(SubmitError.OutOfMemory(_)):
            Err("submission allocation failed")
        case Err(SubmitError.CapacityOverflow(_)):
            Err("submission capacity overflow")

def work(stop: CancellationToken, ready: Sender[i64]) -> Result[i64, SendError[i64]]:
    ready.send(1)?
    stop.wait()
    # Release resources and return through ordinary control flow.
    Ok(42)

type CancellableWorkError = SendError[i64] | FutureError | RecvError | str | ChannelError | PoolError

def main() -> Result[(), AllocError | IoError | CancellableWorkError]:
    stop = CancellationToken()?
    with ThreadPoolExecutor(1, 1)? as pool:
        ready, started = channel[i64](1)?
        future = submitted(pool.submit(work, stop.share(), ready))?
        started.recv()?
        stop.cancel()
        drop(started)
        print(future.result()??)?
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
def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(SubmitError.Full(_)):
            Err("submission queue is full")
        case Err(SubmitError.Shutdown(_)):
            Err("executor has shut down")
        case Err(SubmitError.OutOfMemory(_)):
            Err("submission allocation failed")
        case Err(SubmitError.CapacityOverflow(_)):
            Err("submission capacity overflow")

def report(ready: Sender[i64]) -> Result[(), SendError[i64]]:
    ready.send(1)

def main() -> Result[(), ChannelError | PoolError | RecvError | str]:
    ready, started = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = submitted(pool.submit(report, ready))?
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

Submission errors retain an unstarted job. Here `submitted` deliberately
releases that job and preserves the failure reason as a message. Match the
original `SubmitError` instead when you want to recover or retry the job.
