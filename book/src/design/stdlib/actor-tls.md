# `std.actor.tls`

TLS for [`std.actor.net`](actor-net.md) connections. A `TlsConnection` runs a
[`std.tls`](tls.md) session over a `Connection`: it moves ciphertext between
the session and the connection's buffers, decrypts into a bounded plaintext
buffer, and reports the same `Events`. Nothing waits, so one stage serves
many TLS connections. Certificates, verification and configuration are
those of `std.tls`; cryptography is rustls.

Supported target: x86-64 Linux, as for [`std.actor`](actor.md).

A server echoes one TLS connection, and a client actor dials it, sends a
request once the handshake completes and prints the reply:

```plenty
from std.actor import Address, Context, Mail, TimerError, heaps_live, stage
from std.actor.net import Admission, Dialing, Limits, Listener, connection, dial, listener
from std.actor.tls import TlsConnection, client, server
from std.bytes import BytesError
from std.net import NetError, SocketAddress, listen, loopback_v4
from std.ready import WaitError
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds
from std.tls import ClientConfig, ServerConfig, TlsError, client_config, roots_from_pem, server_config

type Fault = NetError | BytesError | TimerError | TlsError | IoError

# Test-only CA and a localhost leaf it signed; never reuse a published key.
const CA_PEM: str = "-----BEGIN CERTIFICATE-----\nMIIBmjCCAT+gAwIBAgIULOQ2Qxr8CVZY2bG0uRYKKEdMD3swCgYIKoZIzj0EAwIw\nGTEXMBUGA1UEAwwOUGxlbnR5IFRlc3QgQ0EwIBcNMjQwMTAxMDAwMDAwWhgPMjEy\nNDAxMDEwMDAwMDBaMBkxFzAVBgNVBAMMDlBsZW50eSBUZXN0IENBMFkwEwYHKoZI\nzj0CAQYIKoZIzj0DAQcDQgAEkiB3Ogehrrt6YbQghwDPv6tkhE+z9zTCVih8fvtR\nZ1/691Nfh7/J9IgaNgNSTMJjGTolzGnjaCUDqoARlFyM+6NjMGEwHQYDVR0OBBYE\nFOTFBDgWXHj/qmVJDMlAHYCEZFX5MB8GA1UdIwQYMBaAFOTFBDgWXHj/qmVJDMlA\nHYCEZFX5MA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgIEMAoGCCqGSM49\nBAMCA0kAMEYCIQD41ATQnIjKkKMLQAb9AyoyOFmljYZyz348XJEIbqMgKwIhAL3m\nmWd8On5gvkfytspatH5mDWOcSyW+au7vuIC2gHhO\n-----END CERTIFICATE-----\n"
const LEAF_PEM: str = "-----BEGIN CERTIFICATE-----\nMIIBnDCCAUKgAwIBAgIBATAKBggqhkjOPQQDAjAZMRcwFQYDVQQDDA5QbGVudHkg\nVGVzdCBDQTAgFw0yNDAxMDEwMDAwMDBaGA8yMTI0MDEwMTAwMDAwMFowFDESMBAG\nA1UEAwwJbG9jYWxob3N0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE8THNQeTi\nJ6Eq0tSxtHW5Q4TCsbIqB3MOtM98ERyKgl6kHeHCeDZYKx8B+VRGT2jkWuTeN24Q\nljwUfh7uh7Iaz6N+MHwwGgYDVR0RBBMwEYIJbG9jYWxob3N0hwR/AAABMAkGA1Ud\nEwQCMAAwEwYDVR0lBAwwCgYIKwYBBQUHAwEwHQYDVR0OBBYEFEIFC2Fyd4cfAKTx\nMiNCMdDkqDNbMB8GA1UdIwQYMBaAFOTFBDgWXHj/qmVJDMlAHYCEZFX5MAoGCCqG\nSM49BAMCA0gAMEUCIAUF3HH43lnirgSN1Vr4twZrFmXKBxlbYNQni/h4ZaxhAiEA\n5rD/yE1B4IsbEGCMlyAeX507leXfGY9+wrHaxikU/w0=\n-----END CERTIFICATE-----\n"
const LEAF_KEY: str = "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQglvSEE3H7v+mInxSF\nAyWFDKYpzpXr20wH8mfFY3bCaB2hRANCAATxMc1B5OInoSrS1LG0dblDhMKxsioH\ncw60z3wRHIqCXqQd4cJ4NlgrHwH5VEZPaORa5N43bhCWPBR+Hu6HshrP\n-----END PRIVATE KEY-----\n"

pub enum Step:
    Waiting(SocketAddress)
    Dialing(Dialing)
    Talking(TlsConnection)

# Connects, sends one request once the handshake completes, and prints the
# reply.
pub class Caller:
    pub config: ClientConfig
    pub step: Step
    pub sent: bool

    pub def started(self: &mut Caller, context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match &self.step:
            case Step.Waiting(address):
                self.step = Step.Dialing(dial(address, &seconds(5), context)?)
            case _:
                pass
        Ok(())

    pub def receive(self: &mut Caller, mail: &Mail[i64, Fault], context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match &mut self.step:
            case Step.Dialing(attempt):
                match attempt.handle(mail, context)?:
                    case Some(stream):
                        net = connection(stream, &Limits(4096, 4096, seconds(5), seconds(5)))?
                        # The session is allocated in this actor's heap.
                        mut call = client(net, &self.config, "localhost", 1024)?
                        call.start(context)?
                        self.step = Step.Talking(call)
                    case Nothing:
                        pass
            case Step.Talking(call):
                events = call.handle(mail, context)?
                # `writable` first reports the end of the handshake.
                if events.writable and not self.sent:
                    self.sent = True
                    # The empty output takes all 14 bytes.
                    call.write(b"hello over TLS", 0, 14, context)?
                    call.shutdown_write(context)?
                if events.ended:
                    print("reply", call.input().to_text_range(call.input_start(), call.input_stop())?)?
                    call.close(context)?
                    context.stop()
            case Step.Waiting(_):
                pass
        Ok(())

pub class Server:
    pub listener: Listener
    pub config: ServerConfig

pub enum Node:
    Serving(Server)
    Echoing(TlsConnection)
    Calling(Caller)

    pub def started(self: &mut Node, context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match self:
            case Node.Serving(serving):
                serving.listener.start(context)?
            case Node.Echoing(echo):
                echo.start(context)?
            case Node.Calling(caller):
                caller.started(context)?
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, Fault], context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match self:
            case Node.Serving(serving):
                serving.listener.handle(&mail, context)?
                match mail:
                    # This server handles one connection.
                    case Mail.Down(_, _):
                        context.stop()
                    case _:
                        pass
            case Node.Echoing(echo):
                echo.handle(&mail, context)?
                # Forwarding frees input room, so more may be decrypted at once.
                while echo.available() > 0:
                    if echo.forward(echo.available(), context)? == 0:
                        break
                if not echo.reading() and echo.available() == 0:
                    echo.shutdown_write(context)?
                    if echo.pending() == 0:
                        context.stop()
            case Node.Calling(caller):
                caller.receive(&mail, context)?
        Ok(())

    # Builds each connection's actor, with its buffers and TLS session in
    # the child's heap.
    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, Fault]) -> Result[Option[Node], Fault]:
        match self:
            case Node.Serving(serving):
                match serving.listener.accept()?:
                    case Some(net):
                        Ok(Some(Node.Echoing(server(net, &serving.config, 1024)?)))
                    case Nothing:
                        Ok(Nothing)
            case _:
                Ok(Nothing)

def main() -> Result[(), AllocError | IoError | WaitError | NetError | TlsError]:
    socket = listen(&loopback_v4(0u16), 16)?
    address = socket.local_address()?
    limits = Limits(4096, 4096, seconds(30), seconds(30))
    serving = Server(listener(socket, limits, 100, 65536, 0, Admission.Refuse)?, server_config(&LEAF_PEM, &LEAF_KEY)?)
    caller = Caller(client_config(&roots_from_pem(&CA_PEM)?)?, Step.Waiting(address), False)
    mut actors = stage[Node, i64, Fault](8, 4, 8)?
    actors.add_timers(8)?
    actors.start(Node.Serving(serving), 65536).unwrap()
    actors.start(Node.Calling(caller), 65536).unwrap()
    actors.run(&deadline_after(&monotonic_now(), &seconds(10)).unwrap(), &CancellationToken()?)?
    stats = actors.stats()
    print(stats.started, stats.stopped, heaps_live())?
    Ok(())
```
```output
reply hello over TLS
3 3 0
```

