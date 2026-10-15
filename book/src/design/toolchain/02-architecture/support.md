# Support index

The reference describes the supported subset, not Python's full API. Detailed
restrictions live with their contracts.

| Area | Contract and principal limits |
|---|---|
| Source and modules | [Declarations, imports, visibility, and entrypoints](../../language/02-source/index.md); one source root per package, no import cycles or re-exports |
| Packages | [`plenty.toml`, path dependencies, `plenty.lock`, debug/release profiles, and `plenty test` with per-process test functions](../01-building/packages.md); git dependencies are validated but not fetched; no build cache |
| Types and data | [Types](../../language/03-types/index.md); fixed layouts and recursive data through heap boundaries |
| Expressions | [Operators, assignment, and control flow](../../language/04-expressions/index.md); blocks and expressions [nest to 64 levels](../../language/04-expressions/index.md#nesting) |
| Ownership | [Moves, checked borrows, returned references, and cleanup](../../language/05-ownership/index.md); general stored references unsupported |
| Functions | [Named functions and concrete closures](../../language/06-functions/index.md); explicit fallible [shared-call erasure](../../language/06-functions/05-boxed-callables.md) |
| Generics | [Concrete specialization and structural protocols](../../language/07-generics/index.md); no dynamic protocol dispatch |
| Errors and resources | [Sums, propagation, allocation, and context scopes](../../language/08-errors/index.md); traps do not unwind; explicit [std.error](../../stdlib/error.md) contexts |
| Iteration | [Loops, comprehensions, and native generators](../../language/09-iteration/index.md); no reference yield or public borrow across suspension |
| Built-ins | [Numeric, text, and collection operations](../../facilities/01-builtins/index.md); explicitly fallible allocating operations |
| Standard library | Embedded [std.math](../../stdlib/math.md), [std.path](../../stdlib/path.md), [std.fs](../../stdlib/fs.md), [std.deque](../../stdlib/deque.md), and [std.bytes](../../stdlib/bytes.md); typed errors, Linux paths/metadata and walking, owned double-ended queues, and compact byte buffers. [std.time](../../stdlib/time.md) adds checked durations, monotonic deadlines, and wall timestamps. [std.io](../../stdlib/io.md) adds partial-progress byte streams, retry helpers, and fixed-capacity buffering over files, process streams, and memory. [std.encoding](../../stdlib/encoding.md) adds strict hex/base64 codecs and explicit-endian integer packing. [std.url](../../stdlib/url.md) adds strict RFC 3986 URL parsing, component-aware percent encoding, and query pairs. [std.collections](../../stdlib/collections.md) adds binary heaps and [std.iter](../../stdlib/iter.md) generator adapters. [std.csv](../../stdlib/csv.md) adds bounded streaming CSV records and writers. [std.random](../../stdlib/random.md) adds caller-owned seeded generators and [std.secrets](../../stdlib/secrets.md) OS-backed secret bytes and tokens. [std.crypto](../../stdlib/crypto.md) adds SHA-256/SHA-512 digests, HMAC, and constant-time comparison. [std.compress](../../stdlib/compress.md) adds bounded gzip, zlib, and DEFLATE streams. [std.archive](../../stdlib/archive.md) adds TAR and ZIP reading, writing, and confined extraction. [std.regex](../../stdlib/regex.md) adds linear-time regular expressions with explicit resource limits. [std.json](../../stdlib/json.md) adds strict, bounded JSON: a resumable event decoder, a validating encoder, and a value tree. [std.log](../../stdlib/log.md) adds leveled structured events with explicit, bounded sinks. [std.toml](../../stdlib/toml.md) adds strict, bounded TOML 1.0 parsing and writing over a value tree. [std.env](../../stdlib/env.md) reads variables and the working directory, and [std.process](../../stdlib/process.md) spawns owned children from argument vectors without a shell, with pipes, bounded output collection, signals, and explicit reaping. [std.ready](../../stdlib/ready.md) adds readiness waits on handles under a deadline and cancellation token. [std.test](../../stdlib/test.md) adds assertions and fixtures for `plenty test` |
| I/O | [Text/byte streams, raw Unix paths, and owned UTF-8 files](../../facilities/02-io/index.md); direct file iteration unsupported |
| Concurrency | [Threads, channels, pools, futures, and parallel operations](../../facilities/03-concurrency/index.md); certified worker effects, serial ordinary loops |
| Interoperability | [Trusted C imports, exports, metadata, and runtime loading](../../facilities/04-interop/index.md) |
| Native toolchain | [Commands and artifacts](../01-building/index.md); `x86_64-unknown-linux-gnu` only |

There is no interpreter, REPL, JIT, async/await, inheritance, dynamic attributes,
or exception unwinding. Future work and priorities remain in the
[backlog](../../../backlog.md).
