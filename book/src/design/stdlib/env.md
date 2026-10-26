# std.env

`import std.env` reads the process environment and the working directory.
Names, values, and directories are operating-system byte strings; text is a
checked view of them.

Encoding and validation are checked Plenty; four native primitives read a
variable, snapshot the environment, and read or change the working directory.
The module is available on the supported x86-64 Linux GNU target; elsewhere
every operation reports `IoError.Unsupported`.

## Variables

| Function | Result | Contract |
|---|---|---|
| `get(name: &str)` | `Result[Option[str], IoError]` | The value as text; `Nothing` when unset |
| `get_bytes(name: &list[u8])` | `Result[Option[list[u8]], IoError]` | The value unchanged |
| `vars()` | `Result[list[Variable], IoError]` | A snapshot of every variable |
| `valid_name(name: &list[u8])` | `bool` | Nonempty, with no `=` and no NUL |

`Variable` has two public `list[u8]` fields, `name` and `value`.

- An unset variable is `Ok(Nothing)`. A variable set to the empty string is
  `Ok(Some(""))`.
- A name that is not `valid_name` is `IoError.InvalidInput`: no variable can
  have it.
- `get` reports a value that is not UTF-8 as
  `IoError.Data(DataError.InvalidUtf8)`. It never replaces or drops bytes; read
  such a value with `get_bytes`.
- `vars` returns each variable once, in the order the process stores them. The
  snapshot is detached: it does not follow later changes.
- Each result is a new allocation; failure is
  `IoError.Data(DataError.Allocation(...))`.

There is no operation that sets or removes a variable of the running program.
The environment is process-wide state that other threads and native code read
without synchronization. Give a child its variables through
[`Command.env`](process.md#building-a-command) instead.
Native code embedding Plenty must likewise avoid concurrent mutation of the
process environment while runtime calls read it. Both single-variable lookup
and snapshots copy directly into fallible Plenty storage, without intermediate
host strings or snapshots.

Command-line arguments are the prelude's
[`args` and `args_bytes`](../facilities/02-io/streams-args.md), and typed
options are [`std.cli`](cli.md).

## Working directory

| Function | Result | Contract |
|---|---|---|
| `current_dir()` | `Result[Path, IoError]` | The absolute working directory |
| `set_current_dir(path: &Path)` | `Result[(), IoError]` | Change it for the whole process |

Both use [`std.path.Path`](path.md), so a directory name that is not UTF-8
round-trips. `current_dir` fails with the system error when the directory has
been removed. `set_current_dir` reports a missing directory as
`IoError.System(2)` and leaves the working directory unchanged.
`current_dir()` grows a fallible native buffer and reports allocation failure
as `IoError.Data(DataError.Allocation(...))`.

The working directory is shared by every thread, and relative paths resolve
against it at the moment of each call. Prefer absolute paths, or
[`Command.current_dir`](process.md#building-a-command) for a child, over
changing it in a program with more than one thread. A worker thread may not
call `set_current_dir` at all; the compiler rejects any worker that can reach
it (see [native effects](../facilities/03-concurrency/effects.md)).

## Limits

- Linux x86-64 GNU only.
- No mutation of the program's own environment.
- No executable-path lookup. The home and XDG base directories are
  [`std.fs.dirs`](fs/dirs.md); temporary-directory selection is
  [`std.tempfile.default_dir`](tempfile.md#constructors).
