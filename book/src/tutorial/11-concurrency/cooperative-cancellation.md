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
    ready, started = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(work, stop.share(), ready)?
        started.recv()?
        stop.cancel()
        print(future.result()?)?
    print(stop.is_cancelled())?
    Ok(())
```

```output
42
True
```

Check `stop.is_cancelled()` between work units and return an appropriate result.
Cancellation does not interrupt instructions or skip cleanup.

`cancel()` is safe to repeat. Dropping a token handle does not cancel work.
`wait_timeout(milliseconds)` waits for a request and returns whether it arrived.
A token does not wake an ordinary `recv()` call. Use a shutdown message or a
[timed receive](channel-timeouts.md) when a worker must periodically check for
cancellation. Automatic joining still waits for that worker to return.

In a receive loop, check `is_cancelled()` between bounded receives. Avoid a
zero-timeout retry loop that consumes a CPU while idle.
