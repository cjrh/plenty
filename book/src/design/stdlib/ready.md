# `std.ready`

`import std.ready` waits for a native handle, or any of several handles, to
become readable or writable,
bounded by a [`Deadline`](time.md) and optionally by a
[cancellation token](sync.md). It is the blocking model for I/O on worker
threads: each worker makes bounded blocking waits, and no event loop, callback
registration or `async` syntax is involved. See
[native effects on worker threads](../facilities/03-concurrency/effects.md).

The runtime supplies four primitives (opcodes 419–422): a single `ppoll` on a
handle's descriptor with a nanosecond timeout, and opening, registering with
and waiting on a selector. Deadlines, interruption retries, cancellation and
event decoding are checked Plenty in `stdlib/std/ready.plenty`.

```plenty
import std.bytes
import std.io
import std.process
from std.process import Stdio
from std.ready import WaitError, read_before
from std.sync import CancellationToken
from std.time import deadline_after, milliseconds, monotonic_now

def main() -> Result[(), AllocError | IoError | RangeError | WaitError | std.bytes.BytesError | str]:
    mut command = std.process.shell("echo ready")?
    command.stdout(Stdio.Piped)
    mut child = command.spawn()?
    pipe: Result[File, str] = child.take_stdout().ok_or("missing piped stdout")
    mut output = std.io.from_file(pipe?)
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000))?
    mut buffer = std.bytes.with_capacity(16)?
    buffer.resize(16, 0u8)?
    count = read_before(&mut output, &mut buffer, 0, 16, &deadline, &cancel)?
    print(count, buffer.to_text_range(0, count)?.strip()?)?
    child.wait()?
    Ok(())
```
```output
6 ready
```

## Types

