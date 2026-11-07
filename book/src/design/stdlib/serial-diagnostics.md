# Serialization diagnostics

JSON and TOML keep typed errors for programmatic handling. Their `format`
methods also support bounded human-readable reports through
[`std.report`](report.md). Rendering borrows the error, so a formatting limit
or allocation failure preserves the original cause and path.

## Descriptions and paths

`JsonError` and `TomlError` implement `std.fmt.Formattable`. Reports describe
the fault and include a byte offset and one-based Unicode-scalar line/column
when the operation has input coordinates. `SerialError[E]` implements the same
protocol when its cause does; it adds a root-first path such as
`$["servers"][2]["port"]`. Field/key names use quoted string escaping, indices
are zero-based, and enum payloads add `<"Variant">`. An incomplete collected
path ends with `(path incomplete)`; its available suffix is retained.

`std.serial.Kind` names null, boolean, integer, number, string, array, object,
date/time, enum-tag, object-key and end kinds. Built-in adapters report
`Fault.KindMismatch(expected, found)` without copying the rejected value.
JSON event numbers have kind `Number`; owning trees distinguish integer and
floating-point values. Integer range/number-spelling failures remain
`OutOfRange`. The payload-free `UnexpectedKind` remains available for custom
hooks that cannot supply more detail. An empty tagged enum object reports
`WrongLength(1, 0)`.

Descriptions omit rejected scalar contents, including unknown enum tag text.
Field/key names appear where they identify schema failures and paths; they
are still bounded by the formatter's total byte limit. Descriptions are useful
messages, not a byte-for-byte wording contract with another JSON/TOML library.

## Opt-in source excerpts

`JsonError` and `TomlError` also provide
`source_offset() -> Option[i64]`; `SerialError[E]` forwards this method when
its cause supports it. `Some(offset)` means an input location is known.
`Nothing` means no source coordinates are available. Tree conversion and TOML
schema decoding operate on owning trees without retained source spans, so
their errors have paths but no input location. Output byte counts are never
presented as input coordinates.

Use `std.report.to_string(&error, max_bytes, max_depth)` for a description
without input text. Explicitly opt into an excerpt with
`std.report.diagnostic(&error, source, max_bytes=4096, max_context=80)`.
The supplied bytes must be the original input for the failed operation. A known
offset outside that view is a caller bug and traps with the operation's name.
No source is retained by the error or report helper.

The helper borrows input, scans a bounded local context window, and shows
one local line fragment with a caret, including at EOF or on an empty final
line. `max_context` bounds the selected source-byte window around the location,
with fixed UTF-8/boundary lookahead of at most three adjacent bytes;
`0` suppresses the excerpt. Ordinary Unicode scalars stay readable, malformed
UTF-8 bytes become `\xNN`, and control/direction-changing characters are escaped.
Clipped fragments use `...`; UTF-8 scalars are never cut into partial display
sequences. Caret spacing counts rendered scalars and escapes, matching the
parser's scalar-column convention rather than terminal grapheme/cell widths.

The report's total UTF-8 output, including description, path, excerpt, escapes
and caret, stays within `max_bytes`; an insufficient budget returns
`FormatError.LimitExceeded`. Formatting uses a fixed nesting budget of 16.
Collection and finish allocation failures return `FormatError.Allocation`.
There is no input-sized copy or temporary path string. An opted-in excerpt
can contain sensitive source text within its explicit window; ordinary
reports never include source excerpts automatically.

```plenty
import std.json
import std.report
import std.fmt

def main() -> Result[(), IoError | std.fmt.FormatError]:
    source = b"[1,"
    match std.json.parse(&source, &std.json.limits()):
        case Ok(_):
            print("valid")?
        case Err(error):
            print(std.report.diagnostic(&error, &source)?)?
    Ok(())
```
```output
JSON: unexpected end of input at line 1, column 4 (byte 3)
  [1,
     ^
```
