//! `std.markdown`: CommonMark block and inline parsing, the event stream,
//! HTML rendering, limits and positioned errors, writer retries, and inputs
//! that make naive parsers quadratic (issue #470). Expectations follow the
//! CommonMark 0.31.2 specification; the cases are written for these tests.
mod support;
use support::assert_visible_stdout;

/// Tight items omit `Paragraph`, adjacent text merges, and `Done` repeats.
#[test]
fn event_stream_reports_structure_and_resolved_text() {
    assert_visible_stdout(
        r##"import std.markdown
from std.markdown import Event, MarkdownError

def main() -> Result[(), AllocError | IoError | MarkdownError]:
    bounds = std.markdown.limits()
    mut events = std.markdown.parser_text("# Title\n\n- one *two*\n- [three](/x \"T\")\n\n```py\ncode\n```\n<div>\nraw\n</div>\n", &bounds)?
    while True:
        event = events.next()?
        print(str.repr(&event)?)?
        match event:
            case Event.Done:
                break
            case _:
                pass
    print(str.repr(&events.next()?)?)?
    Ok(())
"##,
        r##"std.markdown.Event.Start(std.markdown.Tag.Heading(1))
std.markdown.Event.Text("Title")
std.markdown.Event.End(std.markdown.Tag.Heading(1))
std.markdown.Event.Start(std.markdown.Tag.BulletList)
std.markdown.Event.Start(std.markdown.Tag.Item)
std.markdown.Event.Text("one ")
std.markdown.Event.Start(std.markdown.Tag.Emphasis)
std.markdown.Event.Text("two")
std.markdown.Event.End(std.markdown.Tag.Emphasis)
std.markdown.Event.End(std.markdown.Tag.Item)
std.markdown.Event.Start(std.markdown.Tag.Item)
std.markdown.Event.Start(std.markdown.Tag.Link("/x", "T"))
std.markdown.Event.Text("three")
std.markdown.Event.End(std.markdown.Tag.Link("/x", "T"))
std.markdown.Event.End(std.markdown.Tag.Item)
std.markdown.Event.End(std.markdown.Tag.BulletList)
std.markdown.Event.Start(std.markdown.Tag.CodeBlock("py"))
std.markdown.Event.Text("code\n")
std.markdown.Event.End(std.markdown.Tag.CodeBlock("py"))
std.markdown.Event.Html("<div>\nraw\n</div>")
std.markdown.Event.Done
std.markdown.Event.Done
"##,
    );
}

/// One case per construct: blocks, lists, code, emphasis, references,
/// autolinks, entities, raw HTML, escapes, and line endings.
#[test]
fn html_matches_commonmark_rendering() {
    assert_visible_stdout(
        r##"import std.markdown
from std.markdown import MarkdownError

def show(text: &str) -> Result[(), AllocError | IoError | MarkdownError]:
    bounds = std.markdown.limits()
    html = std.markdown.to_html(text, &bounds)?
    print(str.repr(&html)?)?
    Ok(())

def main() -> Result[(), AllocError | IoError | MarkdownError]:
    show("Setext\n===\n\nText with  \nhard break and\nsoft one.")?
    show("> quoted\nlazy line\n\n---")?
    show("1. a\n2. b\n\n3) c")?
    show("- tight\n- list\n\n<!-- -->\n\n- loose\n\n- list")?
    show("    indented\n      code\n\n\n")?
    show("~~~ rust extra\nfn x() {}\n~~~")?
    show("**strong** and *emph* and ***both*** and _under_score_")?
    show("`code ``span`` here` and ` `")?
    show("[ref] and [Ref][] and ![alt *text*][ref]\n\n[REF]: /url%20x \"Title\"")?
    show("<https://example.com/a?b=c&d> <me@example.com>")?
    show("&copy; &#169; &#xA9; &bogus; &#0;")?
    show("a <span class=\"x\">b</span> \\*c\\*")?
    show("[link](</a b> 'single') [é](/café)")?
    show("Tab\tin\ttext\r\nCRLF line\rCR line")?
    show("")?
    Ok(())
"##,
        r##""<h1>Setext</h1>\n<p>Text with<br />\nhard break and\nsoft one.</p>\n"
"<blockquote>\n<p>quoted\nlazy line</p>\n</blockquote>\n<hr />\n"
"<ol>\n<li>a</li>\n<li>b</li>\n</ol>\n<ol start=\"3\">\n<li>c</li>\n</ol>\n"
"<ul>\n<li>tight</li>\n<li>list</li>\n</ul>\n<!-- -->\n<ul>\n<li>\n<p>loose</p>\n</li>\n<li>\n<p>list</p>\n</li>\n</ul>\n"
"<pre><code>indented\n  code\n</code></pre>\n"
"<pre><code class=\"language-rust\">fn x() {}\n</code></pre>\n"
"<p><strong>strong</strong> and <em>emph</em> and <em><strong>both</strong></em> and <em>under_score</em></p>\n"
"<p><code>code ``span`` here</code> and <code> </code></p>\n"
"<p><a href=\"/url%20x\" title=\"Title\">ref</a> and <a href=\"/url%20x\" title=\"Title\">Ref</a> and <img src=\"/url%20x\" alt=\"alt text\" title=\"Title\" /></p>\n"
"<p><a href=\"https://example.com/a?b=c&amp;d\">https://example.com/a?b=c&amp;d</a> <a href=\"mailto:me@example.com\">me@example.com</a></p>\n"
"<p>© © © &amp;bogus; �</p>\n"
"<p>a <span class=\"x\">b</span> *c*</p>\n"
"<p><a href=\"/a%20b\" title=\"single\">link</a> <a href=\"/caf%C3%A9\">é</a></p>\n"
"<p>Tab\tin\ttext\nCRLF line\nCR line</p>\n"
""
"##,
    );
}

