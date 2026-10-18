# `std.report`

`std.report` provides a small, bounded formatter for human-readable reports.
Its concrete `Formatter` has two destinations: stderr or an owned UTF-8
collector. `stderr_formatter(max_bytes, max_depth)` validates its arguments and
constructs the formatter without allocating. Text, integer and boolean output
to stderr also need no heap allocation. User methods are responsible for their
own allocations.

## Formatting contract

The structural `Formattable` protocol requires a public shared-receiver method
on a public class or enum, since the helper checks the bound in `std.report`:

```text
pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
    pass
```

`render[T: Formattable](&T, &mut Formatter)` borrows the value, enters one depth
level, calls `format`, and restores the depth even when the method returns an
error. Both classes and named enums can satisfy the protocol. The maximum depth
is an integer from 0 through 64; zero permits direct scalar writes but rejects
`render`. Nested values should use `render` to participate in the depth limit.
Direct calls to a user method do not count depth automatically.

## API

| Operation | Behavior |
| --- | --- |
| `stderr_formatter(max_bytes, max_depth)` | Fallibly constructs a stderr formatter |
| `string_formatter(max_bytes, max_depth)` | Fallibly constructs an allocating collector |
| `out.text(&str)` | Emits UTF-8 unchanged, including NUL; checks the whole text's byte budget first |
| `out.integer(i64)` | Decimal signed spelling, including `i64.MIN` |
| `out.unsigned(u64, base)` | Lowercase digits in base 2, 8, 10 or 16, without a prefix |
| `out.boolean(bool)` | `True` or `False` |
| `out.completed()` | Number of bytes accepted by the destination |
| `out.failed()` | Whether a terminal stderr output failure has occurred |
| `out.enter()` / `out.leave()` | Enter or leave one nesting level; leaving zero is harmless |
| `finish(&out)` | Fallibly copies a collector into owned `str`; stderr gives `InvalidSpec` |
| `to_string(&value, max_bytes, max_depth)` | Constructs a collector, renders, and finishes |

`max_bytes` must be nonnegative. All output, including application punctuation,
counts toward it. Integers stream their sign and digits separately: a budget
failure can leave a numeric prefix. No prefix or newline is added automatically.
The formatter does not flush stdout or lock a complete record. Separate text
and digit writes may interleave with output from other threads.

## Errors and partial output

`FormatError` is an ordinary enum:

- `InvalidSpec`: invalid construction arguments or base, or finishing stderr.
- `LimitExceeded`: an emission would exceed the remaining byte budget.
- `RecursionLimit`: entering would exceed the nesting budget.
- `Bytes(BytesError)`: collector allocation or conversion failure.
- `Output(IoError)`: terminal stderr write failure.
- `WriteZero`: stderr accepted zero bytes from a nonempty emission.
- `OutputClosed`: an emission attempted after a terminal output failure.

Stderr writes retry interruptions and account for each successful partial write.
On the supported x86-64 Linux target, the native write suppresses SIGPIPE for
that operation, so a closed pipe becomes
an error value and normal cleanup still runs. A failed transport is sticky:
later writes emit no bytes and return `OutputClosed`. Byte/depth limits and
collector allocation failures do not mark the transport failed.

A collector reserves space for each entire text emission before appending its
UTF-8 bytes. Failed growth preserves its valid UTF-8 prefix and byte count.
`finish` borrows the collector, so a failed string allocation can be retried.
Do not replay a record blindly after a partial write; inspect `completed()`.

## Current boundaries

This module is an explicit API. Entrypoint reports still use their existing
[structural rendering](../toolchain/01-building/entry-report.md); anonymous
sums do not automatically forward `Formattable`. Floating-point formatting,
width, escaping and arbitrary writer destinations remain in
[`std.fmt`](fmt.md). The [tutorial](../../tutorial/08-resources/report-formatting.md)
shows borrowed enum reporting.
