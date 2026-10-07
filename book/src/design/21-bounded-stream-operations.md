# Bounded stream operations

`writelines(lines: list[str]) -> Result[(), IoError]` observes the list (or a
shared list reference) once before borrowing the destination exclusively. It
writes each string's exact UTF-8 bytes, inserts no separators, and leaves the
list available. After argument construction it allocates nothing. Empty lists
still validate the file and write capability. Errors may leave a written prefix;
the list and file remain valid. Generic iterable sources are deferred.

`readlines() -> Result[list[str], IoError]` reads the remaining lines into an
independent list, keeping translated terminators and omitting a synthetic empty
line at EOF. Empty input yields an empty list. Every buffer, line, and list growth
is fallible; errors discard the initialized prefix and leave the file closable.
Input may have advanced on error. This eager operation requires exclusive access;
use bounded `readline` for bounded memory. Size hints and lazy iteration are deferred.

`File.readable()` and `File.writable()` return `Result[bool, IoError]` using
shared access and no allocation. They report the open mode's capabilities,
not a guarantee that a future OS operation will succeed. Closed files return an
error. All reads, including zero-length reads, reject write-only files.

`readline(count: i64)` uses the same scalar limits as `read(count)` but stops
after the first translated newline. A negative count reads one complete line.
The next call resumes a partial line; a zero limit does not consume a pending LF.

File `read(count: i64)` reads at most that many Unicode scalars after universal
newline translation. Zero returns an allocated empty string without consuming
input; negative counts read to EOF. Bounded reads do not read past a scalar.
Malformed or truncated UTF-8 returns `IoError.Data(InvalidUtf8)`; errors may
consume input, and allocation failure leaves the owner valid for cleanup.
