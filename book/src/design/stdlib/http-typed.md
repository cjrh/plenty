# `std.http.typed`

Body framing over [`std.io.typed.Reader[E]`](io-typed.md) preserves the
transport cause in `FramingError[E]`, with no stored-error retrieval API.
These adapters assume the caller has already validated the HTTP head,
Content-Length, and method/status semantics.

| Constructor | Framing |
| --- | --- |
| `fixed_body(inner, length)` | `Result[FixedBody[E, R], FramingError[E]]`; rejects negative length |
| `close_body(inner)` | `CloseBody[E, R]`; reads until transport EOF |
| `chunked_body(inner, limits)` | `Result[ChunkedBody[E, R], FramingError[E]]`; bounded chunked decoding with [`std.http.Limits`](http.md) |

All readers expose `read(buffer, start, stop)` returning
`Result[i64, StepError[FramingError[E]]]`. A fixed body caps each read to
its remaining length and reports `Truncated` on premature EOF. It never
consumes bytes after the body. A close-delimited body treats EOF as successful
completion and remembers it, avoiding further transport reads. **A
close-delimited connection must never be reused.**

`FramingError` distinguishes `Transport(E)`, `Truncated`,
`InvalidProgress`, `OutOfRange`, `Broken`, and `Protocol(HttpError)`. The latter
retains chunk syntax, limit, trailer and framing-allocation errors using the
existing [`HttpError`](http.md) variants; transport errors always use
`Transport(E)`. Interruption is forwarded as
a retry signal without poisoning the body. Transport errors and invalid
progress make it broken; later reads report `Broken`. Truncation also breaks
a fixed or chunked body. Chunk syntax, limit and trailer failures also break
the chunked body. Invalid buffer ranges do not consume input or break a healthy
body. Valid empty reads return zero without reading or marking EOF.

## Chunked bodies

`chunked_body` requires a validated head selecting chunked transfer coding.
It checks `Limits` at construction (`max_line` and `max_head` at least two,
nonnegative `max_headers` and `max_body`). Chunk-size lines and trailer fields
include CRLF in their line limit. `max_head` bounds the entire trailer section,
including its final CRLF; `max_headers` bounds trailer fields, and `max_body`
bounds decoded payload. A chunk exceeding the remaining body budget is rejected
before its payload is read. Hex sizes are overflow-checked.

The decoder requires CRLF and uses the same extension and field policy as
`std.http`: extensions after `;` are ignored after rejecting control bytes
other than tab; field names must be tokens, surrounding spaces/tabs are removed
from values, and values must be valid UTF-8 without other control bytes or DEL.
Obsolete folded fields are rejected. Trailers are metadata, not a second head:
they do not change framing or replace validated head fields. Parsed fields are
available in `body.trailers`; treat them as complete only after `body.is_done()`.
`body.total()` counts decoded payload bytes returned to the caller.

Parsing resumes after `Interrupted` even within a size line, a CRLF, or a trailer.
A successful payload read returns immediately, before parsing subsequent
framing. Consequently `read_exact` and `copy_all` retain all previously returned
payload progress when later metadata or transport fails. Metadata bytes do not
count as delivered bytes. As with other readers, an unsuccessful read may have
modified scratch storage; only an `Ok(count)` identifies payload to use.

The terminal blank trailer line completes the body. Metadata reads request one
byte at a time, and payload reads are capped to the current chunk; the reader
does not request any bytes from the next message. Repeated completed reads
return zero without consulting the transport. Empty reads do not advance parsing
or imply completion. Construction reserves one line buffer of `max_line` bytes;
trailer strings and their list allocate fallibly while reading. This simple
decoder prioritizes bounded, resumable behavior; it has no internal read-ahead
buffer or throughput qualification yet.

The adapters own their transports. They do not provide connection pooling,
head parsing, TLS setup, cancellation polling, or a
replacement for [`std.http.client`](http-client.md). Cancellation delivered
as a typed transport cause survives intact. Existing HTTP side channels remain
in place until those paths are migrated. No adapter exposes its transport
for connection reuse.
