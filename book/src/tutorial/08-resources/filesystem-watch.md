# Watch a directory for changes

Use `std.fs.watch` when a tool should notice that files changed: a local preview,
an indexer, or a configuration reloader. Watch the parent directory when another
program might replace a file by renaming a new version over it.

This example watches a temporary directory, writes a file, then inspects the
queued batch. It handles overflow by requesting a full rescan instead of
assuming every change was delivered:

```plenty
import std.fs
import std.fs.watch
import std.path
import std.tempfile

def main() -> Result[(), AllocError | IoError | std.fs.FsError | std.fs.watch.WatchError]:
    with std.tempfile.directory()? as scratch:
        mut watcher = std.fs.watch.watcher()?
        watcher.add(scratch.path())?
        note = scratch.path().join(&std.path.from_text("note.txt")?)?
        write_text(&note.to_text()?, "hello")?
        count = watcher.poll()?
        for index in range(count):
            event = watcher.event(index)?
            if event.overflowed():
                print("rescan the directory")?
            elif event.created():
                print("created", event.name.display()?)?
        watcher.close()?
        scratch.cleanup()?
    Ok(())
```

```output
created note.txt
```

`poll()` never waits. For a long-running program, use `wait(&deadline, &cancel)`
with a [`std.time` deadline and cancellation token](../11-concurrency/deadlines.md),
or register the watcher with a `std.ready.Selector`. Consume the batch before
the next call: `event(index)` copies its name, so saved events remain valid.
The default buffer bounds each read to 4 KiB, and polling does not allocate.

A directory watch covers its immediate children, not an entire tree. Events
may be coalesced and an event's path may have changed again before you read it.
Treat notifications as a reason to check the current filesystem state, rather
than as a complete history. Names retain native bytes; the example uses
`display()` so a non-UTF-8 name does not prevent reporting the change.

See the [reference](../../design/stdlib/fs/watch.md) for rename cookies, watch
lifecycle events, overflow recovery, and resource limits.
