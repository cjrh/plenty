# Send HTTPS requests

[`std.http.https`](../../design/stdlib/http-client.md#https) gives the
[HTTP client](http-client.md) `https` URLs. It runs [TLS](tls-streams.md)
on each connection, checking that the server's certificate chains to a
trusted root and names the host in the URL. Everything else is the same
client: pooled connections, deadlines, redirects and replays.

This program serves HTTPS on a loopback port with
[`std.http.tls`](../../design/stdlib/http-server.md#https) and sends it two
requests from another thread. The second streams its body from a reader
instead of holding it in the request. Like the TLS lesson, it embeds a test
CA so that it runs offline.

```plenty
import std.bytes
import std.http
import std.http.client
import std.http.https
import std.http.server
import std.http.tls
import std.io
import std.net
import std.time
import std.tls
from std.http.server import Exchange
from std.http.tls import TlsTransport
from std.sync import CancellationToken

# Test-only certificates: a CA, and a leaf it signed for localhost and
# 127.0.0.1. Never use a published key outside an example.
def ca_pem() -> str:
    return "-----BEGIN CERTIFICATE-----\nMIIBmjCCAT+gAwIBAgIULOQ2Qxr8CVZY2bG0uRYKKEdMD3swCgYIKoZIzj0EAwIw\nGTEXMBUGA1UEAwwOUGxlbnR5IFRlc3QgQ0EwIBcNMjQwMTAxMDAwMDAwWhgPMjEy\nNDAxMDEwMDAwMDBaMBkxFzAVBgNVBAMMDlBsZW50eSBUZXN0IENBMFkwEwYHKoZI\nzj0CAQYIKoZIzj0DAQcDQgAEkiB3Ogehrrt6YbQghwDPv6tkhE+z9zTCVih8fvtR\nZ1/691Nfh7/J9IgaNgNSTMJjGTolzGnjaCUDqoARlFyM+6NjMGEwHQYDVR0OBBYE\nFOTFBDgWXHj/qmVJDMlAHYCEZFX5MB8GA1UdIwQYMBaAFOTFBDgWXHj/qmVJDMlA\nHYCEZFX5MA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgIEMAoGCCqGSM49\nBAMCA0kAMEYCIQD41ATQnIjKkKMLQAb9AyoyOFmljYZyz348XJEIbqMgKwIhAL3m\nmWd8On5gvkfytspatH5mDWOcSyW+au7vuIC2gHhO\n-----END CERTIFICATE-----\n"

def leaf_pem() -> str:
    return "-----BEGIN CERTIFICATE-----\nMIIBnDCCAUKgAwIBAgIBATAKBggqhkjOPQQDAjAZMRcwFQYDVQQDDA5QbGVudHkg\nVGVzdCBDQTAgFw0yNDAxMDEwMDAwMDBaGA8yMTI0MDEwMTAwMDAwMFowFDESMBAG\nA1UEAwwJbG9jYWxob3N0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE8THNQeTi\nJ6Eq0tSxtHW5Q4TCsbIqB3MOtM98ERyKgl6kHeHCeDZYKx8B+VRGT2jkWuTeN24Q\nljwUfh7uh7Iaz6N+MHwwGgYDVR0RBBMwEYIJbG9jYWxob3N0hwR/AAABMAkGA1Ud\nEwQCMAAwEwYDVR0lBAwwCgYIKwYBBQUHAwEwHQYDVR0OBBYEFEIFC2Fyd4cfAKTx\nMiNCMdDkqDNbMB8GA1UdIwQYMBaAFOTFBDgWXHj/qmVJDMlAHYCEZFX5MAoGCCqG\nSM49BAMCA0gAMEUCIAUF3HH43lnirgSN1Vr4twZrFmXKBxlbYNQni/h4ZaxhAiEA\n5rD/yE1B4IsbEGCMlyAeX507leXfGY9+wrHaxikU/w0=\n-----END CERTIFICATE-----\n"

def leaf_key() -> str:
    return "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQglvSEE3H7v+mInxSF\nAyWFDKYpzpXr20wH8mfFY3bCaB2hRANCAATxMc1B5OInoSrS1LG0dblDhMKxsioH\ncw60z3wRHIqCXqQd4cJ4NlgrHwH5VEZPaORa5N43bhCWPBR+Hu6HshrP\n-----END PRIVATE KEY-----\n"

pub enum Route:
    Count

pub class Site:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TlsTransport]) -> Result[(), AllocError | std.http.HttpError | std.bytes.BytesError]:
        mut body = std.bytes.empty()?
        exchange.read_all(&mut body)?
        exchange.send_text(200, &str.from(body.len())?.concat(" bytes")?)?
        Ok(())

def visit(port: u16, stop: CancellationToken) -> Result[(), AllocError | IoError | std.bytes.BytesError | std.http.client.ClientError | std.tls.TlsError | RangeError]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
    base = "https://localhost:".concat(&str.from(i64(port))?)?
    roots = std.tls.roots_from_pem(&ca_pem())?
    tls = std.tls.client_config(&roots)?
    client = std.http.https.client(std.http.client.config()?, tls)?

    home = client.get(&base.concat("/")?, &deadline, &never)?
    print(home.status, home.body.to_text()?)?

    mut source = std.io.cursor(std.bytes.from_text("a body read in pieces")?)
    request = std.http.client.request("PUT", &base.concat("/upload")?)?
    mut response = client.send_from(request, &mut source, &std.http.client.chunked(), &deadline, &never)?
    mut answer = std.bytes.empty()?
    response.read_all(&mut answer)?
    print(response.status, answer.to_text()?)?
    client.finish(response)

    stats = client.stats()
    print(stats.opened, stats.reused)?
    stop.cancel()
    Ok(())

def main() -> Result[(), ThreadError | std.http.server.ServerError | std.net.NetError | AllocError | std.tls.TlsError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Count)?
    routes.add("PUT", "/upload", Route.Count)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    server = std.tls.server_config(&leaf_pem(), &leaf_key())?
    with spawn(visit, port, stop.share())? as task:
        drop(std.http.tls.serve(listener, server, &routes, &Site(), &std.http.server.config(), &stop)?)
    Ok(())
```
```output
200 0 bytes
200 21 bytes
1 1
```

`std.http.https.client` takes the client config and a TLS `ClientConfig`.
The client uses the URL's host, here `localhost`, as the name the
certificate must carry. Verification cannot be turned off: to trust a
private server, build the `ClientConfig` from its CA as this program does,
and use `std.http.https.system_client` for public servers. A certificate
from an unknown CA fails with `Tls(UntrustedCertificate)`, and one for
another host with `Tls(WrongHost)`. Both requests used one TLS connection.

`send_from` reads the body from any `std.io` reader in 16 KiB pieces, so a
large upload never sits in memory whole. `chunked()` sends it in chunks of
unknown total length; `sized(n)` sends `Content-Length: n` and fails with
`WrongBodyLength` if the reader yields a different amount. Setting
`expect_continue` on the upload asks the server to approve the request
before the body is sent.

A streamed body can be read only once. The client still replays a request
on a fresh connection when a pooled one was closed before any of the body
was read, but it returns a redirect that would need the body again rather
than following it.

With `config.proxy` set, an `https` request asks the proxy to open a tunnel
with `CONNECT`, then runs TLS to the server through it, so the proxy sees
neither the request nor the response.

The HTTPS support is a separate module: a program that imports only
`std.http.client` does not include the TLS library.
