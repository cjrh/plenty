# Whole-file helpers

Paths are UTF-8 strings, absolute or relative to the process working directory.
NUL in a path returns `IoError.InvalidInput`; ordinary symlink resolution applies.
Arguments are observed once in source order and remain available. All temporary
storage is fallible, and descriptors are closed on success and failure.

| Operation | Result | Behavior |
| --- | --- | --- |
| `read_text(path)` | `Result[str, IoError]` | Read a whole file, translating CRLF and bare CR to LF |
| `write_text(path, text)` | `Result[i64, IoError]` | Create or truncate, then write exact UTF-8 |
| `append_text(path, text)` | `Result[i64, IoError]` | Create or append exact UTF-8 without truncating |

`read_text` preserves content NUL, validates UTF-8 strictly, and does not remove
a BOM or use a locale. Path conversion, buffer growth, and output construction
are recoverable; errors reclaim partial buffers. It does not promise a snapshot
against concurrent writers.

`write_text` and `append_text` return the Unicode-scalar count. Path allocation
precedes opening, so allocation failure cannot truncate or append to the file.
No output allocation follows opening. OS errors can leave a truncated destination
or written prefix. Success checks close errors but does not imply synchronization
or atomic replacement.

Creation uses `0666` restricted by the process umask. Append creates a missing
file even for empty text. Each native write appends at the current end, but one
logical call may use several writes and is not an atomic concurrent record.
For explicit lifetime and cursor control, use [File](files.md).
For raw path bytes and untranslated contents, use [read_bytes and write_bytes](bytes-paths.md).
