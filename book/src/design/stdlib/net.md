# `std.net`

`import std.net` provides TCP listeners and streams, UDP sockets, IPv4 and IPv6
addresses, and name lookup. Its deadline operations take a
[`Deadline`](time.md) and a [cancellation token](sync.md) and wait through
[`std.ready`](ready.md). Immediate TCP operations make one nonblocking attempt;
the plain `std.io` methods wait without a deadline. There is no required event
loop or `async` syntax. Use deadline operations to keep a worker's waits bounded.

The runtime supplies one primitive per socket call (opcodes 410–418).
Selectors and cancellable waits come from `std.ready` (opcodes 419–422).
Addresses, waits, retries and error naming are checked Plenty in
`stdlib/std/net.plenty`.

```plenty
import std.bytes
from std.io import StreamError
from std.net import NetError, TcpListener, connect, listen, loopback_v4
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now

def greet(listener: &TcpListener, deadline: &Deadline, cancel: &CancellationToken) -> Result[(), NetError | StreamError | std.bytes.BytesError]:
    mut stream = listener.accept(deadline, cancel)?
    reply = std.bytes.from_text("hello from the server")?
    stream.write_all_before(&reply, 0, reply.len(), deadline, cancel)?
    stream.close()

def main() -> Result[(), ThreadError | IoError | AllocError | RangeError | NetError | StreamError | std.bytes.BytesError]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000))?
    listener = listen(&loopback_v4(0u16), 8)?
    address = listener.local_address()?
    with spawn(greet, &listener, &deadline, &cancel)? as server:
        mut stream = connect(&address, &deadline, &cancel)?
        mut buffer = std.bytes.with_capacity(64)?
        buffer.resize(64, 0u8)?
        count = stream.read_before(&mut buffer, 0, 64, &deadline, &cancel)?
        print(buffer.to_text_range(0, count)?)?
        server.join()?
    Ok(())
```
```output
hello from the server
```

## Addresses

| Item | Meaning |
|---|---|
| `IpAddress.V4(bits: u32)` | An IPv4 address; `V4(0x7f000001)` is 127.0.0.1 |
| `IpAddress.V6(high: u64, low: u64)` | An IPv6 address as two 64-bit halves in network order |
| `SocketAddress(ip, port: u16, scope: u32)` | An address and port; `scope` selects a link-local IPv6 interface and is otherwise 0 |
| `ipv4(a, b, c, d)` | `IpAddress` from four `u8` parts |
| `socket_address(ip, port)` | `SocketAddress` with scope 0 |
| `loopback_v4(port)`, `loopback_v6(port)` | 127.0.0.1 and ::1 |
| `any_v4(port)`, `any_v6(port)` | 0.0.0.0 and ::, which bind every interface of the family |
| `parse_ip(text)` | `Result[IpAddress, NetError]` |
| `parse_address(text)` | `Result[SocketAddress, NetError]` from `1.2.3.4:80`, `[::1]:80` or `[fe80::1%2]:80` |
| `format_ip(ip)` | `Result[str, NetError]` |
| `address.format()` | `Result[str, NetError]`, which `parse_address` reads back |

`IpAddress` is copyable; `SocketAddress` is a class and moves. Parsing is
strict and repairs nothing: IPv4 needs four decimal parts of at most three
digits without leading zeros; IPv6 accepts hex groups, at most one `::`, and a
dotted IPv4 tail. Ports and scopes are decimal. Anything else is
`InvalidAddress`. IPv6 text follows RFC 5952: lowercase, no leading zeros, the
first longest run of two or more zero groups written `::`, and IPv4-mapped
addresses in dotted form (`::ffff:192.0.2.1`).

## Errors

`NetError` names the outcomes a caller handles differently:

