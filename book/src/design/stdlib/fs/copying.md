# File metadata and recursive copying

All operations here are rooted, component-only, no-follow operations on 64-bit
Linux. They retain the [Root trust and rename contract](helpers.md): concurrent
renames can invalidate identity checks, and these operations are not an
adversarial sandbox. Filesystem capabilities, mount topology and permissions
remain subject to the operating system. The implementation is in Plenty over
native handle observation and metadata syscalls.

## Copy metadata

`CopyMetadata` has two modes:

- `ContentsOnly`, the default: file data only; new files use mode 0600 restricted
  by umask and directories use 0700 restricted by umask.
- `PermissionsAndTimes`: strictly attempt the source's ordinary permission bits
  (`mode & 0o777`) and access/modification timestamps. Read the source metadata
  from its opened handle before reading contents or enumerating a directory.
  Apply timestamps and then permissions to the destination handle. Directory
  metadata is applied after children, so copying a read-only source directory
  does not make its destination unwritable before the children are copied.

The second mode excludes ownership, setuid/setgid/sticky bits, ACLs, extended
attributes, creation time and inode change time. It does not claim Python
`copy2` parity. Destination ACLs/xattrs inherited from its filesystem are not
copied from the source or stripped by this API. Timestamp precision and permission
semantics depend on the destination filesystem; syscall success is the guarantee,
not preservation of precision that filesystem cannot represent. A missing source
timestamp or a failed metadata syscall returns an error; there is no silent
best-effort downgrade. The observation predates copy reads that may update source
atime. Changes to the source after observation are not reflected consistently:
this is not a filesystem snapshot.

`copy_file(..., &cancel, preserve=CopyMetadata.PermissionsAndTimes)` retains
its [anonymous staging and no-replace contract](operations.md). Metadata failure
leaves no published destination. Existing calls without `preserve` retain their
contents-only behavior. The optional named policy follows cancellation to preserve
that call shape; there is no required positional boolean. `copy_from` accepts an
arbitrary reader and therefore has no metadata policy.

## Recursive copying

```text
copy_tree(source_parent: &Root, source: &Path,
          destination_parent: &Root, destination: &Path,
          limits: &CopyLimits, cancel: &CancellationToken,
          preserve: CopyMetadata = CopyMetadata.ContentsOnly)
    -> Result[i64, TreeCopyError]
```

The source must name a real directory. The destination must be a new component;
existing files, directories and dangling symlinks are refused. Source and target
parents may differ, including different filesystems. An opened source directory
cannot contain the destination parent: a bounded descriptor-based ancestor walk
rejects that overlap, including the same directory opened through another root.
An existing destination that contains the source is refused by exclusive creation.
The traversal also compares source directory identities with the original
destination parent before descent. This catches a bind mount of a source
descendant whose namespace parents hide that ancestry; it reports an `Entry`
overlap error after destination creation, leaving partial output. No destination
parent is descended into or recursively copied into itself.
The ancestry walk holds at most two transient handles and stops after 4096 levels;
a longer or changing ancestry produces an I/O invalid-input error. Mount namespace
changes and hostile renames are outside the Root stability contract.

Only real directories and regular files are supported. Symlinks, including
relative, dangling and cyclic links, and special files return
`FsError.UnsupportedType`; no link is recreated or followed. Native byte names,
hidden entries, empty directories and binary contents remain lossless. A type
observation is followed by a no-follow typed open. Concurrent replacements can
cause an error or change which same-type inode is copied; no snapshot is promised.
Enumeration is streaming and native-order, not a materialized/sorted tree.

`copy_limits()` returns a `CopyLimits` with public fields:

| Field | Default | Contract |
|---|---:|---|
| `max_entries` | 10000 | Source root and every encountered child count once; positive |
| `max_depth` | 32 | Root is level 1; valid range 1–64 |
| `max_bytes` | 67108864 | Total bytes successfully copied across all files; nonnegative |
| `max_open_handles` | 128 | Conservative operation-owned descriptor budget; at least 11 |

Invalid limits fail before destination creation. Descent additionally requires
`3 * level + 8 <= max_open_handles`, covering source/destination directories,
streaming enumeration and file staging; caller-owned handles are outside this
budget. Budgets are shared across the traversal. Exceeding an entry, depth,
handle or byte budget reports `FsError.Limit`. The returned count is copied file
data bytes, with zero for an empty tree. Files are transferred with one shared
8192-byte buffer, so time is O(entries + bytes) and storage is O(8192 + depth *
name length), with O(depth) descriptors. Individual names are bounded by the
native directory-entry limit. An over-limit file can consume one read beyond
its remaining allowance before being rejected, without publishing that file.

Cancellation is checked before creation, between entries, during file transfer,
and after traversal. It is cooperative and cannot interrupt a syscall or
`write_all`; completion may race cancellation after the final observation.
No durability guarantee is made.

## Partial output and recovery

`TreeCopyError.Setup(FsError)` means the destination was not created by this
operation. `TreeCopyError.Partial(FsError)` means its newly created destination
may remain with a partial tree. `TreeCopyError.Entry(name, depth, cause)`
also means partial output; it transfers the immediate failing child name and its
1-based tree depth, preserving the cause without allocating while reporting an
error. It does not carry the full relative path. Errors while enumerating or
finishing a child directory identify that directory; failures on the source
root itself use `Partial`. Earlier copied files remain complete; the current
file is staged anonymously and is absent on failure before publication. Empty
or populated directories may remain, and metadata can be partly applied to them.
The caller retains both destination root and name and can inspect the tree or
explicitly call `remove_tree` with separate cleanup budgets and a fresh token.
There is no automatic recursive cleanup that could hide a second failure, erase
progress, or pretend rollback is guaranteed. Concurrent actors must not replace
or mutate the destination while copying or cleaning it. An error opening the
newly created destination is also `Partial`.

```plenty
import std.fs as fs
import std.path as path
from std.sync import CancellationToken

def main() -> Result[(), AllocError | IoError | fs.FsError | fs.TreeCopyError]:
    root = fs.root(&path.from_text(".")?)?
    source = path.from_text("assets")?
    root.create_dir(&source)?
    write_text("assets/icon.bin", "binary data")?
    destination = path.from_text("assets-backup")?
    limits = fs.copy_limits()
    cancel = CancellationToken()?
    print(fs.copy_tree(&root, &source, &root, &destination, &limits, &cancel)?)?
    print(read_text("assets-backup/icon.bin")?)?
    Ok(())
```
```output
11
binary data
```

Owned names, metadata observations, buffers, and native owners allocate fallibly.
Allocation failure preserves the same file-publication and tree-partial contracts;
held descriptors close on ordinary return/drop without allocating. Fatal traps
or termination do not unwind. Roots must remain accessible under the selected
permissions for any later cleanup; preserving read-only directory permissions
can require the caller to restore writable permissions outside this API.

## Acknowledgements

- Python's [`shutil.copytree` and `copy2`](https://docs.python.org/3/library/shutil.html)
  provide the API precedent. The Plenty implementation uses explicit budgets,
  rooted no-follow operations and a narrower declared metadata policy; no Python
  source code is shipped.
