# `std.http`

`import std.http` reads and writes HTTP/1.1 messages (RFC 9112) over
[`std.io`](io.md) streams: request and response heads, `Content-Length`,
chunked and close-delimited bodies, and the connection reuse rules. It is the
framing layer only. Sockets, deadlines, routing, clients, servers and TLS
belong to the caller or to later work in the [backlog](../../backlog.md).

The module is checked Plenty in `stdlib/std/http.plenty`; it adds no runtime
primitives. The same code runs over a [`std.net`](net.md) `TcpStream`, a
`Cursor` in a test, or any other `Reader` and `Writer`.

## Messages

| Item | Meaning |
|---|---|
| `Header(name: str, value: str)` | One field line |
| `RequestHead(method, target, minor: i64, headers: list[Header])` | `minor` is the HTTP/1.x minor version, 0 or 1 |
| `ResponseHead(minor, status: i64, reason: str, headers)` | |
| `request(method, target)`, `response(status, reason)` | A head with HTTP/1.1 and no fields |
| `head.add(name, value)` | `Result[(), HttpError]`: append a field |
| `find(headers: &list[Header], name: &str)` | `Option[i64]`: index of the first field named `name`, ignoring ASCII case |

Parsed names keep the case they arrived in. Values have surrounding spaces
and tabs removed. Fields stay in arrival order and repeated names stay
separate; nothing is combined.

## Limits

`Limits(max_line, max_head, max_headers, max_body)` bounds every read:

| Field | Bounds | `limits()` |
|---|---|---|
| `max_line` | One request, status, field or chunk-size line, including its CRLF | 8192 |
| `max_head` | All bytes of a head, blank lines before a request line included; also a trailer section | 65536 |
| `max_headers` | Field lines in a head or trailer section | 100 |
| `max_body` | Decoded body bytes | 1048576 |

`max_line` and `max_head` must be at least 2 and the others at least 0, or
the call fails with `InvalidLimit`. Memory use is one line buffer of at most
`max_line` bytes plus the parsed fields; the body is never held unless the
caller collects it.

## Reading heads

| Function | Result |
|---|---|
| `read_request(conn: &mut BufReader[R], limits)` | `Result[Option[RequestHead], HttpError]` |
| `read_response(conn: &mut BufReader[R], limits)` | `Result[Option[ResponseHead], HttpError]` |

`Nothing` means the input ended before the message's first byte, as when the
peer closes an idle connection; an end inside a head is `Truncated`. Blank
lines before a request line are skipped within `max_head`. A 1xx response is
returned like any other; read again for the final response.

Parsing is strict. Every line ends in CRLF; a bare LF is `BareLineFeed`, and a
CR inside a line is invalid. A request line is a token method, one space, a
target of visible ASCII, one space, and `HTTP/1.0` or `HTTP/1.1`. A status
line is the version, a space, three digits from 100, and an optional space
and reason; `HTTP/1.1 200` with no reason is accepted. A well-formed other
version such as `HTTP/2.0` is `UnsupportedVersion`. A field line is a token
name, a colon with no space before it, and a value of visible bytes, spaces
and tabs that is valid UTF-8. A line starting with a space or tab (an obsolete
line fold) is `InvalidHeader`.

## Framing

`Framing` is `Empty`, `Length(i64)`, `Chunked`, or `UntilClose`.

| Function | Result |
|---|---|
| `request_framing(head: &RequestHead)` | `Result[Framing, HttpError]`; no framing field is `Empty` |
| `response_framing(head: &ResponseHead, method: &str)` | `Result[Framing, HttpError]`; no framing field is `UntilClose` |
| `request_keeps_alive(head)` | HTTP/1.1 unless `Connection: close`; HTTP/1.0 only with `Connection: keep-alive` |
| `response_keeps_alive(head, framing: &Framing)` | As for requests, and never after an `UntilClose` body |

`method` is the method of the request a response answers. Responses to
`HEAD`, 1xx, 204 and 304 responses, and 2xx responses to `CONNECT` are `Empty`
whatever their fields say.

Framing that two implementations could read differently is an error, never a
guess, because that disagreement is how request smuggling works:

| Fields | Error |
|---|---|
| `Content-Length` and `Transfer-Encoding` together | `ConflictingFraming` |
| Either field twice, even with equal values | `DuplicateFraming` |
| `Content-Length` other than decimal digits (`+5`, `1,1`, empty), or above `i64` | `InvalidContentLength` |
| `Transfer-Encoding` other than exactly `chunked`, or in HTTP/1.0 | `UnsupportedTransferEncoding` |

The framing functions check both fields even where the status decides the
framing, so a malformed response is rejected consistently.

## Reading bodies

| Function or method | Result |
|---|---|
| `body_reader(conn: BufReader[R], framing: &Framing, limits)` | `Result[BodyReader[R], HttpError]`; takes the connection |
| `body.read_body(buffer, start, stop)` | `Result[i64, HttpError]`: decoded bytes; 0 is the end of the body |
| `body.read(buffer, start, stop)` | The `std.io` `Reader` method |
| `read_to_end(body, out: &mut Bytes)` | `Result[i64, HttpError]`: append the rest |
| `discard(body)` | `Result[i64, HttpError]`: read and drop the rest |
| `finish(body: &mut BodyReader[R])` | `Result[BufReader[R], HttpError]`: the connection, at the next message |
| `release(body)` | `Option[BufReader[R]]`: the connection in any state |
| `body.trailers` | Trailer fields, once a chunked body has ended |
| `body.total()`, `body.is_done()` | Bytes delivered; whether the end was reached |
| `body.take_failure()` | `Option[HttpError]`: the framing error behind a `read` failure |

