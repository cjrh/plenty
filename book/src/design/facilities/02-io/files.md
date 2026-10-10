# Owned files and modes

The builtin accepts two positional call forms:

- `open(path: str) -> Result[File, IoError]`
- `open(path: str, mode: str) -> Result[File, IoError]`

Both create an opaque, affine file owner. `open(path)` opens an existing file
for reading, equivalent to `open(path, "r")`. This builtin's optional mode does
not imply support for default parameters or keyword arguments; Plenty supports
neither. Pass an explicit mode positionally. Literal modes are checked at
compile time; computed invalid modes return `IoError.InvalidMode`.

| Mode | Capabilities | Opening behavior |
| --- | --- | --- |
| `r` / `r+` | Read / read and write | Existing file required |
| `w` / `w+` | Write / read and write | Create or truncate |
| `a` / `a+` | Write / read and write | Create or append |
| `x` / `x+` | Write / read and write | Atomic exclusive creation |

Exclusive creation fails for an existing path, including a symlink, without
truncation. `a+` starts at EOF; other modes start at byte zero. Append writes use
OS append semantics even after seeking. Paths and creation permissions follow
the [whole-file rules](whole-files.md). Owner and path storage are allocated
fallibly before opening, so allocation failure cannot truncate the destination.

Files can move, return, live in aggregates, and be borrowed, but cannot be copied.
`file.closed -> bool` observes state. Printing shows `File(open)` or
`File(closed)`; equality is owner identity rather than path equality.
`file.readable()` and `file.writable()` return `Result[bool, IoError]` through
shared access, without allocation. They report the open mode, not a guarantee
that a subsequent OS operation will succeed; closed files return `Closed`.

## Closing and resource scopes

`file.close() -> Result[(), IoError]` requires exclusive access and is idempotent.
It marks the owner closed before closing its descriptor, reports native errors,
and allocates nothing. Automatic destruction closes any live descriptor without
reporting errors or allocating. Closing does not promise durability.

File is an intrinsic context manager: entry lends `&mut File`, exit closes it,
and destruction releases the owner. `with &mut file as stream:` leaves the original
owner closed afterward. Entry permits an already closed file; its operations
report `Closed`.

Exit discards close errors to preserve the body's control flow. Call `close()?`
inside the body when those errors matter. All ordinary
[resource-scope cleanup rules](../../language/08-errors/index.md) apply.
