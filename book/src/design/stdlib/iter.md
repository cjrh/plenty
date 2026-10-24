# std.iter (removed)

`std.iter` has been removed. Its generator functions `map`, `filter`, `take`,
`fold` and `try_fold` are replaced by
[fluent pipelines](../language/09-iteration/06-pipelines.md), which need no
import:

| Removed call | Pipeline |
|---|---|
| `map(source, f)` | `source.map(f)` |
| `filter(source, keep)` | `source.filter(keep)` |
| `take(source, n)` | `source.take(n)` |
| `fold(source, initial, f)` | `source.fold(initial, f)` |
| `try_fold(source, initial, f)` | `source.try_fold(initial, f)` |

Generators have these methods directly; start other sources with `iter(source)`.
End an owned chain with `.generator()` to return or pass it as a resumable
stream. Importing `std.iter` is an unknown-module error.

This page remains so older links resolve.
