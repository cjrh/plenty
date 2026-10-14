# Formatting into a reusable writer

Use `std.fmt` when output must go to a string or a chosen byte writer. Numeric
widths count bytes and include the sign. Every call is fallible, and the byte
budget includes punctuation and padding.

```plenty
import std.fmt

def main() -> Result[(), Failure]:
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

def main() -> Result[(), Failure]:
    label = Label("ready")
    print(std.fmt.to_string(&label, 80, 8)?)?
    print(std.fmt.to_debug(&label, 80, 8)?)?
    Ok(())
```

```output
ready
Label("ready")
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
