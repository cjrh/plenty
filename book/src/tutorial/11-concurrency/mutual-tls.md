# Verify clients and choose a protocol

A server can ask the client for a certificate too. This is mutual TLS: both
sides prove who they are, so a service can accept only clients holding a key
that its own authority signed. Build such a configuration with a builder,
which gathers settings and checks them together in `build`.

The same builders negotiate an application protocol (ALPN), the name a
client and server agree on during the handshake, such as `h2` for HTTP/2.

```plenty
import std.bytes
import std.tls
from std.net import NetError, TcpListener, connect, listen, loopback_v4
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now
from std.tls import TlsError, ClientCertificates, ClientConfig, ServerConfig, TlsStream

type ConnectionError = IoError | NetError | TlsError | std.bytes.BytesError

# Test-only certificates: a server CA and a localhost leaf it signed, and a
# client CA and a client certificate it signed. Never use a published key
# outside an example.
def ca_pem() -> str:
    return "-----BEGIN CERTIFICATE-----\nMIIBmTCCAT+gAwIBAgIUUResO5RQujUgwbO6wnWnB6CmmckwCgYIKoZIzj0EAwIw\nGTEXMBUGA1UEAwwOUGxlbnR5IFRlc3QgQ0EwIBcNMjQwMTAxMDAwMDAwWhgPMjEy\nNDAxMDEwMDAwMDBaMBkxFzAVBgNVBAMMDlBsZW50eSBUZXN0IENBMFkwEwYHKoZI\nzj0CAQYIKoZIzj0DAQcDQgAEnJV8Ts/yoTQC8df/IwhCPwjYuY+siUFEkwoyRG82\nFbjVeUydEKr7G3fG2hp21P46YwzRUsYal8Y/dZNwDPHKcKNjMGEwHQYDVR0OBBYE\nFIRt2tnXzddX1P2dNz1kPdQrraOuMB8GA1UdIwQYMBaAFIRt2tnXzddX1P2dNz1k\nPdQrraOuMA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgEGMAoGCCqGSM49\nBAMCA0gAMEUCIQCvPKJicaEPA6BwyR27QUz2f+h4RXSkJ9AX48/WuZ9K/QIgcV5e\nt2woOIP3sGU2PAopPcJVH2eC2068REEAlU7QFWc=\n-----END CERTIFICATE-----\n"

def leaf_pem() -> str:
    return "-----BEGIN CERTIFICATE-----\nMIIBnDCCAUKgAwIBAgIBATAKBggqhkjOPQQDAjAZMRcwFQYDVQQDDA5QbGVudHkg\nVGVzdCBDQTAgFw0yNDAxMDEwMDAwMDBaGA8yMTI0MDEwMTAwMDAwMFowFDESMBAG\nA1UEAwwJbG9jYWxob3N0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEh9wzUObb\nKhx8QP/mmT2p56niQY0XpFwOiXAC8qThDYNrqtZY+f+gzGFjFamRWJBgegZ7lTWc\n3XACctwTZV7F7aN+MHwwGgYDVR0RBBMwEYIJbG9jYWxob3N0hwR/AAABMBMGA1Ud\nJQQMMAoGCCsGAQUFBwMBMAkGA1UdEwQCMAAwHQYDVR0OBBYEFP2y8OrD2DL5XVhP\ntwRYSPXCUxblMB8GA1UdIwQYMBaAFIRt2tnXzddX1P2dNz1kPdQrraOuMAoGCCqG\nSM49BAMCA0gAMEUCIQD7b3qMvbfg125pcOxRnlgi9d1kA/XO06NcKtspGn/chAIg\nRsvsbrsb0St1++9xCNovakguzYFGDNOUGEecwkoMwRw=\n-----END CERTIFICATE-----\n"

def leaf_key() -> str:
    return "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgkVCAyTuCJRiqL7JD\nJEWjEGpE5Jas61PHxV76daBTJaChRANCAASH3DNQ5tsqHHxA/+aZPannqeJBjRek\nXA6JcALypOENg2uq1lj5/6DMYWMVqZFYkGB6BnuVNZzdcAJy3BNlXsXt\n-----END PRIVATE KEY-----\n"

def client_ca_pem() -> str:
    return "-----BEGIN CERTIFICATE-----\nMIIBnTCCAUOgAwIBAgIUdjiICLarpx0Q9y7yY6grXgRasaAwCgYIKoZIzj0EAwIw\nGzEZMBcGA1UEAwwQUGxlbnR5IENsaWVudCBDQTAgFw0yNDAxMDEwMDAwMDBaGA8y\nMTI0MDEwMTAwMDAwMFowGzEZMBcGA1UEAwwQUGxlbnR5IENsaWVudCBDQTBZMBMG\nByqGSM49AgEGCCqGSM49AwEHA0IABOMjuGk6yTyYHlHTSFqdxWo2pgdqU5/oUzGq\naI+Hwz342D5kZPevdjFf1uyS+wa8XAdRdGqcxoTfqMMKaaj5vw+jYzBhMB0GA1Ud\nDgQWBBS89JCv7pReUSMZUl8kRbl5orgjjjAfBgNVHSMEGDAWgBS89JCv7pReUSMZ\nUl8kRbl5orgjjjAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB/wQEAwIBBjAKBggq\nhkjOPQQDAgNIADBFAiEA5H1xwC1vK6Fx1Zn8HqI2Ob8wry5EdjCO3Usej/KfXG8C\nIF3oydaIMmuMWw6yVxTJ6XMLbvMf4TXIxlPt7uZwpBAc\n-----END CERTIFICATE-----\n"

def client_pem() -> str:
    return "-----BEGIN CERTIFICATE-----\nMIIBhjCCASygAwIBAgIBATAKBggqhkjOPQQDAjAbMRkwFwYDVQQDDBBQbGVudHkg\nQ2xpZW50IENBMCAXDTI0MDEwMTAwMDAwMFoYDzIxMjQwMTAxMDAwMDAwWjAYMRYw\nFAYDVQQDDA1QbGVudHkgQ2xpZW50MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE\nT5GBs10j6jCpjnrqvhM72PkArJoUAQHTk30vDaFUfzqLjJLvNnGG3mxcJ6nhmZIH\nl4K0HSop5C/aewN7v67bWaNiMGAwEwYDVR0lBAwwCgYIKwYBBQUHAwIwCQYDVR0T\nBAIwADAdBgNVHQ4EFgQUd3LfI5CS7q5ORpggbgx2rsLnrsQwHwYDVR0jBBgwFoAU\nvPSQr+6UXlEjGVJfJEW5eaK4I44wCgYIKoZIzj0EAwIDSAAwRQIgUAs5ByLBx0fx\nhNbzolmRLRLUgo0jwQJwhc3dINKcY2ECIQC5MGMpqLAhsdUm00Kdjco/wNwFiPq8\nUypR3xWb5//Lkw==\n-----END CERTIFICATE-----\n"

def client_key() -> str:
    return "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgIVqyz+J6RT5Qaqpt\ndcpJwQnZR3muh8UkZ4GdOuUB1aShRANCAARPkYGzXSPqMKmOeuq+EzvY+QCsmhQB\nAdOTfS8NoVR/OouMku82cYbebFwnqeGZkgeXgrQdKinkL9p7A3u/rttZ\n-----END PRIVATE KEY-----\n"

# Reports who connected, then answers one message.
def answer(stream: &mut TlsStream, deadline: &Deadline, cancel: &CancellationToken) -> Result[(), ConnectionError]:
    match stream.peer_certificate()?:
        case Some(der):
            print(der.len() > 0)?
        case Nothing:
            print("no certificate")?
    print(stream.application_protocol()?)?
    mut buffer = std.bytes.with_capacity(64)?
    buffer.resize(64, 0u8)?
    count = stream.read_before(&mut buffer, 0, 64, deadline, cancel)?
    stream.write_all_before(&buffer, 0, count, deadline, cancel)?
    stream.close_before(deadline, cancel)?
    Ok(())

def greet(listener: &TcpListener, config: &ServerConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[(), ConnectionError]:
    tcp = listener.accept(deadline, cancel)?
    match std.tls.accept(tcp, config, deadline, cancel):
        case Ok(opened):
            mut stream = opened
            answer(&mut stream, deadline, cancel)?
        case Err(error):
            print(error)?
    Ok(())

# The client's outcome is printed after the server's, once the server is done.
def ask(listener: &TcpListener, server: &ServerConfig, client: &ClientConfig, deadline: &Deadline, cancel: &CancellationToken) -> Result[(), Failure]:
    address = listener.local_address()?
    mut reply = std.bytes.with_capacity(16)?
    reply.resize(16, 0u8)?
    with spawn(greet, listener, server, deadline, cancel)? as worker:
        tcp = connect(&address, deadline, cancel)?
        mut stream = std.tls.connect(tcp, client, "localhost", deadline, cancel)?
        message = std.bytes.from_text("hello")?
        stream.write_all_before(&message, 0, message.len(), deadline, cancel)?
        outcome = stream.read_before(&mut reply, 0, 16, deadline, cancel)
        worker.join()?
        match outcome:
            case Ok(count):
                print(reply.slice(0, count)?.to_text()?)?
            case Err(error):
                print(error)?
    Ok(())

def main() -> Result[(), Failure]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000))?
    mut server = std.tls.server_builder(&leaf_pem(), &leaf_key())?
    server.verify_clients(&std.tls.roots_from_pem(&client_ca_pem())?, ClientCertificates.Required)
    server.application_protocol("greet/1")?
    server_config = server.build()?
    roots = std.tls.roots_from_pem(&ca_pem())?
    mut client = std.tls.client_builder(&roots)?
    client.certificate(&client_pem(), &client_key())?
    client.application_protocol("greet/1")?
    client_config = client.build()?
    listener = listen(&loopback_v4(0u16), 8)?
    ask(&listener, &server_config, &client_config, &deadline, &cancel)?
    ask(&listener, &server_config, &std.tls.client_config(&roots)?, &deadline, &cancel)?
    Ok(())
```

