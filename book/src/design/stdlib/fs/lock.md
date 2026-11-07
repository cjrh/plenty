# std.fs.lock — advisory file locks

`std.fs.lock` coordinates cooperating processes through Linux `flock` locks
on a regular file. It supports shared readers and an exclusive owner, with
nonblocking acquisition or a total deadline and cancellation token. The lock
is advisory: other processes can still read or write without acquiring it.

```plenty
import std.fs.lock as lock
import std.path as path
from std.fs.lock import Mode, LockError

def main() -> Result[(), IoError | LockError]:
    file = path.from_text("job.lock")?
    mut owner = lock.acquire_now(&file, Mode.Exclusive)?
    print(lock.acquire_now(&file, Mode.Shared).is_err())?
    owner.close()?
    another = lock.acquire_now(&file, Mode.Shared)?
    print(another.is_closed())?
    Ok(())
```
```output
True
False
```

## Acquisition

| Function | Result and behavior |
|---|---|
| `acquire_now(path: &Path, mode: Mode)` | `Result[Lock, LockError]`: one nonblocking attempt; contention returns `WouldBlock` |
| `acquire_before(path: &Path, mode: Mode, deadline: &Deadline, cancel: &CancellationToken)` | `Result[Lock, LockError]`: retries contention until acquired, cancelled or timed out |

`Mode.Shared` permits other shared owners. `Mode.Exclusive` excludes both
modes. Each call opens its own file description, so independent acquisitions
in the same process contend too. There is no upgrade/downgrade operation or
fairness guarantee. Use a consistent acquisition order for multiple resources.

A missing file is created with ordinary permission bits `0600`, restricted by
the process umask. An existing file is opened read/write without truncating
its contents or changing its permissions. No writes are made. The final path
component must not be a symlink and the opened object must be a regular file;
opening a FIFO cannot block. Ancestor components follow normal OS resolution.
Use a trusted parent directory. Do not unlink or replace a live lockfile:
locks protect the opened inode, and a replacement would create an independent
lock domain. These functions never remove the lockfile, even after failure.

The borrowed native-byte `Path` must contain 1–4096 bytes and no NUL, otherwise
`InvalidPath` is returned before opening. OS path limits still apply. The
implementation allocates bounded path scratch and one owned `File`; allocation
failure returns `Allocation(AllocError)`. The small lock facade adds no buffer
or queue. Failures close any descriptor already opened.

Waiting checks cancellation before deadline expiry, and both before opening
and before every lock attempt. An already expired deadline returns `TimedOut`
even if the lock would be available. After contention it waits on the token
for at most ten milliseconds or the remaining budget, whichever is smaller;
cancellation wakes that wait. `EINTR` from a lock attempt retries after checking
the token and deadline; the nonblocking form preserves it as `Io(IoError.System)`.
Filesystem opening and metadata syscalls themselves cannot be interrupted by
the token or bounded by the deadline, including on remote filesystems.
Completion can race cancellation after the final observation.

## Ownership and release

`Lock` is an affine owner with a private descriptor. It can move to a worker
or return from one; dropping it in that worker releases its ownership normally.

- `close(self: &mut Lock) -> Result[(), LockError]` relinquishes this
  descriptor. Repeated close is harmless, including after a close error.
- `inherit_into(self, command: &mut Command) -> Result[i32, LockError]` adds an
  owned copy to a [process command](../process.md#explicit-file-inheritance)
  and returns its child descriptor. The source stays owned here.
- `is_closed(self) -> bool` reports whether this owner has been closed.
- Drop closes the descriptor without allocating or reporting a close error,
  including on return, `?`, and scope exit. It never issues `LOCK_UN`.

On local Linux filesystems a lock belongs to the open file description.
Closing its final descriptor, or process death without surviving inherited
copies, releases it. A descriptor inherited through `fork` retains the lock
until that copy is also closed, even if the original owner closes or dies.
An explicit `LOCK_UN` would instead unlock all copies, which is why this API
has no separate unlock operation.

Descriptors are created close-on-exec. Normal [`std.process`](../process.md)
children do **not** retain them across exec. To keep a lock alive in a child,
call `owner.inherit_into(&mut command)`, pass the returned descriptor in the
child's argv or environment according to its protocol, then spawn. It adds an
owned copy to that command only, without changing the parent's descriptor flags.
The child can close its descriptor to release its share of the lock.

The command itself also keeps a copy for reuse. After a successful spawn,
`command.clear_inherited_files()` relinquishes those copies; closing the
original owner then leaves the child as the sole holder. Runner death releases
all parent copies, but the child still holds the lock until it closes its copy
or exits. Multiple children can share that description, and release waits for
the last copy, including copies passed on to further descendants by the child.
The child's descriptor protocol must close copies it no longer needs. A failed
spawn leaves the command copies owned for retry or
explicit clearing. This is an open-file-description lifetime, not a lease.

Inheritance accepts at most 64 files per command. A closed lock returns
`Io(IoError.Closed)`; too many selections return `Io(IoError.InvalidInput)`.
Native duplication failures preserve their OS error, and allocation failures
return `Allocation(AllocError)`. Failure leaves the lock and command unchanged.
No raw descriptor close or unlock operation is exposed.

This API is supported on 64-bit Linux; other targets return
`Io(IoError.Unsupported)`. Network filesystems can emulate or restrict `flock`
and have different failure/mandatory-I/O semantics; consult the mounted
filesystem's contract. It does not provide a distributed lease or fencing token.

## Errors

`LockError` has `WouldBlock`, `TimedOut`, `Cancelled`, `InvalidPath`,
`Io(IoError)`, and `Allocation(AllocError)`. Native errors retain their OS code,
including access denial, unsupported locking, symlinks, and missing parents.
Closing only reports `Io` failures. Cancellation/timeouts consume no caller
resource: paths, deadlines, and tokens are borrowed, and no partial owner escapes.

## Acknowledgements

- [Linux flock](https://man7.org/linux/man-pages/man2/flock.2.html) defines the
  advisory locking and open-file-description lifetime semantics.
- [libc](https://github.com/rust-lang/libc), MIT OR Apache-2.0: the narrow native
  wrappers use its Linux syscall bindings. Waiting and error policy are Plenty.
