# std.fs — directory enumeration

Import `std.fs` and pass a borrowed [Path](path.md).
For owned temporary resources, copy/move/remove and bounded glob filtering,
see [rooted helpers](fs/helpers.md). The home and XDG base directories are in
[`std.fs.dirs`](fs/dirs.md). For change notifications, see
[`std.fs.watch`](fs/watch.md).
`read_dir(&path) -> Result[ReadDir, IoError]` opens one directory immediately.
`ReadDir` is affine and owns a lazy native stream. `next()` requires a mutable
owner and returns `Result[Option[DirEntry], IoError]`: `Some` is one entry,
`Nothing` is EOF or an already closed stream, and `Err` is an advancement failure
after opening. Any advancement error, including allocation failure, terminates
and closes the stream. Exhaustion also closes it before returning EOF.

`close() -> Result[(), IoError]` explicitly relinquishes the stream; repeated
close is harmless, including after a close error. `closed() -> bool` observes
the state. Drop closes any remaining stream without allocating or reporting a
close error. Scope exit, return, `?`, and dropping a suspended consuming generator
release their owned stream normally.

`entries(read_dir(&root)?)` consumes the owner and returns
`Generator[Result[DirEntry, IoError]]`. Handle each item's `Result` explicitly.
The existing generator/iteration system supplies deterministic cleanup on an
early `break`; no separate fallible `for` protocol is introduced. Breaking out
of a manual `while` leaves its named ReadDir alive until close or scope exit.

Only structural `.` and `..` are excluded. Hidden names remain visible, order
is unspecified native order, and enumeration is not a snapshot. Sorting and
ignore rules belong to callers. Additions/removals during enumeration can affect
which entries appear.

| `DirEntry` operation | Result |
| --- | --- |
| `file_name()` | Fallible detached Path containing the exact native name |
| `path()` | Fallible detached Path joined to the supplied root |
| `file_type()` | `Result[FileType, IoError]`, without following a link |
| `metadata()` | Fresh `Result[Metadata, IoError]`, without following a link |

Entries own copied path bytes and cached native type information; they never
borrow libc's reused `readdir` buffer or retain a directory descriptor. Keeping
entries in a list survives advancing, closing, or dropping the stream.
Name/path copies can fail without invalidating the entry. Relative roots remain
relative, and the joined path is not canonicalized.

Type-only queries use `d_type` when available. An unknown native type triggers a
non-following metadata lookup and propagates any lookup error. `FileType` is
`File`, `Directory`, `Symlink`, or `Other`; sockets, FIFOs and devices are Other,
never regular files. Cached native types can become stale; entry metadata is
fresh on every call. See [metadata and links](fs/metadata.md).

Python's `os.scandir` and Rust's `read_dir`/`DirEntry` provide the API baseline.
The runtime follows their Unix errno-reset/EOF distinction but copies detached
entry data fallibly instead of adopting Rust DirEntry's descriptor retention.
Native enumeration currently supports 64-bit Linux; unsupported hosts return
`IoError.Unsupported`. See [walking](fs/walking.md) for tree traversal.
