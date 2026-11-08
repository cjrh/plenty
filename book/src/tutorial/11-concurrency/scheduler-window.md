# Keep several jobs in flight

The [helper-loop scheduler](scheduler-loops.md) keeps one job outstanding.
To keep several running, do not simply call `submit` several times before
receiving. With one worker, a queue of one and a completion channel of one,
four blocking submissions hang forever: the first result fills the channel, the
second job blocks sending, the third fills the queue, and the fourth `submit`
waits for queue space while the only receiver is the scheduler itself.

Submit with `submit_nowait` instead. When the queue is full, receive one
completion, which lets a blocked worker continue, then retry the **same** job:

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

## Why it cannot hang

`outstanding` counts jobs the pool **accepted** and whose completion has not
been received. Each one is queued, running, blocked sending, or waiting in the
channel. When `submit_nowait` reports `Full` with `outstanding` above zero, one
of those jobs will deliver a completion, so `receive_one` makes progress and
frees space for a blocked worker. The scheduler waits on completions, not on
the queue, so this is not a busy retry. The next attempt may still find the
queue full; `admit` then receives again.

The same counting bounds memory: unreceived work can never exceed the worker
count plus the queue capacity plus the channel capacity. No separate limit is
needed for this cycle. Choose the three capacities independently; they need not
be equal.

`Full(job)` returns the complete unstarted job, so `admit` retries it without
preparing it again. A rejected job is never counted and no completion is
awaited for it. Every other submission result goes through `.drop_job()?`:
success drops only the future handle, and `Shutdown`, `OutOfMemory` and
`CapacityOverflow` release the rejected job's captures and propagate with their
variant. To recover such a job, match those variants as in
[recovering rejected submissions](submission-backpressure.md).

## Assumptions

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
