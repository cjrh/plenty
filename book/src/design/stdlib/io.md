# std.io

`import std.io` provides byte streams with explicit partial progress: the
stream contracts, helpers that retry until a range is complete, fixed-capacity
buffering, and adapters for files, the process streams, and memory. Data moves
through [`std.bytes.Bytes`](bytes.md) ranges; no operation creates a `list[u8]`.

The module is embedded Plenty code over five native transport calls. It is
available on the supported x86-64 Linux GNU target.

## Stream contracts

A stream is any `pub` class whose `pub` methods match one of these protocols.
Calls are resolved for each concrete class at compile time; there is no dynamic
dispatch and no stream object type.

| Protocol | Method | Result |
|---|---|---|
| `Reader` | `read(self: &mut, buffer: &mut Bytes, start: i64, stop: i64)` | `Result[i64, StreamError]` |
| `Writer` | `write(self: &mut, source: &Bytes, start: i64, stop: i64)` | `Result[i64, StreamError]` |
| `Writer` | `flush(self: &mut)` | `Result[(), StreamError]` |
| `Seeker` | `seek(self: &mut, target: SeekFrom)` | `Result[i64, StreamError]` |
| `Closer` | `close(self: &mut)` | `Result[(), StreamError]` |

One call makes at most one attempt and returns the number of bytes it moved.

- `read` fills a prefix of `buffer[start:stop]` and never changes the buffer's
  length. A count below the range length is a short read, not an error. Zero
  for a nonempty range means end of input.
- `write` consumes a prefix of `source[start:stop]`. A short count is normal.
  Zero for a nonempty range means the stream accepted nothing; `write_all`
  reports that as `WriteZero` instead of looping.
- An empty range returns `Ok(0)` and says nothing about end of input.
- An error means the call moved no bytes. A stream that fails after moving
  some bytes returns the count and reports the failure on the next call.
- `Interrupted` means nothing moved and the same call can be repeated.
- `seek` returns the new byte offset from the start.

A range outside its buffer is `OutOfRange`. A stream that returns a negative
count or more than it was offered is reported as `InvalidProgress` by every
helper and buffer, which then stop using that count.

A helper names the one protocol it needs, such as `[R: Reader]`. Protocols
cannot be combined in one bound, so `close_buffered` requires `Closer` and
relies on the buffer's own use of `write` and `flush`.

## Errors

| `StreamError` | Meaning |
|---|---|
| `Interrupted` | The native call was interrupted (`EINTR`); retry |
| `UnexpectedEof` | Input ended before `read_exact` filled its range |
| `WriteZero` | A writer accepted no bytes of a nonempty range |
| `OutOfRange` | A range, capacity, limit, or seek target is invalid |
| `InvalidProgress` | A stream reported an impossible byte count |
| `Io(IoError)` | Any other native failure, including `Closed`, `NotReadable`, `NotWritable`, and `System(code)` |
| `Allocation(AllocError)` | A buffer could not be allocated or grown |

`std.io.classify(error: IoError)` performs this mapping for code that wraps
another native operation.

Helpers that take several steps keep the progress made before a failure:

| Type | Contents |
|---|---|
| `Partial` | `completed: i64` bytes moved, then `cause: StreamError` |
| `CopyError.Read(count, cause)` | The reader failed after `count` bytes reached the writer |
| `CopyError.Write(count, cause)` | The writer failed after accepting `count` bytes |
| `CloseError.Flush(cause)` | Flushing failed; the stream was closed |
| `CloseError.Close(cause)` | Flushing succeeded; closing failed |
| `CloseError.FlushAndClose(flush, close)` | Both failed |

These types do not convert to each other. Match the result, or use `?` in a
function returning `Failure`.

## Helpers