## Connections

Make a `TlsConnection` from an admitted or dialled `Connection` with
`server(net, &config, input)` or `client(net, &config, server_name, input)`,
where `input` is the size of the decrypted input buffer. Call them where the
connection's actor heap is current, such as the spawn hook or a handler, so
the session lives in that heap. `over(net, session, input)` takes a session
made with `std.tls.client_session` or `server_session`.

Call `start` from the actor's `started`, or once a dialled connection is
made; a client then queues its first handshake flight. Pass every mail to
`handle`. Its `Events` count plaintext:

| Field | Meaning |
|---|---|
| `mine` | The mail was about this connection |
| `received` | Decrypted bytes this mail added to the input |
| `ended` | The peer sent `close_notify` |
| `writable` | The handshake completed, or space freed after `write` accepted less than it was offered |
| `drained` | All ciphertext has been sent |
| `expired` | A deadline passed, as for [`Connection`](actor-net.md#deadlines) |

A TLS failure, such as an untrusted certificate or a peer's alert, is
returned by `handle` as a `TlsError`, after the alert rustls queued is sent
if the socket takes it at once. A peer that ends its stream without
`close_notify` is `Truncated`, as in `std.tls`. Returning the error ends the
actor with `Exit.Failed`.

### Reading

Decrypted bytes are `input()[input_start():input_stop()]`, read in place.
`consume(count)` releases them. Releasing input room decrypts what waited,
so `available()` may grow again without new mail: handle input in a loop
until `available()` is 0, as the example's echo does. When the plaintext
buffer is full the connection stops decrypting, its ciphertext buffer fills,
and the connection stops reading the socket, so back-pressure reaches the
peer as for `std.actor.net`.

`reading()` is false once `close_notify` or the end of the stream has
arrived.

### Writing and back-pressure

`write(source, start, stop)` encrypts as much of `source[start:stop]` as
`space()` allows into the connection's output, sends what the socket takes
at once, and returns the plaintext count accepted. `space()` is 0 during the
handshake, after `shutdown_write`, and while the session holds ciphertext
that did not fit the output; otherwise it is the free output less 32 bytes
of record overhead per 16 KiB. A short count is back-pressure: offer the
rest when `Events.writable` is set. `forward(count)` encrypts decrypted
input back out, for echoing and relaying. `pending()` is ciphertext not yet
sent.

### Closing

`shutdown_write` queues `close_notify`, then ends the TCP write side once it
is sent; reading continues until the peer's `close_notify`. `close` closes
the socket at once without `close_notify`. Stop the actor once
`reading()` is false, input is handled and `pending()` is 0.

## Deadlines

`Limits` deadlines and `read_within` work as for
[`std.actor.net`](actor-net.md#deadlines) and count ciphertext progress. Set
`read_within` in `started` to bound the handshake and the first request, and
`clear_read_deadline` once `established()` is true.

## Memory

| Item | Where | Size |
|---|---|---|
| `ClientConfig`, `ServerConfig`, trust roots | Outside every actor heap | Parsed certificates and key, shared by all sessions |
| Session | The connection actor's heap | rustls state; its record buffers grow with record size |
| Ciphertext buffers | The connection actor's heap | `Limits.input` and `Limits.output` |
| Plaintext input | The connection actor's heap | The `input` argument |

Measured with `Context.heap_used()` on an echo server: with 1 KiB buffers,
an established connection uses 12,944 bytes. After echoing 64 KiB it holds
17,168 bytes when the peer sent 1000-byte records, and 41,744 when it sent
full 16 KiB records, which rustls buffers whole. With 4 KiB buffers the
figures are 22,160 and 26,512 for 1000-byte records, and 50,960 for full
records. A peer controls its record size, so size the heap passed to
`listener` for full records: while a full record arrives rustls grows its
record buffer to a 32 KiB allocation, so 64 KiB per connection is not
enough. 80 KiB served full records with 1 KiB buffers, and 96 KiB with
4 KiB buffers.

rustls allocates through the Rust allocator, which in a handler is the
current actor's heap. A session that exceeds its heap traps that actor
only, and its heap is discarded with the session; other connections
continue. Outside actors the same failure stops the program, as described
for [`std.tls`](tls.md#costs).

Configurations and trust roots are allocated outside every actor heap, even
when built in a handler, because sessions in other heaps share them. A
discarded session does not release its share, so each trapped connection
keeps its configuration alive until the program exits; this is the
configuration's memory once, not per trap. An actor node may hold a
configuration and the spawn hook may borrow it, as in the example; other
`std.tls` values cannot cross the actor boundary.

## Exports

| Export | Contract |
|---|---|
| `server(net, &config, input)` | `Result[TlsConnection, TlsError]`: a server session in the current heap |
| `client(net, &config, server_name, input)` | `Result[TlsConnection, TlsError]`: verifies the server against `config` and `server_name` |
| `over(net, session, input)` | `Result[TlsConnection, TlsError]`; `Net(InvalidInput)` for `input` below 1 |
| `TlsConnection.start(context)` | `Result[(), TlsError \| TimerError]` |
| `TlsConnection.handle(&mail, context)` | `Result[Events, TlsError \| TimerError]` |
| `TlsConnection.input()`, `input_start()`, `input_stop()`, `available()` | The plaintext buffer and the unconsumed range |
| `TlsConnection.consume(count, context)` | `Result[(), TlsError \| TimerError]`; may decrypt more |
| `TlsConnection.write(source, start, stop, context)` | `Result[i64, TlsError \| TimerError]`: plaintext accepted; `Closed` after `shutdown_write` or `close` |
| `TlsConnection.forward(count, context)` | `Result[i64, TlsError \| TimerError]`: plaintext moved from input to output |
| `TlsConnection.pending()`, `space()` | Unsent ciphertext; plaintext `write` would accept now |
| `TlsConnection.read_within(&span, context)`, `clear_read_deadline()` | Set or clear the read deadline |
| `TlsConnection.shutdown_write(context)`, `close(context)` | Send `close_notify` then end output; close at once |
| `TlsConnection.established()`, `reading()`, `closed()` | Handshake complete; input may still arrive; socket closed |
| `TlsConnection.session()`, `connection()` | The [`Session`](tls.md#sessions), for the negotiated protocol, version and peer certificate; the `Connection` |

## Limits

- `consume` and `forward` may decrypt more input without new mail; loop
  until `available()` is 0.
- Compacting the plaintext buffer copies one byte at a time, since `Bytes`
  has no overlapping copy.
- A trapped connection leaks one reference to its configuration; see
  [Memory](#memory).
- No session resumption, as in `std.tls`.
- `std.http` has no actor HTTPS server yet.
