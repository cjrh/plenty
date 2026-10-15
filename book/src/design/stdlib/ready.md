# `std.ready`

`import std.ready` waits for a native handle to become readable or writable,
bounded by a [`Deadline`](time.md) and optionally by a
[cancellation token](sync.md). It is the blocking model for I/O on worker
threads: each worker makes bounded blocking waits, and no event loop, callback
registration or `async` syntax is involved. See
[native effects on worker threads](../facilities/03-concurrency/effects.md).

The runtime supplies one primitive, a single `ppoll` on the handle's descriptor
with a nanosecond timeout. Deadlines, interruption retries and cancellation are
checked Plenty in `stdlib/std/ready.plenty`.

## Types

| Type | Contents |
|---|---|
| `Interest` | `Read`, `Write` or `ReadWrite` |
| `Readiness` | Public `bool` fields `readable`, `writable`, `hangup`, `error` |
| `WaitError` | `TimedOut`, `Cancelled`, or `Stream(StreamError)` |

`hangup` and `error` are reported whatever the interest. On Linux a pipe whose
writer has closed reports `hangup` without `readable`; a read then returns end
of input. `error` means a read or write will report the pending error.

## Functions

| Function | Result |
|---|---|
| `wait_until(file: &File, interest: Interest, deadline: &Deadline)` | `Result[Readiness, WaitError]` |
| `wait(file: &File, interest: Interest, deadline: &Deadline, cancel: &CancellationToken)` | `Result[Readiness, WaitError]` |
| `read_before(stream: &mut FileStream, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken)` | `Result[i64, WaitError]` |
| `write_before(stream: &mut FileStream, source: &Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken)` | `Result[i64, WaitError]` |
| `cancellation_slice()` | `Duration`: the longest a cancellable wait blocks without checking its token |

- A handle that is already ready succeeds, even when the deadline has passed.
  Otherwise the wait returns `TimedOut` once the deadline passes.
- `wait` checks the token before each native wait and waits at most one slice
  (10 milliseconds) at a time. A token cancelled before the call returns
  `Cancelled` even when the handle is ready; one cancelled during the wait is
  seen within one slice.
- An interrupted wait (`EINTR`) is retried against the same deadline.
- A closed `File` is `Stream(StreamError.Io(IoError.Closed))`. An invalid
  descriptor is reported the same way.
- `read_before` waits for input and then makes one
  [`FileStream.read`](io.md); zero is end of input. With a single reader of a
  pipe, socket or terminal, that read does not block.
- `write_before` waits for space and then makes one write of at most 4096
  bytes, which a writable pipe or socket accepts without blocking. It returns
  the bytes written; loop to write more.
- A wait borrows the handle and allocates nothing. On `TimedOut` or
  `Cancelled`, the handle, the stream position and the caller's buffer are
  unchanged.

Regular files are always ready; readiness is useful for pipes, terminals and,
later, sockets. A blocking `read` on a handle another thread also reads can
still block after a readiness report. The loan rules prevent that within one
program unless the descriptor is shared with a child process.

## Example

```text
from std.ready import WaitError, read_before
from std.sync import CancellationToken
from std.time import deadline_after, milliseconds, monotonic_now

def drain(stream: FileStream, cancel: CancellationToken) -> Result[i64, WaitError]:
    mut owned = stream
    mut buffer = std.bytes.with_capacity(64).unwrap()
    buffer.resize(64, 0u8).unwrap()
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000)).unwrap()
    mut total = 0
    while True:
        count = read_before(&mut owned, &mut buffer, 0, 64, &deadline, &cancel)?
        if count == 0:
            return Ok(total)
        total = total + count
    Ok(total)
```

The [tutorial lesson](../../tutorial/11-concurrency/bounded-io.md) runs a
complete program.

## Limits

- x86_64 Linux GNU only; elsewhere every wait reports `IoError.Unsupported`.
- One handle per wait. Waiting on several handles at once, and waking a wait
  through its token without the 10-millisecond slice, are not provided.
- Sockets and DNS are not part of this module; see the backlog.
