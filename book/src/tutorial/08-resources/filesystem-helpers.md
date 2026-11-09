# Stage files and clean temporary directories

Open a `std.fs.Root` to anchor operations to a directory. Child names are
single-component lossless paths: no slash, NUL, `.` or `..`. Child opens reject
symlinks. The initial root's ancestors remain the caller's trust decision.

An anonymous temporary file has no name until `publish` succeeds. Publication
refuses an existing destination. Returning early before publication closes the
file and releases its partial contents without deleting a named path.

This example uses a random private directory inside the working directory:

```plenty
import std.io

import std.fs as fs
import std.path as path
from std.sync import CancellationToken

def main() -> Result[(), AllocError | IoError | std.io.StreamError | fs.FsError]:
    current = path.from_text(".")?
    parent = fs.root(&current)?
    prefix = path.from_text("plenty-example-")?
    mut directory = fs.temporary_dir(&parent, &prefix, 8)?
    original = path.from_text("original.txt")?
    copied = path.from_text("copied.txt")?
    mut staged = fs.temporary_file(directory.root())?
    staged.stream.file.write("hello")?
    staged.publish(&original)?
    staged.close()?

    cancel = CancellationToken()?
    print(fs.copy_file(directory.root(), &original, &copied, 1024, &cancel, preserve=fs.CopyMetadata.PermissionsAndTimes)?)?
    pattern = path.from_text("*.txt")?
    names = fs.glob(directory.root(), &pattern, 10, 10, 1000)?
    print(len(names))?
    directory.root().remove_file(&original)?
    directory.root().remove_file(&copied)?
    directory.cleanup()?
    Ok(())
```

```output
5
2
```

These operations require Linux, a filesystem supporting `O_TMPFILE`, and
`/proc/self/fd` for publication. Unsupported environments return errors rather
than using a weaker fallback. Single-file copying uses bounded scratch storage, returns
errors for byte limits/cancellation, and leaves no partial destination name.
To preserve ordinary permission bits and access/modification times, pass
`preserve=fs.CopyMetadata.PermissionsAndTimes` to `copy_file`; ownership,
ACLs, xattrs and special permission bits are excluded.

Temporary-directory cleanup removes only an empty directory. Its destructor
is best-effort; call `cleanup()` when you need to observe failure. Other actors
must not rename entries in the parent while the temporary directory is live.
An identity check detects existing replacements but is not atomic with rmdir.
Use explicit bounded `remove_tree` only when intentionally deleting contents;
it can leave partial progress on cancellation or errors.

See the reference for [root contracts](../../design/stdlib/fs/helpers.md),
[temporary-resource limits](../../design/stdlib/fs/temporary.md), and
[copy, deletion and glob budgets](../../design/stdlib/fs/operations.md).

For a directory tree, use `copy_tree` with explicit entry, depth, byte and
handle budgets from `copy_limits()`. Its default copies contents only.
The destination must be new, symlinks and special files are rejected, and a
failure after creation leaves partial output for the caller to inspect or
remove. The [recursive-copy example and recovery contract](../../design/stdlib/fs/copying.md)
show the complete call and explain `TreeCopyError.Setup` versus partial-output `Partial`/`Entry` errors.

For cooperating processes that must not operate on a resource simultaneously,
use [`std.fs.lock`](../../design/stdlib/fs/lock.md). Keep the returned owner alive
for the protected work. `acquire_now` reports contention immediately;
`acquire_before` takes a total deadline and a cancellation token. Use a stable
lockfile in a trusted directory and leave that file in place after releasing
the lock: removing it while another process owns it can split coordination
across different inodes. Locks close automatically, and normal subprocesses
do not inherit them across exec. When a child must keep the lock after its
runner exits, use `owner.inherit_into(&mut command)` and pass the returned
number through the child's descriptor protocol. After spawning, clear the
command's copies with `clear_inherited_files()`; the original owner and the
child then control the remaining lifetime. See [explicit lock inheritance](../../design/stdlib/fs/lock.md#ownership-and-release).

## Create a private named file

When a file should be visible immediately and must not already exist, use
`Root.create_file`. Its default 0600 mode prevents group and other access from
creation; `create_dir` defaults to 0700. Both are restricted by the process
umask. An existing file, directory or symlink causes an error.

```plenty
import std.io
import std.fs as fs
import std.path as path

def main() -> Result[(), AllocError | IoError | std.io.StreamError]:
    parent = fs.root(&path.from_text(".")?)?
    name = path.from_text("private-state")?
    parent.create_dir(&name)?
    directory = parent.open_dir(&name)?
    token = path.from_text("token")?
    mut file = directory.create_file(&token)?
    file.file.write("secret")?
    fs.set_file_unix_mode(&file, 0o400u32)?
    file.close()?
    directory.remove_file(&token)?
    parent.remove_dir(&name)?
    Ok(())
```

```output
```

`set_file_unix_mode` changes the held file, so a replacement at its old path
cannot redirect the update. `Root.set_unix_mode` does the same for a directory.
Setting a mode does not close the handle or revoke existing access. Unlike
staging, a named file remains after a write failure; use the staging pattern
when readers must see only complete output. See the
[permission contracts](../../design/stdlib/fs/helpers.md#private-creation-and-permission-changes)
for custom modes, ACLs, umask and errors.

## Create a directory tree

For an ordinary configured path, use `fs.create_dirs`. It follows symlinks and
resolves `.` and `..` as the OS does, accepts absolute paths, and needs no
readable directory handle:

```plenty
import std.fs as fs
import std.path as path

def main() -> Result[(), AllocError | IoError]:
    state = path.from_text("app/state/runs")?
    fs.create_dirs(&state)?
    fs.create_dirs(&state)?
    Ok(())
```
```output
```

Each missing level requests mode 0700 by default, subject to OS permissions,
umask and ACLs. Existing directories keep their modes, so success does not
certify that an existing path is private. See the
[ordinary-path contract and API comparison](../../design/stdlib/fs/creation.md).

For traversal from an already-open directory that must reject symlinks and dot
segments, use the rooted operation:

An application often needs a private directory and its parents, and must accept
them if an earlier run already made them. `Root.create_dirs` takes a relative
path, creates each missing directory with mode 0700 by default and returns the
last one as a `Root`:

```plenty
import std.io
import std.fs as fs
import std.path as path

def main() -> Result[(), AllocError | IoError | std.io.StreamError]:
    parent = fs.root(&path.from_text(".")?)?
    state = parent.create_dirs(&path.from_text("app/state")?)?
    mut file = state.create_file(&path.from_text("token")?)?
    file.file.write("secret")?
    file.close()?
    # A second run finds the directories and opens them.
    again = parent.create_dirs(&path.from_text("app/state")?)?
    again.remove_file(&path.from_text("token")?)?
    Ok(())
```

```output
```

Existing directories keep their modes. A component that is a file or a symlink
stops the call with an error, as does `..`, so the walk stays inside `parent`.
Directories made before a failure remain. See
[creating a directory tree](../../design/stdlib/fs/helpers.md#creating-a-directory-tree)
for concurrent creators and the exact path rules.