The body reader owns the connection, so nothing can read the next message
while a body is in progress. `finish` returns it only after the body's end,
including buffered bytes of pipelined messages; a body with bytes left is
`Unfinished` and a failed one is `Broken`. `release` returns the connection
regardless, for writing a final error response or closing; its read position
is then unknown, so it must not carry another message.

A `Length` body larger than `max_body` fails before reading. A chunked body
fails as soon as a chunk size would pass `max_body`, and an `UntilClose` body
as soon as a byte arrives past it. Chunk sizes are hexadecimal without signs
or leading spaces. Chunk extensions after `;` are checked for control bytes
and ignored. Each chunk's data must be followed by CRLF.

Through `read`, a body is an ordinary `Reader` for `std.io.copy_all` and
similar helpers. Stream and allocation errors keep their `StreamError`;
`Truncated` becomes `UnexpectedEof`; any other framing error becomes
`Io(IoError.InvalidInput)` and is kept for `take_failure`. After any error
the body reports only `Broken`.

## Writing

| Function | Result |
|---|---|
| `write_request(writer: &mut W, head)` | `Result[(), HttpError]` |
| `write_response(writer, head)` | `Result[(), HttpError]` |
| `write_chunk(writer, source: &Bytes, start, stop)` | One chunk; an empty range writes nothing |
| `write_last_chunk(writer, trailers: &list[Header])` | The zero-size chunk and trailer section |
| `write_body(writer, framing, source, start, stop)` | A whole body under `framing`; `Length` and `Empty` must match the range |

Writers validate before writing and write nothing when validation fails:
token methods and names, targets of visible ASCII, reasons and values without
CR, LF or other control bytes, values without surrounding blanks, status 100–999,
and the framing rules above. A value cannot add a field of its own. A stream
failure may leave part of a message delivered. Heads are written with one
`write_all`; a chunk with three. The caller flushes.

## Errors

| Variant | Cause |
|---|---|
| `Stream(StreamError)` | The underlying stream failed, or a range was out of bounds |
| `Allocation(AllocError)` | Memory for a line, field, or output ran out |
| `InvalidLimit` | A `Limits` field out of range |
| `Truncated` | Input ended inside a head, a body, or a chunk |
| `LineTooLong`, `HeadTooLarge`, `TooManyHeaders`, `BodyTooLarge` | A limit was reached |
| `BareLineFeed` | A line ended in LF without CR |
| `InvalidRequestLine`, `InvalidStatusLine`, `InvalidHeader` | Syntax outside the grammar above |
| `UnsupportedVersion` | Not HTTP/1.0 or HTTP/1.1 |
| `InvalidContentLength`, `DuplicateFraming`, `ConflictingFraming`, `UnsupportedTransferEncoding` | See [Framing](#framing) |
| `InvalidChunk` | A chunk-size line or chunk terminator outside the grammar |
| `LengthMismatch` | `write_body` given a range that does not match the framing |
| `Unfinished`, `Broken` | `finish` before the end, or after a failure |

## Costs, ownership and threads

- Reading a head is linear in its bytes. It allocates one line buffer, which
  grows to the longest line read (at most `max_line`), plus one `str` per
  method, target, reason, name and value, and the field list.
- A body reader allocates its line buffer and trailer list once. Body bytes
  are copied once, from the `BufReader` into the caller's buffer; nothing is
  retained. `read_to_end` grows `out` 4096 bytes at a time; `discard` uses one
  4096-byte scratch buffer.
- A writer builds each head or chunk-size line in one `Bytes` and writes it;
  chunk data is written from the caller's buffer without copying.
- Heads, `Limits` and `Framing` are ordinary owned values. A `BodyReader`
  owns its connection, so it moves to a worker with it when `R` can. Nothing
  is shared or global, and no call starts a thread or waits beyond the
  underlying stream's own calls.
- Supported targets are those of `std.io`: x86_64 Linux GNU.

## Example

The [tutorial lesson](../../tutorial/08-resources/30-http-messages.md) reads
pipelined requests from memory and writes a chunked response. The tests in
`tests/test_std_http.rs` serve several requests on one loopback connection.

## Limits

- HTTP/1.0 and HTTP/1.1 only. No HTTP/2, upgrades, or `100-continue`
  handling beyond returning the interim response.
- No client, server, routing, connection pool, redirects, proxies, cookies,
  or TLS. Deadlines are the caller's: use `std.net`'s `*_before` calls, or
  read with a deadline-aware `Reader`.
- Only the `chunked` transfer coding. A body with another coding is rejected,
  not decoded.
- Targets are returned as text, not parsed; use [`std.url`](url.md).
- Field values must be UTF-8; a peer that sends Latin-1 bytes is rejected.
