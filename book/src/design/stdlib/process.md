# std.process

`import std.process` starts child processes from an argument vector and owns
them until they are reaped. Arguments, variables, and directories are
operating-system byte strings; pipes carry [`std.bytes`](bytes.md) data.

No shell takes part unless the program is one. An argument reaches the child
exactly as given: spaces, quotes, `$`, `;`, `|`, `*`, and newlines are ordinary
bytes. [`shell`](#building-a-command) is the one, named way to run shell
syntax. [`std.shlex`](shlex.md) can split a shell-style argument string before
passing each resulting word to `arg`; it does not execute or expand shell syntax.

Request encoding, pipe draining, status decoding, and the reaping rules are
checked Plenty. Five native primitives spawn, hand over a pipe end, wait,
signal a child or its isolated group, and poll. The module is available on the supported x86-64 Linux GNU
target; elsewhere `spawn` reports `IoError.Unsupported`.

```plenty
import std.process
from std.bytes import BytesError
from std.io import StreamError
from std.process import Stdio

def main() -> Result[(), AllocError | IoError | StreamError | BytesError]:
    mut echo = std.process.command("echo")?
    echo.arg("hello from a child")?
    echo.stdout(Stdio.Piped)
    mut child = echo.spawn()?
    output = child.output(4096)?
    print(output.status.success(), output.stdout.to_text()?.strip()?)?
    Ok(())
```
```output
True hello from a child
```

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
| `process_group(mode: ProcessGroup)` | `()` | Select inherited group, new group or new session before execution |
| `stdin(mode: Stdio)`, `stdout(mode: Stdio)`, `stderr(mode: Stdio)` | `()` | Choose a stream |
| `stderr_to_stdout()` | `()` | The child's stderr goes wherever its stdout goes |

- A child inherits the parent's environment and working directory unless the
  command says otherwise. Edits apply in order, so the last one for a name
  wins. Nothing changes in the parent.
- A piece containing a NUL byte, an empty program or directory, and a variable
  name that is empty or contains `=` are `IoError.InvalidInput`. A failed
  method leaves the command exactly as it was.
- An empty argument is valid and is passed as an empty argument.
- Allocation failure is `IoError.Data(DataError.Allocation(...))`. Unrepresentable
  argument, environment and combined request sizes preserve
  `AllocError.CapacityOverflow` in that cause, before mutation or starting a child.
- Appending a terminated byte string reserves only its new bytes and NUL, not
  bytes already held. Existing capacity can satisfy the operation without an
  allocation; failed growth leaves the original command unchanged.

`Stdio` selects each standard stream:

| Mode | The child's stream |
|---|---|
| `Stdio.Inherit` | The parent's own stream. The default |
| `Stdio.Null` | `/dev/null` |
| `Stdio.Piped` | A new pipe whose other end the `Child` owns |
| `Stdio.File(file)` | A duplicate of `file`'s descriptor |

`Stdio.File` moves the [`File`](../facilities/02-io/files.md) into the
command, which keeps it for every later `spawn`. The open mode decides what
the child writes over: open with `"a"` to append to a log and keep its bytes,
`"w"` to truncate it once when it is opened. The child shares the file's open
description, so a file opened for appending stays in append mode and every
write by a child, another child, or the program lands at the end. A stdout or
stderr file must be writable and a stdin file readable. Choosing another mode
for that stream closes the file; dropping the command closes all of them.
Setting a stream never allocates.

`stderr_to_stdout()` makes stderr the same stream as stdout, whatever stdout
is when the child starts: the same file, the parent's stdout, `/dev/null`, or
the one pipe, so `communicate` and `output` return both streams in `stdout`
and `stderr` stays empty. A later `stderr(mode)` replaces it.

```plenty
import std.process
from std.process import Stdio

def main() -> Result[(), AllocError | IoError]:
    with open("build.log", "w")? as log:
        log.write("earlier run\n")?
    mut build = std.process.shell("echo compiled; echo warning >&2")?
    build.stdout(Stdio.File(open("build.log", "a")?))
    build.stderr_to_stdout()
    mut child = build.spawn()?
    print(child.wait()?.success())?
    with open("build.log")? as log:
        print(log.read()?.strip()?)?
    Ok(())
```
```output
True
earlier run
compiled
warning
```

## Spawning

`command.spawn()` borrows the command and returns `Result[Child, IoError]`. A
command is reusable: each call starts another child.

A program that cannot be started is an error value, not a child that exits
with a conventional code: a missing program or directory is
`IoError.System(2)`, a file without execute permission `IoError.System(13)`.
A failed spawn leaves no process and no open pipe. A redirection file that
is closed is `IoError.Closed`; one without the needed access is
`IoError.NotWritable` for stdout or stderr and `IoError.NotReadable` for
stdin. The command keeps its files either way.

Pipe ends, and the duplicates of redirection files, are close-on-exec, so
one child never inherits another child's pipes or files. Other descriptors
the program opened are close-on-exec as well.

Close-on-exec still leaves a short window: a child forked by one thread holds
a copy of every descriptor open at that moment until its exec closes them. If
another thread had a program open for writing then, running that program just
after closing it fails with "text file busy" (`IoError.System(26)`). `spawn`
retries that error after waits of 1, 2, 4, 8, 16, 32 and 64 ms, then reports
it, so a busy program delays `spawn` by at most 127 ms. Spawns in one program
take turns, and a retrying spawn keeps its turn while it waits, so no new child
can pick up a descriptor meanwhile; other threads' child operations other than
a blocking `wait` wait with it. `spawn` takes no deadline or cancellation
token. Waiting allocates nothing; each attempt repeats the brief host
allocation described under [Limits](#limits). A program another process keeps
open for writing still fails after the last wait.

### Process groups and sessions

`ProcessGroup.Inherit` is the default. `ProcessGroup.NewGroup` creates a new
process group in the parent's session; `ProcessGroup.NewSession` creates a new
session and its initial process group. In both isolated modes the child is the
group leader and its group ID equals `child.id()`. Select the mode with
`command.process_group(mode)`; selecting another mode replaces the choice.
Setup happens in the child **before execution**, without a shell, a `setsid`
executable or a parent-side polling race. Setup failures are spawn errors, with
no child returned. Session setup uses a narrow native `setsid` call; no user
pre-execution callback is exposed.

Descendants normally inherit the group. `signal_group(number)`,
`terminate_group()` and `kill_group()` signal every process still in it.
Inherited-group children return `IoError.InvalidInput` from these methods:
they cannot accidentally signal the parent's group. Ordinary `signal`,
`terminate` and `kill` still target only the direct child.

Send every required group signal **before reaping the leader**. While it is
unreaped, including after it exits, its PID pins the group ID against reuse.
After reaping, group signals return `Ok(())` and send nothing. Consequently a
`try_wait` loop that reaps the leader cannot then clean up its surviving group;
apply cleanup before that wait, or use a supervisor that keeps the leader
alive. A timeout or cancellation that returns without a status preserves the
child for group cleanup. A successful wait that returns a status has reaped it.
Signalling does not wait for descendants to finish or reap them; only the
owned direct child can be waited for here. OS signal errors remain
`IoError.System(code)`.

A process group is not a containment boundary: a descendant can escape by
creating another group or session. Session creation does not detach the child,
redirect streams or change the explicit reaping requirement. Group/session
selection composes with file redirection and `stderr_to_stdout()`; both merged
pipe writers close when their owning group members exit.

## Child

A `Child` is the single owner of one process. It is not copyable.

| Method | Result | Contract |
|---|---|---|
| `id()` | `i64` | The process id |
| `take_stdin()`, `take_stdout()`, `take_stderr()` | `Option[File]` | Move a pipe end out, once |
| `close_stdin()` | `Result[(), IoError]` | End the child's input |
| `wait()` | `Result[ExitStatus, IoError]` | Close stdin, block until the child ends, reap it |
| `try_wait()` | `Result[Option[ExitStatus], IoError]` | Reap if ended; `Nothing` while running |
| `wait_timeout(budget: &Duration, cancel: &CancellationToken)` | `Result[Option[ExitStatus], WaitError]` | `Nothing` when the budget ran out |
| `signal(number: i32)` | `Result[(), IoError]` | Send signal 1 to 64 |
| `terminate()` | `Result[(), IoError]` | `SIGTERM`: a request the child may ignore |
| `kill()` | `Result[(), IoError]` | `SIGKILL`: cannot be caught |
| `signal_group(number: i32)` | `Result[(), IoError]` | Send signal 1 to 64 to an isolated group, before reaping its leader |
| `terminate_group()`, `kill_group()` | `Result[(), IoError]` | Group `SIGTERM` or `SIGKILL` |
| `communicate(input: &[u8], limit: NonNegative[i64])` | `Result[Output, StreamError]` | Feed stdin, collect both outputs, wait |
| `output(limit: NonNegative[i64])` | `Result[Output, StreamError]` | `communicate` with no input |

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
- `wait_before(deadline: &Deadline, cancel: &CancellationToken)` polls
  `try_wait` with pauses of at most 10 ms. `wait_timeout` constructs one total
  deadline from a duration and delegates to it. Both leave stdin open and the
  child running when cancelled or timed out. `WaitError.Cancelled` only ends
  this wait; it never sends a signal. `WaitError.Io` preserves operating-system
  errors; an overflowing budget is `WaitError.Io(IoError.InvalidInput)`.
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
  pieces and is not copied. A next-buffer extent that cannot fit `i64` returns
  `StreamError.Allocation(AllocError.CapacityOverflow)` before growing or reading
  that chunk, including when the extra limit-probe byte is unrepresentable.

Readiness flags and process identifiers use bit masks. Argument counts, status
subtraction and pipe cursors use explicit arithmetic opt-outs justified by
successful storage growth, validated statuses and concrete file progress.
Caller arithmetic modes do not change these boundaries.

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
  its argument vector and the fixed session-setup callback when selected;
  these host allocations are outside fallible accounting.
- No redirection of a stream to another child; `shell` can build a pipeline.
  A file redirection takes a `File`, so open it before setting the stream.
- Timed child waits poll; scheduling and lock contention can delay return
  beyond the 10 ms polling interval.
- No joining arbitrary process groups, user or group changes, resource limits,
  or detached children. Groups cannot be signalled after their leader is reaped,
  and descendants can escape a group or session.
- `communicate` holds whole outputs in memory. For streaming, take the pipes
  and give each its own [thread](thread.md).

## Acknowledgements

- API modelled on Rust's [`std::process::Command`](https://doc.rust-lang.org/std/process/struct.Command.html) and Python's [`subprocess`](https://docs.python.org/3/library/subprocess.html), whose `communicate` it follows.
