# Keep stream errors and progress

Use `std.io.typed` when a transfer needs the original cause and the number of
bytes delivered before failure. Its copy error distinguishes reading from
writing. Existing `std.io` streams can enter this API through bridges:

```plenty
import std.http.typed

import std.bytes
import std.io
import std.io.typed
from std.http.typed import fixed_body
from std.io.typed import CopyFailure, copy_all

def main() -> Result[(), AllocError | IoError | std.http.typed.FramingError[std.io.StreamError] | std.bytes.BytesError]:
    mut bytes = std.bytes.with_capacity(3)?
    bytes.resize(3, 65u8)?
    mut scratch = std.bytes.with_capacity(2)?
    scratch.resize(2, 0u8)?
    mut body = fixed_body(std.io.typed.reader(std.io.cursor(bytes)), 3)?
    mut output = std.io.typed.writer(std.io.cursor(std.bytes.with_capacity(3)?))
    match copy_all(&mut body, &mut output, &mut scratch):
        case Ok(count):
            print(count)?
        case Err(CopyFailure.Read(count, cause)):
            print("read failed after")?
            print(count)?
            drop(cause)
        case Err(CopyFailure.Write(count, cause)):
            print("write failed after")?
            print(count)?
            drop(cause)
    cursor = std.io.typed.unwrap_writer(output)
    print(cursor.position())?
    Ok(())
```

```output
3
3
```

The fixed body assumes an already validated Content-Length; it does not parse
HTTP. `close_body` instead finishes at EOF and requires a connection that will
never be reused. The copy helper retries explicit interruption and preserves
other transport causes. A write failure can leave unwritten data from the last
chunk in scratch; the delivered count alone is not a resume token.

For an HTTP message, use typed head parsing followed by `request_body` or
`response_body`. They check framing fields and select the body decoder with
explicit limits. This in-memory response uses chunked framing. Keep the entire
buffered reader when handing it to the body, so prefetched bytes are preserved:

```plenty
import std.bytes
import std.http
import std.io
import std.io.typed
from std.http.typed import FramingError, read_response, response_body
from std.io.typed import CopyFailure, Partial, ReadFailure, copy_all

type TransportCause = ReadFailure[std.io.StreamError]
type HttpCause = FramingError[TransportCause]

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.io.StreamError | std.http.HttpError | Partial[HttpCause] | HttpCause | CopyFailure[HttpCause, std.io.StreamError]]:
    wire = std.bytes.from_text("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nABC\r\n0\r\nX-Tag: sample\r\n\r\n")?
    limits = std.http.Limits(64, 128, 4, 1024)
    mut conn = std.io.typed.buffered_reader(std.io.typed.reader(std.io.cursor(wire)), 128)?
    head = read_response(&mut conn, &limits)?.ok_or(std.http.HttpError.Truncated)?
    # Our GET request allows keep-alive; the response must also allow it.
    mut body = response_body(conn, &head, "GET", True, &limits)?
    mut output = std.io.typed.writer(std.io.cursor(std.bytes.with_capacity(3)?))
    mut scratch = std.bytes.with_capacity(2)?
    scratch.resize(2, 0u8)?
    print(copy_all(&mut body, &mut output, &mut scratch)?)?
    print(body.is_done(), body.trailers()[0].value)?
    # finish checks completion and persistence before handing back ownership.
    recovered = body.finish()?
    drop(recovered)
    Ok(())
```

```output
3
True sample
```

Only decoded payload contributes to the copy count. The decoder keeps partial
framing state across interruption and stops after the trailers; it does not
parse an HTTP head. The parsed-head constructor validates framing; `finish`
checks whether this completed message permits recovering its connection.
Dropping an unfinished or failed body drops its connection. Close-delimited,
closing, upgraded and successful CONNECT connections cannot be recovered by
this API. The [reference](../../design/stdlib/http-typed.md) describes the
remaining caller responsibilities and the lower-level `chunked_body` adapter.

For real requests, use [`std.http.client`](../../design/stdlib/http-client.md),
which uses these typed paths and also handles deadlines, connection ownership
and reuse. `typed_response(response)` adapts its response to the typed reader
protocol while preserving `ClientError`; unwrap it before `client.finish`.
`response.read_all_progress` is convenient when collecting a body and retaining
both its original error and the number of bytes appended before failure.

See the [typed streams reference](../../design/stdlib/io-typed.md) for nested
error payloads and the [framing reference](../../design/stdlib/http-typed.md)
for terminal-state rules.
