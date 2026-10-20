# Use temporary files and directories

Use `std.tempfile` when a task needs scratch storage. A `with` statement owns the
resource until its body ends and then cleans it up, including on an early return
or an error propagated with `?`.

For a tool that needs a filename, use `named_file`. You can write through the
owner and pass its path to other code while the owner is alive:

```plenty
import std.tempfile
import std.io
import std.fs

def main() -> Result[(), AllocError | IoError | std.fs.FsError | std.io.Partial]:
    with std.tempfile.named_file(suffix=".txt")? as temporary:
        std.io.write_all(temporary, &b"hello", 0, 5)?
        print(read_text(&temporary.path().to_text()?)?)?
        temporary.close()?
    Ok(())
```

```output
hello
```

The explicit `close()?` reports cleanup failures. Without it, leaving `with`
still attempts cleanup, but cannot report an error from that attempt.
For a stream with no filename, `with std.tempfile.file()? as stream:` lends a
`std.io.FileStream` instead.

A temporary directory can contain an entire working tree. Cleanup removes its
contents recursively; symlinks do not cause traversal outside that tree:

```plenty
import std.tempfile
import std.path
import std.fs

def main() -> Result[(), AllocError | IoError | std.fs.FsError]:
    with std.tempfile.directory()? as scratch:
        note = scratch.path().join(&std.path.from_text("note.txt")?)?
        write_text(&note.to_text()?, "working copy")?
        print(read_text(&note.to_text()?)?)?
        scratch.cleanup()?
    Ok(())
```

```output
working copy
```

To preserve a named file or directory, call `keep()?` before cleanup. It returns
an owned path and transfers responsibility for removing it to your code.
Use the `parent` keyword argument to choose a directory explicitly. Otherwise,
the module uses the first nonempty `TMPDIR`, `TEMP`, or `TMP`, falling back to
`/tmp` when none is set.

See the [reference](../../design/stdlib/tempfile.md) for cleanup budgets, Linux
filesystem requirements, and the trusted-parent requirement. For atomic
publication of a completed file, continue with
[filesystem staging](filesystem-helpers.md).
