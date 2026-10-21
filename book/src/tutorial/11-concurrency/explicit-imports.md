# Import concurrency explicitly

The concurrency operations in these lessons also have explicit standard-library
imports. Module aliases and item aliases name the same facilities and types as
the familiar prelude spellings. Existing programs continue to work.

```plenty
import std.thread as threads
from std.channel import channel as bounded, Receiver as Inbox
from std.concurrent import ThreadPoolExecutor as Pool
from std.sync import CancellationToken as Stop

def receive(inbox: Inbox[str]) -> Result[str, RecvError]:
    inbox.recv()

def stopped(stop: Stop) -> bool:
    stop.wait()
    stop.is_cancelled()

type Errors = AllocError | ChannelError | IoError | SendError[str] | RecvError | ThreadError | PoolError | FutureError | SubmitError[()]

def main() -> Result[(), Errors]:
    sender, receiver = bounded[str](1)?
    sender.send("hello")?
    drop(sender)
    with threads.spawn(receive, receiver)? as task:
        print(task.join()?)?
    stop = Stop()?
    with Pool(1, 1)? as pool:
        future = pool.submit(stopped, stop.share()).drop_job()?
        stop.cancel()
        print(future.result()?)?
    Ok(())
```

```output
hello
True
```

Dropping the sender before joining permits a receiving worker to finish even
when it loops until disconnection. Imported names retain the same loan,
worker-effect, queue-bound and hang diagnostics as prelude names. Error types
can also be imported: `from std.concurrent import FutureError`, for example.
Importing an existing builtin under its own name is allowed only when it names
that exact builtin; unrelated imports still cannot replace it.

The module references list the complete surfaces:
[`std.thread`](../../design/stdlib/thread.md),
[`std.channel`](../../design/stdlib/channel.md),
[`std.concurrent`](../../design/stdlib/concurrent.md), and
[`std.sync`](../../design/stdlib/sync.md). Imports ship inside the compiler and
need no network or separately installed Plenty sources.

A failed submission returns its unstarted job inside `SubmitError`;
`drop_job()` drops it and leaves `SubmitError[()]`, as the
[executor lesson](executors-and-futures.md) explains. A spawn of a named
function with arguments reports `ThreadError`: its moved arguments are dropped,
not returned.
