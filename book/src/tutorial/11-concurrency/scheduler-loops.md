# Submit and receive in a helper loop

A scheduler may need its completion receiver throughout a pool's lifetime.
Start with ordinary helpers: neither submission nor the scheduling loop must
always be inline. This finite example keeps **one accepted job outstanding**,
receives its result, then prepares another. That deliberately simple protocol
avoids a scheduler filling the work queue while workers fill the completion
queue. To keep several jobs in flight, collect their results with
`std.sync.Completions`, as in [keeping several jobs in flight](scheduler-window.md);
a channel capacity equal to the worker count is not enough.

```plenty
enum Stopped:
    Cancelled

type WorkError = OverflowError[i64] | Stopped
type ScheduleError = ParseError | WorkError | RecvError | SubmitError[()]

def calculate(value: i64, stop: &CancellationToken) -> Result[i64, WorkError]:
    if stop.is_cancelled():
        return Err(Stopped.Cancelled)
    Ok(value * value)

def work(value: i64, stop: CancellationToken, sender: Sender[Result[i64, WorkError]]) -> ():
    outcome = calculate(value, &stop)
    # Disconnection means the scheduler abandoned reception during cleanup.
    # Computation failures travel in the message, not in an ignored future.
    drop(sender.send(outcome))

def submit_run(pool: &ThreadPoolExecutor, value: i64, stop: CancellationToken, sender: Sender[Result[i64, WorkError]]) -> Result[(), SubmitError[()]]:
    future = pool.submit(work, value, stop, sender).drop_job()?
    drop(future)
    Ok(())

def dispatch(pool: &ThreadPoolExecutor, inputs: &list[str], stop: &CancellationToken, outbox: &Sender[Result[i64, WorkError]], inbox: &Receiver[Result[i64, WorkError]]) -> Result[i64, ScheduleError]:
    mut total = 0
    for text in inputs:
        value = i64.parse(*text)?
        submit_run(pool, value, stop.share(), outbox.share())?
        total += inbox.recv()??
    Ok(total)

def run(inputs: &list[str], stop: &CancellationToken) -> Result[i64, AllocError | PoolError | ScheduleError]:
    with stop.cancel_on_exit():
        outbox, receiver = channel[Result[i64, WorkError]](1)?
        with ThreadPoolExecutor(1, 1)? as pool, stop.cancel_on_exit(), receiver as inbox:
            return dispatch(pool, inputs, stop, &outbox, inbox)

def main() -> Result[(), AllocError | IoError | PoolError | ScheduleError]:
    inputs = ["1", "2", "3"]?
    stop = CancellationToken()?
    print(run(&inputs, &stop)?)?
    invalid = ["1", "invalid"]?
    invalid_stop = CancellationToken()?
    match run(&invalid, &invalid_stop):
        case Err(ParseError.Invalid):
            print("setup error preserved")?
        case _ as other:
            drop(other?)
    overflowing = ["9223372036854775807"]?
    overflow_stop = CancellationToken()?
    match run(&overflowing, &overflow_stop):
        case Err(OverflowError(_)):
            print("worker error preserved")?
        case _ as other:
            drop(other?)
    cancelled = CancellationToken()?
    cancelled.cancel()
    match run(&inputs, &cancelled):
        case Err(Stopped.Cancelled):
            print("cancelled")?
        case _ as other:
            drop(other?)
    Ok(())
```
```output
14
setup error preserved
worker error preserved
cancelled
```

The completion carries the worker's typed error. `recv()??` first checks
reception, then the computation. Setup and allocation failures propagate too;
printing an error and returning success would not preserve that contract.
The handled failures in `main` intentionally exercise setup, worker overflow
and cancellation.

`submit_run` drops only the **future handle** on success: that neither joins nor
cancels the accepted job. On failure, `.drop_job()` releases the **unstarted
job's captures** and keeps the original `SubmitError[()]` variant. Use the
[owned-job form](owned-worker-jobs.md) instead if rejected work needs retry or
resource-specific recovery. Never silently treat a rejected submission as a
job from which a completion will arrive.

## Cleanup at exit is not cleanup during a call

The receiver is the last manager. On normal return or any propagated error,
it drops first, the cancellation guard requests stop, then the pool drains
accepted jobs and joins workers. The outer cancellation guard also covers
channel/pool construction failure; cancelling twice is harmless. There are no
other receiver aliases here;
with aliases, dropping one endpoint need not disconnect the queue. The worker
explicitly tolerates send disconnection because the scheduler is abandoning
reception. A successful run receives every computation result.

Cancellation is cooperative. Checking a token before computing does not
interrupt an ordinary blocking `send`, a child wait, or a foreign call.
This example releases a blocked send by dropping the last receiver. For waits
that must observe cancellation while the receiver remains alive, use an
appropriate [cancellable deadline operation](deadlines.md), such as
`std.sync.send_before`, and handle its unsent message and error. Release held
locks before joining too.

A receiver guard remains **live during `dispatch`**. If that helper actually
shuts down a pool while a producer is blocked sending, neither a later drain
nor the eventual guard exit can unblock that mid-call shutdown. Similarly,
end the receiver guard before [waiting on a future](future-cancellation-and-waits.md#jobs-that-send).

## When the current checker rejects a larger helper

The example's helper loop is accepted. Capture-free `Callable` calls with
recognized data-only input and result types also do not imply shutdown.
Other unknown effects, including capability-bearing or erased calls and missing
bodies, and unrelated owned pools can still combine with transitive submissions
to reject a larger helper. This remaining limitation does not require all
scheduler loops to be inline. The checker matches endpoint types rather than
queue/pool identities; it does not prove capacity bounds or that a drained
running set means jobs have finished. See [helper-call limitations](../../design/facilities/03-concurrency/executors.md#helper-based-schedulers).

Before changing the loop, read the diagnostic notes. A **discovered** shutdown
comes from an explicit operation or owned-pool cleanup; an **assumed** shutdown
comes from an unresolved call, not proof that it shuts down. Follow the bounded
call chain and available source positions. Even a discovered shutdown may involve
an unrelated pool: the explanation does not prove pool identity or call order.

A narrow fallback is to keep `submit_run`, move the dispatch loop into the
pool-owning block, and separate preparation and completion bookkeeping from
the pool-handed helper. The loop body can still call `prepare`, `submit_run`,
`hand_off`, and `await_one`; only the small submission helper needs the pool.
Do this only where a rejected call graph requires it, not as a universal
architecture. The data-only `Callable` case is also accepted for a `Submitter`
helper. `Submitter` is still not a universal workaround for other unknown
effects or unrelated owned-pool cleanup.

When preparation creates resources for several runs, a later error must not
lose earlier cleanup obligations. Return or accumulate already-prepared runs
in caller-owned state even on failure; hand them off or explicitly clean them
up before propagating the original error. This example prepares one value at
a time and receives its completion before parsing another, so the failing
second parse leaves no earlier run outstanding. For batching, see
[partial preparation](scheduler-preparation.md).

Do not wrap the pool in a class to evade the check. Do not replace blocking
submission or completion delivery with ignored `Full` errors or endless
`nowait` retries: these lose work or can still hang, and change backpressure
semantics. Retrying a `Full` job after receiving a completion is different: it
waits on progress, as in [keeping several jobs in flight](scheduler-window.md).
See [recovering rejected submissions](submission-backpressure.md).
