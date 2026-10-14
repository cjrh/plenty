# std.process

`import std.process` starts child processes from an argument vector and owns
them until they are reaped. Arguments, variables, and directories are
operating-system byte strings; pipes carry [`std.bytes`](bytes.md) data.

No shell takes part unless the program is one. An argument reaches the child
exactly as given: spaces, quotes, `$`, `;`, `|`, `*`, and newlines are ordinary
bytes. [`shell`](#building-a-command) is the one, named way to run shell
syntax.

Request encoding, pipe draining, status decoding, and the reaping rules are
checked Plenty. Five native primitives spawn, hand over a pipe end, wait,
signal, and poll. The module is available on the supported x86-64 Linux GNU
target; elsewhere `spawn` reports `IoError.Unsupported`.

## Building a command

| Function | Result | Contract |
|---|---|---|
| `command(program: &str)` | `Result[Command, IoError]` | A command with no arguments |
| `command_bytes(program: &list[u8])` | `Result[Command, IoError]` | The same for a name that is not UTF-8 |
| `shell(script: &str)` | `Result[Command, IoError]` | `/bin/sh -c script` |

A program name containing `/` is a path. Any other name is looked up in the
directories of the child's `PATH`. The name is also the child's `argv[0]`.

`shell` passes `script` to `/bin/sh` as one argument. The script is shell
syntax, so text placed in it is interpreted: never build one from untrusted
input. Pass such data as an argument or a variable instead.

`Command` methods take `self: &mut Command`:

| Method | Result | Effect |
|---|---|---|
| `arg(value: &str)` | `Result[(), IoError]` | Append one argument |
| `arg_bytes(value: &list[u8])` | `Result[(), IoError]` | Append one argument of any bytes |
| `arg_path(value: &Path)` | `Result[(), IoError]` | Append a [path](path.md) losslessly |
| `env(name: &str, value: &str)` | `Result[(), IoError]` | Set a variable for the child |
| `env_bytes(name: &list[u8], value: &list[u8])` | `Result[(), IoError]` | The same for any bytes |
| `env_remove(name: &str)`, `env_remove_bytes(name: &list[u8])` | `Result[(), IoError]` | The child does not inherit it |
| `env_clear()` | `()` | The child inherits nothing; earlier edits are dropped |
| `current_dir(path: &Path)` | `Result[(), IoError]` | The child's working directory |
| `stdin(mode: Stdio)`, `stdout(mode: Stdio)`, `stderr(mode: Stdio)` | `()` | Choose a stream |

- A child inherits the parent's environment and working directory unless the
  command says otherwise. Edits apply in order, so the last one for a name
  wins. Nothing changes in the parent.
- A piece containing a NUL byte, an empty program or directory, and a variable
  name that is empty or contains `=` are `IoError.InvalidInput`. A failed
  method leaves the command exactly as it was.
- An empty argument is valid and is passed as an empty argument.
- Allocation failure is `IoError.Data(DataError.Allocation(...))`.

`Stdio` selects each standard stream:

| Mode | The child's stream |
|---|---|
| `Stdio.Inherit` | The parent's own stream. The default |
| `Stdio.Null` | `/dev/null` |
| `Stdio.Piped` | A new pipe whose other end the `Child` owns |

## Spawning

`command.spawn()` borrows the command and returns `Result[Child, IoError]`. A
command is reusable: each call starts another child.

A program that cannot be started is an error value, not a child that exits
with a conventional code: a missing program or directory is
`IoError.System(2)`, a file without execute permission `IoError.System(13)`.
A failed spawn leaves no process and no open pipe.

Pipe ends are close-on-exec, so one child never inherits another child's
pipes. Other descriptors the program opened are close-on-exec as well.

## Child

A `Child` is the single owner of one process. It is not copyable.

| Method | Result | Contract |
|---|---|---|
| `id()` | `i64` | The process id |
| `take_stdin()`, `take_stdout()`, `take_stderr()` | `Option[File]` | Move a pipe end out, once |
| `close_stdin()` | `Result[(), IoError]` | End the child's input |
| `wait()` | `Result[ExitStatus, IoError]` | Close stdin, block until the child ends, reap it |
| `try_wait()` | `Result[Option[ExitStatus], IoError]` | Reap if ended; `Nothing` while running |
| `wait_timeout(budget: &Duration)` | `Result[Option[ExitStatus], IoError]` | `Nothing` when the budget ran out |
| `signal(number: i32)` | `Result[(), IoError]` | Send signal 1 to 64 |
| `terminate()` | `Result[(), IoError]` | `SIGTERM`: a request the child may ignore |
| `kill()` | `Result[(), IoError]` | `SIGKILL`: cannot be caught |
| `communicate(input: &Bytes, limit: i64)` | `Result[Output, StreamError]` | Feed stdin, collect both outputs, wait |
| `output(limit: i64)` | `Result[Output, StreamError]` | `communicate` with no input |

Methods that reap or move a pipe take `self: &mut Child`; `id` and the signal
methods borrow.

### Pipes

A taken pipe end is an ordinary [`File`](../facilities/02-io/files.md): stdin
is write-only, stdout and stderr are read-only, and none can seek. Use
[`Bytes.read_file` and `Bytes.write_file`](bytes.md) directly, or wrap the end
with [`std.io.from_file`](io.md) for the retry helpers and buffering.

- A read returns the bytes that have arrived, possibly fewer than asked, and 0
  at end of input, which is when the child and every process that inherited
  the stream have closed it.
- A write to a child that no longer reads is `IoError.System(32)` (`EPIPE`).
  The program is not sent `SIGPIPE` for it.
- The child sees end of input only when stdin is closed. `wait` and
  `communicate` close a stdin the `Child` still holds; a taken end is the
  caller's to close.

Reading one pipe to the end while the child blocks writing another, or writing
a large input while the child blocks writing output, is a deadlock: a pipe
holds 64 KiB. Use `communicate`, which serves all three in one loop.

### Waiting and status

`ExitStatus` describes how the child ended:

| Method | Result | Contract |
|---|---|---|
| `success()` | `bool` | Exit code 0 |
| `code()` | `Option[i32]` | The exit code, 0 to 255; `Nothing` if a signal ended it |
| `signal()` | `Option[i32]` | The terminating signal; `Nothing` if it exited |

- The first successful `wait`, `try_wait`, or `wait_timeout` that sees the
  child ended reaps it. The status is kept, and later calls return it without
  asking the operating system.
- `wait` blocks without a limit. It does not drain pipes: a child blocked
  writing to a full pipe that nobody reads never ends.
- `wait_timeout` polls `try_wait` with pauses doubling from 1 ms to 32 ms, so
  it can report an exit up to 32 ms late. It leaves stdin open and the child
  running; a budget beyond the [monotonic clock](time.md) is
  `IoError.InvalidInput`.
- Signals reach only a child that has not been reaped. Afterwards the methods
  return `Ok(())` and send nothing, so a number can never reach an unrelated
  process that received the same id. A number outside 1 to 64 is
  `IoError.InvalidInput`.
- A signalled child still has to be waited for.

### Collecting output

`communicate` writes `input` to a piped stdin, appends piped stdout and stderr
to two new buffers as data arrives, closes each stream at its end, then waits.
`Output` has public fields `status: ExitStatus`, `stdout: Bytes`, and
`stderr: Bytes`.

- Streams that are not piped, or whose end was taken, are skipped and their
  buffers stay empty.
- `limit` bounds each output separately. A stream that produces more is
  `StreamError.OutOfRange`; at most `limit + 1` of its bytes were buffered. The
  child is then still running and still owned: kill or wait for it.
- A child that stops reading ends the feed without an error. Its status says
  what happened.
- Interrupted calls are retried. Other failures are the
  [`StreamError`](io.md) of the failing transfer.
- Memory is the two outputs, grown in 8 KiB steps. Input is written in 4 KiB
  pieces and is not copied.

## Ownership and reaping

Plenty has no user destructors, so dropping a `Child` closes the pipe ends it
still holds and does nothing else: the process is neither stopped nor reaped.
Its runtime record, and its zombie once it ends, last until the program exits.
A build with runtime allocation checks reports that record as a leak. Every
child should end in a `wait`, `try_wait`, or `wait_timeout` that returns a
status.

A child that outlives the program is adopted by the system as usual.

## Limits

- Linux x86-64 GNU only. Starting a child uses the host allocator briefly for
  its argument vector; that one allocation is outside fallible accounting.
- No redirection of a stream to a file or to another child, and no merging of
  stderr into stdout. `shell` can do both.
- `wait_timeout` polls. There is no blocking wait with a deadline and no
  cancellation token.
- No process groups, sessions, user or group changes, resource limits, or
  detached children.
- `communicate` holds whole outputs in memory. For streaming, take the pipes
  and give each its own [thread](thread.md).
