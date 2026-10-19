# Read and write HTTP messages

An HTTP/1.1 connection carries one message after another. Each message is a
head, which ends at a blank line, and an optional body. A reader must find
where each body ends, or it reads the next request as part of the last one.
`std.http` does that work over any `std.io` stream, so you can test it with
bytes in memory before any socket is involved.

```plenty
import std.bytes
import std.http
import std.io

def main() -> Result[(), Failure]:
    wire = "POST /notes HTTP/1.1\r\nHost: example.com\r\nContent-Length: 5\r\n\r\nhelloGET /notes HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"
    limits = std.http.limits()
    mut conn = std.io.buffered_reader(std.io.cursor(std.bytes.from_text(wire)?), 1024)?
    while True:
        match std.http.read_request(&mut conn, &limits)?:
            case Nothing:
                break
            case Some(head):
                framing = std.http.request_framing(&head)?
                mut body = std.http.body_reader(conn, &framing, &limits)?
                mut data = std.bytes.empty()?
                std.http.read_to_end(&mut body, &mut data)?
                print(head.method, head.target, str.repr(&framing)?, str.repr(data.to_text()?)?, std.http.request_keeps_alive(&head))?
                conn = std.http.finish(&mut body)?
    Ok(())
```
```output
POST /notes std.http.Framing.Length(5) "hello" True
GET /notes std.http.Framing.Empty "" False
```

`read_request` reads one head and returns `Nothing` when the input ends
between messages. `request_framing` says how the body is delimited: here by
`Content-Length`, then not at all. The body reader takes the connection, so
nothing else can read from it until `finish` hands it back, positioned at the
next request. `finish` fails if the body was not read to its end. Every read
is bounded by `limits()`: line length, head size, header count, and body size.

Writing goes the other way. A chunked body is sent in pieces whose total size
the writer need not know in advance:

```plenty
import std.bytes
import std.http
import std.io
from std.http import Header

def main() -> Result[(), Failure]:
    mut sink = std.io.cursor(std.bytes.empty()?)
    mut reply = std.http.response(200, "OK")?
    reply.add("Transfer-Encoding", "chunked")?
    std.http.write_response(&mut sink, &reply)?
    text = std.bytes.from_text("first second")?
    std.http.write_chunk(&mut sink, &text, 0, 6)?
    std.http.write_chunk(&mut sink, &text, 6, 12)?
    std.http.write_last_chunk(&mut sink, &list[Header].new()?)?
    print(str.repr(sink.data.to_text()?)?)?
    mut forged = std.http.response(200, "OK")?
    forged.add("Location", "/a\r\nSet-Cookie: x=1")?
    print(str.repr(std.http.write_response(&mut sink, &forged))?)?
    smuggled = "POST / HTTP/1.1\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\n\r\n"
    mut conn = std.io.buffered_reader(std.io.cursor(std.bytes.from_text(smuggled)?), 1024)?
    match std.http.read_request(&mut conn, &std.http.limits())?:
        case Some(head):
            print(str.repr(std.http.request_framing(&head))?)?
        case Nothing:
            print("connection ended before a request")?
    Ok(())
```
```output
"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nfirst \r\n6\r\nsecond\r\n0\r\n\r\n"
Result[(), std.http.HttpError].Err(std.http.HttpError.InvalidHeader)
Result[std.http.Framing, std.http.HttpError].Err(std.http.HttpError.ConflictingFraming)
```

The writer refuses a header value with a line break, which would let text
from a request add headers of its own. The reader refuses a message with both
`Content-Length` and `Transfer-Encoding`: two servers could disagree about
where its body ends, which is how request smuggling works. Neither guesses.

The same functions work over a `std.net` stream wrapped in a `BufReader`.
Deadlines stay with the socket calls. See
[the module reference](../../design/stdlib/http.md) for every error, response
framing rules, and the body reader's use as a `std.io` stream.
