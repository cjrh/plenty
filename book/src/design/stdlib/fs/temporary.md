# Temporary resources

For ordinary scratch storage with `with` support, prefer
[`std.tempfile`](../tempfile.md). These lower-level owners provide rooted staging
and explicit publication.

`temporary_file(&root) -> Result[TempFile, IoError]` creates an **unnamed**
`O_TMPFILE` inode in the root's filesystem, mode 0600 restricted by umask.
Its public `stream: std.io.FileStream` is readable and writable; `seek` back
to read what was written. `publish(&name)` adds a
name with atomic no-replace `linkat`; existing files, directories and dangling
links are refused. A successful publication can occur once. It leaves the
stream open; `close()` reports close errors, and ordinary drop closes it.
Closing/dropping before publication releases the unnamed inode without an
unlink-by-name race. After publication the name persists.

`remove_published(&name) -> Result[(), FsError]` explicitly removes a published
name after comparing it with the still-open stream's device and inode. A missing
name succeeds; a different regular file returns `FsError.Replaced`, and symlinks
are refused. The stream remains open. Publication is still allowed only once.
As with directory cleanup, identity checking and removal are not atomic against
hostile concurrent renames. This operation supports `std.tempfile` ownership.

Publication uses `/proc/self/fd` so unprivileged users need no special
capability. The filesystem must support `O_TMPFILE`, and procfs must be
available for publication. Unsupported filesystems and unavailable procfs
produce `IoError` (normally a native `System` code); there is no named-file
fallback. A successful publish is not a durability promise; call
`stream.sync()` before publishing if needed. Directory durability is not
provided here. No metadata, ownership, xattrs or source permissions are copied.

`temporary_dir_named(&root, &name)` exclusively creates a 0700 directory and
returns `Result[TempDir, FsError]`. A collision is reported and never removed.
`temporary_dir(&root, &prefix, attempts)` appends 32 lowercase hex digits from
16 OS secret bytes, retrying only EEXIST up to `attempts` (1–1024). Prefixes
are at most 200 bytes without slash/NUL; no UTF-8 conversion is imposed.
Entropy errors are reported without weakening the name source.

`TempDir.name()` and `root()` lend its name and directory handle. Explicit
`cleanup()` removes only an empty directory and reports errors. It is
idempotent after success. `keep()` disarms cleanup; the directory remains.
The destructor attempts the same empty-only cleanup and ignores errors.
This low-level destructor never recursively deletes contents. Fatal traps/process termination
do not unwind; explicit cleanup is required when its success matters.
Cleanup's native name conversion can allocate: if allocation remains exhausted
during drop, even an empty directory can remain. File/directory handles still
close without allocation. Creation reserves names and owner storage before
mkdir, so allocation failure during construction leaves no directory behind.

`cleanup_tree(max_entries, max_depth, &cancel) -> Result[(), FsError]` is an
explicit recursive alternative, using the [bounded deletion](operations.md)
budgets. It checks identity before traversing the held directory and again
before removing its entry. Successful cleanup disarms the owner. Failure can
leave partial cleanup and leaves it armed. `std.tempfile.TemporaryDirectory`
uses this operation for its best-effort destructor.

Temporary directories require a parent whose entries other actors do not
rename while the owner is live. Cleanup compares the held directory's device
and inode to the named directory and reports `FsError.Replaced` for a changed
identity. A missing name or symlink is an I/O error. This catches replacements
already visible at the check; Linux has no compare-inode-and-unlink syscall,
so the check and rmdir are **not atomic against a concurrent rename**. Empty
replacement directories can still be removed by a racing actor. Nonempty
replacements are never traversed by the empty-only cleanup. A child-open failure
after mkdir attempts empty-only rollback under the same parent-trust contract.

Temporary names/owners and copies are allocated fallibly. A temporary file
holds its file descriptor and a duplicate root descriptor. A temporary
directory holds its directory and parent descriptors plus its name bytes.
See [root rules](helpers.md) and [bounded operations](operations.md).
