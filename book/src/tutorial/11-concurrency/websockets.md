# Talk over WebSockets

A WebSocket starts as an HTTP request and then stays open, carrying
messages both ways. [`std.http.websocket`](../../design/stdlib/websocket.md)
does both halves: `accept` turns a [server](http-server.md) exchange into a
WebSocket, and `connect` opens one as a client.

This program serves `/chat` on a loopback port. The handler echoes each
text message in upper case until the client closes. The client, on another
thread, sends two messages and then closes the connection.

```plenty
import std.bytes
import std.http.server
import std.http.websocket
import std.net
import std.time
from std.fmt import FormatError
from std.http.server import Exchange
from std.http.websocket import Kind, Message, WebSocketError
from std.net import TcpStream
from std.sync import CancellationToken
from std.time import Deadline

type ChatError = WebSocketError | std.bytes.BytesError | RangeError | AllocError

def soon() -> Result[Deadline, RangeError]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?)

pub enum Route:
    Chat

pub class Echo:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), ChatError]:
        mut socket = std.http.websocket.accept(exchange, &std.http.websocket.config())?
        mut message = std.bytes.empty()?
        while True:
            message.clear()
            match socket.receive(&mut message, &soon()?)?:
                case Message.Text:
                    shout = message.to_text()?.upper()?
                    socket.send_text(&shout, &soon()?)?
                case Message.Binary:
                    socket.close(1003, "text only", &soon()?)?
                    return Ok(())
                case Message.Close(code):
                    return Ok(())
        Ok(())

def chat(port: u16, stop: CancellationToken) -> Result[(), ChatError | FormatError | IoError]:
    with stop.cancel_on_exit():
        never = CancellationToken()?
        url = f"ws://localhost:{port}/chat"?
        mut socket = std.http.websocket.connect(&url, &std.http.websocket.config(), &soon()?, &never)?
        mut reply = std.bytes.empty()?
        for line in ["hello", "still there?"]?:
            socket.send_text(&line, &soon()?)?
            reply.clear()
            match socket.receive(&mut reply, &soon()?)?:
                case Message.Text:
                    print(reply.to_text()?)?
                case _ as other:
                    print(other)?
        socket.close(1000, "bye", &soon()?)?
    Ok(())

def main() -> Result[(), ThreadError | AllocError | std.http.server.ServerError | std.net.NetError | ChatError | FormatError | IoError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/chat", Route.Chat)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(chat, port, stop.share())? as task:
        drop(std.http.server.serve(listener, &routes, &Echo(), &std.http.server.config(), &stop)?)
        task.join()?
    Ok(())
```
```output
HELLO
STILL THERE?
```

`receive` appends one whole message to a buffer you own and says what kind
it was. A message sent in several frames arrives joined; pings are answered
on the way. `Config` bounds each frame and each message, 64 KiB and 1 MiB by
default, and a peer that sends more is closed with code 1009 and the error
`TooLarge`. Text that is not UTF-8 closes the connection with 1007.

Every call takes a deadline. A handler waiting in `receive` also wakes when
the server begins to shut down, with `Stopping`; it should then `close` with
1001, "going away". Each open WebSocket holds a server worker until its
handler returns, so set `config.workers` for the connections you expect.

`close` sends a close code and reason, waits for the other side's close and
then shuts the connection down. When the other side closes first,
`receive` answers for you and returns `Close` with its code.

For `wss` URLs, `std.http.wss.connect` takes a TLS `ClientConfig` as the
[HTTPS client](https-client.md) does; on the server, `accept` works the same
on exchanges from `std.http.tls.serve`.
