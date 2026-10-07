# Owned files

`open(path: str, mode: str = "r") -> Result[File, IoError]` creates an opaque,
affine file owner. Supported modes are `r` (existing read-only), `w` (create or
truncate), `a` (create or append), and `x` (exclusive creation). Invalid modes or NUL paths return errors.
Exclusive creation uses the OS atomic create-new operation and fails if the path
already exists, including a symlink; it never truncates an existing destination.
The initial backend is Linux, with the same flags, permissions, path rules, and
unsupported-host behavior as the whole-file helpers. Binary modes, encodings,
and public descriptors are not exposed yet.
Modes `r+`, `w+`, `a+`, and `x+` add both read and write capabilities with the same
existence/truncation rules. `a+` initially seeks to EOF; all append writes use
OS append semantics even after seeking. Other modes begin at byte zero.
Update streams are unbuffered, so switching operations needs no flush. Writes
use the physical byte position, may overwrite part of a UTF-8 sequence, and
nonempty writes discard pending CRLF translation state. They do not insert text.

`file.closed` observes its state. `file.close() -> Result[(), IoError]` requires
exclusive access; it marks the owner closed before closing the descriptor and is
idempotent. Automatic destruction closes any remaining handle without allocating
or reporting errors. Explicit close reports OS errors; neither form promises
durability. File owners cannot be copied, but can be moved, stored in aggregates,
returned, or borrowed. Printing shows `File(open)` or `File(closed)`; equality is
owner identity, not path equality.

Both the owner header and path conversion allocate fallibly before opening the
descriptor, so allocation failure cannot truncate a destination. Open failures
release the reserved owner. The Rust runtime owns the native descriptor in an
Option and uses the ordinary intrusive destruction queue; File adds no public
ABI layout or descriptor escape hatch.

`file.read() -> Result[str, IoError]` exclusively borrows a live file and reads
from its current position to EOF. It returns independent UTF-8 text with CRLF
and bare CR normalized to LF, preserving NUL. EOF returns an empty string.
Closed handles and rejected access modes return `IoError.System(0)`; native OS
errors retain their codes. Read or allocation errors may advance the position, and invalid
UTF-8 is reported after consuming input. No rollback or concurrent-file snapshot
is promised. Output and temporary buffers allocate fallibly.

File implements an intrinsic context protocol: entry returns `&mut File`, exit
closes it, and normal destruction releases its owner. It uses the same checked
loans and exit paths as user-defined classes. `with &mut file as stream:` leaves
the original owner closed on exit. Entry of an already closed file is allowed;
operations report the closed-state error. Automatic exit discards close errors
to preserve the body's control flow; use explicit `close()?` in the body when
the caller must handle them.

`file.write(text) -> Result[i64, IoError]` borrows and writes exact UTF-8 bytes,
without adding or translating newlines. Its count is Unicode scalars. Evaluate
the text once before taking the exclusive receiver loan. The handle must be
open for writing, including for empty text. Errors can leave partial output;
append mode uses OS append semantics but a whole logical write need not be one
atomic record. The runtime does not allocate a write buffer.

`file.flush() -> Result[(), IoError]` flushes runtime buffers; files are currently
unbuffered so it validates the open state and normally does no extra work.
`file.sync() -> Result[(), IoError]` requests OS synchronization of file data and
metadata. Neither close nor flush substitutes for sync. Filesystem, device, and
directory-entry durability rules still apply; sync is not an atomic-save API.
Both methods require exclusive access and report errors without allocating.
`file.readline() -> Result[str, IoError]` retains a line's terminating newline,
normalizing LF, CRLF, or bare CR to LF. EOF returns `""`; an empty line is `"\n"`.
A final unterminated line is returned without adding a newline. It validates
UTF-8 per line and allocates both its buffer and result fallibly. An error can
consume part or all of a line; the owner remains valid for another operation or
close. Reading EOF still constructs a fallible empty string.

The reader stops at CR immediately and records a one-bit pending-LF state. Its
next read consumes an optional LF, so it never reads beyond a line just to find
out whether CR was followed by LF. `read()` honors the same state before resuming
bulk reads. Line reading currently uses byte reads; buffering,
file iteration and configurable encodings remain future work. Saved text positions
are supported as described below.
