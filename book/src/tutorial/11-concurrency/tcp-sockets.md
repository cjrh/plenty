# Serve a TCP connection

[`std.net`](../../design/stdlib/net.md) opens TCP and UDP sockets. Every call
that waits for the network takes a deadline and a cancellation token, the same
as the [bounded I/O waits](bounded-io.md), so a worker serving a connection
cannot hang forever.

```plenty
import std.bytes
from std.bytes import Bytes
from std.net import NetError, Shutdown, TcpListener, connect, listen, loopback_v4
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now

# Echoes one connection until the client stops sending.
def echo(listener: &TcpListener, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError | std.bytes.BytesError]:
    mut stream = listener.accept(deadline, cancel)?
    mut buffer = std.bytes.with_capacity(64)?
    buffer.resize(64, 0u8)?
    mut total = 0
    while True:
        count = stream.read_before(&mut buffer, 0, 64, deadline, cancel)?
        if count == 0:
            return Ok(total)
        stream.write_all_before(&buffer, 0, count, deadline, cancel)?
        total = total + count
    Ok(total)

type SocketOperationError = RangeError | NetError | std.bytes.BytesError

def main() -> Result[(), ThreadError | IoError | AllocError | SocketOperationError]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000))?
    mut listener = listen(&loopback_v4(0u16), 8)?
    address = listener.local_address()?
    with spawn(echo, &listener, &deadline, &cancel)? as server:
        mut stream = connect(&address, &deadline, &cancel)?
        message = std.bytes.from_text("hello")?
        stream.write_all_before(&message, 0, message.len(), &deadline, &cancel)?
        stream.shutdown(Shutdown.Write)?
        mut reply = std.bytes.with_capacity(16)?
        reply.resize(16, 0u8)?
        count = stream.read_before(&mut reply, 0, 16, &deadline, &cancel)?
        print(reply.to_text_range(0, count)?)?
        print(server.join()?)?
    listener.close()?
    match connect(&address, &deadline, &cancel):
        case Err(NetError.Refused):
            print("refused")?
        case _:
            print("unexpected")?
    Ok(())
```

```output
hello
5
refused
```

Port 0 asks the system for a free port, and `local_address` reports which one
it chose. `shutdown(Shutdown.Write)` tells the server that no more data is
coming, so its read returns 0 and it returns how many bytes it echoed. After
the listener is closed, nothing listens on the port, so the last attempt is
`NetError.Refused`: an error value to handle, not a crash.

`read_before` makes one read, which may return fewer bytes than were sent; a
real client loops until it has what it needs.

## Wait on many connections

A worker per connection is simple, but a server with many idle connections
can wait on all of them in one call with a
[selector](../../design/stdlib/ready.md#selectors). Each socket is registered
under a key you choose, and `wait` reports which keys are ready:

```plenty
import std.ready

import std.net

import std.bytes
from std.net import connect, listen, loopback_v4
from std.ready import Interest, selector
from std.sync import CancellationToken
from std.time import deadline_after, milliseconds, monotonic_now

type SocketOperationError = std.bytes.BytesError | std.ready.WaitError | std.net.NetError | RangeError

def main() -> Result[(), AllocError | IoError | SocketOperationError]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000))?
    listener = listen(&loopback_v4(0u16), 8)?
    address = listener.local_address()?
    quiet = connect(&address, &deadline, &cancel)?
    mut chatty = connect(&address, &deadline, &cancel)?
    first = listener.accept(&deadline, &cancel)?
    mut second = listener.accept(&deadline, &cancel)?
    mut chosen = selector(8)?
    first.watch(&mut chosen, 1u64, Interest.Read)?
    second.watch(&mut chosen, 2u64, Interest.Read)?
    chatty.write_all_before(&b"x", 0, 1, &deadline, &cancel)?
    mut buffer = std.bytes.with_capacity(16)?
    buffer.resize(16, 0u8)?
    while True:
        count = chosen.wait(&deadline, &cancel)?
        match second.read_now(&mut buffer, 0, 16):
            case Ok(received):
                print(count, chosen.event(0)?.key)?
                print(buffer.to_text_range(0, received)?)?
                break
            case Err(std.net.NetError.WouldBlock):
                continue
            case Err(error):
                return Err(error)
    Ok(())
```

```output
1 2
x
```

Only the second connection has input, so the wait reports one event, key 2.
`read_now` makes a single nonblocking read without another readiness wait.
If readiness has become stale, `WouldBlock` sends the loop back to the selector.
This example has only one possible sender and one byte of input. A server
dispatches each event by its key, handles zero from a nonempty read as EOF,
and keeps any unsent suffix after a partial `write_now`. `accept_now` similarly
accepts one queued connection or returns `WouldBlock` without waiting.
Name lookup, UDP datagrams and IPv6 addresses are in the
[reference](../../design/stdlib/net.md).
