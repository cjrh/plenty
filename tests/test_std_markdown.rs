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

/// Input, depth, and output limits, invalid UTF-8, and `NUL`.
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
"##,
    );
}

/// The fields are `NonNegative[i64]`, so a negative limit is a compile error
/// rather than the old `InvalidLimits` fault.
#[test]
fn negative_limits_are_compile_errors() {
    let error = support::check_file_error(
        r#"import std.markdown

def main() -> ():
    mut bounds = std.markdown.limits()
    bounds.max_depth = -1
"#,
    );
    assert!(error.contains("cannot be negative"), "{error}");
}

/// `write_html` to a writer, and `write_pending` after a writer refuses.
#[test]
fn writers_receive_html_and_retries_do_not_repeat() {
    assert_visible_stdout(
        r##"import std.bytes
import std.markdown
from std.bytes import Bytes
from std.io import StreamError
from std.markdown import Event, MarkdownError

# Accepts one byte per call and fails once `room` bytes have arrived.
pub class Sip:
    pub data: Bytes
    pub room: i64

    pub def write(self: &mut Sip, source: &[u8]) -> Result[i64, StreamError]:
        if len(source) == 0:
            return Ok(0)
        if self.data.len() >= self.room:
            return Err(StreamError.Io(IoError.NotWritable))
        match self.data.extend(&source[:1]):
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

def repeated(unit: &str, count: i64) -> Result[Bytes, AllocError | std.bytes.BytesError | OverflowError[i64]]:
    piece = std.bytes.from_text(unit)?
    mut out = std.bytes.with_capacity(piece.len() * count)?
    for _ in range(count):
        out.extend(&piece)?
    Ok(out)

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | MarkdownError | OverflowError[i64]]:
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

fn output_with_optimization(source: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    let options = plenty::CompileOptions {
        optimize,
        test_hooks: true,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&path, &executable, None, &options)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn source_item<'a>(library: &'a str, declaration: &str) -> &'a str {
    library[library.find(declaration).unwrap()..]
        .split("\n\n")
        .next()
        .unwrap()
}

