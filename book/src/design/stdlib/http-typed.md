# `std.http.typed`

Body framing over [`std.io.typed.Reader[E]`](io-typed.md) preserves the
transport cause in `FramingError[E]`, with no stored-error retrieval API.
These adapters assume the caller has already validated the HTTP head,
Content-Length, and method/status semantics.

| Constructor | Framing |
| --- | --- |
| `fixed_body(inner, length)` | `Result[FixedBody[E, R], FramingError[E]]`; rejects negative length |
| `close_body(inner)` | `CloseBody[E, R]`; reads until transport EOF |

Both readers expose `read(buffer, start, stop)` returning
`Result[i64, StepError[FramingError[E]]]`. A fixed body caps each read to
its remaining length and reports `Truncated` on premature EOF. It never
consumes bytes after the body. A close-delimited body treats EOF as successful
completion and remembers it, avoiding further transport reads. **A
close-delimited connection must never be reused.**

`FramingError` distinguishes `Transport(E)`, `Truncated`,
`InvalidProgress`, `OutOfRange`, and `Broken`. Interruption is forwarded as
a retry signal without poisoning the body. Transport errors and invalid
progress make it broken; later reads report `Broken`. Truncation also breaks
a fixed body. Invalid buffer ranges do not consume input or break a healthy
body. Valid empty reads return zero without reading or marking EOF.

The adapters own their transports. They do not provide connection pooling,
head parsing, chunked framing, trailers, TLS setup, cancellation polling, or a
replacement for [`std.http.client`](http-client.md). Cancellation delivered
as a typed transport cause survives intact. Existing HTTP side channels remain
in place until those paths are migrated. Neither adapter exposes its transport
for connection reuse.
