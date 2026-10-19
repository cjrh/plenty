# Formatting a bounded report

Use `std.report` to give a value a human-readable spelling through a concrete
formatter. The method borrows its receiver, so an error can be formatted again
or examined afterward.

```plenty
import std.report
from std.report import Formatter, FormatError

pub enum ReadingError:
    Invalid(i64)

    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        match self:
            case ReadingError.Invalid(line):
                output.text("invalid reading on line ")?
                output.integer(*line)

def main() -> Result[(), FormatError | IoError]:
    error = ReadingError.Invalid(12)
    print(std.report.to_string(&error, 80, 8)?)?
    print(std.report.to_string(&error, 80, 8)?)?
    Ok(())
```

```output
invalid reading on line 12
invalid reading on line 12
```

The public enum and its public `format` method satisfy the structural
`Formattable` protocol. A class uses the same visibility and method signature.
`std.report.render(&value, &mut output)` counts a nesting level and restores it
on success or error. Use it when a formatting method delegates to another value.

An anonymous sum supports the same reporting API when every member satisfies
`std.report.Formattable`. You can keep distinct error types and let the compiler
select the active member's method:

```plenty
import std.report
from std.report import Formatter, FormatError

pub enum ReadingError:
    Invalid(i64)

    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        match self:
            case ReadingError.Invalid(line):
                output.text("invalid reading on line ")?
                output.integer(*line)

pub enum ConnectionError:
    Closed

    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text("connection closed")

def show(error: &(ReadingError | ConnectionError)) -> Result[(), FormatError | IoError]:
    print(std.report.to_string(error, 80, 1)?)?
    Ok(())

def main() -> Result[(), FormatError | IoError]:
    reading: ReadingError | ConnectionError = ReadingError.Invalid(12)
    connection: ReadingError | ConnectionError = ConnectionError.Closed
    show(&reading)?
    show(&connection)?
    show(&reading)?
    Ok(())
```

```output
invalid reading on line 12
connection closed
invalid reading on line 12
```

Dispatch borrows the payload without allocating; string collection still
allocates. The original sum remains usable afterward. `render` counts one depth
level for the sum, with no extra level for selecting its member. Calling
`error.format(&mut output)` directly also dispatches, but does not count depth.
All members must have the required public method, even if the value currently
holds a different member. Other protocols are not automatically forwarded.

For stderr, construct `std.report.stderr_formatter(max_bytes, max_depth)` and
call `render` explicitly. Construction and scalar writes allocate no heap
memory, so simple report methods remain usable when allocation is unavailable.
Add any desired `"error: "` prefix and final `"\n"` with `output.text` yourself;
they count toward the byte limit. User methods can still allocate, so this
guarantee depends on what those methods do.

The collector has the same formatting contract but allocates fallibly:

```plenty
import std.report
from std.report import FormatError

def main() -> Result[(), FormatError | IoError]:
    mut output = std.report.string_formatter(4, 0)?
    output.text("a")?
    match output.text("é🙂"):
        case Err(_):
            print("too long", output.completed())?
        case Ok(_):
            pass
    output.text("é")?
    print(std.report.finish(&output)?)?
    Ok(())
```

```output
too long 1
aé
```

Text checks its whole UTF-8 byte length before writing. Numeric output streams
digits, so a limit can leave part of a number. On stderr failure, inspect
`completed()` and `failed()`; subsequent writes return `OutputClosed` without
writing again. A limit error is recoverable, as the example shows.

When `main` returns `Err`, its concrete error type selects the reporting path.
A type with this public `format` method produces a human-readable report with
the prefix `error: main returned ` and a final newline. A sum selects this path
only when all its members support it; other types retain structural reporting.
The automatic reporter allows 16,384 bytes and 64 nesting levels. If formatting
fails after writing part of the message, it appends ` [report unavailable]` and
a newline without trying the method again. Stderr failures stop output.

Use [`std.fmt`](formatting.md) for floats, widths,
debug escaping or an arbitrary byte writer. The
[reference](../../design/stdlib/report.md) lists the complete reporting API.
