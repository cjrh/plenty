# Cancel queued work and bound a future wait

`future.cancel()` returns `True` when it prevents a queued job from starting.
It cannot interrupt running work. A cancelled future reports
`FutureError.Cancelled` from `result()`. `future.done()` observes completion or
cancellation without consuming the handle. Explicit `pool.shutdown(True)` cancels
jobs still queued and waits for running jobs; ordinary scope exit drains all work.
If a job communicates through channels, finish or disconnect that communication
before waiting for shutdown, as in the channel lessons.

For a future, `future.wait_timeout(milliseconds)` returns a boolean: true means
completed or cancelled. This only observes the future; call `result()` afterward
to take its result. A false return leaves the job running or queued.

A worker that may run indefinitely needs a cooperative shutdown protocol:
scope exit still waits after a timed wait returns.
