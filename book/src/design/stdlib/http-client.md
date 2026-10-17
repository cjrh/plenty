# `std.http.client`

`import std.http.client` sends HTTP/1.1 requests over pooled keep-alive
connections. Each call has a caller deadline and cancellation token and a
timeout for each phase. Redirects are bounded and drop credentials when the
origin changes. A proxy is used only when configured, and a request is
replayed only when that is safe. Framing comes from [`std.http`](http.md)
and sockets from [`std.net`](net.md). The module is checked Plenty in
`stdlib/std/http/client.plenty` and adds no runtime primitives. The tutorial
lesson [Send HTTP requests](../../tutorial/11-concurrency/http-client.md)
shows a complete program.

## Clients

| Item | Meaning |
|---|---|
| `client(config: Config)` | `Result[Client[Plain, TcpConnector], ClientError]`: plain `http` over TCP |
| `connector[T, C](opener: C, config: Config)` | `Result[Client[T, C], ClientError]`: connections come from `opener`; give both type arguments |
| `client.send(request, &deadline, &cancel)` | `Result[Response[T], ClientError]`: the final response head, after redirects |
| `client.fetch(request, &deadline, &cancel)` | `Result[Fetched, ClientError]`: `send`, read the whole body, `finish` |
| `client.get(url: &str, &deadline, &cancel)` | `fetch` of a `GET` |
| `client.finish(response)` | Pool the connection if the body was read to its end and both sides allow reuse; otherwise close it |
| `client.close_idle()` | Close every pooled connection |
| `client.idle()` | `i64`: pooled connections |
| `client.stats()` | `Stats` |

Both constructors fail with `InvalidConfig` for a config outside the bounds
below. A client is used by one thread at a time; its methods take it by
exclusive borrow. Give each thread its own client.

`deadline` bounds the whole exchange, including later reads of the body of
the `Response` that `send` returns. `cancel` stops it: a wait in progress
ends with `Net(Cancelled)`.

## Requests and responses

| Item | Meaning |
|---|---|
| `request(method: &str, url: &str)` | `Result[Request, ClientError]` with no fields and an empty body |
| `Request.method`, `url`, `headers: list[Header]`, `body: Bytes` | Public fields |
| `request.add(name, value)` | `Result[(), ClientError]`: append a field |
| `Response.status`, `head: ResponseHead`, `url`, `redirects` | Final status and head, the URL that answered, and redirects followed |
| `response.read_body(&mut buffer, start, stop)` | `Result[i64, ClientError]`: part of the body; `0` is the end |
| `response.read_all(&mut out)` | `Result[i64, ClientError]`: append the rest, within `limits.max_body` |
| `response.read(...)`, `take_failure()` | A `std.io` `Reader`; a client failure is `Io(InvalidInput)` and `take_failure` returns it, a short body is `UnexpectedEof` |
| `response.is_done()` | The whole body has been read |
| `Fetched.status`, `head`, `url`, `redirects`, `body: Bytes` | A whole response |

The client writes `Host` from the URL, and `Content-Length` when the body is
not empty or the method is `POST`, `PUT` or `PATCH`. The body is sent with
`Content-Length`; chunked request bodies are not sent. A request that sets
`Host`, `Content-Length`, `Transfer-Encoding`, `Connection`, `Keep-Alive`,
`Upgrade`, `TE`, `Trailer`, `Expect`, `Proxy-Authorization` or
`Proxy-Connection` fails with `InvalidRequest`, as do `CONNECT` and a method
that is not an HTTP token. These checks run before any connection opens.

URLs must be absolute `http` URLs with a host. Credentials in the URL
(`user:pass@`) are `InvalidUrl`; send an `Authorization` field instead. The
fragment is not sent. An `https` URL passes the URL checks but the TCP
connector refuses it with `UnsupportedScheme`.

Interim `1xx` responses are skipped, up to 16; `101 Switching Protocols` is
`UnexpectedInterim`. A response to `HEAD`, and a `204` or `304`, has no body.

## Connections and the pool

After `fetch`, or `finish` on a fully read response, the connection waits in
the pool for another request to the same scheme, host and port (and proxy).
The most recently used connection is taken first. A connection is closed
instead of pooled when:

- either side sent `Connection: close`, or the response is HTTP/1.0 without
  keep-alive, or its body ran to the end of the connection;
- the body was not read to its end, or reading it failed;
- bytes beyond the response are already buffered;
- the pool already holds `max_idle_per_origin` connections for that origin,
  or `max_idle` in all (the oldest is closed to make room).

Dropping a `Response` without `finish` closes its connection. Before reusing
a pooled connection the client checks, without waiting, that nothing has
arrived on it: a connection the server closed, or one with unexpected
bytes, is closed and another is used. A connection idle longer than
`idle_timeout` is closed when the pool is next used.

## Replaying requests

A server may close an idle connection at the moment the client reuses it.
When a request on a reused connection fails before any byte of a response
arrives, by end of input, a reset, or a failed write, the client sends it
again on a new connection, at most `retries` times, but only if:

