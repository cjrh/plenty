# Bounded copy, deletion and glob filtering

`copy_file(&root, &source, &destination, max_bytes, &cancel)` copies one
regular file to a new name in the same root and returns `Result[i64, FsError]`.
`copy_from(&mut reader, &root, &destination, max_bytes, &cancel)` accepts any
checked `std.io.Reader`. Both stage into an anonymous temporary file and
publish only after EOF, flushing, and a final cancellation observation.
They never replace an existing name. Limits, cancellation, read/write errors,
allocation failures and publication errors leave no partial destination name.
Unnamed partial data is released by descriptor destruction, not path unlink.

```plenty
import std.fs as fs
import std.path as path
from std.sync import CancellationToken

def main() -> Result[(), AllocError | IoError | fs.FsError]:
    write_text("report.txt", "quarterly totals")?
    current = path.from_text(".")?
    root = fs.root(&current)?
    cancel = CancellationToken()?
    source = path.from_text("report.txt")?
    backup = path.from_text("report.bak.txt")?
    print(fs.copy_file(&root, &source, &backup, 4096, &cancel)?)?
    pattern = path.from_text("report*")?
    matches = fs.glob(&root, &pattern, 100, 10, 10000)?
    print(len(matches))?
    Ok(())
```
```output
16
2
```

The loop uses one 8192-byte buffer, retries interrupted reads, checks reader
progress, and checks cancellation before and after each read and before
publication. Writes use `std.io.write_all`. Cancellation is cooperative,
cannot interrupt an in-progress syscall/write-all, and may race a successful
publication after the final check. Negative bounds are invalid; exceeding the
byte bound reports `Limit`. One read of up to 8192 bytes can consume beyond the
bound before rejecting it. A concurrently changing source is not a snapshot.
The count is bytes published. Costs are O(bytes) time and O(8192 + path lengths)
scratch storage, without a whole-file allocation. A successful result does not
promise durable storage. See [temporary-file requirements](temporary.md).

`remove_tree(&root, &name, max_entries, max_depth, &cancel)` explicitly removes
a child directory tree. It requires positive entry/depth budgets, with depth
at most 64. The root directory counts as one entry and one level. Each
encountered child consumes the shared entry budget. Depth is checked before
opening a recursive frame. File/link unlinking and empty-directory removal
use the anchored parent handle. Links (including cycles) are unlinked and
never descended into; the initial name must itself be a real directory.

Deletion is not transactional: cancellation, limits, permissions, concurrent
changes or I/O errors can leave a partially removed tree. It checks
cancellation between entries. Descent keeps O(depth) directory descriptors and
O(depth * name length) path storage, without materializing the tree. It never
accepts root/dot/dot-dot names. The initial root's ancestors, mount points and
rename races have the [Root contract](helpers.md); do not use this as an
adversarial sandbox. A directory identity check precedes the final rmdir, with
the same non-atomic race limitation as temporary-directory cleanup. Other
processes must not relocate entries during destructive traversal.

`glob(&root, &pattern, max_entries, max_matches, max_steps)` returns
`Result[list[Path], FsError]` for names directly inside the root. `*` matches
any byte sequence, `?` one byte, and every other byte is literal (including
brackets, backslash and leading dot). Slash/NUL and patterns over 4096 bytes
are invalid; there is no recursive `**`, character class, escaping, sorting or
Unicode normalization. Consecutive stars retain their ordinary star meaning.
Invalid UTF-8 names remain lossless.

Entry, result and matching-step budgets are independent nonnegative limits.
Each examined entry and each matcher iteration consumes its budget; exceeding
one returns `Limit`, releasing partial results. Greedy star backtracking takes
O(pattern length * name length) worst-case time, bounded by `max_steps` across
the entire scan. Storage is O(returned name bytes) plus one streaming entry.
Enumeration is native order, includes hidden names, and is not a snapshot.

`FsError` distinguishes `Io`, `Stream`, `Allocation`, `Entropy`, `InvalidInput`,
`Limit`, `Cancelled`, `Replaced` and `Collisions`. Basic Root methods preserve
`IoError` directly. Drop is best-effort cleanup, never proof of completion.
