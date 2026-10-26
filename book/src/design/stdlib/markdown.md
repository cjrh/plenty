# std.markdown

`import std.markdown` parses [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
into a stream of events and renders events as HTML, under explicit limits and
with one error type, `MarkdownError`. It has two levels:

- **Events**: a `Parser` yields `Start`, `End`, text, and break events in
  document order. Programs can inspect, filter, or rewrite them.
- **HTML**: an `HtmlRenderer` turns events into HTML text. `to_html`, `render`,
  and `write_html` connect a parser to a renderer.

The module is checked Plenty code with no native primitives of its own. It
follows the algorithms of the reference implementation, commonmark.js, and
renders the HTML that the specification's examples show. It reads
[`std.bytes`](bytes.md) buffers, writes [`std.io`](io.md) streams, and is
available on the supported x86-64 Linux GNU target.

## Accepted text

| Rule | Behavior |
|---|---|
| Syntax | CommonMark 0.31.2: every block and inline construct of the specification, and nothing more |
| Extensions | None: no tables, task lists, strikethrough, footnotes, extended autolinks, or front matter |
| Encoding | UTF-8, validated strictly; invalid input is `InvalidUtf8` at the first bad byte |
| Line endings | `\n`, `\r\n`, and `\r` all end a line |
| `NUL` | Replaced by U+FFFD, as the specification requires |
| Raw HTML | HTML blocks and inline tags pass through unchanged as `Html` and `InlineHtml` events |
| Unicode | Punctuation and whitespace classes for emphasis come from Unicode 16.0.0; link labels match after Unicode case folding by lowering then raising |
| Entities | All HTML5 named character references, and decimal and hexadecimal references |

Raw HTML is not sanitized. For untrusted input, drop or escape `Html` and
`InlineHtml` events before rendering, and check link and image destinations,
which may use any scheme, including `javascript:`.

## Limits

```text
pub class Limits:
    pub max_input: i64
    pub max_depth: i64
    pub max_output: i64
```

`limits()` returns `Limits(67108864, 64, 67108864)`. The fields are public;
adjust a copy. Every constructor and whole-document function takes `&Limits`
and returns `InvalidLimits` when a field is negative.

| Field | Bounds | When exceeded |
|---|---|---|
| `max_input` | Bytes of Markdown given to one parser | `InputTooLarge`; the offset is the limit |
| `max_depth` | Block quotes and list items open at once. `0` allows neither | `DepthExceeded` on the line that would exceed it |
| `max_output` | Total bytes one renderer produces, including bytes already taken or written | `OutputTooLarge` |

Inline nesting, such as emphasis inside links inside emphasis, is not limited
by `max_depth`: the parser and renderer walk it without recursion. Running time
is linear in the input, including on the inputs that make naive parsers
quadratic, such as long runs of unclosed brackets, backticks, or HTML comments.
Link destinations may nest parentheses 32 deep, as in cmark.

## Errors

```text
pub enum MarkdownFault:
    InvalidUtf8
    InputTooLarge
    OutputTooLarge
    DepthExceeded
    InvalidLimits
    InvalidState
    Stream(StreamError)
    Allocation(AllocError)

pub class MarkdownError:
    pub fault: MarkdownFault
    pub offset: i64
    pub line: i64
```

| Fault | `offset` | `line` |
|---|---|---|
| `InvalidUtf8` | The first invalid input byte | 0 |
| `InputTooLarge` | `max_input` | 0 |
| `DepthExceeded` | The start of the offending line in the input | That line, from 1 |
| `OutputTooLarge`, `Stream` | Bytes the renderer had produced | 0 |
| Others | 0, or bytes produced for a renderer | 0 |

After any error, a `Parser` or `HtmlRenderer` returns `InvalidState` from
every later call.

## Events

```text
pub enum Tag:
    Paragraph
    Heading(i64)              # level 1 to 6
    BlockQuote
    CodeBlock(str)            # info string; empty for indented code
    BulletList
    OrderedList(i64)          # start number
    Item
    Emphasis
    Strong
    Link(str, str)            # destination, title
    Image(str, str)           # destination, title

pub enum Event:
    Start(Tag)
    End(Tag)
    Text(str)
    Code(str)                 # inline code span
    Html(str)                 # raw HTML block
    InlineHtml(str)           # raw inline tag, comment, or declaration
    SoftBreak
    HardBreak
    Rule
    Done
```

```text
pub def parser(source: &Bytes, bounds: &Limits) -> Result[Parser, MarkdownError]
pub def parser_text(text: &str, bounds: &Limits) -> Result[Parser, MarkdownError]
Parser.next(self: &mut Parser) -> Result[Event, MarkdownError]
```

`parser` validates the input and parses its block structure: paragraphs,
headings, lists, quotes, code, and link reference definitions. It copies what
it needs, so `source` stays free. `next` parses inline content one paragraph
or heading at a time, so a parser holds the block tree and one leaf's inline
events, not the whole event stream.

Every `Start` has a matching `End` with an equal tag. Text, entities, and
backslash escapes are resolved: `Text` holds the characters to show, and
adjacent text arrives as one event. A code block's content, including its final
line ending, is one `Text` event between `Start(CodeBlock)` and
`End(CodeBlock)`; an empty block has none. `Html` holds a raw block without its
final line ending.

Paragraphs directly inside items of a tight list produce no `Paragraph` events,
so a renderer need not track tightness. Link reference definitions produce no
events; references to them arrive as `Link` and `Image` with the definition's
destination and title. Inside `Image`, the events between `Start` and `End`
are its description; HTML renders them as plain text in `alt`.

After the last event, `next` returns `Done`, and keeps returning it.

## HTML

```text
pub def renderer(bounds: &Limits) -> Result[HtmlRenderer, MarkdownError]
HtmlRenderer.event(self: &mut HtmlRenderer, event: &Event) -> Result[(), MarkdownError]
HtmlRenderer.pending(self) -> i64
HtmlRenderer.written(self) -> i64
HtmlRenderer.take(self: &mut HtmlRenderer) -> Result[Bytes, MarkdownError]
pub def write_pending[W: Writer](source: &mut HtmlRenderer, writer: &mut W) -> Result[(), MarkdownError]
pub def write_html[W: Writer](source: &mut Parser, writer: &mut W, bounds: &Limits) -> Result[(), MarkdownError]
pub def render(source: &Bytes, bounds: &Limits) -> Result[Bytes, MarkdownError]
pub def to_html(text: &str, bounds: &Limits) -> Result[str, MarkdownError]
```

A renderer appends the HTML for each event to an owned buffer. `pending` counts
bytes not yet taken; `written` counts every byte produced. `take` returns the
buffer and leaves an empty one. `write_pending` writes the buffer to a writer;
on failure only the bytes the writer refused stay pending, so a retry cannot
repeat output. `write_html` renders every remaining event of a parser, writing
whenever 64 KiB is pending; neither function flushes.

The output matches the specification's examples:

| Event | HTML |
|---|---|
| `Paragraph`, `Heading(n)` | `<p>…</p>`, `<hn>…</hn>` |
| `BlockQuote` | `<blockquote>` and `</blockquote>` on their own lines |
| `BulletList`, `OrderedList(n)` | `<ul>`, `<ol>`, or `<ol start="n">` when `n` is not 1 |
| `Item` | `<li>…</li>` |
| `CodeBlock(info)` | `<pre><code class="language-w">…</code></pre>`, where `w` is the first word of `info`; no `class` when `info` is empty |
| `Emphasis`, `Strong` | `<em>`, `<strong>` |
| `Link(d, t)` | `<a href="d" title="t">`; no `title` when `t` is empty |
| `Image(d, t)` | `<img src="d" alt="…" title="t" />` |
| `Code(s)` | `<code>s</code>` |
| `Html(s)`, `InlineHtml(s)` | `s`, unescaped |
| `SoftBreak`, `HardBreak`, `Rule` | a line ending, `<br />`, `<hr />` |

Text and attributes escape `&`, `<`, `>`, and `"`. Destinations in `href` and
`src` are percent-encoded except for ASCII letters, digits, and
``!#$%()*+,-./:;=?@_~^``, with `&` written as `&amp;` and `'` as `&#x27;`.
Each block starts on its own line, and the output ends with a line ending
unless it is empty.

## Example

```plenty
import std.markdown
from std.markdown import Event, MarkdownError, Tag

def main() -> Result[(), AllocError | IoError | MarkdownError]:
    bounds = std.markdown.limits()
    text = "# Notes\n\nSee [the guide](/guide) and *more*.\n"
    print(std.markdown.to_html(text, &bounds)?)?
    mut events = std.markdown.parser_text(text, &bounds)?
    while True:
        event = events.next()?
        match event:
            case Event.Start(tag):
                match tag:
                    case Tag.Link(destination, _):
                        print("link to", destination)?
                    case _:
                        pass
            case Event.Done:
                break
            case _:
                pass
    Ok(())
```
```output
<h1>Notes</h1>
<p>See <a href="/guide">the guide</a> and <em>more</em>.</p>

link to /guide
```

See the runnable [Markdown lesson](../../tutorial/07-modules/21-standard-markdown.md).

## Implementation notes

The Unicode punctuation ranges and the HTML5 entity table are generated into
the module by `scripts/gen_markdown_tables.py` from Python's `unicodedata` and
`html.entities`; rerun it to move to a newer Unicode version.