| Function | Result | Contract |
|---|---|---|
| `read_exact(reader, buffer, start, stop)` | `Result[(), Partial]` | Fill the whole range. End of input first is `UnexpectedEof` |
| `write_all(writer, source, start, stop)` | `Result[(), Partial]` | Deliver the whole range. No progress is `WriteZero`. Does not flush |
| `read_up_to(reader, out, limit)` | `Result[i64, Partial]` | Append at most `limit` bytes to `out`; fewer only at end of input |
| `copy_all(reader, writer, scratch)` | `Result[i64, CopyError]` | Move bytes until end of input through the whole of `scratch`. Does not flush |

All four retry `Interrupted` and continue after short progress. They stop at
the first other error and return the bytes moved so far. After a failed
`read_exact`, `buffer[start:start + completed]` holds the bytes read. After a
failed `read_up_to`, `out` has grown by exactly `completed` bytes.

`read_exact`, `write_all`, and `copy_all` do not allocate. `copy_all` needs a
nonempty `scratch`, otherwise `CopyError.Read(0, OutOfRange)`; its length is the
transfer size. `read_up_to` grows `out` by at most 8192 bytes ahead of the data
it has received and removes unused bytes before returning, so memory follows
the input actually read, not `limit`.

`copy` is not used as a name because the builtin `copy` already exists.

## Bounded buffering

| Function | Result |
|---|---|
| `buffered_reader(inner: R, capacity: i64)` | `Result[BufReader[R], StreamError]` |
| `buffered_writer(inner: W, capacity: i64)` | `Result[BufWriter[W], StreamError]` |

Each takes ownership of `inner` and allocates one buffer of exactly `capacity`
bytes. The buffer never grows. A capacity below one is `OutOfRange`.

| `BufReader[R]` | Result | Contract |
|---|---|---|
| `read(buffer, start, stop)` | `Result[i64, StreamError]` | Serve held bytes, otherwise make one inner read |
| `read_until(delimiter, out, limit)` | `Result[i64, StreamError]` | Append through the first `delimiter`, at most `limit` bytes |
| `buffered()` | `i64` | Bytes held and not yet consumed |
| `capacity()` | `i64` | The fixed buffer size |

`read` makes at most one inner read per call. A request at least as large as
the capacity, made while the buffer is empty, goes straight to the inner
stream. `read_until` returns the count appended: zero is end of input, and a
count whose last byte is not the delimiter means the limit or end of input came
first. It retries `Interrupted`. `limit` below one is `OutOfRange`. After an
error, bytes already appended stay in `out` and bytes not yet appended stay in
the buffer.

| `BufWriter[W]` | Result | Contract |
|---|---|---|
| `write(source, start, stop)` | `Result[i64, StreamError]` | Hold the range, delivering held bytes first when it does not fit |
| `flush()` | `Result[(), StreamError]` | Deliver every held byte, then flush the inner stream |
| `pending()` | `i64` | Bytes held and not yet delivered |
| `capacity()` | `i64` | The fixed buffer size |

Held bytes never exceed the capacity and reach the inner stream in order. A
range at least as large as the capacity is passed to the inner stream directly
after the held bytes, and may be accepted short. Delivery retries `Interrupted`
and short writes. When delivery fails, the undelivered bytes stay held and the
range passed to `write` is not accepted; a later `flush` resumes at the first
undelivered byte.

Nothing is written when a `BufWriter` is dropped. Call `flush` or
`close_buffered`; otherwise held bytes are discarded.

| Function | Result | Contract |
|---|---|---|
| `seek_reader(reader, target)` | `Result[i64, StreamError]` | Discard held input and seek. `Current` is relative to the bytes consumed |
| `seek_writer(writer, target)` | `Result[i64, StreamError]` | Deliver held bytes, then seek |
| `close_buffered(writer)` | `Result[(), CloseError]` | Flush, then close the inner stream even if the flush failed |

Both classes expose the wrapped stream as the public field `inner`, for calls
such as `reader.inner.close()`. Reading or writing `inner` directly bypasses
the buffer. The language cannot move a field out of a consumed value, so there
is no method returning the inner stream; exchange it with
`replace(buffer.inner, other)`.

## Adapters

