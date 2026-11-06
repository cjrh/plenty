# Metadata and links

In `std.fs`, `metadata(&path)` follows symbolic links, while
`symlink_metadata(&path)` inspects the link itself. Both return
`Result[Metadata, IoError]`; missing paths, inaccessible paths and broken link
targets preserve their OS error. `DirEntry.metadata()` follows the latter rule.
No primitive converts an error to False. Broken links remain enumerable and
inspectable with non-following operations.

```plenty
import std.fs as fs
import std.path as path

def main() -> Result[(), AllocError | IoError]:
    write_text("notes.txt", "hello")?
    notes = path.from_text("notes.txt")?
    info = fs.metadata(&notes)?
    print(info.is_file(), info.is_dir(), info.size())?
    current = path.from_text(".")?
    print(fs.metadata(&current)?.file_type())?
    missing = path.from_text("missing.txt")?
    match fs.metadata(&missing):
        case Ok(_):
            print("found")?
        case Err(error):
            print(error)?
    Ok(())
```
```output
True False 5
std.fs.FileType.Directory
IoError.System(2)
```

Metadata is an affine, detached observation, not an open file or a promise about
a later operation. No metadata cache is kept on an entry. Request another
path-level observation to refresh it; replacing or changing a file can make
earlier data stale.

`file_metadata(&stream) -> Result[Metadata, IoError]` observes an open
`FileStream` itself (`statx` with `AT_EMPTY_PATH`), not whatever its path names
now. A file opened through a [`Root`](helpers.md) keeps that confinement:
renaming or replacing the path afterwards cannot redirect the observation.
| Metadata operation | Value |
| --- | --- |
| `file_type()` | `FileType.File`, `.Directory`, `.Symlink`, or `.Other` |
| `is_file()`, `is_dir()`, `is_symlink()` | Predicates on that observation |
| `size()` | `u64` size in bytes, also for links and other native types |
| `permissions()` | `Permissions(readonly: bool, unix_mode: u32)` |
| `modified()`, `accessed()`, `created()` | `Option[Timestamp]` |
| `unix_changed()` | Unix status-change Timestamp, not creation time |
| `unix_uid()`, `unix_gid()` | `u32` Unix identity fields |
| `unix_device()`, `unix_inode()` | `u64` Unix directory-identity fields |

`Timestamp` is [`std.time.Timestamp`](../time.md); import the name from
`std.time`. It has signed `i64` seconds since the Unix epoch and `u32` nanoseconds
in `[0, 1_000_000_000)`. Negative times use floor seconds: half a second before
the epoch is `(-1, 500_000_000)`. Nothing means the filesystem did not supply
that timestamp. Unsupported creation time is never filled with Unix change time.
Metadata getters do not allocate. `readonly` means no Unix write mode bits;
it does not account for ACLs, privileges or mount policy and does not establish
that a later operation will succeed.

`read_link(&path) -> Result[Path, IoError]` returns the stored target exactly,
including a relative target or invalid UTF-8. It does not resolve it, make it
absolute or require that the target exists. Buffer growth and copying are
fallible; a full native buffer is retried rather than silently truncated.

## Creating and resolving links

`symlink(target: &Path, link: &Path) -> Result[(), IoError]` creates a new
symbolic link containing the target's exact bytes. A relative target is
interpreted relative to the link's parent when followed, not relative to the
caller's working directory. The target need not exist. An existing destination,
including a dangling link, causes an error without replacing it. Parent
components of `link` follow ordinary filesystem resolution; this is not a
rooted no-follow operation. Use [Root and TempFile](helpers.md) for their
separate confinement and publication contracts.

`canonicalize(path: &Path) -> Result[Path, IoError]` returns a new absolute
native-byte path after resolving symbolic links, `.` and `..`. Components are
resolved in filesystem order: `link/..` refers to the target's parent. Every
component must exist; dangling links and missing components return the native
error, a non-directory intermediate component or a trailing slash on a file
returns `ENOTDIR`, and loops or excessive link traversal return `ELOOP`.
Empty paths fail. No text decoding or lossy replacement occurs.

Both functions borrow their inputs, are usable on worker threads, and resolve
relative input paths against the process working directory. Embedded NUL bytes
return `IoError.InvalidInput`. On the current 64-bit Linux backend each input
and the canonical output must fit in `PATH_MAX` including the terminating NUL
(4095 content bytes); excess returns `IoError.System(ENAMETOOLONG)`. Input
lengths are checked before copying. Native path buffers are bounded stack
storage; `symlink` needs no Plenty heap allocation. Canonical output allocation
is fallible and reports `IoError.Data(DataError.Allocation(...))`; OS/C-library
failures retain `IoError.System(errno)`. Unsupported hosts return
`IoError.Unsupported`.

Canonicalization is a transient observation, **not a security boundary** or a
snapshot. Concurrent rename, removal or replacement can change resolution or
invalidate the returned name immediately. A later open need not access the same
object; use held handles and descriptor-rooted operations when identity matters.
The narrow native wrappers use `symlink` and caller-buffer `realpath` so libc
implements platform link traversal, directory checks and error behavior instead
of duplicating that resolver in Plenty. No `realpath(NULL)` allocation escapes
into the language.

The baseline is Rust's explicit `metadata`/`symlink_metadata` split and
non-following DirEntry behavior. This differs from Python's default followed
DirEntry predicates/stat; those defaults are not mixed. The native backend uses
Linux's stable 256-byte `statx` ABI, also used by Rust's Unix fs backend, to avoid
std's infallible long-path CString allocation. It requires 64-bit Linux and
libc/kernel statx support; absent kernel support or required basic fields returns
`IoError.Unsupported`. Native layouts and descriptors are private.

## Acknowledgements

- Uses the C library’s [`symlink`](https://pubs.opengroup.org/onlinepubs/9799919799/functions/symlink.html) and [`realpath`](https://pubs.opengroup.org/onlinepubs/9799919799/functions/realpath.html) for native link creation and filesystem resolution.

- Follows Rust's [`metadata`/`symlink_metadata`](https://doc.rust-lang.org/std/fs/fn.symlink_metadata.html) split and its Unix `statx` backend.

To update ordinary permission bits, use the held-handle operations
[`Root.set_unix_mode` and `set_file_unix_mode`](helpers.md#private-creation-and-permission-changes).
Changing a `Permissions` observation's fields does not change the filesystem.
