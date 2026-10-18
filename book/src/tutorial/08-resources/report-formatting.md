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

This explicit formatting API does not yet change the report produced when
`main` returns an error. Use [`std.fmt`](formatting.md) for floats, widths,
debug escaping or an arbitrary byte writer. The
[reference](../../design/stdlib/report.md) lists the complete reporting API.
