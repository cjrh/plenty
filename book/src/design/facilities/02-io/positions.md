# Positions and truncation

`file.tell() -> Result[u64, IoError]` returns an opaque text-position cookie.
`file.seek(cookie: u64) -> Result[(), IoError]` restores it. Both require
exclusive access, allocate nothing, and report native seek errors, including
nonseekable streams.

Zero rewinds. Other cookies must come from `tell()` on the same stream with
unchanged contents. They preserve pending CRLF translation state, rather than
representing a plain byte offset. Arithmetic and persistence of cookies are
unsupported. A failed seek leaves decoder state unchanged; arbitrary cookies
remain memory-safe but can yield invalid text or surprising positions. Relative
and end-relative seeking are unsupported.

`file.truncate(size: i64) -> Result[i64, IoError]` changes a writable file's
length in **bytes**, returns that length, and leaves the cursor unchanged.
Omitting size uses the current physical byte position, not a text cookie.
Negative sizes fail before mutation. The operation requires exclusive access,
allocates nothing, can fail at the OS, and does not synchronize storage.

Extending regular Linux files adds zero bytes. Shrinking can split UTF-8, which
later reading reports as invalid data.