| Type | Contents |
|---|---|
| `Interest` | `Read`, `Write` or `ReadWrite` |
| `Readiness` | Public `bool` fields `readable`, `writable`, `hangup`, `error` |
| `WaitError` | `TimedOut`, `Cancelled`, or `Stream(StreamError)` |
| `Selector` | An owned set of registered handles; see [Selectors](#selectors) |
| `Event` | Public fields `key: u64` and `readiness: Readiness` |

`hangup` and `error` are reported whatever the interest. On Linux a pipe whose
writer has closed reports `hangup` without `readable`; a read then returns end
of input. `error` means a read or write will report the pending error.

## Functions

| Function | Result |
|---|---|
| `wait_until(file: &File, interest: Interest, deadline: &Deadline)` | `Result[Readiness, WaitError]` |
| `wait(file: &File, interest: Interest, deadline: &Deadline, cancel: &CancellationToken)` | `Result[Readiness, WaitError]` |
| `read_before(stream: &mut FileStream, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken)` | `Result[i64, WaitError]` |
| `write_before[S: ByteSource](stream: &mut FileStream, source: &S, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken)` | `Result[i64, WaitError]` |
| `selector(capacity: i64)` | `Result[Selector, WaitError]` |

- A handle that is already ready succeeds, even when the deadline has passed.
  Otherwise the wait returns `TimedOut` once the deadline passes.
- A token cancelled before the call returns `Cancelled` even when the handle
  is ready. One cancelled during the wait wakes it at once: the wait polls the
  token's wake descriptor alongside the handle, and `cancel()` makes that
  descriptor readable. On loopback, 50 cancellations of a blocked `accept`
  woke it in 27 microseconds on average and 80 at worst.
- An interrupted wait (`EINTR`) is retried against the same deadline.
- A closed `File` is `Stream(StreamError.Io(IoError.Closed))`. An invalid
  descriptor is reported the same way.
- `read_before` waits for input and then makes one
  [`FileStream.read`](io.md); zero is end of input.
- `write_before` waits for space and then makes one write of at most 4096
  bytes. This cap bounds pipe writes; it does not guarantee space in a
  blocking socket's send buffer. It returns the bytes written; loop to write more.
- A wait borrows the handle and allocates nothing. On `TimedOut` or
  `Cancelled`, the handle, the stream position and the caller's buffer are
  unchanged.

Regular files are always ready; readiness is useful for pipes, terminals and
[sockets](net.md). These generic `FileStream` transfer helpers neither change
descriptor mode nor retry a transfer after stale readiness. A blocking
descriptor can still block after a readiness report, for example when another
reader consumed the input. Use [`std.net`](net.md#tcp)'s transfer methods for
TCP: its descriptors stay nonblocking and its deadline methods retry against
the original budget.

## Selectors

A selector waits on many handles in one call, so one worker can serve many
idle connections. It owns one epoll descriptor and an event buffer of 16
bytes per event; it starts no thread.

| Method | Result |
|---|---|
| `selector.watch(file: &File, key: u64, interest: Interest)` | `Result[(), WaitError]`: register, or change the key and interest |
| `selector.unwatch(file: &File)` | `Result[(), WaitError]`; a handle that is not registered is not an error |
| `selector.wait(deadline, cancel)` | `Result[i64, WaitError]`: how many events are ready, at least 1 |
| `selector.event(index: i64)` | `Result[Event, WaitError]`: event `index` of the last wait |
| `selector.close()` | `Result[(), WaitError]` |

- `capacity` is the most events one wait reports, clamped to 1..64. Further
  ready handles stay ready for the next wait.
- Registrations are level-triggered: a handle is reported by every wait while
  it is ready, until it is read, written or unwatched. Closing a registered
  handle removes it.
- `wait` follows the single-handle rules: ready handles are reported even when
  the deadline has passed, an earlier cancellation wins, a later one wakes the
  wait at once, and `EINTR` is retried.
- An index outside the last wait's count is `Stream(StreamError.OutOfRange)`.
- `watch`, `unwatch` and `wait` need the selector exclusively (`&mut`); handles
  are borrowed shared only for the call. A registered handle may still be
  read, written or closed by its owner.
- [`std.net`](net.md) streams, listeners and UDP sockets have `watch` and
  `unwatch` methods that report `NetError`.

For TCP, follow an event with [`accept_now`, `read_now` or
`write_now`](net.md#waiting-on-many-sockets). They make one nonblocking attempt
without a second `ppoll`. If the event is stale, `NetError.WouldBlock` returns
control to the selector loop instead of blocking other connections. Keep an
offset after a partial write, and stop watching for write readiness when the
output is drained. EOF is zero from a nonempty read range, not `WouldBlock`.

`examples/net_wait_bench.plenty` measures one worker serving N idle loopback
connections, one ready at a time, with a selector against a loop that polls
each connection with an expired deadline. Mean round trip in nanoseconds over
20,000 one-byte echoes, pinned to eight cores:

| Connections | Selector | Polling loop |
|---|---|---|
| 1 | 28,668 | 21,958 |
| 16 | 29,143 | 30,192 |
| 128 | 30,048 | 134,935 |
| 1024 | 31,515 | 1,099,546 |

The selector's cost does not grow with idle connections; the polling loop's
grows linearly.

## Cancellation descriptors

A token's first cancellable wait creates its wake descriptor, an `eventfd`
opened close-on-exec, which the token keeps until it is dropped. A program
therefore holds one extra descriptor per token that has waited. If the
descriptor cannot be created (for example at the descriptor limit), the wait
falls back to blocking at most 10 milliseconds at a time and rechecking the
token, as before.

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
- Selectors are epoll instances; there is no edge-triggered or one-shot mode,
  and a wait reports at most 64 events.
- A token does not wake an unbounded `read`, `write` or child `wait`.
- Sockets and name lookup are in [`std.net`](net.md).

## Acknowledgements

- `Selector` is modelled on Python's [`selectors`](https://docs.python.org/3/library/selectors.html) module and the Rust [`mio`](https://crates.io/crates/mio) crate, from which `Interest` and caller-chosen event keys come.
