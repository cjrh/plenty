---
name: plenty
description: Write, fix, and explain programs in the Plenty programming language (`.plenty` files, `plenty.toml` packages, the `plenty` compiler). Use whenever a task touches Plenty source, Plenty diagnostics, or the Plenty standard library (`std.*`). Plenty looks like Python but rejects many Python habits; read this before writing any Plenty code. It teaches the language and toolchain; it does not search a project's issues or design history.
---

# Writing Plenty programs

Source files accept `.plenty` or `.p`; trusted interfaces accept `.plentyi` or
`.pi`. Different modules may mix spellings, but multiple spellings of the same
imported module are ambiguous. Package default targets and tests accept `.p`.

Plenty is statically typed, with Python-shaped syntax, single ownership with
checked borrows, no exceptions, and no hidden allocation. It compiles ahead of
time to native code. Code that looks like correct Python is usually wrong
Plenty.

This skill describes the Plenty compiler in this repository. Run `plenty --version`; if the installed
compiler differs, use its matching skill release. When investigating a reported
gap, record the compiler version and the actual skill path/version loaded; a
current checkout does not establish what an earlier agent read.

Builds optimize by default. Use `plenty --fast-compile FILE`,
`plenty script --fast-compile FILE`, or `plenty run --fast-compile` for shorter
edit-run cycles in large projects; runtime checks stay enabled. Package output
is in `target/optimized/` by default or `target/fast-compile/` with that flag.

## Workflow

1. Before writing code, read [the cheat sheet](../../../book/src/design/examples/cheatsheet.md) and, for
   any `std` call, find its signature in
   [the signature index](../../../book/src/design/stdlib/signatures.md). Never guess an API: search
   the index for the name, then read the module page it links for ownership,
   allocation, and error contracts. Its Modules list gives links and line ranges
   for reading one module at a time.
2. Write the program. Executable statements belong in a function; a bare
   top-level statement is an error.
3. Type-check: `plenty --check FILE` for one file, or `plenty check` inside a
   package. For an embedded script manifest, use `plenty script --check FILE`.
   Fix every diagnostic, then check again. Read the whole message, including
   its notes; conservative concurrency checks can report a possible hazard
   without proving it occurs.
4. Run: `plenty FILE`, or `plenty run` / `plenty test` in a package.
   `plenty FILE ARGS` is rejected; to pass arguments, use
   `plenty script FILE ARGS` (runner options precede FILE), or build with
   `plenty --compile FILE -o OUT` and run `./OUT ARGS`, or use
   `plenty run -- ARGS` in a package.

Before porting a library or reporting a missing feature, use the
[capability discovery guide](references/discovery.md): start from the task,
verify the current contract, then check a minimal reproduction. An unfamiliar
name or a rejected spelling is not evidence that the capability is absent.

Use a package (`plenty new NAME`) for several executables or tests. Single-file
utilities can declare path/Git dependencies in an optional script header.
See [workflow and packages](references/workflow.md).

## Pool schedulers and cancellation

For a loop that submits jobs and receives completions, read
[pool scheduler guidance](references/concurrency.md) and the
[runnable helper loop](../../../book/src/tutorial/11-concurrency/scheduler-loops.md);
for several jobs in flight, [`std.sync.Completions`](../../../book/src/tutorial/11-concurrency/scheduler-window.md).
Pool drop joins accepted work; receiver-last guards protect scope exit, not
mid-call shutdown. Use restricted `Submitter` capabilities where appropriate,
but account for current conservative helper-call limitations. Preserve start
errors, worker errors and partially prepared resources when restructuring.

## Typed serialization

Use `@serial` on classes and enums, then `std.json.load[T]` / `dump` or
`std.toml.load[T]` / `dump` for typed documents. `to_value`/`from_value` convert
directly to owning format trees; `from_value` consumes its input. Derivation
generates missing private `__encode__` and receiver-less `__decode__` hooks;
write either hook for a custom schema. Constructor defaults apply only to
absent fields; a missing optional field without a default becomes `Nothing`.
Validated constructors returning `Fault` retain typed failures; other domain
errors need a handwritten decoding hook. TOML omits null class fields and
rejects null dictionary values. Bytes use canonical padded base64; runtime
decoding returns owning `Bytes`, not static `bytes` literals. Read the
[codec contract](../../../book/src/design/stdlib/serial.md) and
[JSON limits and options](../../../book/src/design/stdlib/json-codecs.md)
before writing hooks. JSON `dump`, tree encoding, and `encoder` accept
`indent=Some(2)` for readable output within the same byte limit. For class fields call `Encoder.field`; mapping keys use
`Encoder.key`. Typed failures carry `SerialError[JsonError]` or
`SerialError[TomlError]` with an innermost-first path; propagate that complete
type. `std.json.load_from` borrows an owning `CodecReader[R]` made with
`codec_reader(inner)`; its public `inner` preserves the reader on failure. Render typed errors with
`std.report.to_string(&error, max_bytes, max_depth)` for readable causes and
root-first paths. `std.report.diagnostic(&error, source)` explicitly adds a
bounded source excerpt when coordinates exist; TOML schema/tree errors retain
paths without fabricated source locations. Kind mismatches retain expected and
actual `std.serial.Kind` values. Rendering borrows and preserves the error.

