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

Direct TLS through `serve_connection` and [`std.tls`](../../backlog.md) is
later work.

## Static files

No static file helper ships yet. A safe one must open the request path one
segment at a time through directory handles, as `Root.open_dir` and
`Root.read_file` in [`std.fs`](fs.md) do for single names, refuse `..`,
links and anything but regular files, and bound what it reads; cleaning the
path as a string is not enough, because a link inside the served directory
can still lead out of it. It also needs media types, `Range`, and
conditional requests (`ETag`, `If-Modified-Since`) to be useful, and the
server has none of these. Until then, serve static files from the reverse
proxy, which already does this well.
