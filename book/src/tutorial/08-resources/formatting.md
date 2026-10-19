# Formatting into a reusable writer

An [f-string](../02-values/format-strings.md) is the short way to build a
`str`; it compiles to the calls below. Use `std.fmt` directly when output must
go to a chosen byte writer, reuse storage, or stay within a byte budget. Numeric
widths count bytes and include the sign. Every call is fallible, and the byte
budget includes punctuation and padding.

```plenty
import std.fmt

def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:
    mut out = std.fmt.string_formatter(80, 8)?
    out.text("count=")?
    out.integer(12, 4)?
    out.text(", label=")?
    out.debug_text("one\ntwo")?
    print(std.fmt.finish(&out)?)?
    Ok(())
```

```output
count=  12, label="one\ntwo"
```

Your type chooses separate display and debug methods. Both can be generic over
the output writer; callers need no trait object or format-string parser.

```plenty
import std.fmt
from std.fmt import Formatter, FormatError

pub class Label:
    name: str

    pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.text(&self.name)

    pub def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.text("Label(")?
        out.debug_text(&self.name)?
        out.text(")")

def main() -> Result[(), AllocError | IoError | FormatError]:
    label = Label("ready")
    print(std.fmt.to_string(&label, 80, 8)?)?
    print(std.fmt.to_debug(&label, 80, 8)?)?
    Ok(())
```

```output
ready
Label("ready")
```

Enums use the same methods. Match the shared receiver to format a payload
without consuming it:

```plenty
import std.fmt
from std.fmt import Formatter, FormatError

pub enum ReadError:
    Missing
    Invalid(i64)

    pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        match self:
            case ReadError.Missing:
                out.text("no reading")
            case ReadError.Invalid(line):
                out.text("invalid reading on line ")?
                out.integer(*line, 0)

def main() -> Result[(), FormatError | IoError]:
    error = ReadError.Invalid(12)
    print(std.fmt.to_string(&error, 80, 8)?)?
    print(std.fmt.to_string(&error, 80, 8)?)?
    Ok(())
```

```output
invalid reading on line 12
invalid reading on line 12
```

`std.fmt` calls this method explicitly. Returning the enum from `main` still
uses the [structural entrypoint report](../../design/toolchain/01-building/entry-report.md).

Inside such a method, `out.format(f"...")` writes an f-string straight to the
formatter, with no intermediate `str`. It returns `Result[(), FormatError]`, and
the formatter's byte budget and depth limit apply:

```plenty
import std.fmt
from std.fmt import Formatter, FormatError

pub class Reading:
    line: i64
    value: f64

    pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"line {self.line}: {self.value:.1f}")

def main() -> Result[(), AllocError | IoError | FormatError]:
    reading = Reading(7, 21.25)
    print(f"[{reading}] [{reading:>14}]"?)?
    mut out = std.fmt.string_formatter(10, 8)?
    match out.format(f"{reading}"):
        case Err(FormatError.LimitExceeded):
            print("kept", std.fmt.finish(&out)?)?
        case _:
            print("fits")?
    Ok(())
```

```output
[line 7: 21.2] [  line 7: 21.2]
kept line 7: 21
```

To stream, construct `std.fmt.formatter(writer, scratch, max_bytes, max_depth)`
with a [`std.io` writer](26-byte-streams.md) and initialized `Bytes` scratch of at least
32 bytes. Provision scratch and sink capacity before entering an allocation-free
section. Formatting itself then reuses that memory; user methods remain
responsible for their own allocations. Flush the public `out.writer` explicitly
when needed. On failure, `out.completed()` tells you how many bytes were accepted;
do not blindly restart a partially written record.

The [reference](../../design/stdlib/fmt.md) describes aggregate callbacks, errors,
escaping, depth limits and exact costs.