| Variant | Cause |
|---|---|
| `TimedOut` | The caller's deadline passed |
| `ConnectionTimedOut` | The kernel gave up on the connection (`ETIMEDOUT`) |
| `WouldBlock` | An immediate TCP operation cannot make progress now (`EAGAIN`/`EWOULDBLOCK`); wait for readiness before trying again |
| `Cancelled` | The token was cancelled |
| `Refused` | Nothing listens at the address (`ECONNREFUSED`) |
| `Reset` | The peer reset or closed the connection while data was moving (`ECONNRESET`, `EPIPE`) |
| `AddressInUse` | Another socket is bound to the address (`EADDRINUSE`) |
| `AddressUnavailable` | The address is not local, or IPv6 is unavailable (`EADDRNOTAVAIL`) |
| `Unreachable` | No route to the network or host |
| `NameNotFound` | The resolver found no address for the name |
| `TryAgain` | The resolver failed temporarily (`EAI_AGAIN`), or 32 lookups are already running |
| `HandleLimit` | The process or system has no free descriptors (`EMFILE`, `ENFILE`) |
| `InvalidAddress` | Address text outside the grammar, or an empty or NUL-containing host name |
| `Stream(StreamError)` | Any other failure, with its native code in `StreamError.Io(IoError.System(code))`, or allocation failure |

