# `std.report`

`std.report` provides bounded human-readable reports using the same
[`std.fmt.Formatter[W]` and `FormatError`](fmt.md) as value display and
f-strings. It provides rendering helpers for the canonical `std.fmt.Formattable`
protocol. Source-aware diagnostics additionally use `Diagnostic`, which requires
that same `format` method plus `source_offset() -> Option[i64]`. Import formatter
types and constructors from `std.fmt`; both paths share `FormatError`.

```plenty
import std.report
from std.fmt import FormatError, Formatter

pub class DiskFull:
    path: str
    needed: u64

    pub def format[W](self, output: &mut Formatter[W]) -> Result[(), FormatError]:
        output.text("disk full writing ")?
        output.text(&self.path)?
        output.text(": need ")?
        output.unsigned(self.needed, 10)?
        output.text(" more bytes")

def main() -> Result[(), IoError | FormatError]:
    error = DiskFull("out.csv", 4096u64)
    print(std.report.to_string(&error, 256, 4)?)?
    Ok(())
```
```output
disk full writing out.csv: need 4096 more bytes
```

## Formatting contract

The structural `Formattable` protocol requires a public shared-receiver method
on a public class or enum, since its bound is checked in `std.fmt`:

```text
pub def format[W](self, output: &mut Formatter[W]) -> Result[(), FormatError]:
    pass
```

The writer parameter belongs to the method. A single implementation can render
to a string, stderr, or a caller-owned writer, and can call all the scalar and
format-spec methods described in [`std.fmt`](fmt.md). For example,
`output.float_spec(value, &std.fmt.spec(precision=2, kind="f"))` is available
inside a report. Scratch for advanced numbers can allocate; this does not alter
the allocation-free text/integer/boolean stderr path.

`render[T: Formattable, W](&T, &mut Formatter[W])` borrows the value, enters one
depth level, calls `format`, and restores depth even on error. Depth is in
`0..=64`; zero permits direct scalar writes but rejects `render`. Nested values
should use `render` to participate in the depth limit. Direct user-method calls
do not count depth automatically.

An [anonymous sum](../language/03-types/anonymous-sums.md) satisfies
`Formattable` when every normalized member satisfies it. Dispatch borrows the
active member and calls its writer-generic `format` method without moving the
payload or allocating a wrapper. The sum adds no nesting level beyond the one
entered by `render`. Aliases, reordered members and singleton normalization
preserve this behavior. Direct `sum.format(&mut output)` uses the same dispatch
without entering a depth level. A missing, private, or mismatched method on any
member fails the bound, even when that member is inactive. This forwarding is
specific to `std.fmt.Formattable`; other protocols, including display/debug,
are not automatically forwarded.

## Helpers and destinations

| Operation | Behavior |
| --- | --- |
| `std.report.render(&value, &mut output)` | Borrowed report into any `Formatter[W]` |
| `std.report.to_string(&value, max_bytes: NonNegative[i64], max_depth: NonNegative[i64])` | Create a string formatter, render, and finish |
| `std.fmt.formatter(writer, scratch, max_bytes, max_depth)` | Transfer a writer and caller-provided scratch into a formatter |
| `std.fmt.string_formatter(max_bytes, max_depth)` | Create an allocating `Formatter[Cursor]` |
| `std.fmt.stderr_formatter(max_bytes, max_depth)` | Create an allocation-free `Formatter[Stderr]` |
| `std.fmt.finish(&output)` | Copy a string formatter into an owned `str`; takes `&Formatter[Cursor]` |

The formatter does not add punctuation, flush stdout, close a stream, or lock a
whole record. Each write counts toward its byte budget. `completed()` records
the accepted prefix. `has_failed()` records terminal stderr failure; subsequent
emissions on that formatter return `FormatError.OutputClosed` without writing. Other writer
destinations retain their own recovery policy and do not set this flag.
Formatting limits and allocation failures leave the formatter usable. See
[errors and partial output](fmt.md#errors-and-partial-output) for shared errors
and retry rules. In particular, finishing stderr is rejected by its type.

For allocation-free stderr reports, use `text`, `integer`, `unsigned`, and
`boolean`. Text writes use the native descriptor-2 operation directly, retry
interruptions, and count successful short writes. A closed or read-only stderr
returns `Output(StreamError.Io(IoError.System(9)))` (`EBADF`). On supported
x86-64 Linux the write suppresses SIGPIPE for that operation, so a closed pipe
returns an error and normal cleanup runs. Advanced formatting lazily acquires
scratch and can fail with `Allocation(...)`.

The string formatter stages a complete UTF-8 text emission before calling its
cursor. Failed staging or cursor growth preserves the valid UTF-8 output and
byte count. `finish` borrows the formatter, so an allocation failure can be
retried. A whole record can still have a successful prefix before a later
emission fails; do not replay it blindly.

## Source-aware diagnostics

`Diagnostic` is a structural protocol requiring public shared-receiver
`format[W](self, output: &mut Formatter[W]) -> Result[(), FormatError]` and
`source_offset(self) -> Option[i64]` methods. It adds source-location capability
to the ordinary formatting contract. `Some(offset)` is a zero-based byte offset
in the original input; `Nothing` means no input location is known.

`diagnostic[T: Diagnostic](value: &T, source: &[u8],
max_bytes: NonNegative[i64] = 4096, max_context: NonNegative[i64] = 80)` returns
`Result[str, FormatError]`. It renders the borrowed value with a depth budget of
16, then appends a bounded source excerpt only when a location is available.
`max_context` bounds the local source-byte window; zero omits the excerpt.
`max_bytes` bounds the complete report, with the usual `LimitExceeded` and
`Allocation` failures. A known offset outside the supplied input traps with
`std.report.diagnostic` in the message. The helper never retains the source,
flushes a stream, or consumes the error.

JSON/TOML and their typed serial errors support this protocol. See
[serialization diagnostics](serial-diagnostics.md) for excerpt escaping,
Unicode/EOF handling, path display, privacy and absent source-span limits.
Ordinary `to_string` and entrypoint reports include no source excerpts.

## Current boundaries

This protocol selects human-readable
[entrypoint reports](../toolchain/01-building/entry-report.md); types without it
retain structural rendering. `std.error.Context[E]` supports the protocol when
its cause does. `display`/`debug` remain the value presentation hooks; sharing
the formatter does not change which hook an entrypoint chooses. The
[tutorial](../../tutorial/08-resources/report-formatting.md) shows enum and sum
reporting.

The [completed reporting proposal](../../proposals/completed/error-formatting.md)
records the original reporting design and its later unification.