- the method is `GET`, `HEAD`, `OPTIONS`, `TRACE`, `PUT` or `DELETE`, and
- the body is the request's in-memory `body`, which can be sent again.

`POST`, `PATCH` and other methods are never replayed, since the server may
have acted on them. A failure on a new connection, or after response bytes,
is returned, and so is a timeout or cancellation.

## Redirects

A `301`, `302`, `303`, `307` or `308` response with a `Location` field is
followed, at most `redirects` times; one more is `TooManyRedirects`. With
`redirects = 0` the redirect response itself is returned.

| Status | Next request |
|---|---|
| `303` | `GET` without a body (`HEAD` stays `HEAD`) |
| `301`, `302` after `POST` | `GET` without a body |
| `301`, `302` otherwise, `307`, `308` | Same method and body |

When the body is dropped, so are `Content-*` fields. `Location` may be
absolute, scheme-relative (`//host/path`), absolute-path or relative; it is
resolved against the current URL and `.` and `..` segments are removed.
A location that does not parse is `InvalidRedirect`, another scheme is
`UnsupportedScheme`, and a move from `https` to `http` is `InvalidRedirect`.

When the scheme, host or port changes, the next request and every later one
lose `Authorization`, `Cookie`, and every field named in `config.sensitive`.
They are not restored on a redirect back. The body of a redirect response is
read and discarded, up to 64 KiB, so the connection can be reused; a longer
body closes it.

## Proxies

| Item | Meaning |
|---|---|
| `proxy(url: &str)` | `Result[Proxy, ClientError]` for `http://host:port`, no path or credentials |
| `Proxy.endpoint`, `authorization: Option[str]` | Where to connect, and the `Proxy-Authorization` value |

With `config.proxy` set, every `http` request goes to the proxy with an
absolute-form target (`GET http://host/path HTTP/1.1`). `authorization` is
sent to the proxy as `Proxy-Authorization` and never to an origin. Nothing is
read from the environment: `HTTP_PROXY` and `NO_PROXY` are ignored.

## Configuration

`config()` returns a `Config` with every field public:

| Field | Meaning | `config()` |
|---|---|---|
| `limits` | `std.http` `Limits` for response heads and bodies | `std.http.limits()` |
| `connect_timeout` | Resolve the host and connect | 10 s |
| `write_timeout` | Any one write | 10 s |
| `head_timeout` | From the end of the request to the end of the final response head | 30 s |
| `read_timeout` | Any one read | 30 s |
| `idle_timeout` | Keep a pooled connection | 30 s |
| `max_idle` | Pooled connections in all, at least 0; 0 sends `Connection: close` | 16 |
| `max_idle_per_origin` | Pooled connections for one origin, at least 0 | 4 |
| `redirects` | Redirects to follow, 0 to 100 | 10 |
| `retries` | Replays of a safe request, 0 to 10 | 1 |
| `sensitive` | Further field names dropped across origins | empty |
| `proxy` | `Option[Proxy]` | `Nothing` |

Every wait ends at the earliest of the caller's deadline, its phase timeout
and its per-operation timeout.

## Transports

| Item | Meaning |
|---|---|
| `protocol Transport` | `read_before` and `write_before` with `TcpStream`'s parameters, returning `Result[i64, ClientError]` |
| `protocol Connector[T]` | `open(self: &mut, endpoint: &Endpoint, deadline, cancel) -> Result[T, ClientError]` |
| `Endpoint(scheme, host, port: u16)` | Where to connect: the origin, or the proxy; `host` is lowercase without brackets |
| `Plain`, `TcpConnector` | A TCP connection; the connector resolves the host and tries each address in turn |

A connector decides how to reach an endpoint. A TLS connector that returns
an encrypting stream will make `https` URLs work without changing the
client. An in-memory connector makes code that uses a client testable
without sockets.

## Errors

`ClientError` is `Net(NetError)` for connection failures, timeouts and
cancellation; `Http(HttpError)` for malformed or oversized responses, with
`Truncated` for a connection that closed early; `Url(UrlError)`;
`Allocation`; and `InvalidConfig`, `InvalidUrl`, `UnsupportedScheme`,
`InvalidRequest`, `InvalidRedirect`, `TooManyRedirects` and
`UnexpectedInterim`. No variant carries a URL or field value, so printing an
error does not reveal credentials.

`Stats` counts, as `u64`: `opened` connections, `reused` pooled
connections, `retried` replays, `redirected` hops and `discarded`
connections closed instead of reused: stale, expired, over a pool limit, or
with unread bytes buffered.

## Limits

- No `https` yet: TLS arrives with a [`std.tls`](../../backlog.md)
  connector, and `CONNECT` tunnelling through a proxy with it.
- Request bodies are in memory; streamed and chunked request bodies are
  later work. Response bodies stream.
- No cookie jar, content decoding, `Expect: 100-continue`, or HTTP/2.
- One thread per client. The pool is not shared between threads.
- Connecting tries addresses one after another, not in parallel.
