# Rooted filesystem helpers

`std.fs` mutation helpers operate on an owned `Root`, opened by
`root(&path) -> Result[Root, IoError]`. The handle anchors operations to that
directory even if its pathname changes. Opening rejects a final symlink,
including a path with trailing slashes. Ancestor components of the initial
root path are resolved normally: selecting that root is the caller's trust
decision, not a sandbox or a canonicalization operation.

```plenty
import std.fs as fs
import std.path as path

def main() -> Result[(), AllocError | IoError]:
    current = path.from_text(".")?
    root = fs.root(&current)?
    drafts = path.from_text("drafts")?
    final = path.from_text("final")?
    root.create_dir(&drafts)?
    root.move(&drafts, &final)?
    reports = root.open_dir(&final)?
    reports.create_dir(&path.from_text("2024")?)?
    mut names = reports.names()?
    while True:
        match names.next()?:
            case Some(name):
                print(name.to_string()?)?
            case Nothing:
                break
    Ok(())
```
```output
2024
```

Every child name is a lossless `Path` containing one nonempty component.
Slash, NUL, `.` and `..` are rejected. Child directory/file opens use
`O_NOFOLLOW`; symlinks and symlink cycles cannot escape through these opens.
Mounted directories are not a boundary. A directory moved after opening stays
accessible through its handle. These rules do not provide isolation from a
process with permission to rename entries in the same tree.

| Root method | Behavior |
| --- | --- |
| `open_dir(&name)` | New independently owned Root; final symlink refused |
| `read_file(&name)` | Read-only `std.io.FileStream`; regular files only |
| `create_dir(&name, unix_mode=0o700u32)` | Exclusive directory creation; mode restricted by umask |
| `create_file(&name, unix_mode=0o600u32)` | Exclusive regular-file creation; owned read/write `FileStream`, mode restricted by umask |
| `set_unix_mode(unix_mode)` | Change ordinary permission bits on this held directory |
| `remove_file(&name)` | Unlink one non-directory, including a symlink itself |
| `remove_dir(&name)` | Remove one empty directory; no link following |
| `move(&source, &destination)` | Same-root atomic `renameat2(RENAME_NOREPLACE)` |
| `names()` | Owned lazy `Names`; `next()` yields `Result[Option[Path], IoError]` |

Move refuses every existing destination, including a dangling link. It moves
a source symlink itself; there is no replacement or cross-root/cross-device
copy-and-delete fallback. Removal has ordinary unlink/rmdir permissions and
race semantics. `Names.close()` is fallible and repeatable. EOF and errors close
the stream; drop closes it as well. Names are detached, native-order byte
values, including hidden names. No metadata observation through an unanchored
joined path is exposed by this interface.

`Root`, names, streams and temporary owners are affine. An owned Root or
FileStream can move to a worker; creation and permission operations are
handle effects permitted there. Sharing a handle by loan still follows the
[worker resource rules](../../facilities/03-concurrency/effects.md).
Shared Root loans allow pathname operations within a thread, since they do not
mutate a cursor. Filesystem state is external mutable state; this does not synchronize
concurrent processes. Every syscall retains its native `IoError`; buffer and
owner allocation is fallible. Paths require O(name length) temporary storage.
Native primitives make narrow OS requests; traversal, budgets, retries and
temporary-resource lifecycle are ordinary checked Plenty code.

Current target: x86_64 Linux GNU. No new dependency or linker requirement is
introduced. The matching installed compiler embeds the module and runtime;
imports work offline after relocation. See [temporary resources](temporary.md)
and [bounded copying, deletion and globbing](operations.md).

## Private creation and permission changes

Use `Root.create_file` for a named file that must not already exist. It
returns `Result[FileStream, IoError]`, opened for reading and writing; close it
through `FileStream.close()` or let its owner drop. Allocation for the path and
owner happens before creation. Neither allocation failure nor an existing
entry leaves a new file. Every existing entry, including a dangling symlink,
causes an error without truncation, replacement or permission changes. A
successful creation remains named after close, drop or a later write failure.
Use [anonymous staging](temporary.md) when partial contents must stay unnamed.

`create_dir` returns `Result[(), IoError]` and creates only the named directory,
not its parents. Existing directories and symlinks fail too. Open the result
with `open_dir` when a held directory handle is needed. Creation and this
subsequent open are not atomic; the parent must be trusted against concurrent
entry replacement, as for the other rooted helpers.

Both creation methods accept `unix_mode: u32`. The default modes (0600 for a
file, 0700 for a directory) prohibit group/other access from the instant of
creation. The mode goes directly to `openat`/`mkdirat`, with no create-then-chmod
window. Ordinary Unix umask/default-ACL rules apply: requested bits are an
upper bound, and the process umask can remove them when no default ACL applies.
The library never reads or changes the process-global umask. Directory setgid
inheritance and existing filesystem ACL policy are controlled by the OS.

To change permissions later, call `Root.set_unix_mode(unix_mode)` on a held
directory or `fs.set_file_unix_mode(&stream, unix_mode)` on a held `FileStream`.
Both return `Result[(), IoError]`, perform no input-sized allocation and call
`fchmod` on the descriptor. Umask does not restrict these updates. They neither
reopen a pathname nor follow a newly substituted symlink. The operation changes
the inode shared by all of its hard links, even after renaming or unlinking.
It does not revoke already-open descriptors, change ownership, or promise an
ACL-free filesystem. The OS enforces ownership/capability checks and ACL effects.

Modes outside `0o000u32..0o777u32` return `IoError.InvalidInput` before any
filesystem change: file-type bits, sticky and set-ID bits are deliberately
unsupported. Modes are not silently masked. OS failures retain their native
`IoError.System` code; closed streams return `IoError.Closed`. Creation can
also return an allocation error through `IoError.Data(DataError.Allocation(...))`. These APIs have
the same 64-bit Linux scope as the rooted helpers; unsupported hosts return
`IoError.Unsupported`. Native code is limited to descriptor creation and
`fchmod`, because these operations require OS syscalls; mode validation and
public policy remain Plenty code.
