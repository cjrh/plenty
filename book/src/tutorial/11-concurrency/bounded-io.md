# Wait for I/O with a deadline

A worker can own or borrow a `File` like any other value. To keep it from
blocking forever on a pipe that never delivers, wait with
[`std.ready`](../../design/stdlib/ready.md): every wait has a deadline, and a
cancellable wait checks its token at least every 10 milliseconds.

```plenty
import std.bytes
import std.io
import std.process
from std.io import FileStream
from std.process import Stdio
from std.ready import WaitError, read_before
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now, sleep

def first_bytes(stream: &mut FileStream, deadline: &Deadline, stop: &CancellationToken) -> str:
    mut buffer = std.bytes.with_capacity(16).unwrap()
    buffer.resize(16, 0u8).unwrap()
    match read_before(stream, &mut buffer, 0, 16, deadline, stop):
        case Ok(count):
            str.from(count).unwrap()
        case Err(WaitError.TimedOut):
            "timed out"
        case Err(WaitError.Cancelled):
            "cancelled"
        case Err(WaitError.Stream(_)):
            "failed"

def main() -> Result[(), Failure]:
    mut command = std.process.shell("sleep 10")?
    command.stdout(Stdio.Piped)
    mut child = command.spawn()?
    mut quiet = std.io.from_file(child.take_stdout().unwrap())
    stop = CancellationToken()?
    soon = deadline_after(&monotonic_now(), &milliseconds(20))?
    with spawn(first_bytes, &mut quiet, &soon, &stop)? as task:
        print(task.join())?
    later = deadline_after(&monotonic_now(), &milliseconds(60000))?
    observer = stop.share()
    with spawn(first_bytes, &mut quiet, &later, &observer)? as task:
        sleep(&milliseconds(20))
        stop.cancel()
        print(task.join())?
    child.kill()?
    child.wait()?
    Ok(())
```

```output
timed out
cancelled
```

The child writes nothing, so the first worker's 20-millisecond deadline passes.
The second worker has a minute, but the parent cancels it, and it returns
within one 10-millisecond slice. Either way the worker returns normally and
the scope joins it; the pipe and buffer are unchanged.

A worker may not change the working directory, because every thread resolves
relative paths against it:

```plenty-error
import std.env
import std.path

def worker() -> ():
    std.env.set_current_dir(&std.path.from_text("/").unwrap()).unwrap()

def main() -> Result[(), Failure]:
    with spawn(worker)?:
        pass
    Ok(())
```

```error
changing the working directory affects every thread's relative paths
```

Change it on the main thread before starting workers, or give a child its own
directory with `Command.current_dir`. See
[native effects on worker threads](../../design/facilities/03-concurrency/effects.md)
for the rules each native operation follows.
