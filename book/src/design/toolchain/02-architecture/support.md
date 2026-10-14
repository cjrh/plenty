# Support index

The reference describes the supported subset, not Python's full API. Detailed
restrictions live with their contracts.

| Area | Contract and principal limits |
|---|---|
| Source and modules | [Declarations, imports, visibility, and entrypoints](../../language/02-source/index.md); one source root, no import cycles or re-exports |
| Types and data | [Types](../../language/03-types/index.md); fixed layouts and recursive data through heap boundaries |
| Expressions | [Operators, assignment, and control flow](../../language/04-expressions/index.md); blocks and expressions [nest to 64 levels](../../language/04-expressions/index.md#nesting) |
| Ownership | [Moves, checked borrows, returned references, and cleanup](../../language/05-ownership/index.md); general stored references unsupported |
| Functions | [Named functions and concrete closures](../../language/06-functions/index.md); explicit fallible [shared-call erasure](../../language/06-functions/05-boxed-callables.md) |
| Generics | [Concrete specialization and structural protocols](../../language/07-generics/index.md); no dynamic protocol dispatch |
| Errors and resources | [Sums, propagation, allocation, and context scopes](../../language/08-errors/index.md); traps do not unwind; explicit [std.error](../../stdlib/error.md) contexts |
| Iteration | [Loops, comprehensions, and native generators](../../language/09-iteration/index.md); no reference yield or public borrow across suspension |
| Built-ins | [Numeric, text, and collection operations](../../facilities/01-builtins/index.md); explicitly fallible allocating operations |
| Standard library | Embedded [std.math](../../stdlib/math.md), [std.path](../../stdlib/path.md), [std.fs](../../stdlib/fs.md), [std.deque](../../stdlib/deque.md), and [std.bytes](../../stdlib/bytes.md); typed errors, Linux paths/metadata and walking, owned double-ended queues, and compact byte buffers. [std.time](../../stdlib/time.md) adds checked durations, monotonic deadlines, and wall timestamps. [std.io](../../stdlib/io.md) adds partial-progress byte streams, retry helpers, and fixed-capacity buffering over files, process streams, and memory. [std.encoding](../../stdlib/encoding.md) adds strict hex/base64 codecs and explicit-endian integer packing. [std.collections](../../stdlib/collections.md) adds binary heaps and [std.iter](../../stdlib/iter.md) generator adapters |
| I/O | [Text/byte streams, raw Unix paths, and owned UTF-8 files](../../facilities/02-io/index.md); direct file iteration unsupported |
| Concurrency | [Threads, channels, pools, futures, and parallel operations](../../facilities/03-concurrency/index.md); certified worker effects, serial ordinary loops |
| Interoperability | [Trusted C imports, exports, metadata, and runtime loading](../../facilities/04-interop/index.md) |
| Native toolchain | [Commands and artifacts](../01-building/index.md); `x86_64-unknown-linux-gnu` only |

There is no interpreter, REPL, JIT, async/await, inheritance, dynamic attributes,
or exception unwinding. Future work and priorities remain in the
[backlog](../../../backlog.md).
