# Keep several jobs in flight

The [helper-loop scheduler](scheduler-loops.md) keeps one job outstanding.
To keep several running, collect their results with
[`std.sync.Completions`](../../design/stdlib/sync.md#completionst). It holds up
to a fixed number of accepted jobs and hands back their results in completion
order:

```plenty
from std.sync import Completions

type Outcome = Result[i64, OverflowError[i64]]
type ScheduleError = ParseError | OverflowError[i64] | FutureError | SubmitError[()]

def square(value: i64) -> Outcome:
    Ok(value * value)

def add(total: &mut i64, finished: Option[Result[Outcome, FutureError]]) -> Result[(), ScheduleError]:
    match finished:
        case Some(outcome):
            *total += outcome??
        case Nothing:
            pass
    Ok(())

def dispatch(pool: &ThreadPoolExecutor, inputs: &list[str], done: &mut Completions[Outcome]) -> Result[i64, ScheduleError]:
    mut total = 0
    for text in inputs:
        value = i64.parse(*text)?
        job = def once [value]() -> Outcome:
            square(value)
        add(&mut total, done.submit(pool, job).drop_job()?)?
    while not done.is_empty():
        add(&mut total, done.next())?
    Ok(total)

def run(inputs: &list[str], workers: Positive[i64], queue: Positive[i64], in_flight: Positive[i64]) -> Result[i64, AllocError | PoolError | ScheduleError]:
    mut done = Completions[Outcome](in_flight)?
    with ThreadPoolExecutor(workers, queue)? as pool:
        return dispatch(pool, inputs, &mut done)

def main() -> Result[(), AllocError | IoError | PoolError | ScheduleError]:
    inputs = ["1", "2", "3", "4", "5", "6", "7", "8"]?
    print(run(&inputs, 1, 1, 1)?)?
    print(run(&inputs, 2, 3, 4)?)?
    overflowing = ["1", "9223372036854775807", "2", "3", "4"]?
    match run(&overflowing, 1, 1, 1):
        case Err(OverflowError(_)):
            print("worker error preserved")?
        case _ as other:
            drop(other?)
    Ok(())
```
```output
204
204
worker error preserved
```

Each job returns its result instead of sending it. `submit` accepts the job
while `done` has a free slot and returns `Nothing`. When every slot is occupied
it first waits for a finished job, submits the new one, and returns the
finished result as `Some(outcome)`. `next` has the same result type: a finished
job's result, or `Nothing` when no job is outstanding. So `add` handles both,
and the loop after the inputs drains the rest. Typed failures stay typed:
`outcome??` propagates first a `FutureError`, then the job's own
`OverflowError`.

`.drop_job()?` handles the other submission results. If the pool rejects the
job, with `Shutdown`, `OutOfMemory` or `CapacityOverflow`, the error holds the
job you passed, and no finished result is consumed; it stays in `done` for
`next`. To recover the job, match the variants as in
[recovering rejected submissions](submission-backpressure.md).

## Make room before preparing

`submit` waits for a slot after the job is built. When building it is
expensive, or obtains something that must be released, such as a file lock,
make room first:

```plenty fragment
if done.is_full():
    add(&mut total, done.next())?
job = prepare(text)?   # may fail; nothing was reserved
add(&mut total, done.submit(pool, job).drop_job()?)?
```

With a slot free, `submit` returns `Nothing` without waiting for a result. Only
the scheduler that owns `done` submits through it, so the slot stays free while
the job is prepared, and a failed preparation leaves nothing to release. The
pool queue may still be full; `submit` then waits for a worker to take a job,
which every running job eventually does.

`done` does not own locks, partial state, or cleanup obligations. Keep those in
your own values, as in [partial preparation](scheduler-preparation.md): an
external lock is acquired and released by the application.

## Wait with a deadline or a stop request

`next` waits without limit. To stop on a deadline or a token, use
`done.next_before(&deadline, &cancel)`, which wraps the result of `next` in
`Result[_, WaitError]`. To wait for a result or a message on another channel,
such as a stop request, use
`done.next_or_recv_before(&receiver, &deadline, &cancel)`; it returns
`Selected.First(result)` or `Selected.Second(message)`, like
[deadline-aware selection](channel-selection.md#wait-until-a-deadline-or-a-stop-request).
A waiting message wins over finished jobs, so a stop is not delayed while
results keep arriving.
Waits poll in slices of at most 10 ms, so a result can be handed back up to
10 ms after it finished.

`done.cancel_queued()` cancels jobs that have not started; `next` then returns
`Some(Err(FutureError.Cancelled))` for each. On an early return, as with the
overflowing input above, the remaining results are dropped with `done`, and the
pool still finishes and joins every accepted job.

## Why it cannot hang

Several blocking submissions to a pool whose jobs **send** results to a
bounded channel can hang. With one worker, a queue of one and a completion
channel of one, the first result fills the channel, the second job blocks
sending, the third fills the queue, and a fourth `submit` waits for queue space
while the only receiver is the scheduler itself.

`Completions` breaks that cycle: a job stores its result in its own future and
never waits for the scheduler. Each blocking submission waits only for a worker
to take a queued job, so the capacities can be chosen independently. Memory is
bounded by the slot count, fixed when `done` is created: `submit` does not
accept another job until a result has been handed back.

## Without `Completions`: a completion channel

Use a channel when jobs must send more than one message, or when results are
received by a different thread. Then the scheduler must not wait in a blocking
`submit` while it owes the channel a receive. Submit with `submit_nowait`; when
the queue is full, receive one completion, which lets a blocked worker continue,
and retry the **same** job:

```plenty
type Completion = Result[i64, OverflowError[i64]]
type ScheduleError = ParseError | OverflowError[i64] | RecvError | SubmitError[()]

def square(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value * value)

def work(value: i64, sender: Sender[Completion]) -> ():
    # Disconnection means the scheduler abandoned reception during cleanup.
    drop(sender.send(square(value)))

def receive_one(inbox: &Receiver[Completion], outstanding: &mut i64) -> Result[i64, ScheduleError]:
    completion = inbox.recv()?
    *outstanding -= 1
    Ok(completion?)

def admit[F: OnceCallable[[], ()]](pool: &ThreadPoolExecutor, job: F, inbox: &Receiver[Completion], outstanding: &mut i64, total: &mut i64) -> Result[(), ScheduleError]:
    mut attempt = pool.submit_nowait(job)
    while True:
        match attempt:
            case Err(SubmitError.Full(job)):
                if *outstanding == 0:
                    # No completion of ours is owed, so waiting cannot block a worker of ours.
                    attempt = pool.submit(job)
                else:
                    *total += receive_one(inbox, outstanding)?
                    attempt = pool.submit_nowait(job)
            case _ as other:
                drop(other.drop_job()?)
                break
    *outstanding += 1
    Ok(())

def dispatch(pool: &ThreadPoolExecutor, inputs: &list[str], outbox: &Sender[Completion], inbox: &Receiver[Completion]) -> Result[i64, ScheduleError]:
    mut outstanding = 0
    mut total = 0
    for text in inputs:
        value = i64.parse(*text)?
        sender = outbox.share()
        job = def once [value, sender]() -> ():
            work(value, sender)
        admit(pool, job, inbox, &mut outstanding, &mut total)?
    while outstanding > 0:
        total += receive_one(inbox, &mut outstanding)?
    Ok(total)

def run(inputs: &list[str], workers: Positive[i64], queue: Positive[i64], completions: Positive[i64]) -> Result[i64, AllocError | PoolError | ScheduleError]:
    outbox, receiver = channel[Completion](completions)?
    with ThreadPoolExecutor(workers, queue)? as pool, receiver as inbox:
        return dispatch(pool, inputs, &outbox, inbox)

def main() -> Result[(), AllocError | IoError | PoolError | ScheduleError]:
    inputs = ["1", "2", "3", "4", "5", "6", "7", "8"]?
    print(run(&inputs, 1, 1, 1)?)?
    print(run(&inputs, 2, 3, 1)?)?
    overflowing = ["1", "9223372036854775807", "2", "3", "4"]?
    match run(&overflowing, 1, 1, 1):
        case Err(OverflowError(_)):
            print("worker error preserved")?
        case _ as other:
            drop(other?)
    Ok(())
```
```output
204
204
worker error preserved
```

Eight jobs complete on pools whose worker, queue and channel capacities add up
to only three and five.

`outstanding` counts jobs the pool **accepted** and whose completion has not
been received; `Completions` keeps the same count as its occupied slots. Each
one is queued, running, blocked sending, or waiting in the channel. When
`submit_nowait` reports `Full` with `outstanding` above zero, one of those jobs
will deliver a completion, so `receive_one` makes progress and frees space for
a blocked worker. The scheduler waits on completions, not on the queue, so this
is not a busy retry. Unreceived work never exceeds the worker count plus the
queue capacity plus the channel capacity.

`Full(job)` returns the complete unstarted job, so `admit` retries it without
preparing it again. A rejected job is never counted and no completion is
awaited for it.

### Assumptions of the channel scheduler

The argument holds when:

- each accepted job sends exactly one completion on this channel, and the
  scheduler alone receives it;
- the scheduler is the pool's only submitter, or other submitters' jobs also
  finish without waiting on this scheduler;
- workers eventually reach their send, and nothing cancels queued jobs. A job
  removed by `pool.cancel_pending()` sends nothing, so subtract it from
  `outstanding` yourself.

Running a rejected job inline is not a substitute here: a job that sends to the
scheduler's own channel can block the only receiver.

On an early error, such as the overflowing input above, `dispatch` returns
with jobs still outstanding. The receiver guard is the last manager, so it
drops first, the blocked senders see disconnection, and the pool drains and
joins. A scheduler that also waits for wake-ups or shutdown should replace the
unconditional `recv` with a [deadline-aware selection](channel-selection.md#wait-until-a-deadline-or-a-stop-request)
and keep the retained job and its cleanup obligations owned on every path; see
[partial preparation](scheduler-preparation.md).
