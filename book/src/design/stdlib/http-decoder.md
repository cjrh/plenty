# `std.http.decoder`

A resumable HTTP/1.x request decoder with no I/O. Feed it whatever bytes are
available now, and retain the unconsumed suffix. It shares request-head,
target, framing, chunk-size and trailer validation with [`std.http`](http.md).
Its stored state contains owned bytes rather than shared strings, so it can
live inside an actor node.

`decoder(&limits)` returns `Result[Decoder, HttpError]`. The limits are copied;
invalid line/head sizes, header counts or body bounds return `InvalidLimit`.
Allocation failures are `HttpError.Allocation`. Header bytes and lines are
bounded by those limits; body chunks are at most `feed_budget` (4096) bytes.

```plenty
import std.http
import std.http.decoder
from std.http.decoder import Event

def main() -> Result[(), std.http.HttpError | IoError]:
    mut decoder = std.http.decoder.decoder(&std.http.limits())?
    wire = b"POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\nping"
    mut at = 0
    while not decoder.is_done():
        consumed, event = decoder.feed(&wire[at:])?
        at += consumed
        match event:
            case Some(Event.Head(head, _, _)):
                print(head.method)?
            case Some(Event.Body(body)):
                print(body.len())?
            case Some(Event.End):
                print("complete")?
            case _:
                pass
    decoder.finish()?
    Ok(())
```
```output
POST
4
complete
```

This example already owns the complete request, so it can loop until `End`.
A readiness loop must instead yield when no currently available input can
advance the decoder. Bound the number of feed calls per scheduler turn too;
the per-call byte budget alone does not bound an unbounded caller loop.

## Progress and events

`feed(source: &[u8])` reads a [`&[u8]` view](bytes.md#byte-views) and returns
`Result[(consumed, Option[Event]), HttpError]`. `consumed` counts from the
start of the view, not from the start of the caller's buffer. Advance the source cursor by exactly that
count. Neither a short count nor `Nothing` implies EOF.

| Event | Meaning |
|---|---|
| `Head(RequestHead, RequestTarget, Framing)` | Validated request metadata and body framing |
| `Body(Bytes)` | Owned decoded body bytes |
| `Trailers(list[Header])` | Validated trailer fields after a chunked body |
| `End` | Request complete; no pipelined bytes have been consumed |

An empty feed can emit a pending `End`. The returned head/trailer events own
text, but the decoder does not retain that text after the call. Callers that
keep events across actor callbacks must respect the actor's storage boundary.

`finish()` reports explicit input EOF. It succeeds after a complete body, or
before any nonblank request has begun, and reports truncated or malformed
input otherwise. If body completion is pending, collect its `End` with an
empty feed. `finish` does not make an incomplete request reusable.

## State and reuse

| Method | Contract |
|---|---|
| `is_started()` | Nonblank request input has arrived, even if the line is partial |
| `has_head()` | A valid head was emitted and the decoder has not failed |
| `is_done()` | `End` was emitted |
| `total()` | Decoded body bytes consumed for this request |
| `reset()` | Begin another request after `End`; otherwise `Unfinished` |

Reset only when the application is ready to process pipelined input. Framing
and allocation errors poison the decoder; discard it instead of resuming
with a suffix whose request boundary is no longer trustworthy. Invalid source
ranges report a stream range error. The decoder never reads a socket, waits,
or owns the caller's input buffer.
