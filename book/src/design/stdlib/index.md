# Standard library

The standard library is the set of modules under the reserved `std` namespace.
They are written in Plenty or expose existing compiler facilities, and are embedded in the compiler: importing one needs no
source installation, network, Cargo, or rustc. Native compilation still requires
the supported [linker environment](../toolchain/01-building/linking.md).

A standard module must be imported. Names available without an import belong to
the [prelude](../language/02-source/names.md) and are specified under
[built-in facilities](../facilities/index.md).

## Modules

| Module | Contents | Error type |
|---|---|---|
| [`std.math`](math.md) | Floating-point functions, checked rounding, integer algorithms, generic numeric helpers, and constants | `MathError` |
| [`std.path`](path.md) | Lossless Linux paths, lexical operations, checked text conversion and escaped display | `IoError`, `AllocError` |
| [`std.fs`](fs.md) | Directories, metadata/walking, rooted mutation, owned temporary resources, bounded copy/delete/glob | `IoError`, `AllocError`, `WalkError`, `FsError` |
| [`std.deque`](deque.md) | Owned generic ring buffers with constant-time removal at either end | `AllocError` |
| [`std.error`](error.md) | Operation and path context around a typed cause, descriptions, and standard-error reports | `Context[E]`, `AllocError` |
| [`std.bytes`](bytes.md) | Compact owning byte buffers with checked ranges, explicit copies and validated text conversion | `BytesError` |
| [`std.io`](io.md) | Partial-progress byte stream contracts, retry helpers, fixed-capacity buffering, and file, process-stream and memory adapters | `StreamError`, `Partial`, `CopyError`, `CloseError` |
| [`std.time`](time.md) | Checked durations, monotonic instants and deadlines, wall timestamps, and sleep | `RangeError` |
| [`std.cli`](cli.md) | Explicit typed options/positionals, repeatable values, generated help and lossless path arguments | `CliError` |
| [`std.text`](text.md) | Bounded reusable builders, joining, and scalar wrapping | `TextError` |
| [`std.fmt`](fmt.md) | Static display/debug, bounded writer output and fallible strings | `FormatError` |
| [`std.unicode`](unicode.md) | Unicode 17 normalization, combining classes, and Mark classification | `TextError` |
| [`std.thread`](thread.md) | Explicit scoped thread and owned-job imports | `ThreadError`, `SpawnError[F]` |
| [`std.channel`](channel.md) | Bounded channels, endpoints and selection | Channel/send/receive/selection errors |
| [`std.concurrent`](concurrent.md) | Executors, futures and parallel operations | Pool/submission/future/parallel errors |
| [`std.sync`](sync.md) | Shared cooperative cancellation tokens | `AllocError` |
| [`std.encoding`](encoding.md) | Strict hex and base64 codecs and fixed-width integer packing with explicit byte order, over `std.bytes` | `EncodingError` |
| [`std.url`](url.md) | Strict RFC 3986 absolute URL parsing and formatting without normalization, component-aware percent encoding, and query pairs | `UrlError` |
| [`std.collections`](collections.md) | Owned binary heaps ordered by `<` or by a stored comparator | `AllocError` |
| [`std.iter`](iter.md) | Allocation-free `map`, `filter`, `take`, `fold`, and `try_fold` over generators | None |
| [`std.csv`](csv.md) | Streaming CSV readers and writers over `std.io` with explicit dialects, size limits, and positioned errors | `CsvError` |
| [`std.random`](random.md) | Caller-owned seeded generators with unbiased ranges, shuffling, and index sampling; not for secrets | `RandomError` |
| [`std.secrets`](secrets.md) | Operating-system entropy: secret bytes, hex and base64url tokens, and unbiased integers, with no fallback source | `SecretsError` |
| [`std.crypto`](crypto.md) | SHA-256, SHA-512, and HMAC over `std.bytes`, incremental or one-shot, and constant-time comparison | `CryptoError` |
| [`std.compress`](compress.md) | Bounded gzip, zlib, and DEFLATE compression of buffers and `std.io` streams, and CRC-32 | `CompressError` |
| [`std.archive`](archive.md) | TAR and ZIP readers and writers over `std.io`, and extraction into a new directory under explicit path, link, and size limits | `ArchiveError` |
| [`std.regex`](regex.md) | Linear-time regular expressions over text and bytes: search, captures, iteration, split, and replacement under explicit pattern, memory, and work limits | `RegexError` |
| [`std.test`](test.md) | Assertions that explain their failure on standard error, self-removing scratch directories, the seeded generator for `plenty test`, and fake clocks and streams | `TestError` |
| [`std.db.sqlite`](sqlite.md) | SQLite databases through prepared statements with typed parameters and reads, owned transactions that roll back on drop, and an explicit value-size limit | `DbError` |
| [`std.json`](json.md) | Strict JSON under depth, input, and output limits: resumable event decoder, validating encoder, and a value tree | `JsonError` |
| [`std.log`](log.md) | Leveled events with typed fields, delivered to caller-chosen sinks: logfmt lines, bounded memory, or a nonblocking channel | `LogError` |
| [`std.toml`](toml.md) | Strict TOML 1.0 parsing into a value tree and writing back as text, under depth, input, and output limits | `TomlError` |
| [`std.env`](env.md) | Environment variables as text or bytes, a detached snapshot, and the working directory | `IoError` |
| [`std.process`](process.md) | Children spawned from an argument vector with no implicit shell, piped streams, bounded output collection, signals, and explicit reaping | `IoError`, `StreamError` |
| [`std.ready`](ready.md) | Readiness waits on pipes and other handles, bounded by a deadline and a cancellation token | `WaitError` |
| [`std.net`](net.md) | TCP listeners and streams, UDP sockets, IP addresses and name lookup, with deadlines and cancellation | `NetError` |
| [`std.tls`](tls.md) | TLS 1.2 and 1.3 client and server streams over `std.net` TCP, with verified certificates, client certificates, ALPN, deadlines, and cancellation | `TlsError` |
| [`std.http`](http.md) | Bounded HTTP/1.1 message heads and streamed `Content-Length`, chunked, and close-delimited bodies over `std.io` streams | `HttpError` |
| [`std.http.server`](http-server.md) | HTTP/1.1 server: routing, a bounded worker pool and admission queue, idle connections parked off the workers, per-phase deadlines, and graceful shutdown | `ServerError` |
| [`std.http.tls`](http-server.md#https) | `std.http.server` over [`std.tls`](tls.md), with handshake deadlines | `ServerError` |
| [`std.http.body`](http-body.md) | Bounded request bodies for handlers: URL-encoded forms, JSON, and multipart uploads into unnamed temporary files | `BodyError` |
| [`std.http.cookie`](http-cookie.md) | Cookie fields read leniently and `Set-Cookie` fields validated against injection, with secure defaults | `CookieError` |
| [`std.http.proxy`](http-proxy.md) | The client address and scheme behind explicitly trusted reverse proxies; deployment guidance | `ProxyError` |
| [`std.http.client`](http-client.md) | HTTP/1.1 client: pooled connections, deadlines and cancellation, bounded redirects, an explicit proxy, and replay of idempotent requests only | `ClientError` |

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

The four concurrency modules export canonical builtin identities instead of
wrapper functions. Their imports reach the existing ownership and worker checks
directly, including scoped joins and blocking-wait diagnostics. Their prelude
spellings remain available. Importing one of these under its own builtin name
is permitted only when it denotes the same builtin; unrelated name collisions
remain errors. Generic constructor and error aliases retain the same types.

Embedded modules have toolchain identities such as `std/math.plenty`,
independent of the installation path. A diagnostic located inside a standard
module, such as a note on a generic helper instantiated with an unsupported
type, uses that name. Imports currently require the file-based compilation APIs.

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
native codes, and allocation failure. Keyword and
default arguments are unimplemented, so every argument is explicit.
