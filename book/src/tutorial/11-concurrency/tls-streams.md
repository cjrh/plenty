# Encrypt a TCP connection

`std.tls` wraps a connected [TCP stream](tcp-sockets.md) in TLS. The client
checks that the server's certificate chains to a trust root it chose and that
the certificate names the host it meant to reach. Every handshake, read, and
write takes the same deadline and cancellation token as the TCP calls.

A server needs a certificate chain and its private key, as PEM text.
`roots_from_pem` turns CA certificates into `TrustRoots`; real clients
usually call `std.tls.system_roots()` instead, which reads the operating
system's bundle. This lesson embeds a test CA so that it runs offline.

```plenty
import std.bytes
import std.tls
from std.net import NetError, TcpListener, connect, listen, loopback_v4
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now
from std.tls import ServerConfig, TlsError

# Test-only certificates: a CA, and a leaf it signed for localhost and
# 127.0.0.1. Never use a published key outside an example.
def ca_pem() -> str:
    return "-----BEGIN CERTIFICATE-----\nMIIBmjCCAT+gAwIBAgIULOQ2Qxr8CVZY2bG0uRYKKEdMD3swCgYIKoZIzj0EAwIw\nGTEXMBUGA1UEAwwOUGxlbnR5IFRlc3QgQ0EwIBcNMjQwMTAxMDAwMDAwWhgPMjEy\nNDAxMDEwMDAwMDBaMBkxFzAVBgNVBAMMDlBsZW50eSBUZXN0IENBMFkwEwYHKoZI\nzj0CAQYIKoZIzj0DAQcDQgAEkiB3Ogehrrt6YbQghwDPv6tkhE+z9zTCVih8fvtR\nZ1/691Nfh7/J9IgaNgNSTMJjGTolzGnjaCUDqoARlFyM+6NjMGEwHQYDVR0OBBYE\nFOTFBDgWXHj/qmVJDMlAHYCEZFX5MB8GA1UdIwQYMBaAFOTFBDgWXHj/qmVJDMlA\nHYCEZFX5MA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgIEMAoGCCqGSM49\nBAMCA0kAMEYCIQD41ATQnIjKkKMLQAb9AyoyOFmljYZyz348XJEIbqMgKwIhAL3m\nmWd8On5gvkfytspatH5mDWOcSyW+au7vuIC2gHhO\n-----END CERTIFICATE-----\n"

def leaf_pem() -> str:
    return "-----BEGIN CERTIFICATE-----\nMIIBnDCCAUKgAwIBAgIBATAKBggqhkjOPQQDAjAZMRcwFQYDVQQDDA5QbGVudHkg\nVGVzdCBDQTAgFw0yNDAxMDEwMDAwMDBaGA8yMTI0MDEwMTAwMDAwMFowFDESMBAG\nA1UEAwwJbG9jYWxob3N0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE8THNQeTi\nJ6Eq0tSxtHW5Q4TCsbIqB3MOtM98ERyKgl6kHeHCeDZYKx8B+VRGT2jkWuTeN24Q\nljwUfh7uh7Iaz6N+MHwwGgYDVR0RBBMwEYIJbG9jYWxob3N0hwR/AAABMAkGA1Ud\nEwQCMAAwEwYDVR0lBAwwCgYIKwYBBQUHAwEwHQYDVR0OBBYEFEIFC2Fyd4cfAKTx\nMiNCMdDkqDNbMB8GA1UdIwQYMBaAFOTFBDgWXHj/qmVJDMlAHYCEZFX5MAoGCCqG\nSM49BAMCA0gAMEUCIAUF3HH43lnirgSN1Vr4twZrFmXKBxlbYNQni/h4ZaxhAiEA\n5rD/yE1B4IsbEGCMlyAeX507leXfGY9+wrHaxikU/w0=\n-----END CERTIFICATE-----\n"

def leaf_key() -> str:
    return "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQglvSEE3H7v+mInxSF\nAyWFDKYpzpXr20wH8mfFY3bCaB2hRANCAATxMc1B5OInoSrS1LG0dblDhMKxsioH\ncw60z3wRHIqCXqQd4cJ4NlgrHwH5VEZPaORa5N43bhCWPBR+Hu6HshrP\n-----END PRIVATE KEY-----\n"

# Answers one message on one connection.
def echo(listener: &TcpListener, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, TlsError | std.bytes.BytesError]:
    match listener.accept(deadline, cancel):
        case Ok(tcp):
            mut stream = std.tls.accept(tcp, config, deadline, cancel)?
            mut buffer = std.bytes.with_capacity(64)?
            buffer.resize(64, 0u8)?
            count = stream.read_before(&mut buffer, 0, 64, deadline, cancel)?
            stream.write_all_before(&buffer, 0, count, deadline, cancel)?
            stream.close_before(deadline, cancel)?
            Ok(count)
        case Err(error):
            Err(TlsError.Net(error))

type TlsConnectionError = RangeError | TlsError | NetError | std.bytes.BytesError

def main() -> Result[(), ThreadError | IoError | AllocError | TlsConnectionError]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000))?
    roots = std.tls.roots_from_pem(&ca_pem())?
    client = std.tls.client_config(&roots)?
    server = std.tls.server_config(&leaf_pem(), &leaf_key())?
    listener = listen(&loopback_v4(0u16), 8)?
    address = listener.local_address()?
    with spawn(echo, &listener, &server, &deadline, &cancel)? as worker:
        tcp = connect(&address, &deadline, &cancel)?
        mut stream = std.tls.connect(tcp, &client, "localhost", &deadline, &cancel)?
        message = std.bytes.from_text("hello")?
        stream.write_all_before(&message, 0, message.len(), &deadline, &cancel)?
        mut reply = std.bytes.with_capacity(16)?
        reply.resize(16, 0u8)?
        count = stream.read_before(&mut reply, 0, 16, &deadline, &cancel)?
        print(reply.slice(0, count)?.to_text()?)?
        print(stream.read_before(&mut reply, 0, 16, &deadline, &cancel)?)?
        stream.close_before(&deadline, &cancel)?
        print(worker.join()?)?
    with spawn(echo, &listener, &server, &deadline, &cancel)? as worker:
        tcp = connect(&address, &deadline, &cancel)?
        match std.tls.connect(tcp, &client, "example.test", &deadline, &cancel):
            case Err(error):
                print(error)?
            case Ok(_):
                print("unexpected")?
        match worker.join():
            case Err(error):
                match error:
                    case tls: TlsError:
                        print(tls)?
                    case buffer: std.bytes.BytesError:
                        print(buffer)?
            case Ok(_):
                print("unexpected")?
    Ok(())
```

```output
hello
0
5
std.tls.TlsError.WrongHost
std.tls.TlsError.Rejected
```

`std.tls.connect` takes ownership of the TCP stream and returns only after
the handshake, so a `TlsStream` is always verified. The second read returns
`0` because the server sent `close_notify` before closing. A connection that
ends without it is `TlsError.Truncated`, since an attacker can cut a TCP
connection short.

The second client asks for `example.test`, which the certificate does not
name. It fails with `WrongHost` and tells the server, whose handshake fails
with `Rejected`. An unknown issuer is `UntrustedCertificate`, and an expired
certificate is `ExpiredCertificate`.

`TlsStream` also has `read`, `write`, `flush`, and `close`, so it works with
`std.io` helpers and [`std.http`](../../design/stdlib/http.md) framing. Those
methods wait without a deadline.

See [`std.tls`](../../design/stdlib/tls.md) for the full reference.
