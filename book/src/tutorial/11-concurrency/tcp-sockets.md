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
def echo(listener: &TcpListener, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
    mut stream = listener.accept(deadline, cancel)?
    mut buffer = std.bytes.with_capacity(64).unwrap()
    buffer.resize(64, 0u8).unwrap()
    mut total = 0
    while True:
        count = stream.read_before(&mut buffer, 0, 64, deadline, cancel)?
        if count == 0:
            return Ok(total)
        stream.write_all_before(&buffer, 0, count, deadline, cancel)?
        total = total + count
    Ok(total)

def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000))?
    mut listener = listen(&loopback_v4(0u16), 8).unwrap()
    address = listener.local_address().unwrap()
    with spawn(echo, &listener, &deadline, &cancel)? as server:
        mut stream = connect(&address, &deadline, &cancel).unwrap()
        message = std.bytes.from_text("hello")?
        stream.write_all_before(&message, 0, message.len(), &deadline, &cancel).unwrap()
        stream.shutdown(Shutdown.Write).unwrap()
        mut reply = std.bytes.with_capacity(16)?
        reply.resize(16, 0u8)?
        count = stream.read_before(&mut reply, 0, 16, &deadline, &cancel).unwrap()
        print(reply.slice(0, count)?.to_text()?)?
        print(server.join().unwrap())?
    listener.close().unwrap()
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
real client loops until it has what it needs. Name lookup, UDP datagrams and
IPv6 addresses are in the [reference](../../design/stdlib/net.md).