```output
True
Option[str].Some("greet/1")
hello
std.tls.TlsError.CertificateRequired
std.tls.TlsError.Rejected
```

`verify_clients` takes the roots that client certificates must chain to,
usually a private authority rather than the system bundle. With
`ClientCertificates.Optional` a client may present none, and the server's
`peer_certificate()` is then `Nothing`. `peer_certificate()` returns the DER
bytes of the verified leaf, for code that maps a certificate to an account.

The second client has no certificate. The server fails with
`CertificateRequired`. In TLS 1.3 the client finishes its side of the
handshake before the server checks it, so the client learns of the refusal
at its first read, as `Rejected`.

A server picks the first of its protocol names that the client also offers,
and `application_protocol()` reports it on both sides. A client that offers
only names the server does not accept fails with `Incompatible`; one that
offers none gets `Nothing`.

Builders also choose the minimum TLS version and the cipher suites, add
revocation lists, and give a server several certificates to choose by the
name the client asked for. See [`std.tls`](../../design/stdlib/tls.md).

`ConnectionError` preserves the concrete I/O, networking, TLS, and byte-buffer
errors from the connection helpers. The spawning caller still uses `Failure`:
scoped `spawn(...)?` currently accepts only `ThreadError` or `Failure`, not an
error sum containing `ThreadError`. This is a remaining compiler limitation,
not a reason to unwrap setup failures.
