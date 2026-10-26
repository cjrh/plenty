# `std.fs.watch` — filesystem changes

`import std.fs.watch` observes Linux files and directories through inotify.
It owns one nonblocking, close-on-exec descriptor, starts no threads, and uses
[`std.ready`](../ready.md) for deadlines, cancellation, and selector integration.
For a runnable introduction, see [watch a directory](../../../tutorial/08-resources/filesystem-watch.md).

## Batches and ownership

`watcher(batch_bytes=4096) -> Result[Watcher, WatchError]` allocates a reusable
byte buffer and offset table. `batch_bytes` must be 272..65536: 272 accommodates
the longest Linux filename plus its event header. One read returns at most
`batch_bytes // 16` events, with fewer when filenames occupy buffer space.
The offset table reserves that many ordinary `list[i64]` slots, in addition
to the compact byte buffer. This is a bound on each user-space batch, not on
the independent kernel queue or total number of watched paths.

| Operation | Behavior |
|---|---|
| `add(path: &Path)` | `Result[i32, WatchError]`: watch the file or immediate directory children; return its watch ID |
| `remove(watch: i32)` | `Result[(), WatchError]`: remove that watch; unknown/already removed IDs are errors |
| `poll()` | `Result[i64, WatchError]`: replace the last batch with one nonblocking read; zero means no events or an interrupted read |
| `wait(deadline: &Deadline, cancel: &CancellationToken)` | `Result[i64, WatchError]`: poll, then wait until a batch is available, cancelled, or timed out |
| `count()` | Number of events in the last successful batch |
| `event(index: i64)` | `Result[Event, WatchError]`: copy one event and its native filename |
| `register(selector: &mut Selector, key: u64)` | `Result[(), WaitError]`: register the descriptor for read readiness |
| `unregister(selector: &mut Selector)` | `Result[(), WaitError]`: remove that selector registration |
| `close()` | `Result[(), WatchError]`: discard the batch and close all watches; repeated close is harmless |

`poll` and `wait` reuse their buffers without allocating. Events that do not fit
remain in the kernel queue for later calls; a full user-space batch is not
overflow. Consume the current batch before the next `poll`, `wait`, or `close`,
which invalidates it. `event` is an O(1) lookup plus filename copying. A failed
copy leaves the batch intact for retry, and a successful event owns its name
independently of the watcher. Dropping the watcher closes the descriptor and
all watches automatically.

Cancellation requested before `wait` wins over a ready event; otherwise an
already queued event wins over an expired deadline. After a selector reports
the watcher's key readable, call `poll`. Closing the watcher removes its
descriptor from selectors automatically.

## Event fields

Each `Event` exposes `watch: i32`, `cookie: u32`, and `name: Path`. The name is
the immediate child's **native filename bytes**, not an absolute path. It is
empty for events about the watched object itself. Use `Path.join` with the
appropriate watched directory when a path is needed; `name.to_text()` validates
UTF-8, while `name.display()` provides display text.

Predicates report the event's flags:

- `created()`, `deleted()`, `modified()`, `metadata_changed()` and
  `closed_after_write()` describe ordinary changes.
- `moved_from()` and `moved_to()` identify rename endpoints. Their nonzero
  `cookie` values pair endpoints within this watcher; either endpoint may be
  outside the watched directories or in a later batch. They need not be adjacent.
- `deleted_self()` and `moved_self()` describe the watched object itself.
- `ignored()` says the kernel removed the watch, after explicit removal,
  deletion or unmount. `unmounted()` reports filesystem unmounting.
- `overflowed()` means kernel events were lost. Its watch ID is -1 and its
  filename is empty. **Rescan the watched state** before trusting an incremental
  cache again; overflow is returned as an event, not hidden by a successful read.
- `is_directory()` identifies directory events where the kernel provides it.

Adding the same inode again, including through another hard link, returns its
existing watch ID. Paths follow symlinks. Watches follow inodes, not replaceable
pathnames: to detect a program replacing a file atomically, watch its parent
directory. Directory watches are **not recursive**. Reading/opening files does
not generate events in this API; it requests only the change flags above.

Kernel notifications can coalesce, and are not an exact operation count or a
transaction log. Changes through some remote filesystems, pseudo-filesystems,
or memory mappings may not be reported. Events can be stale by the time they
are read. Removal does not erase already queued events, and IDs can eventually
be reused; use a fresh watcher after rebuilding state if stale IDs would be
ambiguous. There is no automatic rescan, recursive registration, rename pairing,
debouncing, or background callback execution. These native semantics follow
the [Linux inotify contract](https://man7.org/linux/man-pages/man7/inotify.7.html).

## Errors and scope

`WatchError` distinguishes `OutOfRange` (buffer size/event index), `InvalidData`
(malformed native record), `Io(IoError)`, `Wait(WaitError)`, and
`Allocation(AllocError)`. Native operations preserve `IoError` including its
allocation errors; facade buffer/filename allocations use `Allocation`.
Kernel resource limits, missing paths and denied access remain native errors.
No wrapped crate introduces infallible heap allocation. Watchers can move into
worker tasks; their mutable methods require exclusive borrowing.

The native implementation currently targets x86-64 Linux. Unsupported hosts
report `IoError.Unsupported`; there is no polling fallback. This module shares
the existing `File`, `Bytes`, path, and readiness runtime facilities, adding only
three operations for inotify creation, registration and removal.
