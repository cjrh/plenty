# Metadata and links

In `std.fs`, `metadata(&path)` follows symbolic links, while
`symlink_metadata(&path)` inspects the link itself. Both return
`Result[Metadata, IoError]`; missing paths, inaccessible paths and broken link
targets preserve their OS error. `DirEntry.metadata()` follows the latter rule.
No primitive converts an error to False. Broken links remain enumerable and
inspectable with non-following operations.

Metadata is an affine, detached observation, not an open file or a promise about
a later operation. No metadata cache is kept on an entry. Request another
path-level observation to refresh it; replacing or changing a file can make
earlier data stale.

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

The baseline is Rust's explicit `metadata`/`symlink_metadata` split and
non-following DirEntry behavior. This differs from Python's default followed
DirEntry predicates/stat; those defaults are not mixed. The native backend uses
Linux's stable 256-byte `statx` ABI, also used by Rust's Unix fs backend, to avoid
std's infallible long-path CString allocation. It requires 64-bit Linux and
libc/kernel statx support; absent kernel support or required basic fields returns
`IoError.Unsupported`. Native layouts and descriptors are private.
