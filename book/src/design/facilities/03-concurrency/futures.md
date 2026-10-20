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
and the result wait blocked forever. Drop every matching receiver, or end its
guard, before retrieving the result. Guarding a receiver alongside the pool
only disconnects it at pool shutdown; that is too late for a result wait inside
the block.

The diagnostic points to the wait, receiver holders, and submission. Tracking
follows local future moves, `?`, `unwrap()`, and extraction from submission
results. Separate futures with the same result type keep their own job origins.
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
function that makes one, is assumed to wait. Provenance is not tracked through a
future returned from a call, collections, class fields, or multi-field aggregate
storage; indirect submissions and blocking mapping callbacks retain the
[executor limits](executors.md#shutdown-and-live-receivers).
