# `std.http.typed`

HTTP head parsing, writing and body framing over
[`std.io.typed.Reader[E]` and `Writer[E]`](io-typed.md) preserve the
transport cause in `FramingError[E]`, with no stored-error retrieval API.
Use `request_body` or `response_body` with a parsed HTTP head to validate
framing, select the decoder, enforce body limits and recover a completed
persistent connection. The lower-level adapters also remain available for
callers that have already validated Content-Length and method/status semantics.

## Reading and writing heads

`read_request(reader, limits)` and `read_response(reader, limits)` return
`Result[Option[RequestHead or ResponseHead], Partial[FramingError[E]]]`.
They share the legacy parser's grammar, UTF-8 validation and `Limits` policy.
`Nothing` means EOF before a message; request parsing permits leading blank
lines within the head budget. Response parsing returns each interim response
separately. Framing fields and request routing remain separate validation steps.

On failure, `completed` counts bytes consumed for this head, including CRLF and
leading request blank lines. It excludes prefetched bytes still in a buffered
reader. Transport errors remain `Transport(E)`; malformed syntax and limits
are `Protocol(HttpError)`, and an incomplete head is `Truncated`.
Explicit interruption retries without losing the partially read line. Other
errors terminate this parse; the count is diagnostic information, not a token
for resuming a partially parsed head.

These functions request one byte at a time and never consume body bytes. For
sockets, use `std.io.typed.buffered_reader` so those requests are served from a
buffer; its `ReadFailure[E]` envelope preserves the original transport payload.
Pass the same buffered reader to the body constructor to retain read-ahead.

`write_request(writer, head)`, `write_response(writer, head)`,
`write_chunk(writer, source, start, stop)` and
`write_last_chunk(writer, trailers)` return
`Result[(), Partial[FramingError[E]]]`. Head/trailer serialization shares
[`std.http`](http.md)'s validation and completes before any write. Chunk
payload is written directly from the source without copying it. `completed`
counts wire bytes delivered by that operation, including chunk metadata.
Transport errors and invalid progress remain distinct; a zero write is
`Protocol(HttpError.Stream(StreamError.WriteZero))`. Partial output must not
be blindly replayed. Empty chunks write nothing; `write_last_chunk` ends a
chunked body.

## From a parsed head

`request_body(inner, head, limits)` and
`response_body(inner, head, method, request_keep_alive, limits)` return
`Result[Body[E, R], FramingError[E]]`, where `R` satisfies `Reader[E]`.
The transport must be positioned immediately after the head. Pass the **whole
buffered reader**, including any prefetched body or next-message bytes; taking
its inner stream would lose those bytes.

These constructors call [`request_framing` or `response_framing`](http.md),
respectively. Duplicate/conflicting framing, unsupported transfer coding and
invalid lengths remain `Protocol(HttpError)` failures, including on HEAD and
bodyless responses. HEAD, 1xx, 204, 304 and successful CONNECT semantics select
an empty body. Requests without declared framing are empty; responses without
declared framing are close-delimited. The constructors accept **parsed heads**:
they do not parse bytes, revalidate arbitrary hand-built head syntax, validate
request routing/Host, or consume interim responses on the caller's behalf.

Every mode checks `Limits` at construction. A declared payload length above
`max_body` fails before any read. Chunked and close-delimited payloads are
bounded while reading. At exactly the close-delimited limit, the next nonempty
read probes one byte to distinguish EOF from an oversized body; that probe
is not delivered or counted, and an oversized body becomes broken. Empty reads
never probe. HEAD/bodyless responses do not spend a body budget on their
declared representation length.

`Body.read` follows the same typed reader contract as the low-level adapters.
`total()` counts payload bytes returned successfully; `is_done()` is true only
after successful completion. `trailers()` borrows the chunked trailer list
(empty for other modes); it is complete only once `is_done()` is true.
Interruption preserves state; failures retain concrete transport causes and
previously delivered progress through the typed stream helpers. Invalid ranges
leave a healthy body readable. Other source/framing failures break it.

`body.finish()` returns `Result[R, FramingError[E]]`. It returns the complete
transport, with buffered bytes preserved, only after a persistent fixed-length,
chunked or empty body completes. An unfinished body reports
`Protocol(HttpError.Unfinished)` and remains readable. A failed or previously
finished body reports `Broken`. Successful extraction happens exactly once;
subsequent reads report `Broken` and `is_done()` becomes false.

`NotReusable` means completion did not permit another HTTP message: the
connection is close-delimited, has a closing Connection policy, is HTTP/1.0
without keep-alive, is a 101 upgrade or is a successful CONNECT tunnel.
For responses, pass the originating request's keep-alive policy as
`request_keep_alive`; both sides must allow persistence. This is message-boundary
recovery, not pooling: after an interim response, the next message is still
the final response; after reading a request, the server must also honor its
response's closing policy. Upgraded/tunnel streams are deliberately not exposed
by this API. Rejected `finish` calls retain ownership; dropping the body closes
or drops its transport, including on incomplete/error paths. Constructors take
ownership too, so a construction failure drops the supplied transport.

## Low-level framing

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
`InvalidProgress`, `OutOfRange`, `Broken`, `NotReusable`, and `Protocol(HttpError)`. The latter
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

The adapters own their transports. Connection pooling, TLS setup and
cancellation polling belong to [`std.http.client`](http-client.md), which uses
these typed head and body paths, including CONNECT and streamed uploads.
Cancellation delivered as a typed transport cause survives intact. The
client's legacy `Response.read`/`take_failure` bridge remains for compatibility;
its internal transport and framing paths do not retrieve stored errors.
The low-level adapters do not expose
their transports; use the parsed-head constructors and `Body.finish` for
guarded connection recovery. Bridging a legacy head reader preserves only the
error type that the legacy reader itself returns.