#[test]
fn parser_columns_and_renderer_counters_preserve_state_at_representation_limits() {
    let library = include_str!("../stdlib/std/markdown.plenty");
    // Original production field layouts and selected production methods, not
    // replacement state machines. Large positions need no enormous buffers.
    let parser_fields = source_item(library, "pub class Parser:").replace("pub class", "class");
    let renderer_fields =
        source_item(library, "pub class HtmlRenderer:").replace("pub class", "class");
    let classes = ["Block", "Refs", "Inline", "Delimiter", "Bracket"]
        .map(|name| source_item(library, &format!("class {name}:")))
        .join("\n\n");
    let helpers = [
        "span_size",
        "peek",
        "peek_ahead",
        "column_after",
        "tab_width",
        "output_after",
        "normalized_capacity",
        "heading_byte",
        "line_offset",
        "block_error",
    ]
    .map(|name| source_item(library, &format!("def {name}(")))
    .join("\n\n");
    let parser_methods = ["at", "find_next_nonspace", "advance_offset"]
        .map(|name| source_item(library, &format!("    def {name}(")))
        .join("\n\n");
    let renderer_methods = ["room", "nest_image", "event_result"]
        .map(|name| source_item(library, &format!("    def {name}(")))
        .join("\n\n");
    let parser_arguments = parser_fields
        .lines()
        .filter_map(|line| line.trim().split_once(": "))
        .map(|(name, ty)| match name {
            "_text" => "std.bytes.from_bytes(b\"\\t \")?".to_owned(),
            "_line_end" => "2".to_owned(),
            "_column" => "column".to_owned(),
            "_partial_tab" => "True".to_owned(),
            "_next_nonspace" => "19".to_owned(),
            "_next_nonspace_column" => "23".to_owned(),
            "_indent" => "29".to_owned(),
            "_refs" => "Refs(dict[str, i64]()?, []?, []?)".to_owned(),
            _ if ty == "Bytes" => "std.bytes.empty()?".to_owned(),
            _ if ty.starts_with("list[") => "[]?".to_owned(),
            _ if ty == "bool" => "False".to_owned(),
            _ if ty == "str" => "\"\"".to_owned(),
            _ if ty == "i64" => "0".to_owned(),
            _ => panic!("unexpected production field {name}: {ty}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"import std.bytes
from std.bytes import Bytes
from std.markdown import MarkdownFault, MarkdownError, Event
{classes}

{helpers}

{parser_fields}

{parser_methods}

{renderer_fields}

{renderer_methods}

def state(column: i64) -> Result[Parser, AllocError]:
    Ok(Parser({parser_arguments}))

def main() -> Result[(), AllocError | IoError]:
    mut a = state(9223372036854775803)?
    mut b = state(9223372036854775804)?
    mut c = state(9223372036854775805)?
    mut d = state(9223372036854775806)?
    mut e = state(i64.MAX)?
    mut scan = state(i64.MAX)?
    mut progress = state(9223372036854775805)?
    progress._text = std.bytes.from_bytes(b" \t")?
    mut last_byte = state(9223372036854775806)?
    last_byte._offset = 1
    mut partial = state(9223372036854775806)?
    mut renderer = HtmlRenderer(std.bytes.empty()?, 9223372036854775806, i64.MAX, 10, 9223372036854775806, False)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    ra = a.advance_offset(1, False)
    rb = b.advance_offset(1, False)
    rc = c.advance_offset(1, False)
    rd = d.advance_offset(1, False)
    re = e.advance_offset(1, False)
    rs = scan.find_next_nonspace()
    progressed = progress.advance_offset(2, False)
    last_byte_result = last_byte.advance_offset(1, False)
    rp = partial.advance_offset(1, True)
    last = renderer.room(1)
    over = renderer.room(1)
    nested = renderer.nest_image()
    exhausted_outcome = renderer.nest_image()
    exhausted = renderer.event_result(exhausted_outcome)
    capacity = match normalized_capacity(i64.MAX):
        case Err(MarkdownFault.Allocation(AllocError.CapacityOverflow)):
            True
        case _:
            False
    negative_distance = span_size(i64.MAX, -1)
    absent = peek_ahead(b"", i64.MAX, 9, i64.MAX)
    mapped = block_error(MarkdownFault.InputTooLarge, b"x", i64.MAX)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(ra.is_ok(), a._column, a._offset, tab_width(9223372036854775803), tab_width(9223372036854775804), tab_width(9223372036854775805), tab_width(9223372036854775806))?
    print(rb.is_err(), rc.is_err(), rd.is_err(), re.is_err(), b._column, b._offset, b._partial_tab, e._column, e._offset)?
    print(rs.is_err(), scan._column, scan._offset, scan._next_nonspace, scan._next_nonspace_column, scan._indent)?
    print(progressed.is_err(), progress._column, progress._offset, progress._partial_tab, last_byte_result.is_ok(), last_byte._column, last_byte._offset)?
    print(rp.is_ok(), partial._column, partial._offset, partial._partial_tab)?
    print(last.is_ok(), over.is_err(), renderer._written, nested.is_ok(), exhausted.is_err(), renderer._image_depth, renderer._out.len(), renderer._broken)?
    exact_output = match over:
        case Err(MarkdownFault.OutputTooLarge):
            True
        case _:
            False
    exact_nesting = match exhausted:
        case Err(error):
            error.fault == MarkdownFault.InvalidState and error.offset == i64.MAX and error.line == 0
        case _:
            False
    print(exact_output, exact_nesting)?
    print(capacity, negative_distance == i64.MIN, absent == -1, mapped.fault, mapped.offset, mapped.line)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        assert_eq!(
            output_with_optimization(&source, optimize),
            "True 9223372036854775804 1 1 4 3 2\nTrue True True True 9223372036854775804 0 True 9223372036854775807 0\nTrue 9223372036854775807 0 19 23 29\nTrue 9223372036854775806 1 False True 9223372036854775807 2\nTrue 9223372036854775807 0 True\nTrue True 9223372036854775807 True True 9223372036854775807 0 True\nTrue True\nTrue True True std.markdown.MarkdownFault.InputTooLarge 0 0"
        );
    }
}

#[test]
fn synthetic_headings_validate_before_output_and_break_renderer() {
    let source = r#"import std.bytes
import std.markdown
from std.markdown import Event, Tag, MarkdownError

def main() -> Result[(), AllocError | IoError | MarkdownError | std.bytes.BytesError]:
    bounds = std.markdown.limits()
    for level in [i64.MIN, 0, 1, 6, 7, i64.MAX]?:
        for closing in [False, True]?:
            mut renderer = std.markdown.renderer(&bounds)?
            renderer.event(&Event.Text("before"))?
            event = Event.End(Tag.Heading(level)) if closing else Event.Start(Tag.Heading(level))
            result = renderer.event(&event)
            print(level, closing, result.is_ok(), renderer.pending(), renderer.written())?
            if result.is_err():
                print(renderer.event(&Event.Rule).is_err(), renderer.take()?.to_text()?)?
    mut small = std.markdown.limits()
    small.max_input = 1
    match std.markdown.parser_text("xx", &small):
        case Err(error):
            print(error.fault, error.offset, error.line)?
        case Ok(_):
            print("wrong")?
    Ok(())
"#;
    let expected = "-9223372036854775808 False False 6 6\nTrue before\n-9223372036854775808 True False 6 6\nTrue before\n0 False False 6 6\nTrue before\n0 True False 6 6\nTrue before\n1 False True 11 11\n1 True True 12 12\n6 False True 11 11\n6 True True 12 12\n7 False False 6 6\nTrue before\n7 True False 6 6\nTrue before\n9223372036854775807 False False 6 6\nTrue before\n9223372036854775807 True False 6 6\nTrue before\nstd.markdown.MarkdownFault.InputTooLarge 1 0";
    for optimize in [false, true] {
        assert_eq!(output_with_optimization(source, optimize), expected);
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_preserve_precise_faults_and_cleanup() {
    let source = r##"import std.markdown
from std.markdown import MarkdownFault

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    bounds = std.markdown.limits()
    mut failures = 0
    mut success = False
    for budget in range(300):
        marker = ("__test_fail_allocations_after_" + str.from(budget)?)?
        print((marker + "__")?)?
        result = std.markdown.to_html("# Hé\n\n![a *b*](/café \"T\") &copy;\n\n> - x\n", &bounds)
        print("__test_restore_allocations__")?
        match result:
            case Ok(html):
                print(html == "<h1>Hé</h1>\n<p><img src=\"/caf%C3%A9\" alt=\"a b\" title=\"T\" /> ©</p>\n<blockquote>\n<ul>\n<li>x</li>\n</ul>\n</blockquote>\n")?
                success = True
                break
            case Err(error):
                match error.fault:
                    case MarkdownFault.Allocation(AllocError.OutOfMemory):
                        print("allocation")?
                    case _:
                        print("wrong", error.fault)?
                failures += 1
    print(success, failures > 10)?
    Ok(())
"##;
    for optimize in [false, true] {
        let output = output_with_optimization(source, optimize);
        let lines = output.lines().collect::<Vec<_>>();
        assert!(lines.len() > 12, "{output}");
        assert_eq!(&lines[lines.len() - 2..], ["True", "True True"]);
        assert!(
            lines[..lines.len() - 2]
                .iter()
                .all(|line| *line == "allocation"),
            "{output}"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_writer_compaction_and_retry_need_no_allocation() {
    let source = r#"import std.bytes
import std.markdown
from std.bytes import Bytes
from std.io import StreamError
from std.markdown import Event, MarkdownError

pub class Sink:
    data: Bytes
    room: i64

    pub def write(self: &mut Sink, source: &[u8]) -> Result[i64, StreamError]:
        if self.data.len() == self.room:
            return Err(StreamError.Io(IoError.NotWritable))
        if len(source) == 0:
            return Ok(0)
        self.data.push(source[0]).map_error(def(error: AllocError) -> StreamError: StreamError.Allocation(error))?
        Ok(1)

    pub def flush(self: &mut Sink) -> Result[(), StreamError]:
        Ok(())

def main() -> Result[(), AllocError | IoError | MarkdownError | std.bytes.BytesError]:
    bounds = std.markdown.limits()
    mut renderer = std.markdown.renderer(&bounds)?
    renderer.event(&Event.Text("abcdefghijklmnop"))?
    mut first = Sink(std.bytes.with_capacity(32)?, 5)
    mut rest = Sink(std.bytes.with_capacity(32)?, 32)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    failed = std.markdown.write_pending(&mut renderer, &mut first)
    pending = renderer.pending()
    retried = std.markdown.write_pending(&mut renderer, &mut rest)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    match failed:
        case Err(error):
            print(error.fault, error.offset)?
        case _:
            print("wrong")?
    print(pending, retried.is_ok(), renderer.pending(), renderer.written())?
    print(first.data.to_text()?, rest.data.to_text()?)?
    Ok(())
"#;
    for optimize in [false, true] {
        assert_eq!(
            output_with_optimization(source, optimize),
            "std.markdown.MarkdownFault.Stream(std.io.StreamError.Io(IoError.NotWritable)) 16\n11 True 0 16\nabcde fghijklmnop"
        );
    }
}

#[test]
fn markdown_text_entries_accept_unicode_ranges() {
    assert_visible_stdout(
        r#"
import std.markdown
from std.markdown import Event
def main() -> Result[(), AllocError | IoError | std.markdown.MarkdownError]:
    text = "!# café 🙂\n!"
    bounds = std.markdown.limits()
    print(std.markdown.to_html(&text[1:-1], &bounds)? == "<h1>café 🙂</h1>\n")?
    mut parser = std.markdown.parser_text(&text[1:-1], &bounds)?
    print(parser.next()? == Event.Start(std.markdown.Tag.Heading(1)))?
    print(std.markdown.to_html(&text[1:1], &bounds)? == "")?
    Ok(())
"#,
        "True\nTrue\nTrue",
    );
}
