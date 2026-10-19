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
    print(fs.copy_file(directory.root(), &original, &copied, 1024, &cancel)?)?
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
than using a weaker fallback. Copying uses bounded scratch storage, returns
errors for byte limits/cancellation, and leaves no partial destination name.

Temporary-directory cleanup removes only an empty directory. Its destructor
is best-effort; call `cleanup()` when you need to observe failure. Other actors
must not rename entries in the parent while the temporary directory is live.
An identity check detects existing replacements but is not atomic with rmdir.
Use explicit bounded `remove_tree` only when intentionally deleting contents;
it can leave partial progress on cancellation or errors.

See the reference for [root contracts](../../design/stdlib/fs/helpers.md),
[temporary-resource limits](../../design/stdlib/fs/temporary.md), and
[copy, deletion and glob budgets](../../design/stdlib/fs/operations.md).
