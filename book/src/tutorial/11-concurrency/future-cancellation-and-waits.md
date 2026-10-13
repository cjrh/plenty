# Cancel queued work and bound a future wait

`future.cancel()` returns `True` when it prevents a queued job from starting.
It cannot interrupt running work. A cancelled future reports
`FutureError.Cancelled` from `result()`. `future.done()` observes completion or
cancellation without consuming the handle. Explicit `pool.shutdown(True)` cancels
jobs still queued and waits for running jobs; ordinary scope exit drains all work.
If a job sends through a channel, guard the receiver directly after the pool so
it disconnects before shutdown waits. The
[next lesson](cooperative-cancellation.md) shows the rule.

For a future, `future.wait_timeout(milliseconds)` returns a boolean: true means
completed or cancelled. This only observes the future; call `result()` afterward
to take its result. A false return leaves the job running or queued.

Before `result()` on a job that holds a sender, drop matching receivers or end
their guards. Plenty rejects a blocking result wait while those receivers are
still alive, even after `done()` or `wait_timeout()` reports completion. These
observations do not prove the channel protocol to the checker.

A worker that may run indefinitely needs a cooperative shutdown protocol:
scope exit still waits after a timed wait returns.
