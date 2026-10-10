# Writing and synchronization

`file.write(text) -> Result[i64, IoError]` observes text once before exclusive
receiver access. It writes exact UTF-8 without newline insertion or translation,
returning the Unicode-scalar count. The file must be live and writable even for
empty text. No runtime write buffer is allocated; errors can leave partial output.

`file.writelines(lines: list[str]) -> Result[(), IoError]` observes a list or
shared list reference once before exclusive access. It writes each string's
exact bytes without separators, preserves the list, and allocates nothing after
argument construction. Empty lists still validate the file and capability.
Errors may leave a written prefix. General iterable sources are unsupported.

Update streams are unbuffered, so switching reads and writes requires no flush.
Writes use physical byte positions and can overwrite part of a UTF-8 sequence;
they do not insert text. Nonempty writes discard pending CRLF translation state.
Append modes use OS append semantics, but a logical write need not be one atomic
record against concurrent writers.

## Flush and sync

`file.flush() -> Result[(), IoError]` flushes runtime buffers. Files are currently
unbuffered, so it validates the open state and normally does no extra work.
`file.sync() -> Result[(), IoError]` requests OS synchronization of data and
metadata. Both require exclusive access and report errors without allocating.

Neither close nor flush substitutes for sync. Filesystem, device, and directory
entry durability rules still apply; sync is not an atomic-save operation.
