# Text stream positions

`truncate(size: i64) -> Result[i64, IoError]` changes a writable file's length
in **bytes**, returning that length without changing the cursor or allocating.
Omitting size uses the current physical byte position (not a text cookie).
Negative sizes fail before mutation. Extending regular Linux files adds zero
bytes; shrinking may split UTF-8, which a later read reports as invalid data.
This operation requires exclusive access, may fail at the OS, and does not sync.

`tell() -> Result[u64, IoError]` returns an opaque text-position cookie;
`seek(cookie: u64) -> Result[(), IoError]` restores it. Both require exclusive
access, allocate nothing, and report native seek errors (including nonseekable
streams). Zero rewinds. Other values must come from `tell()` on the same stream
with unchanged contents; arithmetic and persistence of cookies are unsupported.
Cookies retain pending CRLF state, not just the physical offset. The private
encoding uses a byte offset and a decoder-state bit. A failed seek leaves decoder
state unchanged. Arbitrary cookies cannot violate memory safety but may produce
invalid text or surprising positions. Relative/end seeks remain deferred.
