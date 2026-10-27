# Process XML events

`std.xml` reads one bounded event at a time from a
[byte stream](26-byte-streams.md). Event text has already been checked as UTF-8
and entity references have been decoded. Match the events your application
needs, and keep reading through `Eof` to check the complete document:

```plenty
import std.bytes
import std.io
import std.xml
from std.xml import Event

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.xml.XmlError]:
    source = "<items><item>Tea &amp; cake</item><item>Milk</item></items>"
    mut input = std.xml.reader(std.io.cursor(std.bytes.from_text(source)?), &std.xml.limits())?
    mut output = std.xml.writer(std.io.cursor(std.bytes.empty()?), &std.xml.limits())?
    mut done = False
    while not done:
        event = input.next()?
        match &event:
            case Event.Text(text):
                print(*text)?
            case Event.Eof:
                done = True
            case _:
                pass
        output.write(&event)?
    print(output.inner.data.to_text()?)?
    Ok(())
```
```output
Tea & cake
Milk
<items><item>Tea &amp; cake</item><item>Milk</item></items>
```

The writer escapes values and checks the element structure. Passing `Eof`
checks that the root has closed and flushes its stream. When constructing
events yourself, call `finish()` after the final element. Events own their
strings, so retaining an event also retains its memory.

Choose limits for the input you accept. `max_token` bounds a complete tag,
comment, or text run; `max_depth` bounds nesting. The reference describes
[all limits and errors](../../design/stdlib/xml.md#limits). This example lets
errors propagate so malformed input and stream failures remain visible.
A failed file writer may have emitted partial output; applications that need
replacement only on success should manage a temporary destination.

This reader accepts UTF-8 XML 1.0 without a DTD. It never loads external
entities. Namespace prefixes remain literal names, so applications needing
namespace URI resolution must supply that policy themselves.
