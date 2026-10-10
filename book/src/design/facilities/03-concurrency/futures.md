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
