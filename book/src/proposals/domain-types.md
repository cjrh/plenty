# Positive and NonNegative integer domain types

**Status: Accepted design, not implemented.** No backlog item yet; it proposes B95,
related to B92. The [numbers reference](../design/language/03-types/numbers.md#nonzero-integers)
defines `NonZero[T]`, the model for this design, and the
[std conventions](../design/stdlib/conventions.md#invalid-arguments) define the
current invalid-argument rule. Surveyed against local `main` at `9c3cb76e` on
2026-11-01. Local PR #804 (open) changes several of the error types named
below; parameter types here are independent of it.

## Problem

A std parameter with a fixed domain, such as a byte limit that must not be
negative or a buffer capacity that must be at least one, is checked inside the
function and reported as an `Err`:

```text
pub def parse(text: &str, max_bytes: i64) -> Result[Url, UrlError]   # InvalidLimit for max_bytes < 0
pub def buffered_reader[R](inner: R, capacity: i64) -> Result[BufReader[R], StreamError]   # OutOfRange for capacity <= 0
```

The failure is a caller bug that the caller could rule out, but every call
pays for it: a `?`, an error variant to match, and a check on each call. Almost
every call passes a literal or a default, so the check can never fail at those
sites. The [consistency rules](consistency-rules.md#rule-5-an-error-type-names-only-what-can-fail)
(rule 5.3) already say "where a type can state the domain, the parameter uses
it", and `NonZero[i64]` does this for range and slice steps. No type states
"at least zero" or "at least one".

The [migration list](#migration-list) covers the public std parameters and
`Limits` fields whose lower bound is `0` or `1` and is checked at run time
today.

## Summary

| Question | Decision |
| --- | --- |
| New types | `Positive[T]` (value > 0) and `NonNegative[T]` (value >= 0) |
| `T` | Signed integer types only: `i8`, `i16`, `i32`, `i64` |
| Construction | Integer literal where one is expected (checked at compile time), or `try_from` |
| `try_from` error | `RangeError.OutOfRange`, as `NonZero.try_from` and `T.try_from` |
| Reading back | `.get()` returns `T`; no implicit conversion to `T` |
| Arithmetic | `+` and `*` keep the type; `-` returns `T`; other operators are compile errors |
| Relation to `NonZero` | Separate nominal types; `NonZero.from(p)` and `NonNegative.from(p)` widen a `Positive` |
| Copy, format, hash | As `NonZero`: copies, prints as its integer, valid dictionary key |
| `len()` | Unchanged, returns `i64`; no implicit conversion |
| Sizes | Stay `i64` ([integer types](../design/stdlib/conventions.md#integer-types)); only the wrapper is new |

## The types

### Element type

`Positive[T]` and `NonNegative[T]` take a signed integer type. An unsigned
type is a compile error that names the existing spelling, because the domain
already has one:

```text
Positive[u64]      # error: every nonzero `u64` is positive; use `NonZero[u64]`
NonNegative[u32]   # error: every `u32` is non-negative; use `u32`
```

One domain has one spelling. Std quantities are `i64` by convention, so
`Positive[i64]` and `NonNegative[i64]` are the forms std uses; the narrower
signed widths exist for user data and cost nothing extra, since the
implementation is generic over `T`.

`Positive` and `NonNegative` are builtin names and cannot be redefined, like
`NonZero`.

### Construction

The rules are those of `NonZero`, with a different domain check.

```text
def per_page(items: i64, size: Positive[i64]) -> i64:
    items // size.get()

def main() -> Result[(), IoError | RangeError]:
    print(per_page(100, 20))?          # literal converts at compile time
    print(per_page(100, 0))?           # error: `0` is not a `Positive[i64]`
    configured = 25
    size = Positive.try_from(configured)?
    print(per_page(100, size))?
    limit: NonNegative[i64] = 0        # 0 is in the domain
    bad: NonNegative[i64] = -1         # error: `-1` is not a `NonNegative[i64]`
    Ok(())
```

- An unsuffixed integer literal, possibly negated, converts where a
  `Positive[T]` or `NonNegative[T]` is expected: an argument, an annotated
  binding, a return value, a class constructor argument, a field assignment,
  a default parameter value, or inside `Some(...)`. A literal outside the
  domain is a compile error. These are the positions where a `NonZero` literal
  converts today; a probe with the current compiler confirmed defaults,
  returns, constructor arguments, field assignment and `Some(3)` for an
  `Option[NonZero[i64]]` parameter.
- `Positive.try_from(value)` returns `Result[Positive[T], RangeError]`, with
  `T` taken from `value`; `Positive[T].try_from(value)` names `T` and requires
  a `T`. `NonNegative.try_from` is the same. Out of domain is
  `Err(RangeError.OutOfRange)`.
- The constructor is private: `Positive(3)` is a compile error that names the
  two conversions.

`RangeError` is the error every integer narrowing already uses
([checked conversion](../design/facilities/01-builtins/numbers.md#checked-conversion)),
so one error type covers `i32.try_from`, `NonZero.try_from`,
`Positive.try_from` and `NonNegative.try_from`, and a function that does
several returns `Result[_, RangeError]` without a union. It has one variant,
so an exhaustive match needs one arm.

### Reading back and mixing with `T`

`.get()` returns the `T`. There is no implicit conversion to `T`: an operation
that mixes `Positive[i64]` with `i64` is a type error whose hint shows
`.get()` or `try_from`, as for `NonZero`. Implicit unwrapping would let a
domain value silently become an `i64` and come back only through `try_from`,
which hides where the check happens.

### Arithmetic and comparison

Operands must have the same type. An integer literal beside a domain operand
takes that operand's type, so `n + 1` is a `NonNegative[i64]` and `size * 0`
is a compile error for a `Positive`.

| Operation | `Positive[T]` | `NonNegative[T]` | Why |
| --- | --- | --- | --- |
| `a + b`, `a += b` | `Positive[T]` | `NonNegative[T]` | Sum stays in the domain; overflow traps |
| `a * b`, `a *= b` | `Positive[T]` | `NonNegative[T]` | Product stays in the domain; overflow traps |
| `a - b` | `T` | `T` | Can leave the domain |
| unary `+` | `Positive[T]` | `NonNegative[T]` | Identity |
| unary `-` | `T` | `T` | Leaves the domain |
| `//`, `%`, bitwise, shifts | Compile error, suggests `.get()` | Same | As `NonZero`; a domain divisor is a later addition ([decision 4](#decisions)) |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | `bool` | `bool` | Same-type operands or a literal |

Overflow in `+` and `*` traps, or is an `Err` in a
[`checked:` block](../design/language/03-types/numbers.md#checked-blocks), so a
sum cannot wrap to a negative value. A comparison with a literal outside the
domain, such as `size > 0` for a `Positive`, is a compile error, as
`step == 0` is for `NonZero`: the literal is not a `Positive`. `n >= 0` for a
`NonNegative` compiles, since `0` is in the domain.

### Relation to `NonZero`

A `Positive[T]` is nonzero, but it is not a `NonZero[T]`: Plenty has no
subtyping, and each is a nominal `@newtype` class. Code that holds a
`Positive` and needs a `NonZero` (a range step) or a `NonNegative` (a limit)
widens it explicitly:

```text
def chunks(total: i64, size: Positive[i64]) -> Result[(), IoError]:
    for start in range(0, total, NonZero.from(size)):
        print(start)?
    Ok(())
```

`NonZero.from(p)` and `NonNegative.from(p)` take a `Positive[T]`, cannot fail,
allocate nothing and compile to nothing. A `from` that cannot fail beside a
`try_from` that can matches the pair the reader already sees in `T.try_from`.
Widening is explicit ([decision 3](#decisions)); see
[Alternatives](#alternatives).

There is no conversion from `NonNegative` to `NonZero` or `Positive` other
than `try_from(n.get())`, since zero is in its domain.

### Copy, format, hash and layout

As `NonZero[T]`:

- the value has `T`'s representation and allocates nothing;
- it copies, is a valid dictionary key and set element, and is usable as a
  field, collection element and `@copy` component;
- `print`, f-strings and `debug` show the integer, and format specs apply as
  to `T`.

Both are declared in the builtin arithmetic-errors module beside `NonZero`
(`src/frontend/modules.rs`), as private `@newtype @copy` classes with a
`get()` method.

## `len()` and sizes

`len()` returns `i64` and stays so. Making it return `NonNegative[i64]` would
break `i < len(xs)` with `i: i64`, `range(len(xs))`, and `len(xs) - 1` used
as an index, which are the most common uses of `len()`. A rule that converts a
`len()` call where a `NonNegative[i64]` is expected, like a literal, is
possible but is not needed now:

- Allocation sizes are out of scope. `list.with_capacity`, `Bytes.reserve`,
  `resize`, `LruCache(capacity)` and `std.actor.stage` report a negative size
  as `CapacityOverflow` (rule 5.1, local PR #804), so `with_capacity(len(x))`
  is unchanged.
- No `Positive` parameter can take `len()`, which can be zero.
- A search of `stdlib/`, the tutorial, the reference, `examples/` and `tests/`
  found two calls that pass a length expression to a migrated parameter, both
  inside std with arithmetic (`text.plenty` `builder(self.max_bytes -
  used_bytes + ...)` and `regex.plenty` `self.scan(2 * len(self._names))`,
  which is private). A `len()`-only rule would cover neither.

Where a length must become a limit, the call is explicit:

```text
mut out = std.text.builder(NonNegative.try_from(2 * len(text))?)?
```

Inside std, public functions call `.get()` once and pass `i64` to private
helpers, so internal arithmetic is unchanged. A `len()` conversion rule is
not part of this design ([decision 5](#decisions)).

## Migration list

A parameter migrates when its whole static domain is `>= 0` or `>= 1`. The
lists below are the public std and builtin parameters with that lower bound
that return `Err` or trap today. `P` is `Positive[i64]` and `N` is
`NonNegative[i64]`; each parameter is `i64` today unless stated. Result types
are unchanged except where noted, and follow PR #804 where it changes them.

### Group A: the invalid-argument failure leaves the error type

| Function or class | Parameters after | Removed today |
| --- | --- | --- |
| `std.url.parse(text, max_bytes)` | `max_bytes: N` | `UrlError.InvalidLimit` (its only use) |
| `std.db.kv.Store.scan(..., max_rows, ...)`, `scan_all(..., max_rows, ...)` | `max_rows: N` | `KvError.InvalidLimit` (its only uses) |
| `std.markdown.Limits` | `max_input, max_depth, max_output: N` | `MarkdownFault.InvalidLimits` |
| `std.http.body.FormLimits` | `max_bytes, max_fields: N` | `BodyError.InvalidLimit`, with the two rows below |
| `std.http.body.MultipartLimits` | `max_parts, max_head, max_field, max_file, max_body: N` | Same |
| `std.http.body.read_json(exchange, limits)` | via `std.json.Limits.max_input: N` | Same |
| `std.actor.net.connection(stream, limits)` | `std.actor.net.Limits` `input, output: P` | `BytesError.OutOfRange`; the result can narrow to `AllocError` |
| builtin `channel[T](capacity)` | `capacity: P` (from `u64`) | `ChannelError.InvalidCapacity` |
| builtin `ThreadPoolExecutor(max_workers, queue_capacity)` | both `P` (from `u64`) | `PoolError.InvalidSize` |
| `.par(&pool, chunks=N)` | `chunks: P` | The `par chunks must be positive` trap |

The two builtins move from `u64` to `Positive[i64]` rather than to
`NonZero[u64]`, which removes their listed deviation from the
[integer-type convention](../design/stdlib/conventions.md#integer-types) in
the same break ([decision 2](#decisions), issue #665).

### Group B: the call loses this failure; the error type keeps the variant for other causes

| Function or class | Parameters after | Variant kept for |
| --- | --- | --- |
| `std.io.buffered_reader(inner, capacity)`, `buffered_writer(inner, capacity)` | `capacity: P` | `StreamError.OutOfRange`: stream ranges |
| `std.io.typed.buffered_reader(inner, capacity)` | `capacity: P` | Same |
| `std.io.BufReader.read_until(delimiter, out, limit)` | `limit: P` | Same |
| `std.io.read_up_to(reader, out, limit)` | `limit: N` | Same |
| `std.process.Child.communicate(input, limit)`, `output(limit)` | `limit: N` | Same |
| `std.csv.reader(inner, dialect, capacity)` | `capacity: P` | `CsvError.OutOfRange`: field indexes |
| `std.log.text_sink(writer, max_line)` | `max_line: P` | `LogError.InvalidSpec`: specs |
| `std.log.memory_sink(capacity)` | `capacity: N` | Same |
| `std.fmt.Formatter.floating(value, width)`, `integer(value, width)`, `unsigned(value, base, width)` | `width: N` | `FormatError.InvalidSpec`: base, specs |
| `std.text.builder(max_bytes)`, `join(..., max_bytes)` | `max_bytes: N` | `TextError.InvalidLimit`: wrapper geometry, below |
| `std.text.hard_wrap(text, width, max_bytes)` | `width: P`, `max_bytes: N` | Same |
| `std.text.wrap`, `fill`, `shorten`, `text_wrapper` | `width: P`, `max_bytes, max_input_bytes: N` | Same |
| `std.text.dedent`, `indent`, `indent_with` | `max_bytes, max_input_bytes: N` | Same |
| `std.text.TextWrapper` fields | `width: P`, `tabsize: N`, `max_lines: Option[P]`, `max_bytes, max_input_bytes: N` | An indent wider than `width`, a placeholder too long |
| `std.fs.copy_from(..., max_bytes, ...)`, `copy_file(..., max_bytes, ...)` | `max_bytes: N` | `FsError.InvalidInput`: names, ceilings |
| `std.fs.glob(parent, pattern, max_entries, max_matches, max_steps)` | all three `N` | Same: pattern length |
| `std.system.System.block_devices`, `disk_io` | `max_devices: N` | `SystemError.InvalidInput`: `max_bytes`, pid |
| `std.system.System.process_ids(max_entries)` | `max_entries: N` | Same |
| `std.archive.zip_reader(inner, max_entries)` | `max_entries: N` | `ArchiveError.InvalidInput`: names, modes |
| `std.compress.inflater(format, limit)`, `decompress(format, data, limit)` | `limit: N` | `CompressError.OutOfRange`: `crc32` seed |
| `std.http.encode_chunk_size(length)` | `length: N` | `StreamError.OutOfRange` in `HttpError` |
| `std.http.typed.fixed_body(inner, length)` | `length: N` | `FramingError.OutOfRange`: read ranges |
| `std.http.Limits`, decoder and client | `max_headers, max_body: N` | `HttpError.InvalidLimit`: `max_line, max_head >= 2` |
| `std.http.server` limits | `max_line, max_head, max_headers: P`, `max_body: N` | Other `Config` fields with other bounds |
| `std.http.websocket.Config` | `max_frame, max_message: P` | `WebSocketError.InvalidConfig`: one other mapping |
| `std.actor.http.server` config | `max_requests: P`, `limits.max_line, max_head, max_headers: P`, `max_body: N` | `ServerError.InvalidConfig`: capacity, heap, buffer minimums |
| `std.actor.net.listener(..., capacity, heap, ...)` | `capacity, heap: P` | `NetError`: transport failures |
| `std.actor.net.Connection.reserve(wanted)` | `wanted: N` | `NetError`: closed connection |
| `std.actor.tls.over(net, session, input)` | `input: P` | `TlsError.Net`: transport failures |
| `std.image.Limits` | `max_width, max_height, max_bytes: N` | `ImageError.OutOfRange`: dimensions |
| `std.stat.quantiles(values, n)` | `n: P` | `StatError.Domain`: float parameters |
| `std.math.factorial(n)` | `n: N` | `MathError.Domain`; `Overflow` above 20 stays |

### Group C: the lower bound is typed; a fixed ceiling remains

| Function or class | Parameters after | Ceiling |
| --- | --- | --- |
| `std.fmt.formatter`, `stderr_formatter`, `string_formatter` | `max_bytes: N`, `max_depth: N` | `max_depth <= 64`; also `scratch.len() >= 32` |
| `std.json.Limits`, `std.toml.Limits` | `max_depth, max_input, max_output: N` | `max_depth` ceiling |
| `std.xml.Limits` | `max_token: P`, others `N` | `max_depth <= 1024` |
| `std.regex.Limits` | all seven `N` | `max_nesting` ceiling |
| `std.template.Limits` | `max_context_nodes, max_depth: P`, others `N` | `max_depth <= 64` |
| `std.unicode.nfd`, `nfc`, `nfkd`, `nfkc` | `max_scalars: N` | `i64.MAX // 4` |
| `std.sketch.quantiles(capacity)`, `quantiles_from_bytes(data, max_capacity)` | `P` | 1048576 |
| `std.sketch.frequent_items(capacity)`, `frequent_items_from_bytes(data, max_capacity)` | `P` | 65536 |
| `std.sketch.xor.build`, `from_bytes` | `max_keys: N` | `key_limit` |
| `std.sketch.cuckoo.cuckoo`, `from_bytes` | `max_buckets: P` | `bucket_limit`; `initial_buckets` must be a power of two |
| `std.fs.TempDir.cleanup_tree`, `std.fs.remove_tree` | `max_entries, max_depth: P` | `max_depth <= 64` |
| `std.fs.temporary_dir(parent, prefix, attempts)` | `attempts: P` | 1024 |
| `std.tempfile.directory(..., max_entries, max_depth)` | `max_entries, max_depth: P` | `max_depth <= 64` |
| `std.jwt` decoding `max_size` | `P` | `max_token_size` |
| `std.crypto.hkdf_sha256(..., size)` | `size: N` | 8160 |
| `std.image.new`, `from_pixels` | `width, height: P` | 2147483647 |
| `std.actor.Context.spawn`, `spawn_isolated`, `Stage.start` | `limit: P` | `max_heap_limit` |
| `std.system.System.process(pid, max_bytes)` and the `max_bytes` of `load_average`, `uptime`, `cpu_count`, `memory` | `max_bytes: P` | `!= i64.MAX`; `pid != 0` |

A ceiling keeps an `Err` variant, so the migration removes the `?` from these
calls only if the ceiling stops being an `Err`. Rule 5.3 says an argument
outside its documented domain traps and a constant outside it is a compile
error; under that rule these ceilings trap, as the run-time domains in PR #804
do, and the variants leave the error types. This design adopts that rule
([decision 1](#decisions)): a ceiling traps, and a literal above a constant
ceiling is a compile error.

### Not migrated

| Parameter | Reason |
| --- | --- |
| `with_capacity`, `Bytes.reserve`/`resize`, `Heap.reserve`, graph `reserve`, `LruCache(capacity)`, `std.actor.stage` counts, `std.secrets.token_bytes`/`token_hex`/`token_urlsafe` | Allocation sizes: `CapacityOverflow` (rule 5.1, PR #804) |
| `str.repeat(count)`, slice bounds | Clamp (rule 5.2) |
| `std.ready.selector(capacity)` | Clamps to `1..most_events` today; a deviation to fix separately |
| `regex` `replace` limit | A negative value means unlimited; a sentinel, not a domain (`Option` would state it) |
| `Bytes` range methods, `std.actor.net.Connection.consume`/`commit`/`forward`, `Context.restart`, `std.image.Image.crop`, `std.random.Rng.index`/`sample_indices`, csv/sqlite/watch indexes | The upper bound depends on the receiver or another argument; one trap covers both ends (PR #804) |
| `std.http` `max_line`, `max_head` (>= 2), `std.sketch.hyperloglog` precision (4..18), `encode_png` level (0..9), actor server `capacity`, `heap_limit`, buffer minimums | Lower bound other than 0 or 1; needs `Bounded` |
| `std.compress.crc32` seed (0..u32 max), archive modes (0..4095) | The parameter should be a narrower type (`u32`, `u16`); separate fix |
| `std.regex.find_all`/`split` `max_matches`/`max_pieces`, `std.http.cookie.cookies` `limit`, `std.db` `max_length` | Not checked today; their negative behaviour needs its own review first |

## Example

Before:

```text
def lines(path: &Path) -> Result[list[str], AppError]:
    file = open_file(path)?
    mut reader = std.io.buffered_reader(file, 65536).map_error(AppError.Stream)?
    mut out = std.text.builder(1048576).map_error(AppError.Text)?
    ...
```

After, with literals: the two `map_error` adapters for impossible failures
go, and the builder's remaining failure is allocation.

```text
def lines(path: &Path) -> Result[list[str], AppError]:
    file = open_file(path)?
    mut reader = std.io.buffered_reader(file, 65536)
    mut out = std.text.builder(1048576)?
    ...
```

`buffered_reader` can still fail to allocate its buffer, so it keeps a
`Result`; whether that result narrows to `AllocError` follows rule 5. With a
configured value, the check moves to where the value is read:

```text
def configured(settings: &Settings) -> Result[Positive[i64], AppError]:
    Positive.try_from(settings.buffer_size).map_error(AppError.Config)
```

## Alternatives

- **`Bounded[T, lo, hi]`.** States every domain in the tables, including
  ceilings and `>= 2`. Needs value generics (B83, accepted, not shipped), and
  the arithmetic rules depend on the bounds: `Bounded[i64, 1, 64] + 1` has no
  simple result type. Rejected (user decision); two fixed domains cover the
  common cases now.
- **Unsigned parameters.** `max_bytes: u64` rules out negatives without a new
  type, but breaks the i64 convention for sizes, forces casts at every
  `len()` and index, and `u64` cannot state `>= 1`. Rejected.
- **Plain `std` classes with `try_from` only.** No compiler change, but no
  literal conversion, so every call writes `Positive.try_from(64)?`, which is
  the `?` this proposal removes. Rejected.
- **Implicit widening to `NonZero` and `NonNegative`.** Saves `NonZero.from`
  at range steps. It adds a third way for a domain value to appear where one
  is expected (literal, `try_from`, widening), and is subtyping in all but
  name. Rejected ([decision 3](#decisions)).
- **Implicit unwrapping to `T`.** `size + offset` with `offset: i64` would
  work, but the domain value disappears silently and comes back only through
  `try_from`. Rejected, as for `NonZero`.
- **Keep run-time checks, document them.** Leaves the `?` and the variant on
  every call. Rejected by rule 5.

## Cost and compatibility

- **Compiler.** `NonZero` is 247 lines in `src/frontend/nonzero.rs`,
  plus hooks in operators, f-strings, metadata and diagnostics (116 mentions
  in 22 files under `src/`). Generalizing it to a domain enum (`NonZero`, `Positive`,
  `NonNegative`) with a per-domain literal check, operator table and
  diagnostic is the expected shape, not three copies. The signed-only bound
  is a new check: `IntType` admits all eight integer types. Codegen needs no
  change, since the representation is `T`.
- **Breaking changes.** Every migrated parameter rejects a plain `i64`
  variable; literals and defaults keep compiling. A call passing a variable
  adds `Positive.try_from(x)?` or `NonNegative.try_from(x)?`, and the
  diagnostic shows that form. Code that matches a removed variant
  (`UrlError.InvalidLimit`, `KvError.InvalidLimit`,
  `MarkdownFault.InvalidLimits`, `BodyError.InvalidLimit`,
  `ChannelError.InvalidCapacity`, `PoolError.InvalidSize`) gets an unknown
  variant error. Code that constructs or mutates a `Limits` with a variable
  converts it; code that reads a field calls `.get()`.
- **Std internals.** Public functions call `.get()` and pass `i64` to private
  helpers, so most of the change is at signatures and in `valid` functions,
  which shrink or disappear.
- **Docs.** The numbers reference gains the two types next to `NonZero`; each
  module page in the tables updates its signature, error and limit rows;
  `conventions.md` "Invalid arguments" and "Limits" state that a fixed lower
  bound is a type; the cheat sheet, tutorial lessons that pass computed
  limits, the `plenty` skill and `signatures.md` follow.
- **Performance.** No change to representation or allocation; each migrated
  call drops one comparison and branch.

## Phasing

1. Language: `Positive[T]` and `NonNegative[T]`, literal conversion,
   `try_from`, `.get()`, operators, `NonZero.from`/`NonNegative.from`,
   formatting and hashing, with the numbers reference and cheat sheet.
2. Group A, which removes variants and needs the largest doc change, with the
   builtins `channel`, `ThreadPoolExecutor` and `par(chunks)`.
3. Group B, module by module; each module's page and tests change together.
4. Group C, whose ceilings change from `Err` to traps in the same step, so
   each module changes once.
5. Later: `//` and `%` by a `NonZero` or `Positive` divisor without the
   zero check (issue #664).

PR #804 merges first; this work changes the same std error types.

## Decisions

Decided 2026-11-01, on review of local PR #812.

1. **Group C ceilings trap** under rule 5.3. A literal above a constant
   ceiling is a compile error, and the ceiling's `Err` variant leaves the
   error type.
2. **`channel` and `ThreadPoolExecutor` take `Positive[i64]`**, not
   `NonZero[u64]`, which removes their `u64` deviation from the integer-type
   convention. Tracked as issue #665, in phase 2.
3. **Widening is explicit**: `NonZero.from(p)` and `NonNegative.from(p)`.
   There is no implicit widening.
4. **Division by a domain value is a later addition.** For now `//` and `%`
   reject `Positive` and `NonZero` operands, as `NonZero` does today, and the
   caller writes `.get()`. Accepting `T // NonZero[T]` and `T // Positive[T]`
   with result `T`, and omitting the zero check, is tracked as issue #664.
5. **`len()` does not convert.** It stays `i64`, and a length becomes a
   limit through `NonNegative.try_from`. This can be reopened if call sites
   show friction.
6. **Unsigned forms are rejected.** `Positive[u64]` is a compile error that
   points to `NonZero[u64]`, and `NonNegative[u32]` one that points to `u32`.