## Rules that prevent most errors

- `main` returns `()`, `i32`, `Result[(), E]`, or `Result[i32, E]`. Use
  `def main() -> Result[(), AllocError | IoError]:` and end with `Ok(())`.
- Every `Result` must be used. `print(...)` returns one: write `print(x)?`.
- Collection literals, comprehensions, `append`, `insert`, `add`, string `+`,
  `split`, `copy`, and f-strings allocate, so they return `Result`: write
  `xs = [1, 2]?`, `(a + b)?`, `f"{x}"?`, `items.append(v)?`.
- `?` propagates `Err` or `Nothing`. The function's error type must equal the
  source's error or contain it in a union (`AllocError | IoError | ParseError`).
  `?` never turns an `Option` into a `Result`; use `.ok_or(error)?`.
- `unwrap_or(default)` and `ok_or(error)` evaluate their arguments eagerly.
  For computed defaults use `unwrap_or_else`, or `map_or_else(fallback, Ok)?`
  when the fallback returns a Result; see [defaults](references/errors.md#eager-and-lazy-defaults).
- No exceptions, `None`, `try`, `raise`, or `lambda`. Use `Result[T, E]`,
  `Option[T]` (`Some(x)` / `Nothing`), and `def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + 1)`.
- Integer arithmetic returns errors from the enclosing function by default:
  declare `OverflowError[T]` for `+`, `-`, `*`, unary `-`, and integer `pow`;
  `//` and `%` by a variable also require `ZeroDivisionError` (signed `//` can
  overflow). A nonzero literal or integer module-constant divisor adds
  neither, except that `x // -1` can overflow: `x // 2` cannot fail.
  `<<` and `>>` add `ShiftCountError[T]` unless the count is a literal or
  constant, or a mask `n & L` with such an `L` below the width. Locals and
  negated constants prove nothing. Casts such as `u8(x)` trap; use `u8.try_from(x)` (integers or floats)
  to recover.
  Write explicit `Ok(...)` on success. Use `checked:` for local recovery or a
  small, justified `unchecked trap:` suite for deliberate termination. Plain
  `unchecked:` asserts arithmetic preconditions: violations trap by default but
  are undefined behavior under `--unchecked assume`, never wrapping arithmetic. Functions and
  closures keep independent declared contracts; a caller's mode never changes
  them. In assume mode, list, slice-view and `Bytes` indices in a plain
  `unchecked:` suite must also be nonnegative and in range; bounds checks and
  negative-index normalization are omitted. Other indexing and collection
  methods keep their contracts. Integer `std.math.abs`/`sum`/`prod` return `Result`; the `*_float`
  helpers stay infallible, and float arithmetic keeps IEEE behavior. Serial
  pipeline integer `.sum()` also returns a public `Result`. For fallible
  accumulation callbacks use `try_fold`, as in [arithmetic and callbacks](references/errors.md#arithmetic-and-callbacks).
- Types are explicit on every parameter and result. `i64` and `f64`, not `int`
  and `float`. No implicit numeric widening; cast with `f64(n)`.
- `Positive[T]` (> 0) and `NonNegative[T]` (>= 0) are signed integer domains.
  Literals convert where expected; computed values use `try_from(n)?`, failing
  with `RangeError.OutOfRange`. `.get()` reads `T`. `+` and `*` preserve the
  domain; subtraction and unary `-` return `T`. Widen a Positive explicitly
  with `NonZero.from(p)` or `NonNegative.from(p)`.
- `Bounded[T, lo, hi]` is an inclusive interval over any integer type. Literals
  convert where expected; computed values use a complete instance or alias,
  such as `Percent.try_from(n)?`. Read `.get()` for arithmetic. Value generic
  parameters use `const N: i64` or a preceding integer parameter
  `[T: IntType, const N: T]`; bracket arguments are literals, module constants
  or forwarded parameters, never arithmetic expressions.
- `channel[T](capacity)` and `ThreadPoolExecutor(workers, queue_capacity)`
  take `Positive[i64]` counts, as does `.par(&pool, chunks=...)`. Positive
  literals convert; computed counts use `Positive.try_from(count)?`. Channel
  construction returns `AllocError`; `PoolError` has `Allocation` and `Thread`.
  For a wake-only signal use `channel[()]` and `send(())`, not a placeholder
  `bool`. Generics that take or bind a `T` value, such as
  `std.sync.send_before`, do not yet accept `T = ()`.
- Bindings are immutable. Declare `mut x = 0` once to reassign or mutate.
- Numbers, `bool`, `str`, `bytes`, ranges, and named function values copy. A
  tuple, `Option`, `Result`, enum, or anonymous sum (`A | B`) copies when all of
  its parts copy. A class copies only when it is declared [`@copy`](../../../book/src/design/language/03-types/classes.md#copy-classes), and a generic one only when its type arguments
  also copy.
  Every other value moves: collections, boxes, closures that capture,
  generators, and other class instances.
  A moved name is unusable. Borrow with `&x` (shared) or `&mut x`
  (exclusive), or copy explicitly with `copy(x)?`. An eligible ordinary class
  can be explicitly copied without `@copy`: owned mutable fields duplicate,
  while nested `Shared` handles retain the same content. Direct handles use
  `.share()`. See the [applied copy example](references/ownership.md#explicit-class-copy-and-shared-content). Iterating `&list[T]`
  yields `&T`; dereference with `*value`. `for x in owner` moves `owner`;
  write `for x in &self.items` to borrow.
- A binding made inside an `if`, loop, or `match` arm ends with that block.
  Select an immutable result with `name = if condition:` or `name = match value:`;
  their selected suite's final expression is the value. Use `a if cond else b`
  for two single expressions, or declare `mut name = default` before the block
  when updating an existing value is the intent. Only `if const` arms open no scope.
- Methods that mutate declare `self: &mut ClassName`; bare `self` is a shared
  borrow.
- Dictionaries have no indexed assignment: `d.insert(k, v)?` adds or
  replaces, and `d.get_mut(k)` returns `Option[&mut V]` to change a value in
  place. To change every value, loop over `(&mut d).values()` or
  `(&mut d).items()`; there is no `values_mut()`. Reading `d[k]` traps on a missing key; `d.get(k)` returns an `Option`.
- Slices copy and allocate: `xs[1:3]?`, `s[::-1]?`. A computed step needs
  `NonZero.try_from(step)?`. Slice assignment is rejected.
- `&xs[1:3]` and `&mut xs[1:3]` borrow a list or `Bytes` view, `&[T]` or
  `&mut [T]`, without copying: no `?`, no step. A `&[T]` parameter takes a
  whole list as `&xs`, and a `&[u8]` one a `b"..."` literal. `&s[1:3]` borrows
  part of a `str` as a `&strview` (scalar positions, shared only); `&substr`
  remains a compatible alias. Plain `&s` keeps its ordinary `&str` type.
  A `&strview` parameter implicitly borrows a whole string passed as `s`; `&s`, a `&str`
  binding, or a literal also works. Annotated view locals still need an
  explicit borrow, such as `whole: &strview = &s`.
  Views cannot be returned or stored. `std` operations that read bytes
  (`Bytes.extend`, stream `write`, `write_all`, crypto `update`) take one
  `&[u8]`, not `source, start, stop`: `w.write(&buf[a:b])`. A writer class
  declares `def write(self: &mut W, source: &[u8]) -> Result[i64, StreamError]`.
  `&mut xs[:m]` and `&mut xs[m:]` are disjoint when `m` is a literal or a
  non-`mut` `i64` name; other overlapping or unprovable ranges conflict.
  Any list view takes `x in v`, `v.count(x)`, `v.find(x)`, `v.rfind(x)`, `v.get(i)`,
  and copies with `list(v)?` or `v[a:b]?`; a `&mut [T]` view also takes
  `v[i] = x`, `v.swap(i, j)` and `v.reverse()`. Its length is fixed, so
  `append`/`pop`/`sort` belong to the owner.
- A plain integer `T` accepts a `NonZero[T]` or `Positive[T]` divisor directly: `n // step`,
  `n % step`, `n //= step`, `n %= step`. The result is `T`; no zero check
  remains. Signed `MIN // -1` still requires `OverflowError[T]` in the
  function's `Result` for a `NonZero` divisor, or local handling with
  `checked:`; a `Positive` divisor cannot cause either division failure.
  A domain dividend still needs `.get()`.
- No `**`, truthiness, chained comparisons, or inheritance. Use
  `x.pow(n)`, `len(xs) > 0`, `a < b and b < c`, and composition or protocols.
- `abs`, `min`, `max`, `sum` come from `std.math`. Standard modules need an
  explicit import; prelude names (`print`, `len`, `range`, `open`,
  `read_text`, `args`, ...) do not.

## Idioms

- Propagate errors to `main`. Returning `Err` from `main` reports the error and
  exits with status 1. A `Context[E]` error prints its raw fields that way;
  for a readable message, call `error.report()` in `main -> i32`. Printing an error and returning `Ok(())` reports
  success; do that only when the program genuinely recovers.
- Name error groups with an alias: `type AppError = AllocError | IoError | ParseError`.
  Define an enum only to add meaning, and map into it with `.map_error(...)`.
- Prefer `&strview` for text readers and `&list[T]` for borrowed lists.
  Ordinary `&str` remains useful for generic references and sharing a whole
  owning string handle; use `str` when the function keeps an owning value.
  On text views, scalar `len` scans; `byte_len()` and `as_bytes()` are O(1).
  `as_bytes()` lends a read-only `&[u8]` with the text owner’s loan;
  `std.bytes.from_bytes(...)` makes an independent mutable copy. Avoid `copy` unless two owners are needed. Never write `copy(x)?`
  or `copy(&x)?` when `x` copies (`str`, numbers, `@copy` classes): write `x`;
  for a reference `r: &str`, write `*r` instead of `copy(r)?`. `plenty --check`
  reports them as `redundant-copy` warnings; apply the suggested replacements.
- Checks also report `unused-import` when an import binding has no resolved use
  in its module, including types and generic bodies. Remove unneeded imports,
  but retain an import that intentionally keeps a module in checking: deleting
  it can reduce validation coverage. Warnings are advisory and do not fail a check.
- Checks report `text-view-parameter` when a `str` or ordinary `&str`
  parameter only supplies recognized text reads. Consider `&strview` to accept
  substring inputs too; this is advisory and preserves ordinary reference and
  ownership-dependent signatures. Scalar `len(view)` scans; this suggestion
  does not promise a speedup.
- Reserve capacity when the size is known: `list[i64].with_capacity(n)?`.
- Mark a class or enum that holds a secret `@redacted`: `print`, `str.repr`
  and f-strings then show `ApiKey(@redacted)` at any depth, while field access is
  unchanged. See [redacted types](../../../book/src/design/language/03-types/classes.md#redacted-types).

See [errors](references/errors.md), [ownership](references/ownership.md), and
[common diagnostics](references/diagnostics.md).

## Choosing types and interfaces

- Use `A | B` or an enum for a closed set of alternatives and `match` to
  distinguish them. Use a generic protocol bound for static dispatch; use
  `Box[P](value)?` when different implementations need one stored owner type.
  Boxing consumes the concrete value and allocates. Bare `P` is not a value
  type; borrow with `&Box[P]`. Boxed protocols cannot be copied, downcast, used
  as keys, or transferred to workers. Read [boxing eligibility and limits](../../../book/src/design/language/07-generics/boxed-protocols.md)
  before designing the interface; generic methods and consuming receivers
  cannot be boxed.
- Dictionary keys and set members can be tuples, classes, enums and anonymous
  sums whose fields and payloads are recursively supported keys. Floats,
  collections, boxes and recursive types are excluded; custom hash/equality
  hooks are unsupported. Use `K: KeyType` in generic collection helpers; it
  does not imply copying. See [collection contracts](../../../book/src/design/facilities/01-builtins/collections.md).
- For physical values, use `Quantity[T, D]` and [std.units](../../../book/src/design/stdlib/units.md)
  rather than manually tracking unit scales. For example, `90.0 * u.km / u.hour`
  constructs a velocity and `speed.to(u.m / u.s)` returns its numeric value in
  that unit. Import with `import std.units as u`. Dimensions are checked at
  compile time; formatting uses coherent SI units. Use `u.from_celsius` and
  `u.to_celsius` for absolute Celsius temperatures, not multiplication by an
  offset unit. Read [quantities](../../../book/src/design/language/03-types/quantities.md)
  for arithmetic and representation contracts.

- For supported HTML layouts, prefer `Engine.render_html(name, context)`; it
  preflights static structure and infers text/quoted-attribute escaping.
  `validate_html(name)` checks without values. Read [the strict subset](../../../book/src/design/stdlib/template-html.md);
  dynamic URLs, scripts/styles, HTML comments and trust filters are rejected.
- Use [std.template](../../../book/src/design/stdlib/template.md) for named text/HTML
  layouts. For directly assembled HTML fragments, `escape(text, HtmlContext, max_bytes)`
  provides bounded text and quote-specific attribute escaping. Compose registered
  fragments with `{% include 'form' with row %}` using an object context; no
  string concatenation or `safe` is needed. Each include has its own loop scope
  and shares the render budgets. Choose the context
  explicitly and escape each original value once. The result is ordinary `str`,
  with no trust marker; keep raw values in `Json.Text` for `Engine.render`.
  Its `escape_text`, `escape_double_quoted` and `escape_single_quoted` filters
  choose an output context per placeholder without `safe`. Keep matching quotes
  around attributes. Put selectors after `upper`/`lower` (which reset policy);
  `trim` preserves selection, the last selector wins, and explicit selectors
  still escape with `html=False`. Reserve `safe` for deliberate trusted markup.
  Conditions accept strict scalar `==`/`!=`, `not`, short-circuit `and`/`or`,
  parentheses and `elif`; keep raw IDs in JSON instead of precomputing flags.
  Boolean operators return booleans; integers, floats and booleans are distinct
  equality kinds. Filters bind before equality, then `not`, `and`, `or`.
  Bound aggregate syntax with `max_expression_nodes` as well as `max_tokens`.

- For child descriptor protocols, use `Command.inherit_file(&file)` or
  `Lock.inherit_into(&mut command)` and pass the returned number through argv
  or the environment. The command owns a close-on-exec copy; only its children
  inherit it. Source close is safe; cursors and locks are shared. Clear command
  copies after spawning with `clear_inherited_files()` when reuse is unnecessary,
  or those copies can keep a lock alive. At most 64 selections per command;
  arbitrary fd remapping is unsupported. `Stdio.File` redirects standard streams
  independently; only selected copies survive as extra descriptors. Read
  [explicit inheritance](../../../book/src/design/stdlib/process.md#explicit-file-inheritance).

- For template layout, `{{- value -}}`, `{%- ... -%}` and `{#- ... -#}` trim
  adjacent literal ASCII whitespace. Plain delimiters preserve it; Unicode
  whitespace and rendered values are unchanged. Markers cannot trim through
  another tag or from an overriding block into its base layout. See
  [whitespace control](../../../book/src/design/stdlib/template.md#whitespace-control).

- For reusable template contexts, declare `std.template.Shape` and call
  `Engine.validate(name, &schema)` to check every reachable path without samples.
  `render_checked(name, &context, &schema)` also checks actual JSON. Prefer required
  nullable fields over optional reads; preflight checks all union alternatives
  and loop element shapes. See [declared context shapes](../../../book/src/design/stdlib/template-schema.md).

## References

| Page | Use |
|---|---|
| [Cheat sheet](../../../book/src/design/examples/cheatsheet.md) | Every everyday form, with the Python habits Plenty rejects |
| [Signature index](../../../book/src/design/stdlib/signatures.md) | Every public `std` declaration, one line each |
| [Standard library](../../../book/src/design/stdlib/index.md) | Module table; one page per module |
| [Language reference](../../../book/src/design/language/index.md) | Types, control flow, generics, ownership and arithmetic contracts |
| [Builtins](../../../book/src/design/facilities/01-builtins/index.md) | Prelude text, numbers, and collections |
| [I/O](../../../book/src/design/facilities/02-io/index.md) | Prelude files, streams, arguments |
| [Packages](../../../book/src/design/toolchain/01-building/packages.md) | `plenty.toml`, dependencies, project commands, tests |
| [Workflow](references/workflow.md) | Commands, module boundaries and a package walkthrough |
| [Capability discovery](references/discovery.md) | Task-to-module pointers and verifying reported gaps |
| [Errors](references/errors.md) | Error types, propagation, context, exit status |
| [Ownership](references/ownership.md) | Moves, borrows, copies, allocation |
| [Diagnostics](references/diagnostics.md) | Frequent compiler errors and their fixes |

For subprocess cleanup, `std.process.Command.process_group(ProcessGroup.NewSession)`
establishes a child-led group before execution. Send `Child.terminate_group()` or
`kill_group()` before reaping the leader, then `wait()`. A completed wait disables
group signalling to avoid reused IDs; descendants can escape by changing groups
or sessions. Read [std.process](../../../book/src/design/stdlib/process.md#process-groups-and-sessions)
for the Linux support and ownership contract.
