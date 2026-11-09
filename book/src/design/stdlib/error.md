# std.error — explicit mapping and contextual errors

Import `std.error`. A `Context[E]` records which operation failed and, when
there is one, the path it acted on, around an unchanged typed cause `E`. The
module is ordinary embedded Plenty code over [`Result`](../language/08-errors/01-sums-and-propagation.md)
and [`std.path`](path.md); it adds no exceptions, implicit conversions, or
runtime support.

```plenty fragment
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
| `map_error(result, mapper)` | `Result[T, G]` | Consumes `Result[T, E]`; invokes an owned `OnceCallable[[E], G]` only on `Err`; same as the `result.map_error(mapper)` method |
| `Context(operation, subject, cause)` | `Context[E]` | Takes ownership of all three; allocates nothing |
| `context(result, operation)` | `Result[T, Context[E]]` | Wraps an `Err` with `Subject.Absent`; allocates nothing |
| `path_context(result, operation, &path)` | `Result[T, Context[E]]` | Wraps an `Err`, copying the path only then |
| `error.describe()` | `Result[str, AllocError]` | One-line text ending in `str.repr` of the cause |
| `error.describe_with(&cause_text)` | `Result[str, AllocError]` | The same text with a caller-supplied cause |
| `error.format(&mut output)` | `Result[(), FormatError]` | Borrowed `std.report` formatting when `E` satisfies `Formattable` |
| `error.report()` | `()` | Writes `error: ` and the description to standard error |
| `error.report_with(description)` | `()` | The same for a description `Result` the caller built |
| `native_code(error)` | `Option[i32]` | The OS code of `IoError.System(code)`; otherwise `Nothing` |
| `native_message(code)` | `Option[str]` | The fixed English description of a Linux error number; otherwise `Nothing`; allocates nothing |

`context` and `path_context` consume the Result and return `Ok` payloads
unchanged, including `()` from operations such as `flush_stdout()` and
`File.close()`. Conversion is always this explicit call: `?` neither wraps an error
in a `Context` nor unwraps one, and the
[identical-error rule](../language/08-errors/01-sums-and-propagation.md)
applies to `Context[E]` as to any error type. A `Context` moves, because it can
own a path. Its fields are public: read `error.cause` or match it to recover the
typed cause, at any nesting depth. `?` in a function returning
`Result[T, Failure]`, the last-resort erased error, discards a `Context` like any
other error; list `Context[E]` in the sum to keep it.

```plenty
from std.error import Context, path_context
from std.path import Path, from_text

def read_settings(path: &Path) -> Result[list[u8], Context[IoError]]:
    path_context(read_bytes(path.as_bytes()), "read settings", path)

def main() -> Result[(), AllocError | IoError]:
    path = from_text("settings.toml")?
    match read_settings(&path):
        case Ok(bytes):
            print(len(bytes), "bytes of settings")?
        case Err(error):
            # A missing settings file is expected: report it and use defaults.
            print(error.describe()?)?
            print(error.cause == IoError.System(2i32))?
            print("using default settings")?
    Ok(())
```
```output
read settings "settings.toml": IoError.System(2)
True
using default settings
```

## Describing a native code

`native_message(code)` returns the description of a Linux error number, the
text the GNU C library gives it in the C locale, such as
`"No such file or directory"` for 2. It takes any OS code, so it also serves
codes held by other errors, such as `ThreadError.System(code)`. The text is a
string literal fixed in `std.error`: the call allocates nothing, reads no
`errno`, locale or other shared state, is safe on any worker, and gives the
same text whatever C library the program links. A code outside the Linux
(x86-64 and aarch64) numbers 1 to 133, or one of the unassigned numbers 41
and 58, returns `Nothing`; the caller chooses the fallback. The experimental
macOS and Windows targets number their errors differently, so there it always
returns `Nothing` rather than a wrong text. The error value keeps its numeric
code: combine the two where the application reports.

```plenty
from std.error import native_code, native_message, path_context
from std.path import from_text

def main() -> Result[(), AllocError | IoError]:
    path = from_text("settings.toml")?
    match path_context(read_bytes(path.as_bytes()), "read settings", &path):
        case Ok(bytes):
            print(len(bytes), "bytes of settings")?
        case Err(error):
            match native_code(error.cause):
                case Some(code):
                    message = native_message(code).unwrap_or("unknown error")
                    cause = f"{message} (os error {code})"?
                    print(error.describe_with(&cause)?)?
                case Nothing:
                    print(error.describe()?)?
    print(native_message(9999i32))?
    Ok(())
