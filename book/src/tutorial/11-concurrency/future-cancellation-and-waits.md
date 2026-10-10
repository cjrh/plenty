# Cancel queued work and bound a future wait

After submitting work, decide how long the caller can wait and what should
happen to work it no longer needs. These choices differ for queued jobs and
jobs that have already started.

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


A timeout leaves the job's lifetime unchanged. Scope exit still waits for
running workers, so a worker that may run indefinitely also needs a cooperative
shutdown protocol. Cancellation and a timed wait are not forced interruption.
