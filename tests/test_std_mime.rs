//! `std.mime`: RFC 9110 media type parsing and formatting, media-range
//! matching, and the extension table shared with `std.http.files` (issue #485).
mod support;

const HEADER: &str = r#"import std.mime
from std.mime import MediaType

def show(text: &str) -> Result[(), AllocError | IoError]:
    match std.mime.parse(text):
        case Ok(value):
            print(value.format()?)?
        case Err(error):
            print(error)?
    Ok(())

"#;

fn native(body: &str, expected: &str) {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    let source = format!(
        "{HEADER}def main() -> Result[(), AllocError | IoError | ParseError]:\n{body}    Ok(())\n"
    );
    let actual = support::run_file_stdout(&source);
    assert_eq!(actual, expected.trim_matches('\n'), "{source}");
}

#[test]
fn parsing_normalizes_names_and_formats_canonically() {
    native(
        r#"
for text in ["Text/HTML; Charset=\"UTF-8\"", "text/plain", " text/plain ; ", "text/plain\t;\tformat=flowed", "multipart/form-data; boundary=\"a b\\\"c\"", "a/b;;x=1", "text/plain;x=\"\"", "text/plain; x=\"\\\\\"", "*/*", "text/plain; Name=\"caf\u{e9}\""]?:
    show(&text)?
"#,
        r#"
text/html; charset=utf-8
text/plain
text/plain
text/plain; format=flowed
multipart/form-data; boundary="a b\"c"
a/b; x=1
text/plain; x=""
text/plain; x="\\"
*/*
text/plain; name="café"
"#,
    );
}

#[test]
fn malformed_media_types_are_rejected() {
    native(
        r#"
for text in ["", "text", "/x", "text/", "te xt/plain", "text/plain x", "text/plain; x", "text/plain; x=", "text/plain; =1", "text/plain; x=\"open", "text/plain; x=\"a\"b", "text/plain; x=1; X=2", "text/plain; x=a b", "text/plain; x=\"a\u{1}\"", "text/pl\u{e9}in"]?:
    show(&text)?
"#,
        &"ParseError.Invalid\n".repeat(15),
    );
}

#[test]
fn accessors_equality_and_ranges() {
    native(
        r#"
html = std.mime.parse("text/html; charset=utf-8")?
print(html.type_name(), html.subtype(), html.param("CHARSET"), html.param("q"), html.essence()?, html.suffix()?)?
print(len(html.params()), html.params()[0].name, html.params()[0].value)?
print(html == std.mime.parse("TEXT/html;charset=UTF-8")?, html == std.mime.parse("text/html")?)?
print(html.matches(&std.mime.parse("text/*")?), html.matches(&std.mime.parse("*/*")?), html.matches(&std.mime.parse("image/*")?))?
print(html.matches(&std.mime.parse("text/html; charset=UTF-8")?), html.matches(&std.mime.parse("text/html; level=1")?))?
print(std.mime.parse("text/html")?.matches(&html), html.is_wildcard(), std.mime.parse("text/*")?.is_wildcard())?
print(std.mime.parse("application/ld+json")?.suffix()?, std.mime.parse("image/svg+xml")?.essence()?)?
print(std.mime.parse("text/plain; Format=Flowed")?.param("format"), std.mime.parse("a/b; abcd=1")?.param("\u{e9}\u{e9}"))?
"#,
        r#"
text html Option[str].Some("utf-8") Option[str].Nothing text/html Option[str].Nothing
1 charset utf-8
True False
True True False
True False
False False True
Option[str].Some("json") image/svg+xml
Option[str].Some("Flowed") Option[str].Nothing
"#,
    );
}

#[test]
fn building_validates_and_replaces_parameters() {
    native(
        r#"
mut built = std.mime.new("Application", "JSON")?
built.set_param("Charset", "UTF-8")?
built.set_param("note", "hi there")?
built.set_param("charset", "ascii")?
print(built.format()?)?
print(built.remove_param("NOTE"), built.remove_param("x"), built.format()?)?
print(built.set_param("bad name", "x").is_err(), built.set_param("x", "a\nb").is_err(), std.mime.new("a b", "c").is_err(), std.mime.new("text", "").is_err())?
built.set_param("empty", "")?
built.set_param("quote", "say \"hi\"")?
text = built.format()?
print(text, std.mime.parse(&text)? == built)?
"#,
        r#"
application/json; charset=ascii; note="hi there"
True False application/json; charset=ascii
True True True True
application/json; charset=ascii; empty=""; quote="say \"hi\"" True
"#,
    );
}

#[test]
fn extension_table() {
    native(
        r#"
for name in ["index.HTML", "a.htm", "b.css", "c.js", "d.MJS", "e.json", "f.txt", "g.csv", "h.xml", "i.svg", "j.png", "k.jpg", "l.jpeg", "m.gif", "n.webp", "o.ico", "p.wasm", "q.pdf", "r.woff2", "s.Woff"]?:
    print(std.mime.from_file_name(&name))?
for name in ["a.tar.gz", "html", ".html", "dir.d/file", "\u{e9}.png", "na\u{ef}ve", "noext.", "x.htmlx"]?:
    print(std.mime.from_file_name(&name))?
print(std.mime.from_extension("JPEG"), std.mime.from_extension(".png"), std.mime.from_extension(""))?
"#,
        r#"
text/html; charset=utf-8
text/html; charset=utf-8
text/css; charset=utf-8
text/javascript; charset=utf-8
text/javascript; charset=utf-8
application/json
text/plain; charset=utf-8
text/csv; charset=utf-8
application/xml
image/svg+xml
image/png
image/jpeg
image/jpeg
image/gif
image/webp
image/vnd.microsoft.icon
application/wasm
application/pdf
font/woff2
font/woff
application/octet-stream
application/octet-stream
text/html; charset=utf-8
application/octet-stream
image/png
application/octet-stream
application/octet-stream
application/octet-stream
Option[str].Some("image/jpeg") Option[str].Nothing Option[str].Nothing
"#,
    );
}

/// Every allocation in parsing and formatting reports `AllocError` rather
/// than trapping or returning a partial value.
#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_returns_alloc_error() {
    let source = format!(
        "{HEADER}{}",
        r#"def attempt() -> Result[str, AllocError | ParseError]:
    mut value = std.mime.parse("Multipart/Form-Data; Boundary=\"a \\\"b\"; charset=UTF-8; x=y")?
    value.set_param("note", "two words")?
    Ok(value.format()?)

def main() -> Result[(), AllocError | IoError]:
    mut failures = 0
    mut limit = 0
    while True:
        marker = ("__test_fail_allocations_after_" + str.from(limit)?)?
        print((marker + "__")?)?
        result = attempt()
        print("__test_restore_allocations__")?
        match result:
            case Ok(text):
                print(text)?
                break
            case Err(error):
                print(error)?
                failures += 1
        limit += 1
    print(failures)?
    Ok(())
"#
    );
    let text = support::run_file_stdout(&source);
    let lines = text.lines().collect::<Vec<_>>();
    let failures = lines.last().unwrap().parse::<usize>().unwrap();
    assert!(failures >= 5, "{text}");
    assert!(
        lines[..failures]
            .iter()
            .all(|line| *line == "AllocError.OutOfMemory"),
        "{text}"
    );
    assert_eq!(
        lines[failures],
        r#"multipart/form-data; boundary="a \"b"; charset=utf-8; x=y; note="two words""#
    );
}
