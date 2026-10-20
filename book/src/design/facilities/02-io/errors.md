# I/O errors

`IoError` is an allocation-free enum:

| Variant | Meaning |
| --- | --- |
| `System(i32)` | Native OS error code, only when the OS supplied one |
| `Data(DataError.InvalidUtf8)` | Invalid or truncated UTF-8 |
| `Data(DataError.Allocation(AllocError))` | Recoverable buffer/result allocation failure |
| `InvalidMode` | Unsupported computed `open` mode |
| `Closed` | File is closed |
| `NotReadable` | File lacks read capability |
| `NotWritable` | File lacks write capability |
| `InvalidInput` | Invalid argument, such as NUL in a path or negative truncation size |
| `Unsupported` | Unsupported host operation |
| `Other` | Failure without a native code or more specific variant |

Constructing, matching, and propagating these errors adds no allocation.
The [ordinary error rules](../../language/08-errors/index.md) determine whether
details are retained or erased by builtin `Failure`.

File operations report `Closed` before mode errors. Reads and writes validate
capabilities even for zero-length work. Negative read limits mean unbounded
reading; they are valid, unlike negative truncation sizes.

An error can follow consumed input, a written prefix, or other external effects.
The operation's page states its cleanup and preservation guarantees.