```
```output
read settings "settings.toml": No such file or directory (os error 2)
Option[str].Nothing
```

The [entrypoint report](../toolchain/01-building/entry-report.md) of an
`IoError.System(code)` asks the C library at run time, so it can differ from
`native_message` under another C library.

## Explicit error mapping

`result.map_error(mapper)` consumes `Result[T, E]` and returns `Result[T, G]`,
where `mapper` is an owned `OnceCallable[[E], G]`. A one-payload enum variant is
the usual mapper; use a named classifier when several error variants need
different policies, or a closure to add other payloads or captured context:

```text
i64.parse(text).map_error(ConfigError.Parse)?
print(line).map_error(def [path](cause: IoError) -> AppError: AppError.Write(path, cause))?
```

The library function
`map_error[T, E, G, F: OnceCallable[[E], G]](result: Result[T, E], mapper: F)`
has the same behavior; the method is a compiler-provided `Result` method, not
an import. A class or enum that defines its own `map_error` keeps it.

The receiver and mapper expressions run once, receiver first, even on success.
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

`?` still requires identical error types (except legacy `Failure` erasure);
the conversion is visible at the `map_error` call. The mapper must be owned and
take exactly the error type: a reference, a variant with several payloads, or
a callable returning unit is rejected. `.map_err(...)` is not implemented.
See [the lesson](../../tutorial/03-absence/error-details.md).

Worker-reachable calls accept thin named or anonymous function values,
including variant constructors such as `ConfigError.Parse`, when every function
address of that callable signature in the program passes
[worker-effect certification](../facilities/03-concurrency/effects.md#thin-callable-targets).
The compiler checks every candidate body and its stored types; one safe target
never excuses an unsafe one. A main-thread-only address with the same signature
can conservatively prevent certification. A concrete closure retains its own
body and capture evidence; it cannot bypass the checks either.

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

```plenty fragment
outer.describe_with(&outer.cause.describe()?)
```

That gives `load settings: read_bytes "missing/settings": IoError.System(2)`.
An application error enum can match its variants and call the method that fits
each payload. [`std.fmt.Formattable`](report.md) provides borrowed formatting
for application types. `Context[E]` supports this protocol when `E` does:
it emits the operation and subject, then calls `render` on the typed cause
exactly once. Nested contexts therefore produce nested operation labels without
duplicating the cause. Each nested `render` counts toward the depth limit.

This conditional method is compiler-supported for the embedded `std.error`
type; it does not introduce conditional methods for arbitrary generic classes.
An unformattable cause keeps the existing description APIs and structural
entrypoint reporting. `describe` and `describe_with` retain their existing
behavior even when the cause has a `format` method.

Formatting an absent or unavailable subject adds no allocation. A present path
uses the allocating `Path.display`; failure is returned as
`FormatError.Allocation(...)`, preserving the cause and any
already-written prefix. Operation and cause text are emitted unchanged.

## One application error with context

A `Context[E]` changes the error type at every layer: `Context[ParseError]` does
not widen to `Context[AllocError | ParseError]`, and wrapping a context gives
`Context[Context[E]]`. An application that wants one error type for all of its
functions, with operation labels added at several layers, can define a small
class for the labelled case and add it to its error sum. No `std` type is
needed, and no cause is turned into text:

```plenty
from std.io import Partial, StreamError, write_all

type Base = AllocError | IoError | ParseError | Partial

# Caps how many labels one error keeps, so a retry loop cannot grow it.
# Labels are normally literals; a formatted label adds its own length.
const FRAMES: i64 = 8

# A typed cause with the innermost operation and later labels, innermost
# first. Wrapping allocates nothing; the label list is made on first use. A
# label past FRAMES, or one without memory to store it, is only counted.
class Trace:
    pub cause: Base
    pub operation: str
    pub outer: Option[list[str]]
    pub omitted: i64

    def add(self: &mut Trace, operation: str) -> ():
        if self.kept() < FRAMES:
            match &mut self.outer:
                case Some(labels):
                    if labels.append(operation).is_ok():
                        return
                case Nothing:
                    match [operation]:
                        case Ok(labels):
                            self.outer = Some(labels)
                            return
                        case Err(_):
                            pass
        if self.omitted < i64.MAX:
            unchecked:
                self.omitted += 1

    def kept(self) -> i64:
        match &self.outer:
            case Some(labels):
                len(labels)
            case Nothing:
                0

    # Outermost first, as nested `Context` descriptions read.
    def describe(self) -> Result[str, AllocError]:
        mut text = self.operation.concat(": ")?.concat(&str.repr(self.cause)?)?
        match &self.outer:
            case Some(labels):
                for label in labels:
                    text = label.concat(": ")?.concat(&text)?
            case Nothing:
                pass
        if self.omitted > 0:
            text = f"[{self.omitted} omitted] {text}"?
        Ok(text)

type Error = Base | Trace

def context[T](result: Result[T, Error], operation: str) -> Result[T, Error]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(error):
            match error:
                case trace: Trace:
                    mut traced = trace
                    traced.add(operation)
                    Err(traced)
                case rest: Base:
                    Err(Trace(rest, operation, Nothing, 0))

