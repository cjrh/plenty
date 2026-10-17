# `std.http.proxy`

`import std.http.proxy` finds the client of a request that came through
reverse proxies. A forwarding header is believed only from a proxy the
program names. The module is checked Plenty in
`stdlib/std/http/proxy.plenty` and adds no runtime primitives. This page
also gives the deployment guidance for [`std.http.server`](http-server.md).

## Clients

| Item | Meaning |
|---|---|
| `proxies(header: Forwarding)` | `Result[Proxies, ProxyError]`: trusts nobody yet |
| `proxies.trust(network)` | Trust `10.0.0.0/8`, `fd00::/8` or one address |
| `proxies.trusts(&ip)` | `bool` |
| `proxies.max_hops` | Forwarding entries allowed in one request; 16 |
| `Forwarding` | `XForwardedFor` (with `X-Forwarded-Proto`) or `Forwarded` (RFC 7239) |
| `client(&peer, &headers, &proxies)` | `Result[Client, ProxyError]` |
| `client_of(exchange, &proxies)` | `client` for an exchange's `peer` and fields |
| `Client` | `address: IpAddress`, `scheme: Option[str]`, `proxied: bool` |

The rule:

1. If the peer is not trusted, it is the client, and the forwarding headers
   are ignored.
2. Otherwise the entries of the chosen header are read from the right. Each
   trusted address is a proxy; the first untrusted one is the client.
3. If every entry is trusted, the leftmost is the client. If there are no
   entries, the peer is.

Entries left of the client were written by the client and are never parsed,
so they cannot cause an error. The other header is always ignored, so a
deployment that uses `Forwarded` cannot be fooled with `X-Forwarded-For`.

An entry is `192.0.2.1`, `192.0.2.1:8080`, `2001:db8::1` or
`[2001:db8::1]:8080`; the port is dropped. An entry that must be read but is
none of these, such as `unknown` or an obfuscated `_name`, fails with
`InvalidForwarded`, as does a malformed `Forwarded` element. More than
`max_hops` entries is `TooManyHops`. An IPv4-mapped IPv6 address, as a
dual-stack listener reports IPv4 peers, is treated as the IPv4 address.

`scheme` is `http` or `https`, lowercased, when a trusted proxy reported it:
the client's element's `proto=` for `Forwarded`, or the last
`X-Forwarded-Proto` value, which the nearest proxy wrote. Any other scheme is
`InvalidForwarded`. It is `Nothing` when the peer is not trusted.

`trust` refuses a network with bits set below its prefix length
(`10.0.0.1/8`) with `InvalidNetwork`, so a typo cannot trust more than
intended. `client_of` fails with `UnknownPeer` when the exchange has no peer,
as with `serve_connection` on a custom transport.

## Deploying behind a reverse proxy

`std.http.server` serves plain HTTP/1.1. For a public service, run it
behind a reverse proxy that terminates TLS:

- Listen on loopback or a private address, never directly on the public
  interface. Then every peer is a proxy.
- Configure the proxy to overwrite, not append to, the header the program
  reads, or to append only its own peer address. Trust exactly the proxy's
  addresses.
- Let the proxy enforce request rates and connection limits per client; the
  server's limits are per connection and per worker.
- Keep `config.limits` and the body limits below the proxy's, so the server
  answers oversized requests itself.
- Cookies default to `Secure`, which browsers send only over HTTPS. The
  program sees plain HTTP from the proxy; check `scheme` rather than
  assuming.

## Serving HTTPS directly

Without a proxy, serve TLS from the program with
[`std.http.tls`](http-server.md#https). Then it faces clients itself:

- Listen on the public address only through `std.http.tls.serve`; a plain
  `serve` on the same host should only redirect or be absent.
- No proxy is trusted, so leave `Proxies` empty: `client_of` returns the
  peer.
- Keep the server's limits, timeouts and `idle_connections` tight; nothing
  in front enforces per-client rates.
- Cookies keep their `Secure` default, and the scheme is always `https`.

```text
pub class Site:
    pub public: Root
    pub keys: list[Bytes]

    pub def handle(self, exchange: &mut Exchange[TlsTransport]) -> Result[(), Failure]:
        if exchange.route == 0:
            path = exchange.params[0].concat("")?
            std.http.files.serve(exchange, &self.public, &path)?
        elif exchange.route == 1:
            mut head = std.http.response(204, "No Content")?
            std.http.cookie.add(&mut head, &std.http.cookie.signed_cookie(&self.keys, "session", "user42")?)?
            exchange.respond(head, &std.bytes.empty()?)?
        else:
            found = std.http.cookie.cookies(&exchange.request.headers, 8)?
            match std.http.cookie.find_signed(&found, &self.keys, "session")?:
                case Some(user):
                    exchange.send_text(200, &user)?
                case Nothing:
                    exchange.send_text(401, "sign in first")?
        Ok(())

def main() -> Result[(), Failure]:
    mut table = std.http.server.router()?
    table.add("GET", "/s/{path...}", 0)?
    table.add("POST", "/login", 1)?
    table.add("GET", "/me", 2)?
    keys = [std.bytes.from_text(&read_text("/etc/site/session.key")?)?]?
    site = Site(std.fs.root(&std.path.from_text("/srv/site")?)?, keys)
    tls = std.tls.server_config(&read_text("/etc/site/chain.pem")?, &read_text("/etc/site/key.pem")?)?
    listener = std.net.listen(&std.net.parse_address("0.0.0.0:443")?, 128)?
    stop = CancellationToken()?
    report = std.http.tls.serve(listener, tls, &table, &site, &std.http.server.config(), &stop)?
    Ok(())
```

`tests/test_std_http_files.rs` runs this program on loopback with the test
certificates in `tests/fixtures/tls`.

## Static files

[`std.http.files`](http-files.md) serves files below a `Root`. It opens
the request path one segment at a time through directory handles, refuses
`..`, hidden names, links and anything but regular files, and answers
`ETag` and `If-Modified-Since`; see its limits (no `Range`, no directory
index). A reverse proxy can still serve static files when it is already in
front.
