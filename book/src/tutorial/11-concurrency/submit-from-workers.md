# Split work from inside a pool job

A job sometimes discovers more work while it runs: a tree to walk, a range to
split. A worker cannot hold the `ThreadPoolExecutor`, but it can hold a
`Submitter` from `pool.submitter()`, which only submits and never waits.

This program sums `0..100` by splitting the range in jobs that submit their
halves, and sends each part's sum to `main`:

```plenty
type Part = Result[i64, OverflowError[i64]]

def sum_range(lo: i64, hi: i64) -> Part:
    mut total = 0
    for n in range(lo, hi):
        total = total + n
    Ok(total)

def submit_half(submitter: &Submitter, out: &Sender[Part], lo: i64, hi: i64) -> ():
    match submitter.submit_nowait(add, submitter.share(), out.share(), lo, hi):
        case Ok(_):
            pass
        case Err(SubmitError.Full(job) | SubmitError.Shutdown(job) | SubmitError.OutOfMemory(job) | SubmitError.CapacityOverflow(job)):
            job()

def split(submitter: &Submitter, out: &Sender[Part], lo: i64, hi: i64) -> Result[(), OverflowError[i64]]:
    if hi - lo <= 25:
        _ = out.send(sum_range(lo, hi))
        return Ok(())
    middle = lo + (hi - lo) // 2
    submit_half(submitter, out, lo, middle)
    submit_half(submitter, out, middle, hi)
    Ok(())

def add(submitter: Submitter, out: Sender[Part], lo: i64, hi: i64) -> ():
    match split(&submitter, &out, lo, hi):
        case Ok(_):
            pass
        case Err(error):
            _ = out.send(Err(error))

def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()] | OverflowError[i64]]:
    sender, receiver = channel[Part](4)?
    with ThreadPoolExecutor(3, 4)? as pool, receiver as inbox:
        submitter = pool.submitter()
        submitter.submit_nowait(add, submitter.share(), sender, 0, 100).drop_job()?
        mut total = 0
        while True:
            match inbox.recv():
                case Ok(part):
                    total = total + part?
                case Err(_):
                    break
        print(total)?
    Ok(())
```

```output
4950
```

Each job takes its own handle and sender with `share()`; neither allocates.
A `Submitter` job returns `()`, since there is no future to read a result
from. Errors travel on the channel instead, so `main` still propagates an
overflow from any part.

There is no blocking `submit` on a `Submitter`: a worker waiting for space in
its own queue could wait forever. When the queue is full, `submit_nowait`
returns the whole job in `Full(job)`, and `submit_half` runs it inline. The
same arm handles `Shutdown(job)`, which the
[submission backpressure lesson](submission-backpressure.md) covers for
`pool.submit_nowait`.

The pool must be quiet before it shuts down: a submission after shutdown
begins returns `Shutdown(job)`. Here the receive loop ends only when every
sender has dropped, which happens only after every job has finished, so no job
is still splitting when the pool's block ends.

The compiler rejects a job that waits, without a time limit, for something a
job it submits would release: a queue, a token, a mutex, or a thread joined on
them. If `add` received its children's sums itself with `recv()`, every worker
could wait while the children sat queued behind them. Sending to `main`, which
is not a pool worker, avoids that. For a running total, an atomic counter works where a
`Shared[Mutex[i64]]` would be rejected.

A wait with a time limit, such as `recv_timeout_ms` or `wait_timeout_ms`, is
allowed, since its timeout always ends it. The cost is lateness: a message or
cancellation from a queued job arrives no sooner than the timeout. A loop that
retries a timed wait until a child acts can still hang, as polling with
`recv_nowait` can, and the compiler cannot catch that. Socket I/O is not
checked: when a queued job holds the peer, use `read_before` or `write_before`
rather than a plain read or write. See
[waits in submitting jobs](../../design/facilities/03-concurrency/executors.md#waits-in-submitting-jobs)
for the full rules.
