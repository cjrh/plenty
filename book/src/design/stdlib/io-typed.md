# `std.io.typed`

Typed byte streams preserve a caller-selected error payload without side
storage. They complement [`std.io`](io.md); existing streams keep their APIs.

`Reader[E].read(&mut self, &mut Bytes, start, stop)` and
`Writer[E].write(&mut self, &Bytes, start, stop)` return
`Result[i64, StepError[E]]`. The range is half-open. A successful call reports
the bytes moved; a failed call reports no progress from that attempt.
`StepError.Interrupted` requests retry. `Source(E)` preserves every other
cause, including cancellation and allocation failure. Reader `Ok(0)` is EOF
for a nonempty range. Writers need only `write`, not `flush`.

| Operation | Result |
| --- | --- |
| `read_exact(reader, buffer, start, stop)` | `Result[(), Partial[ReadFailure[E]]]` |
| `write_all(writer, buffer, start, stop)` | `Result[(), Partial[WriteFailure[E]]]` |
| `copy_all(reader, writer, scratch)` | `Result[i64, CopyFailure[E, G]]` |

Helpers retry only `Interrupted`. `Partial` carries `completed: i64` and
`cause`. `ReadFailure[E]` distinguishes `Source(E)`, `UnexpectedEof`,
`InvalidProgress`, and `OutOfRange`; `WriteFailure[E]` substitutes
`WriteZero` for EOF. Negative or oversized counts are invalid progress.
Invalid ranges and valid empty ranges do not call the underlying stream.

`CopyFailure.Read(completed, ReadFailure[E])` and
`CopyFailure.Write(completed, WriteFailure[G])` preserve the failing operation
and independent source/destination error types. Counts mean bytes delivered to
the writer, including a short write before failure. A read chunk may already
have been consumed when writing fails: this API does not promise resumable
copying or retain unwritten bytes. Empty scratch is an `OutOfRange` read
failure. Helpers allocate nothing and require caller-provided storage.

`buffered_reader(inner, capacity)` returns
`Result[BufReader[E, R], StreamError]` for a `Reader[E]`. Capacity must be
positive; construction allocates the buffer. Its `read` returns
`StepError[ReadFailure[E]]`, preserving a transport cause as
`Source(ReadFailure.Source(E))` and forwarding `Interrupted`. Invalid ranges
and empty reads do not call the transport. Invalid inner counts become
`ReadFailure.InvalidProgress`. Reads allocate nothing; `capacity()` and
`buffered()` report the buffer size and unread bytes.

The public `inner` field permits transport-specific operations, but consuming
it discards prefetched bytes. Keep the whole buffered reader when handing a
connection from head parsing to body decoding or to the next HTTP message.

`reader(inner)` wraps an existing `std.io.Reader` as
`LegacyReader[R]`; `writer(inner)` wraps a `std.io.Writer` as
`LegacyWriter[W]`. Their typed cause is `StreamError`. Only its
`Interrupted` variant becomes a retry signal; other variants survive intact.
`unwrap_reader(bridge)` and `unwrap_writer(bridge)` consume the bridge and
return the owned stream, allowing
explicit flushing, seeking or closing. Copying does not flush or close.
These bridges cannot recover causes already erased by a legacy adapter:
For HTTP client responses, prefer `std.http.client.typed_response(response)`
over bridging `Response.read`: its typed cause is the complete `ClientError`.
The legacy `Response.read`/`take_failure` pair remains a compatibility API.

See [typed HTTP framing](http-typed.md) and the
[tutorial](../../tutorial/03-absence/typed-streams.md).