/// Input, depth, and output limits, invalid UTF-8, `NUL`, and bad limits.
#[test]
fn limits_and_errors_carry_positions() {
    assert_visible_stdout(
        r##"import std.bytes
import std.markdown
from std.bytes import Bytes
from std.markdown import Limits, MarkdownError

def outcome(result: Result[str, MarkdownError]) -> Result[str, AllocError]:
    match result:
        case Ok(html):
            str.repr(&html)
        case Err(error):
            Ok(" ".join(&[str.repr(&error.fault)?, str.from(error.offset)?, str.from(error.line)?]?)?)

def rendered(source: &Bytes, bounds: &Limits) -> Result[str, MarkdownError]:
    html = std.markdown.render(source, bounds)?
    match html.to_text():
        case Ok(text):
            Ok(text)
        case Err(_):
            Ok("")

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    bounds = std.markdown.limits()
    print(bounds.max_input, bounds.max_depth, bounds.max_output)?
    bad = std.bytes.from_bytes(b"ok\n\xff")?
    print(outcome(rendered(&bad, &bounds))?)?
    nul = std.bytes.from_bytes(b"a\x00b")?
    print(outcome(rendered(&nul, &bounds))?)?
    mut small = std.markdown.limits()
    small.max_input = 3
    print(outcome(std.markdown.to_html("abcd", &small))?)?
    small.max_input = 4
    print(outcome(std.markdown.to_html("abcd", &small))?)?
    mut shallow = std.markdown.limits()
    shallow.max_depth = 2
    print(outcome(std.markdown.to_html("> > a", &shallow))?)?
    print(outcome(std.markdown.to_html("x\r\n\r\n- > - b", &shallow))?)?
    mut short = std.markdown.limits()
    short.max_output = 18
    print(outcome(std.markdown.to_html("*a*", &short))?)?
    short.max_output = 17
    print(outcome(std.markdown.to_html("*a*", &short))?)?
    mut broken = std.markdown.limits()
    broken.max_depth = -1
    print(outcome(std.markdown.to_html("a", &broken))?)?
    Ok(())
"##,
        r##"67108864 64 67108864
std.markdown.MarkdownFault.InvalidUtf8 3 0
"<p>a�b</p>\n"
std.markdown.MarkdownFault.InputTooLarge 3 0
"<p>abcd</p>\n"
"<blockquote>\n<blockquote>\n<p>a</p>\n</blockquote>\n</blockquote>\n"
std.markdown.MarkdownFault.DepthExceeded 5 3
"<p><em>a</em></p>\n"
std.markdown.MarkdownFault.OutputTooLarge 17 0
std.markdown.MarkdownFault.InvalidLimits 0 0
"##,
    );
}