| Constructor | Stream | Protocols |
|---|---|---|
| `open_file(path: &str, mode: &str)` | `Result[FileStream, StreamError]` | `Reader`, `Writer`, `Seeker`, `Closer` |
| `from_file(file: File)` | `FileStream` | the same |
| `stdin()` | `Stdin` | `Reader` |
| `stdout()`, `stderr()` | `Stdout`, `Stderr` | `Writer` |
| `cursor(data: Bytes)` | `Cursor` | `Reader`, `Writer`, `Seeker` |

`FileStream` owns a [`File`](../facilities/02-io/files.md), public as `file`.
`read` and `write` each make one native call on the descriptor, with no
newline translation or text decoding. `seek` positions by byte offset and
returns the new offset; this differs from the opaque text cookies of
`File.tell`. `SeekFrom.Start` with a negative offset is `OutOfRange`; another
target the system rejects is `Io(IoError.System(code))`. `flush` and `sync`
forward to the file. `close` reports the native close error once and is `Ok`
when repeated; other operations on a closed file are `Io(IoError.Closed)`.
The open mode is enforced: `Io(IoError.NotReadable)` or `Io(IoError.NotWritable)`.
Dropping an unclosed `FileStream` closes the file and discards any close error.

`Stdin`, `Stdout`, and `Stderr` hold no state and never close the process
descriptors. `Stdin.read` makes one native read. `Stdout` and `Stderr` share the
process buffers used by `print` and the text writers, so output keeps its order
across both; `flush` delivers it.

`Cursor` reads from and writes to an owned buffer, public as `data`, at
`position()`. Reads stop at the end of the buffer. Writes replace existing
bytes and extend the buffer at its end; writing past the end first fills the
gap with zeros. `seek` accepts any nonnegative position. A write can report
`Allocation`; it reserves before changing existing bytes.

## Ownership and threads

Every stream method takes `&mut self`, so a stream has one user at a time and a
buffer passed to `read` cannot be the stream's own storage. A read needs
`&mut Bytes`; a write borrows its source shared. A buffered adapter owns its
inner stream, and the inner stream cannot be used through another name while
the adapter exists. Sharing one stream between threads requires moving it or
placing it in a lock; the module adds no synchronization of its own. Writes to
`Stdout` and `Stderr` from several threads are each atomic with respect to the
process buffer, but their order is unspecified.

## Costs

Each `read` or `write` on `FileStream` or `Stdin` is one system call. Buffered
adapters make one bulk copy per call between the caller's range and the fixed
buffer. `read_until` searches held bytes with `Bytes.find`, one checked access
per byte. Seeking a `BufReader` discards up to `capacity` bytes already read.

| Allocates | Does not allocate |
|---|---|
| `buffered_reader`, `buffered_writer`: one buffer | Every `BufReader` and `BufWriter` method except `read_until` |
| `read_up_to` and `read_until`, when `out` lacks capacity | `read_exact`, `write_all`, `copy_all`, `seek_reader`, `seek_writer`, `close_buffered` |
| `Cursor.write` beyond the buffer's capacity | `FileStream`, `Stdin`, `Stdout`, `Stderr` operations |

## Limits

- Only x86-64 Linux GNU is supported. Nonblocking descriptors are not handled:
  `EAGAIN` arrives as `Io(IoError.System(11))`.
- Streams are bytes only. Text decoding, line iteration over streams, and
  formatted writes are not provided; use `read_until` and `Bytes.to_text`.
- There are no vectored operations, no read-ahead peeking, and no stream
  object that erases the concrete class.
- A class cannot declare which protocols its parameter satisfies. A wrong
  inner stream is rejected at `buffered_reader` or `buffered_writer` with a
  note naming the missing method, not with a protocol diagnostic.
- The native calls `__io_file_read`, `__io_file_write`, `__io_stdin_read`,
  `__io_std_write`, and `__io_file_seek` are private transport for the
  standard library, reached through `Bytes.read_file`, `write_file`,
  `read_stdin`, `write_stdout`, `write_stderr`, and `FileStream.seek`.
