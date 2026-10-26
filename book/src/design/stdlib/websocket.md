# `std.http.websocket`

`import std.http.websocket` speaks the WebSocket protocol of
[RFC 6455](https://www.rfc-editor.org/rfc/rfc6455): the opening handshake,
as a [`std.http.server`](http-server.md) exchange that switches protocols
or as a client, then framed text and binary messages, ping and pong, and
the closing handshake. [`std.http.wss`](#wss) adds `wss` clients through
[`std.tls`](tls.md). Both modules are checked Plenty in `stdlib/std/http/`
and add no runtime primitives; the handshake's SHA-1 comes from
[`std.crypto`](crypto.md). The tutorial lesson
[Talk over WebSockets](../../tutorial/11-concurrency/websockets.md) shows a
complete program.

## Connections

`WebSocket[T]` is one open connection over a stream `T` that reads and
writes like a server [`Transport`](http-server.md#https): `TcpStream`, or
`TlsTransport` for TLS.

| Item | Meaning |
|---|---|
| `socket.receive(&mut out, &deadline)` | `Result[Message, WebSocketError]`: append the next whole message to `out` |
| `socket.send(kind, &data, start, stop, &deadline)` | `Result[(), WebSocketError]`: send `data[start:stop]` as one `Kind.Text` or `Kind.Binary` message |
| `socket.send_text(&text, &deadline)` | `send` of `text`, copied into a buffer first |
| `socket.ping(&data, start, stop, &deadline)` | Send a ping with at most 125 bytes of payload |
| `socket.close(code, &reason, &deadline)` | Run the closing handshake, then shut the stream down |
| `socket.is_open()` | Whether messages can still be sent |
| `socket.subprotocol` | `Option[str]`: the subprotocol both sides agreed on |

`Message` is `Text`, `Binary`, or `Close(code)`. Text has been checked as
UTF-8. A message sent in several frames is joined before `receive` returns
it. Pings are answered with a pong and pongs are skipped while receiving.
When the peer closes, `receive` answers with the same code and returns
`Close` with the peer's code, or 1005 when it gave none, and its reason
appended to `out`; the connection is then closed.

`send` splits a message longer than `max_frame` into several frames. A
`Text` message must be UTF-8 (`InvalidText`). Client frames are masked with
a fresh key from [`std.secrets`](secrets.md) for each frame, as RFC 6455
requires; server frames are not.

`close` accepts the codes a peer may send: 1000 to 1003, 1007 to 1014, and
3000 to 4999, with a reason of at most 123 bytes (`OutOfRange` otherwise).
It sends the close, skips any messages still arriving until the peer's
close, and shuts the stream down. Closing a connection whose closing
handshake already finished returns `Ok`.

On an error, `out` keeps only what it held before the call. Every error
except `Stopping` and `OutOfRange` closes the connection; later calls fail
with `Closed`. A protocol fault first sends a close: 1002 for `Protocol`,
1009 for `TooLarge` and 1007 for `InvalidText`.

Each call has its own deadline. A connection holds one 4 KiB input buffer,
grown only to hold input that arrived with the handshake, and one 4 KiB
output buffer; a sent message is copied through the output buffer in
pieces, so `send` allocates nothing for any message size.

## Configuration

`config()` returns the defaults.

| Field | Default | Meaning |
|---|---|---|
| `max_frame` | 65536 | Largest frame payload received, and the size `send` splits messages into |
| `max_message` | 1048576 | Largest message `receive` joins |
| `subprotocol` | `Nothing` | The subprotocol a client offers, or a server accepts when the client offers it |

Both maxima must be at least 1 (`InvalidConfig`).

## Servers

`accept(exchange: &mut Exchange[T], config: &Config)` returns
`Result[WebSocket[T], WebSocketError]`. It checks that the request is a
handshake: `GET` over HTTP/1.1 with no body, `Upgrade: websocket`,
`Connection: upgrade`, `Sec-WebSocket-Version: 13` and one
`Sec-WebSocket-Key` of 16 bytes in base64. Field names and the `Upgrade`
and `Connection` tokens are compared without case. A request that fails
these checks is answered and `accept` returns `Refused(status)`:

| Request | Answer |
|---|---|
| Missing `websocket` in `Upgrade`, or another version | `426 Upgrade Required` with `Upgrade: websocket` and `Sec-WebSocket-Version: 13` |
| Any other fault | `400 Bad Request` |

Otherwise it answers `101 Switching Protocols` with the accept value, and
the configured subprotocol when the client listed it, and takes the
connection with
[`switch_protocols`](http-server.md#switching-protocols). The server no
longer counts or closes the connection; dropping the `WebSocket` closes it.

The WebSocket keeps the server's shutdown tokens. A `receive` waiting for
the next message ends with `Stopping` when shutdown begins, and the
connection stays open, so the handler can `close` with 1001. A wait in the
middle of a message, or in `send` or `close`, ends with `Net(Cancelled)`
when the drain deadline passes.

The handler keeps its worker for as long as the WebSocket is open, so each
open connection counts against `workers`. Route a WebSocket to a handler
that returns when the connection closes.

## Clients

| Item | Meaning |
|---|---|
| `connect(url: &str, &config, &deadline, &cancel)` | `Result[WebSocket[TcpStream], WebSocketError]`: dial and handshake a `ws` URL |
| `target(url: &str)` | `Result[Target, WebSocketError]`: parse a `ws` or `wss` URL |
| `dial(&target, &deadline, &cancel)` | `Result[TcpStream, WebSocketError]`: a TCP connection to the target |
| `handshake(stream, &target, &config, &deadline, &cancel)` | `Result[WebSocket[T], WebSocketError]`: the client handshake over a connected stream |
| `Target.secure`, `host`, `port` | `wss` or not; the host, lowercase without IPv6 brackets; the port, 80 or 443 by default |

`target` rejects URLs with another scheme (`UnsupportedScheme`), with
credentials or a fragment, without a host, or with port 0
(`InvalidUrl`). The request target is the URL's path, `/` when empty, and
its query.

The handshake sends a random 16-byte key and checks the server's answer: a
status other than 101 is `Rejected(status)`, and a 101 without the
`Upgrade` and `Connection` tokens, with the wrong accept value, with
extensions, or with a subprotocol the client did not offer is
`InvalidHandshake`. The deadline bounds the handshake; later calls take
their own. `cancel` ends every later wait with `Net(Cancelled)`.

`connect` returns `UnsupportedScheme` for `wss` URLs: use
[`std.http.wss`](#wss).

## WSS

`import std.http.wss` runs the client handshake over TLS.

| Item | Meaning |
|---|---|
| `connect(url: &str, &tls: ClientConfig, &config, &deadline, &cancel)` | `Result[WebSocket[TlsTransport], WebSocketError]` for a `wss` URL |

The server's certificate is verified against `tls`'s roots with the URL's
host as the name, as for [HTTPS](http-client.md#https). A TLS failure is
`Connect(ClientError.Tls(...))`. A `ws` URL is `UnsupportedScheme`. The
module is separate so that a program using only `ws` does not link TLS. On
the server side, `accept` takes exchanges from
[`std.http.tls.serve`](http-server.md#https) unchanged.

## Errors

`WebSocketError` has these cases:

| Case | Meaning |
|---|---|
| `Net(NetError)` | The stream failed, timed out, was cancelled, or ended early |
| `Allocation(AllocError)` | A buffer could not grow |
| `Http(HttpError)` | The handshake's HTTP head could not be read or written |
| `Connect(ClientError)` | The client could not connect, including TLS failures |
| `Entropy(StreamError)` | No random bytes for a key or mask |
| `InvalidConfig`, `InvalidUrl`, `UnsupportedScheme` | See above |
| `OutOfRange` | A byte range, ping payload, close code or reason out of bounds |
| `Refused(status)` | The server refused a request that was not a handshake |
| `Rejected(status)` | The server answered the client with another status |
| `InvalidHandshake` | A 101 answer that does not complete the handshake |
| `Protocol` | The peer broke the framing rules |
| `TooLarge` | A frame or message over the configured limits |
| `InvalidText` | Text that is not UTF-8 |
| `Closed` | The connection has closed |
| `Stopping` | Server shutdown began while waiting for a message |

## Limits

- No extensions: `permessage-deflate` is neither offered nor accepted.
- No proxy for clients; `connect` dials the URL's host directly.
- One caller at a time: `receive` and the sending methods all borrow the
  connection exclusively, so one thread cannot wait for a message while
  another sends.
- A server WebSocket holds a worker until its handler returns.
- `receive` places a control frame's payload in `out` while handling it,
  so `out` briefly grows by up to 125 bytes.
- A close answer echoes the peer's code without its reason.
- `send_text` copies its text; `send` sends bytes the caller already holds.
