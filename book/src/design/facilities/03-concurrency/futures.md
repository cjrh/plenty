# Futures

Executor submission returns an affine `Future[T]` with one result consumer.
Futures can move and live in collections, classes, and enums, but cannot be copied,
compared, or passed into worker code. There is no shared-result handle,
result cloning, async/await, or event loop.

| Operation | Result | Ownership and waiting |
| --- | --- | --- |
| `future.result()` | `Result[T, FutureError]` | Consume the future and wait for its result |
| `future.done()` | `bool` | Observe completion or cancellation |
| `future.cancel()` | `bool` | Prevent pending work from starting |
| `future.wait_timeout(timeout_ms: u64)` | `bool` | Observe completion/cancellation within a millisecond budget |

Methods other than result observe their receiver without needing `mut`.
Cancellation succeeds only before execution begins. Repeated cancellation of a
cancelled job returns True; running or completed jobs return False. Successful
cancellation immediately makes `done()` true and `result()` return
`Err(FutureError.Cancelled)` without waiting. It does not immediately reclaim
queue space: captures drop after both the queue and public future release them.

Dropping a future neither cancels nor waits. Its job still finishes; the worker
drops an unclaimed result. A future surviving executor shutdown retains the
completed result until retrieval or drop. Futures do not retain the executor and
cannot prevent shutdown. Mutable results transfer exactly once.

An application Result remains nested: a worker returning `Result[T, E]` yields
`Future[Result[T, E]]`. The first `?` on `result()` handles cancellation; the
next handles application failure. Retrieval, cancellation, status, and error
transport allocate nothing. Timed observation does not cancel or consume work;
see [timeout rules](timed-selection.md).

Scoped `Task.join()` returns `T` directly because scoped tasks cannot be
cancelled. `Future.result()` needs its outer `Result` to report cancellation;
for an application Result, `(future.result()?)?` deliberately handles two
different failure layers.

## Result waits and live receivers

`result()` cannot wait for a locally submitted job holding `Sender[T]` while
the calling function still holds `Receiver[T]`. A full queue could keep the job
and the result wait blocked forever. End the receiver's own guard before
retrieving the result:

```text
with ThreadPoolExecutor(1, 1)? as pool:
    future = pool.submit(produce, sender).drop_job()?
    with receiver as inbox:
        ...
    result = future.result()
```

A guard alongside the pool, `with ThreadPoolExecutor(1, 1)? as pool, receiver
as inbox:`, disconnects only at pool shutdown, which is too late for a result
wait inside the block. A `drop(receiver)` on every path to the wait also
satisfies the rule. See the [recommended patterns](channels.md#recommended-patterns).

The diagnostic points to the wait, receiver holders, and submission. Tracking
follows local future moves, `?`, `unwrap()`, `drop_job()`, extraction from
submission results, and calls that return a future. Separate futures with the same result type keep their own job origins.
Receivers dropped on every continuing path are accepted. `done()`, `cancel()`,
and `wait_timeout()` are unaffected, and result retrieval still consumes the
future and preserves both cancellation and application errors.

The same wait is rejected while the function holds a lock on a `Mutex[T]` and
the job can reach a `Mutex[T]` of the same `T`, for example through a
`Shared[Mutex[T]]` share: the job could be waiting for that lock. End the lock's
`with` block before waiting. Mutexes are matched by type.

This is a conservative check, not a proof of deadlock freedom: endpoints are
matched by message type, not channel identity. Completion, cancellation, queue
capacity, and earlier receives do not discharge the check. Assignments to the
same local are conservatively combined.

A call that receives the future, by value or by reference, is checked as a wait
at the call when a function it reaches, directly or through further calls,
calls `result()` on a `Future[T]` of the same `T`. The diagnostic names the
call, and `finished(future)` below is rejected:

```text
def finished(future: Future[()]) -> bool:
    future.result().is_ok()

def main() -> Result[(), ChannelError | PoolError | SubmitError[OnceClosure[[], ()]]]:
    sender, receiver = channel[i64](1)?
    with ThreadPoolExecutor(1, 1)? as pool:
        future = pool.submit(produce, sender)?
        ok = finished(future)   # rejected: `receiver` is still live
        drop(receiver)
    Ok(())
```

Matching by type means a callee that waits for a different `Future[T]` also
counts. A call through a `Callable`, closure, or boxed callable value, or to a
function that makes one, is assumed to wait.

A future returned from a call keeps its jobs. When the call's result can hold a
`Future[T]`, directly or in a `Result`, `Option`, or other aggregate, it is
assumed to hold every future of the same `T` that its arguments carry, and
every job with a `T` result that the callee, or a function it reaches,
submits. Both waits below are rejected like waits for a local submission:

```text
def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(_):
            Err("submission failed")

def start(pool: &ThreadPoolExecutor, sender: Sender[i64]) -> Result[Future[()], str]:
    submitted(pool.submit(produce, sender))

def main() -> Result[(), ChannelError | PoolError | str]:
    with ThreadPoolExecutor(1, 1)? as pool:
        sender, receiver = channel[i64](1)?
        future = submitted(pool.submit(produce, sender))?
        ok = future.result().is_ok()   # rejected: `receiver` is still live
        other_sender, other = channel[i64](1)?
        later = start(&pool, other_sender)?
        done = later.result().is_ok()  # rejected: `other` is still live
        drop(receiver)
        drop(other)
    Ok(())
```

The result is assumed to hold those futures even when the callee drops or waits
for one of them. A call through a value keeps only its arguments' jobs: jobs
submitted by a callable, closure, or boxed callable value, or by a function
compiled elsewhere, are not seen. Provenance is not tracked through
collections, class fields, or multi-field aggregate storage; indirect
submissions and blocking mapping callbacks retain the
[executor limits](executors.md#shutdown-and-live-receivers).
