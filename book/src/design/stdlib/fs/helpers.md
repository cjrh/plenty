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
                print(name.display()?)?
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
| `create_dir(&name)` | Exclusive creation, mode 0700 restricted by umask |
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

`Root`, names, streams and temporary owners are affine and not worker-transfer
safe. Shared Root loans allow pathname operations, since they do not mutate a
cursor. Filesystem state is external mutable state; this does not synchronize
concurrent processes. Every syscall retains its native `IoError`; buffer and
owner allocation is fallible. Paths require O(name length) temporary storage.
Native primitives make narrow OS requests; traversal, budgets, retries and
temporary-resource lifecycle are ordinary checked Plenty code.

Current target: x86_64 Linux GNU. No new dependency or linker requirement is
introduced. The matching installed compiler embeds the module and runtime;
imports work offline after relocation. See [temporary resources](temporary.md)
and [bounded copying, deletion and globbing](operations.md).