# Accepts `room` bytes, then reports no progress.
pub class Limited:
    pub room: i64

    pub def write(self: &mut Limited, source: &[u8]) -> Result[i64, StreamError]:
        mut taken = len(source)
        if taken > self.room:
            taken = self.room
        unchecked:
            self.room -= taken
        Ok(taken)

    pub def flush(self: &mut Limited) -> Result[(), StreamError]:
        Ok(())

def save(out: &mut Limited) -> Result[(), Error]:
    context(write_all(out, b"settings"), "write settings")

def setup(out: &mut Limited) -> Result[i64, Error]:
    context(save(out), "save settings")?
    Ok(i64.parse("10")?)

def main() -> Result[(), Error]:
    mut out = Limited(3)
    match context(setup(&mut out), "setup"):
        case Ok(limit):
            print(limit)?
        case Err(error):
            match error:
                case trace: Trace:
                    match &trace.cause:
                        # A full device is expected here: report the progress
                        # and continue. Forward every other failure.
                        case partial: Partial:
                            print(trace.describe()?)?
                            print(partial.completed, "bytes written")?
                        case _:
                            return Err(trace)
                case rest: Base:
                    return Err(rest)
    Ok(())
```
```output
setup: save settings: write settings: std.io.Partial(completed=3, cause=std.io.StreamError.WriteZero)
3 bytes written
```

`Error` is `Base | Trace`. Plain `?` still propagates any `Base` member, and
`context` accepts any `Result` whose error widens to `Error`, such as the
`Result[(), Partial]` of `write_all`. It wraps a plain member once and extends
an existing `Trace`, so nesting keeps one type. The
[residual member pattern](../language/03-types/anonymous-sums.md#residual-members)
`case rest: Base:` binds every remaining member with the smaller type; the
alias name is accepted there.

The pattern keeps the contracts of `Context`:

- The cause keeps its type, including the progress in `Partial`; match
  `trace.cause` to recover from it.
- Wrapping a cause allocates nothing. Each later label may allocate the label
  list. A label that cannot be stored, for lack of memory or past `FRAMES`, is
  counted in `omitted`; the cause and earlier labels are kept.
- `FRAMES` caps the number of labels, so a retry loop cannot grow an error
  without limit. It does not cap a label's length; labels are normally string
  literals, and a formatted label costs its own size.
- A `Trace` can cross threads when every `Base` member can, so a worker can
  return `Result[T, Error]` through `join`.

`describe` uses `str.repr` for the cause, as `Context.describe` does. To give
the whole error a `std.report` format instead, every `Base` member needs a
`format` method; builtin errors such as `IoError` do not have one.
Collapsing a cause into a message string, or propagating into `Failure`,
discards what the caller could match. A shared `std` type for this pattern is
[recorded in the backlog](../../backlog.md) (B71).

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

Returning `Err(context)` from `main` produces the
[entrypoint report](../toolchain/01-building/entry-report.md). It selects the
typed formatting method when the cause is formattable, otherwise renders the
context structurally and shows a path as its byte list. The explicit `report`
and `report_with` APIs keep their description-based behavior.

## Conventions for standard modules

1. An operation returns its precise error type: `IoError`, `AllocError`, or the
   module's own enum. It does not attach context, format text, or allocate to
   describe a failure.
2. A module enum that can fail from allocation has a variant carrying the
   `AllocError`, as `WalkError.Allocation` and `DataError.Allocation` do.
   Allocation failure is never folded into `Other` or `Failure`.
3. A native code is kept as `IoError.System(code)` and never translated to
   text in the error value; code that reports it calls
   [`native_message`](#describing-a-native-code).
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

- `describe` and `report` require a cause type `str.repr` accepts. A cause
  holding recursive data or a generator is rejected where one of them is first
  called for that `Context[E]`.
- `describe`, `format` and `report` add no OS description to a native code;
  use [`native_message`](#describing-a-native-code). Descriptions are English
  only and cover the Linux (x86-64 and aarch64) error numbers; other targets
  get `Nothing`.
- The legacy `report` and `report_with` methods use `write_stderr`: a pipe whose
  reader has gone terminates the process with `SIGPIPE`. The `std.report`
  formatter and entrypoint reporter suppress it and preserve normal cleanup.
- A piecewise fallback report is not atomic against concurrent writers.
- `Subject` has no network address or other non-path variant.

A `Context[E]` can cross threads when `E` can. Every operation takes time linear
in the text it produces. Supported target: x86_64 Linux GNU. The lesson is
[Say which operation failed](../../tutorial/08-resources/23-error-context.md).

## Acknowledgements

- `context` and `path_context` are modelled on the [`Context`](https://docs.rs/anyhow/latest/anyhow/trait.Context.html) trait of the Rust [`anyhow`](https://crates.io/crates/anyhow) crate.
