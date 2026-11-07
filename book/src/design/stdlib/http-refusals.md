# HTTP malformed-capture responses

The worker [`std.http.server`](http-server.md) and TLS server can render a
custom `400` response for a successfully matched route whose capture fails
percent decoding. Register a capture-free function on the router:

```plenty
import std.http.server
from std.http.server import PathRefusal, RefusalResponse, RefusalError

def bad_path(why: &PathRefusal, response: &mut RefusalResponse) -> Result[(), RefusalError]:
    response.set_content_type("text/html; charset=utf-8")?
    match why:
        case PathRefusal.InvalidEscape:
            response.append(b"<p>Invalid path escape.</p>")?
        case PathRefusal.InvalidUtf8:
            response.append(b"<p>Path must be UTF-8.</p>")?
    Ok(())

def main() -> Result[(), std.http.server.ServerError]:
    mut routes = std.http.server.router[i64]()?
    routes.add("GET", "/files/{path...}", 0)?
    routes.set_path_refusal(Some(bad_path), 4096)
    Ok(())
```
```output
```

This example configures the router and exits without starting a listener.

`Router.set_path_refusal(renderer, max_bytes)` returns `()` and replaces the
previous function and limit without allocating. Its function type is
`Callable[[&PathRefusal, &mut RefusalResponse], Result[(), RefusalError]]`.
Use a named function or a capture-free anonymous function; capturing closures
are not accepted. `Nothing` restores the built-in response. The callback runs
on the serving worker and must obey the ordinary worker rules, including the
restriction on user FFI. There is no per-request callback allocation.

## Available data and validation

`PathRefusal.InvalidEscape` means a capture contains a malformed or truncated
percent escape. `InvalidUtf8` means percent decoding produced invalid UTF-8.
These variants expose no raw target, capture, query, headers, body, peer,
route value or exchange. Neither ordinary route handlers nor fallback handlers
receive these captures. Callback registration does not change route matching,
`Router.find`, precedence, method selection or slash redirects. In particular,
an unmatched path is still handled by the usual fallback policy; the server
does not introduce whole-path decoding.

Request-head, target/Host, framing and declared-body-size checks run first.
Their failures retain the built-in responses and do not call this renderer.
Allocation failure during capture decoding closes the connection, rather than
being reported to the callback as malformed client input.

## Bounded rendering

The server constructs `RefusalResponse`; applications cannot construct one or
access its fields. It initially has an empty body and content type
`text/plain; charset=utf-8`.

| Method | Result and behavior |
|---|---|
| `append(source: &[u8])` | `Result[(), RefusalError]`; copies bytes into the body, or fails before appending anything |
| `set_content_type(value: str)` | `Result[(), RefusalError]`; consumes a replacement containing 1–128 printable ASCII bytes |
| `check_at(now: &Instant)` | Same check with an explicit monotonic reading for deterministic tests |
| `check()` | `Result[(), RefusalError]`; checks cancellation, the response deadline and any earlier buffer failure without allocating |

Body length never exceeds the smaller of `max_bytes` and
`Config.limits.max_body`. Limits count bytes, including for HEAD requests.
Zero permits only an empty body. A limit failure returns `BodyTooLarge`;
allocation refusal returns `Allocation(AllocError)`. Invalid, empty, oversized
or control-containing content types return `InvalidContentType`.
A failed append or content-type update poisons the response: ignoring its
error cannot publish a partial page. A later check returns `Broken`.

Each operation checks the active-request cancellation token. `check()` and
`check_at(now)` also check the existing `response_timeout`, returning
`Cancelled` or `TimedOut`. Call `check()` during
long CPU work too. Rendering is cooperative; arbitrary callback computation
cannot be forcibly stopped. The server checks again after a successful callback,
so returning `Ok(())` after cancellation or deadline expiry cannot send a page.
The usual shutdown drain period still applies before active cancellation.

The callback's own temporary allocations are application code; the byte cap
bounds the server-owned output bytes; buffer capacity may grow geometrically. Propagate errors with `?`. Returning
`Err`, exceeding a limit or failing the final check closes the connection
without writing any custom response bytes. These failures increment
`handler_errors`, except cancellation and expiry, which increment `cancelled`
and `timeouts`. Every rejected capture increments `protocol_errors`.

## Response framing

After successful rendering, the server supplies status `400`, Content-Type,
exact Content-Length, [Date](http-server.md#response-dates) and Connection:
close. The renderer cannot add headers, change status, read a request body,
send `100 Continue`, reuse the connection or dispatch a typed route. HEAD
sends the same length and headers with no body. The response uses the normal
transport write deadlines and cancellation. Errors from response preparation
or writing retain their cause as `RefusalError.Http(HttpError)`.

The connection closes after the custom response; unread or pipelined data is
never interpreted as another request. Other refusal classes and the actor HTTP
server are outside this API.
