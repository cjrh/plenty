# Standard library

The standard library is the set of modules under the reserved `std` namespace.
They are written in Plenty or expose existing compiler facilities, and are embedded in the compiler: importing one needs no
source installation, network, Cargo, or rustc. Native compilation still requires
the supported [linker environment](../toolchain/01-building/linking.md).
Using `std.dataframe` additionally requires its separately installed
[optional native engine](dataframe-engine.md); unused imports need no engine.

A standard module must be imported. Names available without an import belong to
the [prelude](../language/02-source/names.md) and are specified under
[built-in facilities](../facilities/index.md).

## Modules

| Module | Contents | Error type |
|---|---|---|
| [`std.archive`](archive.md) | TAR and ZIP readers and writers over `std.io`, and extraction into a new directory under explicit path, link, and size limits | `ArchiveError` |
| [`std.atomic`](atomic.md) | Inline atomic integers and booleans with compile-time checked orderings, and fences | None |
| [`std.bytes`](bytes.md) | Compact owning byte buffers with checked ranges, explicit copies and validated text conversion | `BytesError` |
| [`std.channel`](channel.md) | Bounded channels, endpoints and selection | Channel/send/receive/selection errors |
| [`std.cli`](cli.md) | Explicit typed options/positionals, repeatable values, generated help and lossless path arguments | `CliError` |
| [`std.collections`](collections.md) | Owned binary heaps ordered by `<` or by a stored comparator | `AllocError` |
| [`std.compress`](compress.md) | Bounded gzip, zlib, and DEFLATE compression of buffers and `std.io` streams, and CRC-32 | `CompressError` |
| [`std.concurrent`](concurrent.md) | Executors, futures and parallel operations | Pool/submission/future/parallel errors |
| [`std.crypto`](crypto.md) | SHA-256, SHA-512, and HMAC over `std.bytes`, incremental or one-shot, and constant-time comparison | `CryptoError` |
| [`std.csv`](csv.md) | Streaming CSV readers and writers over `std.io` with explicit dialects, size limits, and positioned errors | `CsvError` |
| [`std.dataframe`](dataframe.md) | Optional Polars analytical expression plans, typed nullable columns, schema inspection, grouping, and CSV/Parquet I/O | `DataFrameError` |
| [`std.db.sqlite`](sqlite.md) | SQLite databases through prepared statements with typed parameters and reads, owned transactions that roll back on drop, and an explicit value-size limit | `DbError` |
| [`std.deque`](deque.md) | Owned generic ring buffers with constant-time removal at either end | `AllocError` |
| [`std.encoding`](encoding.md) | Strict hex and base64 codecs and fixed-width integer packing with explicit byte order, over `std.bytes` | `EncodingError` |
| [`std.env`](env.md) | Environment variables as text or bytes, a detached snapshot, and the working directory | `IoError` |
| [`std.error`](error.md) | Explicit error mapping, operation/path context, descriptions, and standard-error reports | Mapper-selected error, `Context[E]`, `AllocError` |
| [`std.fmt`](fmt.md) | Static display/debug, bounded writer output and fallible strings | `FormatError` |
| [`std.fs`](fs.md) | Directories, metadata/walking, rooted mutation, owned temporary resources, bounded copy/delete/glob | `IoError`, `AllocError`, `WalkError`, `FsError` |
| [`std.fs.dirs`](fs/dirs.md) | The home directory and the XDG configuration, cache, data, and state directories, from the environment | `IoError` |
| [`std.http`](http.md) | Bounded HTTP/1.1 message heads and streamed `Content-Length`, chunked, and close-delimited bodies over `std.io` streams | `HttpError` |
| [`std.http.body`](http-body.md) | Bounded request bodies for handlers: URL-encoded forms, JSON, and multipart uploads into unnamed temporary files | `BodyError` |
| [`std.http.client`](http-client.md) | HTTP/1.1 client: pooled connections, deadlines and cancellation, bounded redirects, an explicit proxy, and replay of idempotent requests only | `ClientError` |
| [`std.http.cookie`](http-cookie.md) | Cookie fields read leniently and `Set-Cookie` fields validated against injection, with secure defaults | `CookieError` |
| [`std.http.files`](http-files.md) | Static files below a `Root`, opened one segment at a time without following links, with media types and conditional requests | `FilesError` |
| [`std.http.https`](http-client.md#https) | `std.http.client` for `https` URLs through [`std.tls`](tls.md), verified by default, with `CONNECT` tunnels through a proxy | `ClientError` |
| [`std.http.proxy`](http-proxy.md) | The client address and scheme behind explicitly trusted reverse proxies; deployment guidance | `ProxyError` |
| [`std.http.server`](http-server.md) | HTTP/1.1 server: routing, a bounded worker pool and admission queue, idle connections parked off the workers, per-phase deadlines, and graceful shutdown | `ServerError` |
| [`std.http.tls`](http-server.md#https) | `std.http.server` over [`std.tls`](tls.md), with handshake deadlines | `ServerError` |
| [`std.http.typed`](http-typed.md) | Typed head parsing and writes, fixed/chunked/close-delimited bodies, and guarded connection recovery with concrete causes and progress | `FramingError[E]`, `Partial[E]` |
| [`std.io`](io.md) | Partial-progress byte stream contracts, retry helpers, fixed-capacity buffering, and file, process-stream and memory adapters | `StreamError`, `Partial`, `CopyError`, `CloseError` |
| [`std.io.typed`](io-typed.md) | Byte stream protocols that carry a caller-chosen error type, buffering, adapters for `std.io` streams, and exact read, write, and copy helpers | `StepError[E]`, `Partial[E]`, `CopyFailure[R, W]` |
| [`std.json`](json.md) | Strict JSON under depth, input, and output limits: resumable event decoder, validating encoder, and a value tree | `JsonError` |
| [`std.log`](log.md) | Leveled events with typed fields, delivered to caller-chosen sinks: logfmt lines, bounded memory, or a nonblocking channel | `LogError` |
| [`std.math`](math.md) | Floating-point functions, checked rounding, integer algorithms, generic numeric helpers, and constants | `MathError` |
| [`std.net`](net.md) | TCP listeners and streams, UDP sockets, IP addresses and name lookup, with deadlines and cancellation | `NetError` |
| [`std.path`](path.md) | Lossless Linux paths, lexical operations, checked text conversion and escaped display | `IoError`, `AllocError` |
| [`std.process`](process.md) | Children spawned from an argument vector with no implicit shell, piped streams, bounded output collection, signals, and explicit reaping | `IoError`, `StreamError` |
| [`std.random`](random.md) | Caller-owned seeded generators with unbiased ranges, shuffling, and index sampling; not for secrets | `RandomError` |
| [`std.ready`](ready.md) | Readiness waits on pipes and other handles, bounded by a deadline and a cancellation token | `WaitError` |
| [`std.regex`](regex.md) | Linear-time regular expressions over text and bytes: search, captures, iteration, split, and replacement under explicit pattern, memory, and work limits | `RegexError` |
| [`std.report`](report.md) | A bounded formatter for human-readable reports, to standard error or an owned collector | `FormatError` |
| [`std.secrets`](secrets.md) | Operating-system entropy: secret bytes, hex and base64url tokens, and unbiased integers, with no fallback source | `SecretsError` |
| [`std.signal`](signal.md) | `SIGTERM` and `SIGINT` as an event to wait or select on, and a helper that cancels a token on one | `SignalError`, `WaitError` |
| [`std.simd`](simd.md) | Portable fixed-lane values, masks, checked and wrapping arithmetic, selection, and bounded list loads/stores | None |
| [`std.sync`](sync.md) | Shared cooperative cancellation tokens | `AllocError` |
| [`std.system`](system.md) | Bounded Linux procfs snapshots: CPUs, memory, load, uptime, block devices, disk I/O and process status | `IoError`, `SystemError` |
| [`std.tempfile`](tempfile.md) | Unnamed and named temporary files, recursively cleaned directories, context managers and explicit ownership transfer | `IoError`, `FsError`, `StreamError`, `AllocError` |
| [`std.test`](test.md) | Assertions that explain their failure on standard error, self-removing scratch directories, the seeded generator for `plenty test`, and fake clocks and streams | `TestError` |
| [`std.text`](text.md) | Bounded reusable builders, joining, and scalar wrapping | `TextError` |
| [`std.thread`](thread.md) | Explicit scoped thread and owned-job imports | `ThreadError`, `SpawnError[F]` |
| [`std.time`](time.md) | Checked durations, monotonic instants and deadlines, wall timestamps, and sleep | `RangeError` |
| [`std.tls`](tls.md) | TLS 1.2 and 1.3 client and server streams over `std.net` TCP, with verified certificates, client certificates, ALPN, deadlines, and cancellation | `TlsError` |
| [`std.toml`](toml.md) | Strict TOML 1.0 parsing into a value tree and writing back as text, under depth, input, and output limits | `TomlError` |
| [`std.unicode`](unicode.md) | Unicode 17 normalization, combining classes, and Mark classification | `TextError` |
| [`std.url`](url.md) | Strict RFC 3986 absolute URL parsing and formatting without normalization, component-aware percent encoding, and query pairs | `UrlError` |

The generated [signature index](signatures.md) lists every public declaration
of every module on one page.

Planned modules appear only in the [backlog](../../backlog.md). The tutorial
introduces the library in
[Use the standard math library](../../tutorial/07-modules/03-standard-math.md),
[Enumerate and classify paths](../../tutorial/08-resources/21-directories.md), and
[Prune before descending](../../tutorial/08-resources/22-walking.md).

## Importing a standard module

Standard modules use the ordinary
[import forms](../language/02-source/modules.md):

```python
import std.math
import std.math as math
from std.math import sqrt, pi
from std.math import MathError as Error
```

An import binds only the names it lists. `import std.math` requires the full
`std.math.sqrt` qualification; an alias shortens it.

`std` is reserved. Each of these is an error:

- a `std` directory, `std.plenty`, or `std.plentyi` in the source root;
- a declaration named `std`;
- an import that binds a non-standard module as `std`.

An unknown standard module, including a bare `import std`, is an error that
lists the available modules. It never falls back to a local file.

## How standard modules are compiled

A standard module passes through ordinary parsing, visibility checks, type
checking, specialization, and native emission. Its private helpers stay private.
Repeated imports, through any alias or dependency path, share its nominal
types, so a `MathError` from one module equals a `MathError` from another.
The [scope rationale](../../proposals/standard-library-scope.md#plenty-or-native)
records when a module uses a native library instead of Plenty source.

The four concurrency modules export canonical builtin identities instead of
wrapper functions. Their imports reach the existing ownership and worker checks
directly, including scoped joins and blocking-wait diagnostics. Their prelude
spellings remain available. Importing one of these under its own builtin name
is permitted only when it denotes the same builtin; unrelated name collisions
remain errors. Generic constructor and error aliases retain the same types.

Embedded modules have toolchain identities such as `std/math.plenty`,
independent of the installation path. A diagnostic located inside a standard
module, such as a note on a generic helper instantiated with an unsupported
type, uses that name. The source-only APIs (`check_source`, `compile_source_*`)
have no source root, so they can import `std` modules but not project modules.

The [runtime packaging](../toolchain/02-architecture/runtime.md) page describes
how the sources ship with the compiler.

## Reading a module page

Each module page gives every public function's signature and states:

- whether it returns a plain value or a `Result`, and which error variants it
  reports;
- whether it borrows or consumes its arguments;
- whether it allocates;
- the limits that differ from the Python module of the same name.

Recoverable failures use typed errors: `std.math` defines `MathError`, while
`std.path` and `std.fs` share the native `IoError` and storage `AllocError`.
The filesystem walker adds `WalkError` to retain attempted paths and operations.
`?` propagates an error to a caller returning the same error type or
[`Failure`](../language/08-errors/01-sums-and-propagation.md).
[`std.error`](error.md) states the conventions modules follow for typed causes,
native codes, and allocation failure. Module functions accept
[named arguments and defaults](../language/06-functions/01-signatures-and-calls.md);
the [signature index](signatures.md) shows each default.
