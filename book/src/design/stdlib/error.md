# std.error — explicit mapping and contextual errors

Import `std.error`. A `Context[E]` records which operation failed and, when
there is one, the path it acted on, around an unchanged typed cause `E`. The
module is ordinary embedded Plenty code over [`Result`](../language/08-errors/01-sums-and-propagation.md)
and [`std.path`](path.md); it adds no exceptions, implicit conversions, or
runtime support.

```python
pub enum Subject:
    Absent
    At(Path)
    Unavailable(AllocError)

pub class Context[E]:
    pub operation: str
    pub subject: Subject
    pub cause: E
```

| Operation | Result | Contract |
| --- | --- | --- |
| `map_error(result, mapper)` | `Result[T, G]` | Consumes `Result[T, E]`; invokes an owned `OnceCallable[[E], G]` only on `Err` |
| `Context(operation, subject, cause)` | `Context[E]` | Takes ownership of all three; allocates nothing |
| `context(result, operation)` | `Result[T, Context[E]]` | Wraps an `Err` with `Subject.Absent`; allocates nothing |
| `path_context(result, operation, &path)` | `Result[T, Context[E]]` | Wraps an `Err`, copying the path only then |
| `error.describe()` | `Result[str, AllocError]` | One-line text ending in `str.repr` of the cause |
| `error.describe_with(&cause_text)` | `Result[str, AllocError]` | The same text with a caller-supplied cause |
| `error.report()` | `()` | Writes `error: ` and the description to standard error |
| `error.report_with(description)` | `()` | The same for a description `Result` the caller built |
| `native_code(error)` | `Option[i32]` | The OS code of `IoError.System(code)`; otherwise `Nothing` |

`context` and `path_context` consume the Result and return `Ok` payloads
unchanged, including `()` from operations such as `flush_stdout()` and
`File.close()`. Conversion is always this explicit call: `?` neither wraps an error
in a `Context` nor unwraps one, and the
[identical-error rule](../language/08-errors/01-sums-and-propagation.md)
applies to `Context[E]` as to any error type. A `Context` moves, because it can
own a path. Its fields are public: read `error.cause` or match it to recover the
typed cause, at any nesting depth. `?` in a function returning
`Result[T, Failure]` erases a `Context` like any other error.

## Explicit error mapping

`map_error[T, E, G, F: OnceCallable[[E], G]](result: Result[T, E], mapper: F)`
returns `Result[T, G]`. Use a named classifier when several error variants need
different policies, or a small anonymous function for a single wrapper:

```text
map_error(i64.parse(text), def (cause: ParseError) -> ConfigError: ConfigError.Parse(cause))?
```

The result and mapper expressions run once, left to right, even on success.
The mapper runs exactly once on `Err`, taking ownership of the original error;
on `Ok`, the success payload moves through unchanged and the unused mapper is
dropped. Named functions, reusable closures (including mutable environments),
and consuming closures are accepted. Captures and affine payloads follow normal
cleanup rules, including early return while constructing the mapper.

Mapping adds no allocation or boxing. Callback construction, execution, and
destruction retain their ordinary effects. If the callback returns
`Result[G, X]`, the output is `Result[T, Result[G, X]]`: nothing is flattened
and no cause-preservation policy is invented. Use [`context`](#allocation-failure)
or `path_context` when their explicit context-allocation policy fits.

This is a library function, not a Result method; `.map_err(...)` is not
implemented. `?` still requires identical error types (except deliberate
`Failure` erasure). The conversion is visible at the `map_error` call.
See [the lesson](../../tutorial/03-absence/error-details.md).

Worker-reachable calls need an inspectable concrete closure, such as
`def once (cause: ParseError) -> ConfigError: classify(cause)`. A thin named or
anonymous function value called indirectly inside a generic helper retains the
existing worker-thread effect restriction. The `once` form keeps the
callback body concrete without allocating; it does not bypass its effect check.

## Allocation failure

Attaching context never replaces the cause. `path_context` copies the borrowed
path only on `Err`; if that copy fails, the context holds
`Subject.Unavailable(alloc_error)` with the original operation and cause. To
avoid the copy, move an owned path into `Context(operation, Subject.At(path), cause)`.
The `Ok` path of both helpers, construction, and field access allocate nothing.

`describe` allocates the cause's representation, the escaped path, and
intermediate strings, and returns `AllocError` if any step fails. The context
is unchanged either way.

## Description format

```text
operation: cause
operation "escaped path": cause
operation <path unavailable>: cause
```

The path uses [`Path.display`](path.md): printable ASCII is kept, backslash and
double quote are escaped, and every other byte is `\xhh`, so a description is
one line of ASCII apart from the operation and cause. `describe` renders the
cause as [`str.repr`](../facilities/01-builtins/formatting.md) does; a native
code appears as `IoError.System(2)`.

A nested context is a cause like any other, so `describe` shows its structural
representation. Describe the inner context first and pass the text outward:

```python
outer.describe_with(&outer.cause.describe()?)
```

That gives `load settings: read_bytes "missing/settings": IoError.System(2)`.
An application error enum can match its variants and call the method that fits
each payload. [`std.report.Formattable`](report.md) provides borrowed formatting
for application types; `Context` does not implement it automatically yet.

## Reports

`report` flushes standard output, then writes `error: `, the description, and a
newline to standard error in one write. It returns unit: flush and write
failures are dropped, because standard error is the last channel. The caller
keeps the error and chooses the exit status, normally by returning `i32` from
[`main`](../language/02-source/entrypoint.md).

Neither fallback allocates:

| Condition | Standard error |
| --- | --- |
| The description cannot be allocated | `error: operation: no memory to describe the error` |
| The description exists but the whole line cannot be allocated | The normal line, written in three pieces |

`report_with(description)` applies the same rules to a description the caller
built, such as the nested one above; pass the `Result` unhandled.

Returning `Err(context)` from `main` still produces the
[entrypoint report](../toolchain/01-building/entry-report.md), which renders the
context structurally and shows a path as its byte list. Call `report` for the
format above.

## Conventions for standard modules

1. An operation returns its precise error type: `IoError`, `AllocError`, or the
   module's own enum. It does not attach context, format text, or allocate to
   describe a failure.
2. A module enum that can fail from allocation has a variant carrying the
   `AllocError`, as `WalkError.Allocation` and `DataError.Allocation` do.
   Allocation failure is never folded into `Other` or `Failure`.
3. A native code is kept as `IoError.System(code)` and never translated to
   text in the error value.
4. A composite operation that must say which step or path failed returns
   `Context[E]`, or an enum whose variants hold contexts. The operation label is
   a string literal naming the public operation, such as `read_dir`.
5. Failing to allocate context reports the original cause, with
   `Subject.Unavailable`.
6. Only an application decides to print. Library code returns errors; `report`
   belongs at the boundary that owns the exit status.

[`WalkError.Io`](fs/walking.md) predates this module and keeps its own
`(path, operation, cause)` payload.

## Limits

- `E` must be a type `str.repr` accepts. A cause holding recursive data or a
  generator is rejected where `Context[E]` is first used, because every method
  of a class instance is checked together.
- No OS description text is added to a native code.
- A standard-error pipe whose reader has gone terminates the process with
  `SIGPIPE`, as for any `write_stderr`. Only the entrypoint report suppresses it.
- A piecewise fallback report is not atomic against concurrent writers.
- `Subject` has no network address or other non-path variant.

A `Context[E]` can cross threads when `E` can. Every operation takes time linear
in the text it produces. Supported target: x86_64 Linux GNU. The lesson is
[Say which operation failed](../../tutorial/08-resources/23-error-context.md).
