# std.tls

`import std.tls` runs TLS 1.2 and 1.3 over a [`std.net`](net.md) TCP stream.
A client verifies the server's certificate chain against chosen trust roots
and checks that it names the server; a server presents a certificate chain
and key. Handshakes, reads, and writes take the same deadline and
cancellation token as the TCP stream under them. The module has one error
type, `TlsError`, and is available on the supported x86-64 Linux GNU target.

The protocol and certificate verification are [rustls](https://github.com/rustls/rustls)
with its ring provider, compiled into the runtime. A rustls session never
touches a socket: `std.tls` moves ciphertext between the session and the TCP
stream in checked Plenty, so every wait is a `std.net` wait.

## Trust roots and configuration

| Function | Result | Contract |
|---|---|---|
| `roots_from_pem(pem: &str)` | `Result[TrustRoots, TlsError]` | The `CERTIFICATE` blocks of `pem` as trust anchors |
| `system_roots()` | `Result[TrustRoots, TlsError]` | The operating system's certificate bundle |
| `client_config(roots: &TrustRoots)` | `Result[ClientConfig, TlsError]` | Client settings verifying servers against `roots` |
| `server_config(chain_pem: &str, key_pem: &str)` | `Result[ServerConfig, TlsError]` | Server settings presenting `chain_pem`, leaf first, with its private key |

`roots_from_pem` skips blocks of other types. PEM that does not decode is
`InvalidCertificate`. A decoded certificate that cannot serve as a trust
anchor is skipped, since system bundles may hold some; when none is usable
the result is `NoTrustRoots`. `TrustRoots.len()` counts the anchors kept.

`system_roots` reads the first readable file of:

1. `/etc/ssl/certs/ca-certificates.crt` (Debian, Ubuntu, Arch, Alpine)
2. `/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem` (Fedora, RHEL)
3. `/etc/ssl/ca-bundle.pem` (openSUSE)
4. `/etc/ssl/cert.pem`

None readable is `NoTrustRoots`. It does not consult environment variables;
read another bundle with `read_text` and pass it to `roots_from_pem`. The
bundle is read at the call, so a long-running program sees updates only when
it builds new roots.

`server_config` accepts a PKCS#8, PKCS#1, or SEC1 private key. A chain or key
that does not parse, an empty chain, or a key that does not match the leaf
is `InvalidConfiguration`. Servers do not ask clients for certificates.

Both configurations use TLS 1.2 and 1.3 with rustls's default cipher suites
and key-exchange groups. Neither resumes sessions: clients keep no session
cache and servers issue no tickets. A configuration is immutable and may be
borrowed by any number of connections and workers.

## Connecting

| Function | Result | Contract |
|---|---|---|
| `connect(tcp: TcpStream, config: &ClientConfig, server_name: &str, deadline: &Deadline, cancel: &CancellationToken)` | `Result[TlsStream, TlsError]` | Complete a client handshake over `tcp` |
| `accept(tcp: TcpStream, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken)` | `Result[TlsStream, TlsError]` | Complete a server handshake over `tcp` |

Both take ownership of the TCP stream and return only after the handshake
completes. `server_name` is a DNS name or an IP address; the server's leaf
certificate must name it in its subject alternative names. A name that is
neither is `InvalidName`. Certificates are checked against the wall clock at
the handshake.

When the handshake fails for a TLS reason, the alert rustls prepared is sent
if the deadline allows, so the peer sees `Rejected`. On any failure the TCP
stream is closed when the partial session is dropped.

## Streams

| Method | Result | Contract |
|---|---|---|
| `read_before(buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken)` | `Result[i64, TlsError]` | Decrypted bytes into `buffer[start:stop]`, waiting for at least one |
| `write_before(source: &Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken)` | `Result[i64, TlsError]` | Encrypt and send at most 16 KiB of `source[start:stop]`; the count taken |
| `write_all_before(source: &Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken)` | `Result[(), TlsError]` | Send all of `source[start:stop]` |
| `close_before(deadline: &Deadline, cancel: &CancellationToken)` | `Result[(), TlsError]` | Send `close_notify`, then close the TCP stream |
| `read(buffer: &mut Bytes, start: i64, stop: i64)` | `Result[i64, StreamError]` | `std.io` Reader; waits without a deadline |
| `write(source: &Bytes, start: i64, stop: i64)` | `Result[i64, StreamError]` | `std.io` Writer; waits without a deadline |
| `flush()` | `Result[(), StreamError]` | Nothing to do: writes are sent as they are made |
| `close()` | `Result[(), StreamError]` | `close_before` without a deadline |

Ranges follow the [range rule](bytes.md#the-range-rule). A read of zero bytes
means the peer sent `close_notify`. A connection that ends without one is
`Truncated` (`StreamError.UnexpectedEof` through `read`), because an attacker
can cut a TCP connection short. Code that frames its own messages, such as
`Content-Length` bodies, may treat a truncation after the last expected byte
as an ordinary end.

`close_before` does not wait for the peer's `close_notify`. After a close,
writes are `Closed`. A failed write has sent an unknown prefix of its range.

The `std.io` methods block as [`TcpStream`](net.md)'s do, so `TlsStream`
works with `std.io` helpers and [`std.http`](http.md) framing. Through them,
TLS failures appear as `StreamError.Io(IoError.Other)`; use the `_before`
methods to see the `TlsError`.

## Errors

```text
pub enum TlsError:
    Net(NetError)
    UntrustedCertificate
    ExpiredCertificate
    WrongHost
    InvalidCertificate
    Rejected
    Protocol
    Truncated
    InvalidName
    InvalidConfiguration
    NoTrustRoots
    Closed
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `Net(cause)` | The TCP stream failed, including `TimedOut` and `Cancelled` |
| `UntrustedCertificate` | The peer's chain does not lead to a trust root, or a signature is wrong |
| `ExpiredCertificate` | A certificate in the chain is expired or not yet valid |
| `WrongHost` | The leaf certificate does not name `server_name` |
| `InvalidCertificate` | A certificate is malformed or unusable for TLS, or PEM does not decode |
| `Rejected` | The peer sent a fatal alert, usually because it rejected our certificate |
| `Protocol` | The peer broke the TLS protocol, or no version or cipher suite is shared |
| `Truncated` | The connection ended without `close_notify` |
| `InvalidName` | `server_name` is neither a DNS name nor an IP address |
| `InvalidConfiguration` | A server chain or key is unusable |
| `NoTrustRoots` | No usable trust anchor was found |
| `Closed` | The stream was closed by this side |
| `Allocation(cause)` | A buffer or runtime object could not be allocated |

## Deadlines and cancellation

Every `_before` method passes its deadline and token to each `std.net` call
it makes, so a handshake against a silent peer ends with
`Net(NetError.TimedOut)` and a cancelled token with `Net(NetError.Cancelled)`.
Use one deadline for a whole exchange, as with `std.net`. After a timeout or
cancellation in the middle of a record, the stream's state is unknown: close
or drop it.

## Ownership and threads

`TrustRoots`, `ClientConfig`, `ServerConfig`, and `TlsStream` each own one
runtime object; dropping it releases the object, and dropping a stream closes
its socket without `close_notify`. A stream may move to a worker. Starting a
session draws randomness and verifying a certificate reads the clock; both are
allowed in workers.

The back-end methods `tls_feed`, `tls_take`, `tls_read`, and `tls_write` are
public on `Bytes` only so that `std.tls` can call them.

## Costs

| Item | Memory |
|---|---|
| `TlsStream` | Two 16 KiB ciphertext buffers, plus rustls's session state and its buffer for the incoming record (up to about 17 KiB) |
| Configuration | The parsed certificates and key, shared by every session |
| `TrustRoots` | The parsed anchors; a full system bundle holds about 120 |

rustls allocates through the runtime's Rust allocator, not Plenty's fallible
allocation, so an allocation failure inside rustls stops the program.
Allocation failures in `std.tls` itself are `Allocation`.

Plaintext is copied once into rustls and ciphertext once into the outgoing
buffer; incoming ciphertext is decrypted into rustls's buffer and copied to
the caller.

## Supply

The runtime builds these crates from unmodified copies under
`plenty-runtime/vendor` (ring's pregenerated assembly is trimmed to the
x86-64 ELF files), so building the compiler needs no network:

| Crate | Version | Licence |
|---|---|---|
| `rustls` | 0.23.46 | Apache-2.0, ISC, or MIT |
| `rustls-webpki` | 0.103.15 | ISC |
| `rustls-pki-types` | 1.15.1 | MIT or Apache-2.0 |
| `ring` | 0.17.14 | Apache-2.0 and ISC, with OpenSSL and BoringSSL notices |
| `untrusted`, `zeroize`, `once_cell` | 0.9.0, 1.9.1, 1.21.4 | ISC; MIT or Apache-2.0; MIT or Apache-2.0 |
| `getrandom`, `libc` | 0.2.17, 0.2.186 | MIT or Apache-2.0 |

ring's C and assembly sources are compiled by `build.rs` with the system C
compiler into a static archive, as SQLite's are. The versions are pinned in
`Cargo.lock`, and a test fails if the two disagree. To update, replace the
directories with new releases, run `cargo update -p CRATE --precise VERSION`,
and rerun `tests/test_std_tls.rs`. Follow rustls security advisories: an
update is a change to the compiler, and programs pick it up when rebuilt.

Programs that do not use `std.tls` contain none of rustls or ring.

## Example

The [tutorial lesson](../../tutorial/11-concurrency/tls-streams.md) serves an
echo over TLS on a worker and shows a client rejecting the wrong host name.

## Limits

- No client certificates (mutual TLS) and no certificate pinning.
- No ALPN, so no protocol negotiation for HTTP/2.
- No session resumption or 0-RTT data.
- No revocation checks (CRLs or OCSP).
- No choice of protocol versions, cipher suites, or groups.
- No server name indication (SNI) selection on servers: one chain per
  configuration.
- No nonblocking use with `std.ready` selectors.
- One target, x86-64 Linux GNU.