`TimedOut` ends the current wait; it does not mark an established connection
as failed, so the caller may retry with another deadline. `ConnectionTimedOut`
is a connection failure: close that stream rather than retrying a wait on it.
Both distinctions survive the network-error wrappers in
[`std.tls`](tls.md#errors) and the [HTTP client](http-client.md#errors).

## TCP

| Function or method | Result |
|---|---|
| `listen(address: &SocketAddress, backlog: i64)` | `Result[TcpListener, NetError]` |
| `listener.accept(deadline, cancel)` | `Result[TcpStream, NetError]` |
| `listener.accept_now()` | `Result[TcpStream, NetError]`: one nonblocking accept attempt |
| `listener.local_address()` | `Result[SocketAddress, NetError]` |
| `listener.close()` | `Result[(), NetError]` |
| `connect(address: &SocketAddress, deadline, cancel)` | `Result[TcpStream, NetError]` |
| `start_connect(address: &SocketAddress)` | `Result[Connecting, NetError]`: starts a connection without waiting |
| `connecting.watch(&mut selector, key)`, `unwatch(&mut selector)` | `Result[(), NetError]`: the attempt is ready when it can be finished |
| `connecting.finish_now()` | `Result[(), NetError]`: `WouldBlock` while the attempt continues, then success or why it failed |
| `connecting.stream()` | `TcpStream`: consumes a finished attempt |
| `descriptor()` on `TcpStream`, `TcpListener`, `UdpSocket`, `Connecting`, `Lookup` | `&File`: the handle a selector waits on, for [`std.ready.Watchable`](ready.md#selectors) |
| `stream.read_now(buffer, start, stop)` | `Result[i64, NetError]`: one nonblocking read attempt into `&mut Bytes` |
| `stream.write_now(source, start, stop)` | `Result[i64, NetError]`: one nonblocking write attempt from any [`ByteSource`](bytes.md#byte-sources) |
| `stream.read_before(buffer, start, stop, deadline, cancel)` | `Result[i64, NetError]`: one read; 0 is end of input |
| `stream.write_before(source, start, stop, deadline, cancel)` | `Result[i64, NetError]`: offer the whole range in one write and return the count accepted; `source` is any [`ByteSource`](bytes.md#byte-sources) |
| `stream.write_all_before(source, start, stop, deadline, cancel)` | `Result[(), NetError]` |
| `stream.read`, `write`, `flush` | The [`std.io`](io.md) `Reader` and `Writer` methods, returning `StreamError` on failure; `read` and `write` block without a bound |
| `stream.close()` | `Result[(), StreamError]`, as required by [`std.io.Closer`](io.md#stream-contracts) |
| `stream.shutdown(direction: Shutdown)` | `Result[(), NetError]`; `Shutdown.Read`, `Write` or `Both` |
| `stream.set_no_delay(enabled: bool)` | `Result[(), NetError]`: send small writes immediately |
| `stream.acknowledge_now()` | `Result[(), NetError]`: acknowledge data already received at once, not after the kernel's delay (Linux `TCP_QUICKACK`); later data is acknowledged as usual |
| `stream.local_address()`, `peer_address()` | `Result[SocketAddress, NetError]` |

- Port 0 binds a free port; `local_address` reports it.
- `backlog` bounds connections waiting for `accept` and is clamped to
  1..4096; the kernel may round it. Listeners set `SO_REUSEADDR`, so a port
  in `TIME_WAIT` can be reused, but two listeners on one port are
  `AddressInUse`. An IPv6 listener accepts only IPv6 connections.
- `accept` borrows the listener shared, so several workers can accept from
  one listener; each connection goes to exactly one of them. A connection
  reset before it was accepted is skipped.
- At the descriptor limit, `accept` returns `HandleLimit` and the connection
  stays queued: once the server closes a descriptor, the next `accept` takes
  it. A server can back off or shed idle connections instead of failing.
- Linux queues up to `backlog` + 1 unaccepted connections (measured on
  loopback for backlogs 1, 2, 8 and 64). Further connection attempts are
  dropped, not refused, so `connect` waits until its deadline and returns
  `TimedOut`. The kernel also caps `backlog` at `net.core.somaxconn`.
- `connect` starts a non-blocking connection and waits for it within the
  deadline. A refused or unreachable peer is an error value.
- Accepted and connected descriptors stay nonblocking. `read_before` and
  `write_before` wait for readiness, attempt the transfer, and retry `EAGAIN`
  or interruption against the original deadline and cancellation token. A
  stale readiness report or a full send buffer cannot turn that transfer
  into an unbounded kernel wait. Plain `read` and `write` retain their
  blocking `std.io` contract by waiting without a deadline when the socket
  would block. A read may return fewer bytes than were sent, and a write may
  accept fewer than offered: loop, or use `write_all_before`.
- `write_before` offers all of `source[start:stop]` without a chunk-size cap.
  The nonblocking write accepts only what currently fits in the kernel's send
  buffer; a successful count is progress, not a guarantee that the whole range
  was sent. The full range must satisfy `0 <= start <= stop <= source.len()`;
  it is never clipped to make an invalid range valid.
- Writing to a connection the peer has closed is `Reset`, never `SIGPIPE`.
- On `TimedOut` or `Cancelled` the socket and the caller's buffer are
  unchanged, apart from what earlier steps of `write_all_before` sent.

### Closing a stream

`TcpStream.close` deliberately returns `StreamError`: its signature satisfies
the [`std.io.Closer`](io.md#stream-contracts) protocol. Together with `Reader` and
`Writer`, this lets a TCP stream work with generic I/O helpers, including
[`close_buffered`](io.md#bounded-buffering), which flushes a `BufWriter[TcpStream]`
and closes its underlying stream. Changing `close` to return `NetError` would
break that protocol contract. Listeners and UDP sockets do not implement
`Closer`; their `close` methods return `NetError`.

A function that uses deadline operations and explicitly closes a TCP stream
can propagate both error types with `NetError | StreamError`, as `greet` does
above. Keeping both types preserves the original close failure. Dropping a
stream also releases its descriptor, but cannot report a close error.

## Waiting on many sockets

`stream.watch(selector, key, interest)`, `listener.watch(selector, key)` and
`socket.watch(selector, key, interest)` register a socket with a
[`std.ready` selector](ready.md#selectors); `unwatch(selector)` removes it. A
listener is readable while a connection waits to be accepted. After
`selector.wait`, each `Event` names a ready socket by its key. Use
`accept_now`, `read_now` or `write_now` to attempt the operation without a
second readiness wait. These methods also work without a prior selector wait.

Each immediate method makes at most one socket operation. It never polls,
waits for readiness, or retries interruption. `WouldBlock` means that the
accept queue is empty, no input is available, or the send buffer is full.
It remains possible after a readiness report; return to the selector so other
connections can make progress. No bytes move on `WouldBlock`. An interrupted
attempt returns `Stream(StreamError.Interrupted)`, and an aborted accept
returns the native `Stream(StreamError.Io(IoError.System(103)))` (`ECONNABORTED`)
instead of silently taking another connection. The caller decides when to retry.

A successful read or write may transfer only a prefix of the requested range.
Keep the write offset and unsent bytes between events; subscribe to write
readiness only while output remains. Zero from a nonempty read range is EOF,
not `WouldBlock`. Empty ranges return zero. Ranges are checked before I/O:
`0 <= start <= stop <= buffer.len()` for reads, or `source.len()` for writes;
invalid ranges return `Stream(StreamError.OutOfRange)` and transfer nothing.

Immediate methods take no deadline or cancellation token. Apply the server's
deadline and cancellation policy to its selector waits and between bounded
batches of attempts. The deadline-based methods remain useful when one worker
can wait for that connection alone.

```text
mut chosen = selector(64).unwrap()
listener.watch(&mut chosen, 0u64)?
# ... accept connections, then stream.watch(&mut chosen, key, Interest.Read)
count = chosen.wait(&deadline, &cancel)?
for index in range(count):
    key = chosen.event(index)?.key
```

The [selector tutorial](../../tutorial/11-concurrency/tcp-sockets.md#wait-on-many-connections)
demonstrates reading the reported connection and handling stale readiness.

## UDP

| Function or method | Result |
|---|---|
| `bind_udp(address: &SocketAddress)` | `Result[UdpSocket, NetError]` |
| `socket.send_to(source, start, stop, target: &SocketAddress, deadline, cancel)` | `Result[i64, NetError]`; `source` is any `ByteSource` |
| `socket.receive_from(buffer, start, stop, deadline, cancel)` | `Result[Datagram, NetError]` |
| `socket.set_broadcast(enabled: bool)` | `Result[(), NetError]` |
| `socket.local_address()`, `close()` | As for TCP |

A `Datagram` has public fields `size` (the datagram's full length), `stored`
(the bytes written into `buffer[start:stop]`) and `source`, and a
`truncated()` method. A datagram longer than the buffer is truncated, never
split; the rest is discarded. A datagram too large to send is
`Stream(StreamError.Io(IoError.System(90)))` (`EMSGSIZE`).

## Name lookup

`resolve(host: &str, port: u16, deadline: &Deadline, cancel: &CancellationToken)`
returns `Result[list[SocketAddress], NetError]`. An IP literal returns that
address without a lookup. Otherwise the system resolver (`getaddrinfo`) is
called once and its addresses are returned in its order, each with `port`. No
address is `NameNotFound`.

The C resolver cannot be interrupted, so the lookup runs on a helper thread
with a 1 MiB stack, which writes its result to a socket pair. `resolve` waits
on the reading end within the deadline and the token, so it returns
`TimedOut` or `Cancelled` on time even while the resolver is still working.
An abandoned lookup finishes on the resolver's own timeouts
(`/etc/resolv.conf`), then discards its result and exits. At most 32 lookups
run at once in a process, counting abandoned ones; another is `TryAgain`.
`start_lookup(host: &str, port: u16)` returns `Result[Lookup, NetError]`: the
same lookup without waiting. `lookup.watch(&mut selector, key)` reports it
ready when the reply arrives, and `lookup.finish_now()` returns `WouldBlock`
until then and the addresses or error afterwards. Dropping an unfinished
`Lookup` abandons it. [`std.actor.net`](actor-net.md#outbound-connections-and-lookup)
uses `start_connect` and `start_lookup` so actors connect and resolve without
blocking their stage.

The resolver's result list is allocated by the C library and freed on the
helper thread. Test builds with allocation checks wait at exit for every
helper thread, abandoned ones included, to free its memory.

## Effects and ownership

Opening a socket and resolving a name have the `Network` effect; operations
on an existing socket have the `Handle` effect. Both are allowed on workers;
see [native effects on worker threads](../facilities/03-concurrency/effects.md).
A listener, stream or socket owns one descriptor, opened close-on-exec, and
closes it when explicitly closed or dropped. Only `resolve` and `start_lookup` start a thread,
as above.

## Example

The [tutorial lesson](../../tutorial/11-concurrency/tcp-sockets.md) serves an
echo connection on a worker and handles a refused connection.

## Limits

- x86_64 Linux GNU only; elsewhere every operation reports
  `Stream(StreamError.Io(IoError.Unsupported))`.
- No socket timeouts beyond deadlines, multicast, or socket options other
  than no-delay and broadcast. TLS is in [`std.tls`](tls.md).
- An abandoned name lookup keeps its helper thread until the resolver gives
  up.

### Unix-domain sockets

`std.net` does not provide Unix-domain sockets, by decision rather than as an
omission. A path socket leaves a file that someone must remove, and a stale
one makes the next `bind` fail, so it needs a cleanup policy that ownership
of the socket alone does not give. The abstract namespace avoids files but is
Linux-only. Their distinctive features, passing descriptors and peer
credentials, would move ownership of native handles between processes, which
the [ownership rules](../facilities/03-concurrency/effects.md) do not yet
describe. Local communication with a child is covered by
[`std.process`](process.md) pipes, and with another local process by TCP on
loopback. Revisit this when a scheduled use case, such as talking to a
system daemon, needs them.
