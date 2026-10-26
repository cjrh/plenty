# Render Markdown as HTML

`std.markdown` reads [CommonMark](https://commonmark.org), the common core of
Markdown, and writes HTML. Between the two it offers a stream of events, so a
program can inspect or change a document before it is rendered.

## Convert a document

`to_html` renders a whole document. Like other parsers in the standard
library, it takes limits on input size, nesting depth, and output size:

```plenty
import std.markdown
from std.markdown import MarkdownError

def main() -> Result[(), AllocError | IoError | MarkdownError]:
    bounds = std.markdown.limits()
    text = "# Shopping\n\n- *fresh* bread\n- [cheese](https://example.com/cheese)\n"
    print(std.markdown.to_html(text, &bounds)?)?
    Ok(())
```
```output
<h1>Shopping</h1>
<ul>
<li><em>fresh</em> bread</li>
<li><a href="https://example.com/cheese">cheese</a></li>
</ul>

```

The HTML ends with a line ending, so `print` leaves a blank line after it.

## Walk the events

A parser yields `Start` and `End` events around each block and inline element,
with `Text` and other events between them. This program prints the text of
every second-level heading:

```plenty
import std.markdown
from std.markdown import Event, MarkdownError, Tag

def main() -> Result[(), AllocError | IoError | MarkdownError]:
    bounds = std.markdown.limits()
    text = "# Plenty\n\n## Install\n\nRun `plenty FILE`.\n\n## Learn\n\nRead the *tutorial*.\n"
    mut events = std.markdown.parser_text(text, &bounds)?
    mut in_section = False
    while True:
        event = events.next()?
        match event:
            case Event.Start(tag):
                match tag:
                    case Tag.Heading(level):
                        in_section = level == 2
                    case _:
                        pass
            case Event.End(_):
                in_section = False
            case Event.Text(words):
                if in_section:
                    print("-", words)?
            case Event.Done:
                break
            case _:
                pass
    Ok(())
```
```output
- Install
- Learn
```

`Text` holds the characters to show, with escapes and entities such as `&amp;`
already resolved. The last event is `Done`.

## Change events before rendering

Markdown lets raw HTML through unchanged. When the text comes from someone you
do not trust, render the events yourself and turn raw HTML into text, which the
renderer escapes:

```plenty
import std.bytes
import std.markdown
from std.markdown import Event, MarkdownError

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | MarkdownError]:
    bounds = std.markdown.limits()
    text = "Hi <b onclick=\"steal()\">there</b>!\n\n<script>steal()</script>\n"
    mut events = std.markdown.parser_text(text, &bounds)?
    mut html = std.markdown.renderer(&bounds)?
    while True:
        event = events.next()?
        match event:
            case Event.Html(raw):
                html.event(&Event.Text(raw))?
            case Event.InlineHtml(raw):
                html.event(&Event.Text(raw))?
            case Event.Done:
                break
            case _ as other:
                html.event(&other)?
    print(html.take()?.to_text()?)?
    Ok(())
```
```output
<p>Hi &lt;b onclick=&quot;steal()&quot;&gt;there&lt;/b&gt;!</p>
&lt;script&gt;steal()&lt;/script&gt;
```

Link destinations are not checked either: `[x](javascript:steal())` is a valid
link. A program that publishes untrusted Markdown should also inspect
`Tag.Link` and `Tag.Image` destinations.

`std.markdown.write_html` renders a parser's events straight to a writer, such
as a file, without holding the whole page. See the
[`std.markdown` reference](../../design/stdlib/markdown.md) for the events,
limits, errors, and exact HTML output.