/// `write_html` to a writer, and `write_pending` after a writer refuses.
#[test]
fn writers_receive_html_and_retries_do_not_repeat() {
    assert_visible_stdout(
        r##"import std.bytes
import std.markdown
from std.bytes import ByteSource, Bytes
from std.io import StreamError
from std.markdown import Event, MarkdownError

# Accepts one byte per call and fails once `room` bytes have arrived.
pub class Sip:
    pub data: Bytes
    pub room: i64

    pub def write[S: ByteSource](self: &mut Sip, source: &S, start: i64, stop: i64) -> Result[i64, StreamError]:
        if start == stop:
            return Ok(0)
        if self.data.len() >= self.room:
            return Err(StreamError.Io(IoError.NotWritable))
        match self.data.extend_range(source, start, start + 1):
            case Ok(_):
                Ok(1)
            case Err(_):
                Err(StreamError.OutOfRange)

    pub def flush(self: &mut Sip) -> Result[(), StreamError]:
        Ok(())

def settled(result: Result[(), MarkdownError]) -> Result[str, AllocError]:
    match result:
        case Ok(_):
            Ok("ok")
        case Err(error):
            Ok(" ".join(&[str.repr(&error.fault)?, str.from(error.offset)?]?)?)

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | MarkdownError]:
    bounds = std.markdown.limits()
    mut events = std.markdown.parser_text("# Hi\n\nThere", &bounds)?
    mut whole = Sip(std.bytes.empty()?, 1000)
    print(settled(std.markdown.write_html(&mut events, &mut whole, &bounds))?, str.repr(&whole.data.to_text()?)?)?

    mut renderer = std.markdown.renderer(&bounds)?
    mut source = std.markdown.parser_text("*a*", &bounds)?
    while True:
        event = source.next()?
        match event:
            case Event.Done:
                break
            case _:
                renderer.event(&event)?
    print(renderer.pending(), renderer.written())?
    mut full = Sip(std.bytes.empty()?, 5)
    print(settled(std.markdown.write_pending(&mut renderer, &mut full))?, full.data.to_text()?, renderer.pending())?
    # A retry delivers only what the first writer refused.
    mut rest = Sip(std.bytes.empty()?, 100)
    print(settled(std.markdown.write_pending(&mut renderer, &mut rest))?, str.repr(&rest.data.to_text()?)?, renderer.pending(), renderer.written())?
    renderer.event(&Event.Rule)?
    taken = renderer.take()?
    print(str.repr(&taken.to_text()?)?, renderer.pending(), renderer.written())?
    Ok(())
"##,
        r##"ok "<h1>Hi</h1>\n<p>There</p>\n"
18 18
std.markdown.MarkdownFault.Stream(std.io.StreamError.Io(IoError.NotWritable)) 18 <p><e 13
ok "m>a</em></p>\n" 0 18
"<hr />\n" 0 25
"##,
    );
}

/// Inputs that are quadratic in naive parsers: unclosed links, brackets,
/// HTML comments and declarations, and emphasis between links. Each input is
/// 20,000 repetitions; a quadratic path would take minutes.
#[test]
fn pathological_inputs_finish() {
    assert_visible_stdout(
        r##"import std.bytes
import std.markdown
from std.bytes import Bytes
from std.markdown import MarkdownError

def repeated(unit: &str, count: i64) -> Result[Bytes, std.bytes.BytesError]:
    piece = std.bytes.from_text(unit)?
    mut out = std.bytes.with_capacity(piece.len() * count)?
    for _ in range(count):
        out.extend(&piece)?
    Ok(out)

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | MarkdownError]:
    bounds = std.markdown.limits()
    units = ["[a](", "*a [c](d) ", "[" , "a]", "`", "<!--", "<?", "<!A ", "<![CDATA[", "_a *", "[a\\]", "<a:", "&#"]?
    for unit in units:
        source = repeated(&unit, 20000)?
        html = std.markdown.render(&source, &bounds)?
        print(html.len())?
    Ok(())
"##,
        r##"80008
420007
20008
40008
25
80001
40001
80001
180001
80008
60008
120008
120008
"##,
    );
}
