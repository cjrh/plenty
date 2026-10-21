# Avoid waits that hang

A join waits for a worker. If the worker is blocked on a channel endpoint or a
lock that the waiting function still holds, or waits for a cancel that the
function has not made yet, the join never returns. Plenty
rejects such waits at compile time. These habits keep your programs clear of
the whole class:

1. **Move each endpoint into one worker as an argument:**
   `spawn(produce, sender)?`. A failed start then drops it.
2. **Guard each endpoint you keep as the last manager:**
   `with spawn(produce, sender)?, receiver as inbox:`.
3. **To wait inside the block, end the guard first.** Open it before any
   statement that can leave the block, and wait after it ends.
4. **Wait in a statement of its own.** An endpoint moved into the same
   expression is still held while the wait runs.
5. **Call `drop_job()` on a closure start you will not retry.**
6. **Start workers outside a lock block,** and lock inside them.
7. **Cancel a token on every exit** when a worker waits for it:
   `with spawn(serve, stop.share())?, stop.cancel_on_exit():`.

The [previous lesson](channels-and-shutdown.md) showed habits 1 to 3, and
[the mutex lesson](shared-mutex.md) showed habit 6.
[Cooperative cancellation](cooperative-cancellation.md#cancel-on-every-exit)
shows habit 7. Executor pools follow the
same habits, as the [future wait lesson](future-cancellation-and-waits.md)
shows. This lesson covers several producers and closure starts.

## Several producers

Each producer needs its own sender, and the receiver must outlive none of them.
One guard after several `?` starts is not enough: when a later start fails, `?`
joins the earlier producers before the guard exists.

```plenty-error
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def drain(inbox: &Receiver[i64]) -> i64:
    mut total = 0
    while True:
        match inbox.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    total

def main() -> Result[(), ThreadError | ChannelError | IoError]:
    sender, receiver = channel[i64](2)?
    second = sender.share()
    with spawn(produce, sender)?, spawn(produce, second)?, receiver as inbox:
        print(drain(inbox))?
    Ok(())
```

```error
leaving this `with` block joins a task whose worker holds `Sender[i64]` while `receiver` still holds `Receiver[i64]`
`?` leaves the block here
```

Give the receiver to a consuming worker instead, and start it first. Then guard
the sender and hand each producer a share of it:

```plenty
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def total(receiver: Receiver[i64]) -> i64:
    mut sum = 0
    while True:
        match receiver.recv():
            case Ok(value):
                sum = sum + value
            case Err(_):
                break
    sum

def main() -> Result[(), ThreadError | ChannelError | IoError]:
    sender, receiver = channel[i64](2)?
    with spawn(total, receiver)? as consumer:
        with sender as outbox, spawn(produce, outbox.share())?, spawn(produce, outbox.share())?:
            pass
        print(consumer.join())?
    Ok(())
```

```output
60
```

Here the sender guard comes first: the producers send too, so only the
consumer, joined later, needs the sender gone. The inner block joins both
producers, then drops the last sender. The consumer sees disconnection and
returns, so `consumer.join()` after the block cannot hang. If a producer fails
to start, `?` leaves through the same exits in the same order.

## Closure starts keep their endpoints

A closure job that fails to start returns inside `SpawnError[F]`, with every
endpoint it captured. `?` keeps that error until the function returns, which is
after the block's joins:

```plenty-error
def report(receiver: Receiver[i64]) -> Result[(), IoError]:
    mut total = 0
    while True:
        match receiver.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    print(total)

def produce(sender: Sender[i64], count: i64) -> Result[(), SendError[i64]]:
    for n in range(count):
        sender.send(n * n)?
    Ok(())

type Errors = ChannelError | SpawnError[OnceClosure[[], Result[(), SendError[i64]]]] | ThreadError

def main() -> Result[(), Errors]:
    sender, receiver = channel[i64](2)?
    count = 5
    job = def once [sender, count]() -> Result[(), SendError[i64]]:
        produce(sender, count)
    with spawn(report, receiver)?, spawn(job)?:
        pass
    Ok(())
```

```error
leaving this `with` block joins a task whose worker holds `Receiver[i64]` while the error that `?` propagates still holds `Sender[i64]`
call `drop_job()` on the start
```

When you will not retry the job, call `drop_job()` on the start. It drops the
job, and its sender, before `?` leaves, and the error becomes `SpawnError[()]`:

```plenty
def report(receiver: Receiver[i64]) -> Result[(), IoError]:
    mut total = 0
    while True:
        match receiver.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    print(total)

def produce(sender: Sender[i64], count: i64) -> Result[(), SendError[i64]]:
    for n in range(count):
        sender.send(n * n)?
    Ok(())

type Errors = ChannelError | SpawnError[()] | ThreadError

def main() -> Result[(), Errors]:
    sender, receiver = channel[i64](2)?
    count = 5
    job = def once [sender, count]() -> Result[(), SendError[i64]]:
        produce(sender, count)
    with spawn(report, receiver)?, spawn(job).drop_job()?:
        pass
    Ok(())
```

```output
30
```

A named start, `spawn(produce, sender, count)?`, needs no closure here and
drops its arguments on failure too.

The [channel reference](../../design/facilities/03-concurrency/channels.md#joins-and-live-endpoints)
lists every way a function can hold an endpoint, and the programs the check
rejects although they could not hang.
