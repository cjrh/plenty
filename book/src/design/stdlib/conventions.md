# Standard library conventions

These are the rules public `std` APIs follow. A new module, function, or method
follows them; a change that breaks one explains why on its module page. Each
rule records the majority or most recent practice in the
[signature index](signatures.md). Where existing modules differ, the rule says
so and names the issue or [backlog](../../backlog.md) item that tracks the
difference; those modules are not examples to copy.

Packages outside `std` may use the same rules so their APIs read like the
standard library.

## Implementation and visibility

- Write a module in Plenty when the functionality is reasonably small and
  straightforward to express. Wrap a Rust or C library only for a concrete
  reason, such as substantial complexity (SQLite, Polars) or specialist
  security requirements (cryptography). The wrapper keeps the same
  allocation-failure and error contracts as Plenty code.
- Everything declared `pub` is public API: it appears in the signature index
  and is documented on the module page. Helpers stay private.
- Native hooks are std-only compiler intrinsics, not methods or functions
  of a std module. A std module calls the intrinsic (`__crypto_update`,
  `__compress_new`) with `&[u8]` views for inputs and `&mut Bytes` output
  buffers whose ranges it checks itself; user
  code calling one is rejected as
  [reserved for the standard library](bytes.md#transport).
- A module's error types and protocols are imported from that module. The
  [prelude](../language/02-source/names.md) holds builtin names, plus
  `std.fmt.FormatError`, which user formatting methods return (issue #632).
- Each module page states ownership, allocation, error variants and limits
  for every public function, as described in
  [Reading a module page](index.md#reading-a-module-page), and ends with
  `## Acknowledgements` when there is something to credit.

## Naming

### Constructors

| Form | Use | Examples |
|---|---|---|
| Lower-case noun of the product | Build a value or adapter from parts | `std.io.cursor(data)`, `std.json.decoder(&limits)`, `std.fs.watch.watcher()` |
| `limits()`, `config()` | Default settings value | `std.json.limits()`, `std.http.client.config()` |
| Verb for an operating-system resource | Acquire a file, socket or process | `open_file`, `connect`, `listen`, `dial` |
| `from_<source>` | Convert from another representation | `std.bytes.from_text`, `std.complex.from_polar`, `std.io.from_file` |
| `parse` / `parse_text` | Decode a value from bytes / from `str` | `std.json.parse`, `std.json.parse_text` |
| `with_capacity(capacity)` | Empty container with reserved storage | `std.bytes.with_capacity` |
| `__init__` | Containers built with the class name | `Deque[T]()`, `LruCache(capacity)` |

A format defined over text alone takes `parse(text: &str)` (`std.url`,
`std.uuid`, `std.mime`). `new` is used only by `std.image` and `std.mime`; do
not add it.

### Conversions and rendering

| Name | Meaning |
|---|---|
| `to_<type>()` | New owned value; may allocate and fail |
| `as_<type>()` | Cheap view or reinterpretation; never allocates |
| `to_text()` | Validated conversion of byte content to `str` (`Bytes`, `Path`) |
| `to_string()` | Canonical text of a structured value, as an owned `str` |
| `encode` / `encode_text` | Serialize to `Bytes` / to `str` |
| `describe()` | One-line error description as `Result[str, AllocError]` |
| `display[W](self, out)` | Write to a `std.fmt.Formatter[W]`; `print` and f-string fields call it |
| `debug[W](self, out)` | Write a diagnostic form; `str.repr`, `!r` and aggregate elements call it |
| `format[W](self, output)` | Write to a `std.fmt.Formatter[W]`; the `Formattable` protocol for reports |

`format` is reserved for writing to a formatter. `Url`, `MediaType`,
`SocketAddress` and `SetCookie` return owned canonical text through `to_string()`.
`display` and `debug` are reserved for the formatter form:
formatting a type whose method of either name has another signature is a
compile error. See [Formatting](#formatting).

A `_text` suffix names the `str` form of an operation whose main form takes
bytes (`parse_text`, `encode_text`, `from_text`). A `_bytes` suffix names the
byte form of an operation whose main form takes text (`get_bytes`,
`command_bytes`, `send_bytes`). The suffix does not yet predict the type:
`std.path.from_bytes` takes `list[u8]` and `std.bytes.from_bytes` takes
`bytes` (see [Bytes](#bytes)).

### Predicates

A method returning `bool` about the value's state or contents starts with
`is_` or `has_`: `is_empty`, `is_terminal`, `is_done`, `is_cancelled`.
`Deadline.expired(now)` is an established exception. Flags that report what an
event was (`std.fs.watch` `created()`, `deleted()`) are read like fields and
need no prefix.

Existing deviations: `closed()` on `std.actor.net`, `std.actor.tls` and
`std.fs` readers; `is_finished()` in `std.cli.progress`; `is_complete()` on
the JSON encoder; `complete()` on the inflater; `cancelled()` in
`std.http.server`. New code uses `is_done()` for "no further work or output".

### Sizes and counts

| Name | Meaning |
|---|---|
| `len()` | Elements the value holds, as `i64` |
| `is_empty()` | `len() == 0` |
| `capacity()` | Elements storable without reallocating, as `i64` |
| `count(value, ...)` | Occurrences of `value`, as in Python's `list.count` |

`count()` without an argument also reports a size in `std.fs.watch` (events in
the batch), `std.simd` masks (set lanes) and `std.sketch` (values observed).
New code uses `len()` for held elements and a descriptive noun for totals.
`len()` and `in` do not reach std containers yet (issue #623).

### Ending work

| Method | Contract |
|---|---|
| `flush(&mut self)` | Write buffered output; the object stays usable |
| `finish(&mut self) -> Result[P, E]` | Complete an encoder, writer or digest and return its product (`Bytes`, `str`, or `()`); later writes fail |
| `close(&mut self) -> Result[(), E]` | Release the resource and report the failure that release can produce |

`close` returns the type's own error, except that a type implementing
`std.io.Closer` returns `StreamError` (issue #533 records why `TcpStream.close`
differs from `TcpListener.close`). `Walk.close` and the TLS `Session.close`
return `()`.

An adapter whose protocol cannot carry its own error records it for later:
`failure(self) -> Option[E]` returns a copy, and
`take_failure(&mut self) -> Option[E]` moves out an error that owns data. Do
not add a third spelling.

### Readiness

A selectable type has
`watch(self, selector: &mut Selector, key: u64, interest: Interest) -> Result[(), E]`
(omitting `interest` when only one direction applies) and
`unwatch(self, selector: &mut Selector) -> Result[(), E]`. `std.fs.watch`
spells these `register`/`unregister`, HTTP server transports return `bool`,
and the actor `Context.watch` returns a `Watch` handle.

## Errors

### Domain error types

A module with failures of its own defines one public error named
`<Module>Error` (`JsonError`, `NetError`): an enum, or a class carrying an enum
plus position. Variants name the fault (`Truncated`, `LimitExceeded`,
`InvalidUtf8`). A cause from another module is kept intact in a variant named
after its source: `Stream(StreamError)`, `Io(IoError)`, `Net(NetError)`,
`Http(HttpError)`. [`std.error`](error.md) covers context and mapping.

### Allocation failure

| Module failures | Error type |
|---|---|
| Allocation only | `AllocError` (`std.deque`, `std.cache.lru`, `std.diff`) |
| Allocation and domain faults | The domain error with an `Allocation(AllocError)` variant |

Do not return a union such as `AllocError | GraphError`; add the variant
instead. A function that only forwards another module's error keeps that
error, as `std.bytes` constructors keep `AllocError`. An operation with two
unrelated causes names both in its own enum: `std.actor.StageError` is
`Allocation(AllocError)` or `Wait(WaitError)`.

### Invalid arguments

A size that cannot be allocated, negative included, is
`AllocError.CapacityOverflow`, or `Allocation(CapacityOverflow)` in a domain
error: `std.bytes.with_capacity(-1)`, `std.secrets.token_bytes(-1)`.

A position the caller computes against a value it holds, such as an index or
range into a buffer, or a count that no call could accept, traps when invalid,
as an out-of-range list index does. The message names the operation and the
argument (`Bytes.get: index is outside 0..len()`), and the
[trap report](../language/08-errors/02-allocation-and-traps.md#trap-reports)
names the calling line. The operation's result then names only what can still
fail: `Bytes.get` returns `u8`, `Rng.sample_indices` returns
`Result[list[i64], AllocError]`. `std.bytes` range methods, their
`read_file` and `read_stdin` ranges, and `std.random` counts and ranges follow
this rule. Standard-library code reports these with the std-only `__trap`
statement; a handle from the same object that has become invalid may trap the
same way (`std.graph`).

Another run-time value the caller supplies (a limit or configuration) that the
operation cannot accept is an `Err` with a named variant: `OutOfRange`,
`InvalidLimits`, `InvalidConfig`. It is never clamped or ignored. Buffer
ranges, encoding and UUID offsets, crypto output offsets, and CSV field
indices trap. Builtins follow several policies
today (issue #629); see the
[consistency proposal](../../proposals/consistency-rules.md#rule-5-an-error-type-names-only-what-can-fail).

### Returning moved inputs

An operation that takes ownership of a value and can fail before using it
(send, submit, enqueue, admit) returns the value in its error, generic over the
value's type: `Undelivered[M]` and `RequestError[M]` in `std.actor`,
`SendError[T]` for [channels](../facilities/03-concurrency/channels.md),
`SubmitError[F]` for executors, `SpawnError[F]` for thread starts. Exception:
a scoped `spawn` with borrowed arguments reports the job-free `SpawnError[()]`
and drops its moved arguments, because an error cannot carry a loan.

## Parameters

### Ownership

Borrow (`&T`) what the call only reads. Use `&mut T` for state or a buffer it
updates. Take ownership (`T`) only when the result or a queue keeps the value:
`std.io.cursor(data: Bytes)`, `std.xml.reader(inner: R, ...)`. An adapter that
owns its inner stream exposes it as a public `inner` field so the caller can
recover it.

### Time and cancellation

| Quantity | Type |
|---|---|
| Span | `&Duration` |
| Monotonic point | `&Instant` |
| Wall-clock point | `&Timestamp` |
| Blocking wait | `deadline: &Deadline, cancel: &CancellationToken`, the last two parameters |
| Server or loop shutdown | `stop: &CancellationToken` |

A method that also has a deadline-free form adds the deadline under a
`_before` suffix (`read_before`, `handshake_before`); a nonblocking form uses
`_now` (`read_now`, `accept_now`). An operation that reads the clock accepts
`now: &Instant`, or offers an `_at` variant that does, so tests can supply the
time with the fake clocks in [`std.test`](test.md).

Typed spans and wall-clock values use `Duration` and `Timestamp` in SQLite,
key-value storage, JWT validation, cookies, SSE retry hints and HTTP dates.
The [typed waits in `std.sync`](sync.md#typed-deadline-waits) provide deadline
and cancellation forms for compiler-owned channels, futures and tokens.
Low-level `_timeout_ms` forms remain available; their old `_timeout` names are
compatibility aliases. SSE's optional `event` and `id` follow the required
`deadline, cancel` pair because required parameters precede defaults.

### Limits

Every input-driven allocation has a cap.

- One or two caps: named `max_<thing>: i64` parameters with defaults
  (`std.url.parse(text, max_bytes)`, `std.db.kv.connect(..., max_length)`).
- Several caps: a `Limits` class with public `max_<thing>` fields, a
  `limits()` function returning documented defaults, and a `limits: &Limits`
  parameter. A module with several such classes qualifies both names
  (`FormLimits`, `form_limits()`).
- An invalid `Limits` is `InvalidLimits` when the operation starts; an input
  that exceeds one is `LimitExceeded` or a specific variant (`TooLarge`).

`std.json`, `std.toml`, `std.markdown`, `std.template`, `std.regex` and
`std.xml` name the parameter `bounds: &Limits`; `std.http` and `std.image` use
`limits:`. A parameter follows its type's name, as `config: &Config` and
`dialect: &Dialect` do. `std.actor.net.Limits` has no `limits()` function and
no `max_` prefixes, and `InvalidLimit` appears beside `InvalidLimits`.

### Bytes

| Role | Type |
|---|---|
| Owned buffer, argument or result | `Bytes` |
| Read-only input | `source: &[u8]`, accepting `&buffer`, `&buffer[a:b]`, `&list` and `b"..."` literals |
| Caller's output buffer | `buffer: &mut Bytes, start: i64, stop: i64`, a half-open range |
| A list of byte-sized numbers | `list[u8]` |

A `&[u8]` input carries its own range, so it takes no `start` and `stop`
and has no out-of-range error: the caller passes `&buffer[a:b]`, whose bounds
clamp. [Slice views](../language/04-expressions/slices.md#slice-views)
describe the conversion.

`std.path`, `std.env`, `std.process` and `std.cli` carry operating-system
strings as `list[u8]` (B39). `std.dataframe.lit_binary` still takes `&Bytes`
because its C adapter accepts only lists (issue #657).

### Streams

Byte streams implement the [`std.io`](io.md) protocols `Reader`, `Writer`,
`Seeker` and `Closer`: `read(buffer, start, stop) -> Result[i64, StreamError]`
reports partial progress. Read ranges outside `0 <= start <= stop <= buffer.len()`
trap before transport access, including empty ranges and EOF; `read_exact`
validates the same bounds. A function borrows a stream as `reader: &mut R`
with `R: Reader`; an adapter takes `inner: R`.
[`std.io.typed`](io-typed.md) is for callers that must recover the
transport's own error type (`std.http.typed`); other modules do not add a
second protocol. Network streams add deadline and nonblocking forms
(`read_before`, `read_now`) that return the module's error.

### Booleans and enums

A choice between modes is an enum: `Shutdown`, `Interest`, `Endian`,
`DuplicateKeys`. A `bool` parameter is acceptable when its name says what
`True` means (`enabled`, `descending`, `html`), it has a default, and callers
pass it by name. Two `bool` parameters that combine into modes are an enum.
Exception: `std.fs.walk(root, top_down)` requires a positional `bool`. An
operation that offers a mode by a separate method instead, such as
`ThreadPoolExecutor.cancel_pending()` before `shutdown()`, needs neither.

### Integer types

| Quantity | Type |
|---|---|
| Length, index, offset, size, capacity, limit | `i64`, matching `len()` and indexing |
| Hash, key, seed, bit pattern, selector key | `u64` |
| Value fixed by a format or the OS (file size, inode, wire integer) | Its own width |
| Running statistics counter | `u64` |

Fixed positive builtin capacities and worker/chunk counts use `Positive[i64]`:
`channel(capacity)`, `ThreadPoolExecutor(max_workers, queue_capacity)` and
`.par(pool, chunks=...)`. `CuckooFilter.len()` returns `u64`; low-level timed
waits still take `u64` milliseconds.

## Formatting

`std.fmt.Formatter[W]` and its `FormatError` serve value formatting and
reports. Import types, constructors, and the writer-generic `Formattable`
protocol from `std.fmt`; `std.report` provides rendering helpers.

- a value that f-strings display implements `display[W]`;
- an error that renders into a report implements
  `format[W](self, output: &mut Formatter[W])`;
- an owned canonical string is `to_string() -> Result[str, E]`; reserve
  `format` for writing to a formatter (including compiler-handled f-strings).
- `print` and f-strings call `display`; `str.repr`, `!r` and elements of
  formatted aggregates call `debug`. A type without `display` uses its
  `debug`, and one without `debug` uses its structure. When a user method can
  run, `print` returns `Result[(), IoError | FormatError]`, and `str.repr`
  and f-strings return `Result[str, AllocError | FormatError]`; otherwise
  `print` reports `IoError` and both others `AllocError`. See
  [formatting](../facilities/01-builtins/formatting.md).
- `std.log.Logger.debug` and `std.cli.Progress.display` have other
  signatures, so `print` rejects those types; call their methods instead.

## Deviations

| Area | Deviation | Tracking |
|---|---|---|
| Invalid arguments | Builtin policies differ | Issue #629 |
| Containers | `len()` and `in` reject std containers | Issue #623 |
| Bytes | `list[u8]` OS strings; `dataframe.lit_binary(&Bytes)` | B39 |
| Streams | `TcpStream.close` returns `StreamError` | Issue #533 (intentional) |
| Limits | `bounds:` spelling; `std.actor.net.Limits` shape | Not yet tracked |
| Names | Predicates without `is_`; `count()` as size; `watch` shapes; `new` | Not yet tracked |
