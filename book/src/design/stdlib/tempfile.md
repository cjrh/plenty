# `std.tempfile`

Owned temporary files and directories, with automatic cleanup and explicit
fallible cleanup methods. Prefer this module for scratch storage; use
[`std.fs`](fs/temporary.md) for descriptor-rooted staging and atomic publication.

## Constructors

| Function | Result | Defaults |
|---|---|---|
| `default_dir()` | `Result[Path, IoError]` | First nonempty `TMPDIR`, `TEMP`, `TMP`; otherwise `/tmp` |
| `file(parent: &Path = default_dir()?)` | `Result[TemporaryFile, IoError]` | Unnamed file |
| `named_file(parent, prefix="plenty-", suffix="")` | `Result[NamedTemporaryFile, FsError]` | Parent is `default_dir()` |
| `directory(parent, prefix="plenty-", max_entries=100000, max_depth=64)` | `Result[TemporaryDirectory, FsError]` | Parent is `default_dir()` |

`parent` is a borrowed `Path`; prefix and suffix are borrowed strings. Explicit
parents bypass environment selection. Relative parents are resolved against the
current directory. Environment paths preserve native bytes. Selection is fresh
on every call, without caching or a trial file: an unusable configured directory
causes construction to fail, rather than silently falling back elsewhere.

Named resources use 16 random bytes encoded as 32 hexadecimal characters, between
the prefix and suffix. Prefix and suffix together may occupy at most 200 UTF-8
bytes and must contain neither `/` nor NUL. Directories accept only a prefix.
Creation retries up to 16 name collisions. Files are created with owner-only
permissions, directories with owner-only access (subject to the process umask).
No API returns an uncreated, supposedly unique filename.

## Files

`TemporaryFile.stream()` lends a mutable `std.io.FileStream`. Its context manager
also yields that stream. `close()` returns `Result[(), StreamError]`; dropping the
owner closes the descriptor. The file has no pathname.

`NamedTemporaryFile.path()` returns a borrowed absolute `Path`, usable by another
program while the owner is alive. The file supports `read`, `write`, `flush`,
`sync`, and `seek` with the same signatures as `FileStream`, and works with
[`std.io`](io.md) reader/writer helpers. Its context manager yields the named
owner itself. The underlying descriptor stays private for cleanup identity checks.

`close()` returns `Result[(), FsError]`: it removes the name, then closes the
stream. An already removed name is accepted. A different file at that name is
rejected; a failed removal leaves the owner armed for another cleanup attempt.
`keep()` returns `Result[Path, AllocError]` and disarms removal after successfully
copying the path. The stream remains open until closed or dropped. Call `keep()`
before cleanup; it does not recreate an already removed file.

## Directories

`TemporaryDirectory.path()` returns a borrowed absolute `Path`; `root()` borrows
its descriptor-rooted `std.fs.Root`. Its context manager yields the directory
owner. `cleanup()` returns `Result[(), FsError]` and recursively removes contents
and the directory. Successful cleanup is idempotent. `keep()` returns an owned
path and disarms cleanup, allocating the path before changing ownership policy.

Each cleanup attempt is bounded: `max_entries` must be positive and `max_depth` must be in
`1..=64`. The root directory counts toward both budgets. A limit or I/O failure
can leave a partially removed tree; cleanup remains armed for a later attempt.
Symlinks are removed without traversing their targets. Before traversal, the
parent entry must still identify the owned directory; otherwise cleanup returns
`FsError.Replaced` without traversing the replacement.

## Cleanup and platform limits

Context exits and destructors perform **best-effort** cleanup. Use explicit
`close()?` or `cleanup()?` inside the context when failure must be reported.
Normal return and `?` propagation run cleanup; process termination and runtime
traps do not promise destructor execution. Cleanup itself can fail to allocate.

These APIs currently use the Linux facilities of `std.fs`: unnamed and named
files require filesystem `O_TMPFILE` support; named publication additionally
requires `/proc/self/fd`. There is no fallback that exposes an intermediate
predictable name. Memory-to-disk spooling is not provided.

Parents must be trusted against concurrent hostile mutation. Identity checks
detect replaced entries, but are not atomic with deletion; they do not make
cleanup safe against an attacker racing renames. Named files may also be removed
by external temporary-directory cleaners. Keep the owner alive while another
program uses its path.

See the [temporary-resource tutorial](../../tutorial/08-resources/tempfile.md)
for complete examples.

## Acknowledgements

- Type names follow Python's [`tempfile`](https://docs.python.org/3/library/tempfile.html); `keep` and fallible `close` follow the Rust [`tempfile`](https://crates.io/crates/tempfile) crate.
