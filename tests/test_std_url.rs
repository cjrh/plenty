//! `std.url`: RFC 3986 absolute URL parsing, formatting, percent encoding,
//! and query pairs.

use std::net::Ipv6Addr;
use std::str::FromStr;

const HEADER: &str = "import std.bytes\nimport std.encoding\nimport std.url\nfrom std.bytes import Bytes\nfrom std.url import Url, Pair, Component, UrlError\n";

/// Prints one line per case: `ok <roundtrips> scheme userinfo host port path
/// query fragment`, absent components as `-` and present ones after `+`, or
/// `err <error>`. `tag` classifies a result without allocating.
const HELPERS: &str = r#"
def opt(value: &Option[str]) -> Result[str, AllocError]:
    match value:
        case Some(text):
            "+".concat(text)
        case Nothing:
            Ok("-")

def show(text: &str, limit: i64) -> Result[(), Failure]:
    match std.url.parse(text, limit):
        case Ok(url):
            mut port = -1
            match url.port:
                case Some(value):
                    port = value
                case Nothing:
                    pass
            print("ok", url.format()? == text, url.scheme, opt(&url.userinfo)?, opt(&url.host)?, port, url.path, opt(&url.query)?, opt(&url.fragment)?)?
        case Err(error):
            print("err", str.repr(&error)?)?
    Ok(())

def tag[T](result: &Result[T, UrlError]) -> str:
    match result:
        case Ok(_):
            "ok"
        case Err(error):
            match error:
                case UrlError.Allocation(_):
                    "alloc"
                case _:
                    "other"

def some(text: &str) -> Result[Option[str], AllocError]:
    Ok(Some("".concat(text)?))
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}{HELPERS}\ndef main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.url.UrlError", "E")
        .replace("std.url.", "")
        .replace("std.bytes.Bytes", "Bytes")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

fn rejected(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    match plenty::check_file(&path, None) {
        Ok(()) => panic!("accepted:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Parses every input in one program. Inputs travel as hex so that they may
/// hold any text; `0xff`, which UTF-8 never contains, ends each one.
fn parse_all(inputs: &[String]) -> Vec<String> {
    let mut blob = String::new();
    for input in inputs {
        blob.push_str(&hex(input.as_bytes()));
        blob.push_str("ff");
    }
    let body = format!(
        r#"
blob = std.encoding.hex_from_text("{blob}")?
mut start = 0
while start < blob.len():
    stop = blob.find(255u8, start, blob.len())?.unwrap()
    text = blob.slice(start, stop)?.to_text()?
    show(&text, 4096)?
    start = stop + 1
"#
    );
    let lines: Vec<String> = output(&program(&body))
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), inputs.len());
    lines
}

const UNRESERVED: &str = "-._~";
const SUB_DELIMS: &str = "!$&'()*+,;=";

fn written(text: &str, extra: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index] as char;
        if byte == '%' {
            let digits = bytes.get(index + 1..index + 3);
            if !digits.is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit)) {
                return false;
            }
            index += 3;
            continue;
        }
        if !(byte.is_ascii_alphanumeric()
            || UNRESERVED.contains(byte)
            || SUB_DELIMS.contains(byte)
            || extra.contains(byte))
        {
            return false;
        }
        index += 1;
    }
    true
}

fn reference_host(host: &str) -> bool {
    let Some(inner) = host.strip_prefix('[') else {
        return written(host, "");
    };
    let Some(inner) = inner.strip_suffix(']') else {
        return false;
    };
    if let Some(future) = inner.strip_prefix(['v', 'V']) {
        let Some((version, address)) = future.split_once('.') else {
            return false;
        };
        return !version.is_empty()
            && version.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !address.is_empty()
            && !address.contains('%')
            && written(address, ":");
    }
    Ipv6Addr::from_str(inner).is_ok()
}

fn reference_port(port: &str) -> Option<u32> {
    let plain = !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && (port.len() == 1 || !port.starts_with('0'));
    let value: u32 = if plain {
        port.parse().ok()?
    } else {
        return None;
    };
    (value <= 65535).then_some(value)
}

/// An independent splitter in the style of RFC 3986 appendix B, with the
/// standard library's IPv6 parser as the address oracle. Returns the line
/// `show` prints for an accepted URL.
fn reference(text: &str) -> Option<String> {
    if !text.is_ascii() {
        return None;
    }
    let (scheme, rest) = text.split_once(':')?;
    let mut letters = scheme.chars();
    if !letters.next()?.is_ascii_alphabetic()
        || !letters.all(|letter| letter.is_ascii_alphanumeric() || "+-.".contains(letter))
    {
        return None;
    }
    let (rest, fragment) = match rest.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (rest, None),
    };
    let (hier, query) = match rest.split_once('?') {
        Some((hier, query)) => (hier, Some(query)),
        None => (rest, None),
    };
    let (authority, path) = match hier.strip_prefix("//") {
        Some(after) => {
            let end = after.find('/').unwrap_or(after.len());
            (Some(&after[..end]), &after[end..])
        }
        None => (None, hier),
    };
    let mut userinfo = None;
    let mut host = None;
    let mut port = None;
    if let Some(authority) = authority {
        let hostport = match authority.split_once('@') {
            Some((user, hostport)) => {
                if !written(user, ":") {
                    return None;
                }
                userinfo = Some(user);
                hostport
            }
            None => authority,
        };
        let (name, number) = if hostport.starts_with('[') {
            let close = hostport.find(']')?;
            let tail = &hostport[close + 1..];
            if tail.is_empty() {
                (hostport, None)
            } else {
                (&hostport[..=close], Some(tail.strip_prefix(':')?))
            }
        } else {
            match hostport.split_once(':') {
                Some((name, number)) => (name, Some(number)),
                None => (hostport, None),
            }
        };
        if !reference_host(name) {
            return None;
        }
        if let Some(number) = number {
            port = Some(reference_port(number)?);
        }
        host = Some(name);
    }
    if !written(path, ":@/")
        || !query.is_none_or(|query| written(query, ":@/?"))
        || !fragment.is_none_or(|fragment| written(fragment, ":@/?"))
    {
        return None;
    }
    let opt = |value: Option<&str>| value.map_or("-".to_string(), |value| format!("+{value}"));
    Some(format!(
        "ok True {scheme} {} {} {} {path} {} {}",
        opt(userinfo),
        opt(host),
        port.map_or(-1, i64::from),
        opt(query),
        opt(fragment)
    ))
}

/// Checks hand-written expectations against both the module and the
/// reference splitter, so the fuzz oracle is itself pinned by fixtures.
fn fixtures(cases: &[(&str, &str)]) {
    let inputs: Vec<String> = cases.iter().map(|(input, _)| input.to_string()).collect();
    let lines = parse_all(&inputs);
    for ((input, expected), line) in cases.iter().zip(&lines) {
        let expected = if expected.starts_with("E.") {
            format!("err {expected}")
        } else {
            format!("ok True {expected}")
        };
        assert_eq!(*line, expected, "{input}");
        let oracle = reference(input);
        assert_eq!(oracle.is_some(), line.starts_with("ok"), "{input}");
        if let Some(oracle) = oracle {
            assert_eq!(oracle, *line, "{input}");
        }
    }
}

#[test]
fn rfc_3986_examples_split_into_components() {
    // RFC 3986 sections 1.1.2 and 3.
    fixtures(&[
        (
            "ftp://ftp.is.co.za/rfc/rfc1808.txt",
            "ftp - +ftp.is.co.za -1 /rfc/rfc1808.txt - -",
        ),
        (
            "http://www.ietf.org/rfc/rfc2396.txt",
            "http - +www.ietf.org -1 /rfc/rfc2396.txt - -",
        ),
        (
            "ldap://[2001:db8::7]/c=GB?objectClass?one",
            "ldap - +[2001:db8::7] -1 /c=GB +objectClass?one -",
        ),
        (
            "mailto:John.Doe@example.com",
            "mailto - - -1 John.Doe@example.com - -",
        ),
        (
            "news:comp.infosystems.www.servers.unix",
            "news - - -1 comp.infosystems.www.servers.unix - -",
        ),
        ("tel:+1-816-555-1212", "tel - - -1 +1-816-555-1212 - -"),
        ("telnet://192.0.2.16:80/", "telnet - +192.0.2.16 80 / - -"),
        (
            "urn:oasis:names:specification:docbook:dtd:xml:4.1.2",
            "urn - - -1 oasis:names:specification:docbook:dtd:xml:4.1.2 - -",
        ),
        (
            "foo://example.com:8042/over/there?name=ferret#nose",
            "foo - +example.com 8042 /over/there +name=ferret +nose",
        ),
        (
            "urn:example:animal:ferret:nose",
            "urn - - -1 example:animal:ferret:nose - -",
        ),
        (
            "https://user:p%40ss@example.com:8443/a/b%20c;v=1?x=1&y=2#top",
            "https +user:p%40ss +example.com 8443 /a/b%20c;v=1 +x=1&y=2 +top",
        ),
        // Nothing is normalized: case, dot segments, and escapes are kept.
        (
            "HTTP://EXAMPLE.com:80/a/./b/../%7euser/%7Euser",
            "HTTP - +EXAMPLE.com 80 /a/./b/../%7euser/%7Euser - -",
        ),
        ("a+b-c.1://h", "a+b-c.1 - +h -1  - -"),
    ]);
}

#[test]
fn empty_components_are_distinct_from_absent_ones() {
    fixtures(&[
        ("s:", "s - - -1  - -"),
        ("s://", "s - + -1  - -"),
        ("s:///", "s - + -1 / - -"),
        ("file:///etc/hosts", "file - + -1 /etc/hosts - -"),
        ("s:?", "s - - -1  + -"),
        ("s:#", "s - - -1  - +"),
        ("s:?#", "s - - -1  + +"),
        ("s://h?", "s - +h -1  + -"),
        ("s://h#", "s - +h -1  - +"),
        ("s://h/?#", "s - +h -1 / + +"),
        ("s://@h", "s + +h -1  - -"),
        ("s://:@h", "s +: +h -1  - -"),
        ("s://@", "s + + -1  - -"),
        ("s://:80", "s - + 80  - -"),
        ("s://u@:80/", "s +u + 80 / - -"),
        ("s:/", "s - - -1 / - -"),
        ("s:/a//b", "s - - -1 /a//b - -"),
        ("s://h//", "s - +h -1 // - -"),
        ("s:a", "s - - -1 a - -"),
    ]);
}

#[test]
fn ipv6_and_future_address_literals_follow_the_grammar() {
    fixtures(&[
        ("s://[::]", "s - +[::] -1  - -"),
        ("s://[::1]:8080/x", "s - +[::1] 8080 /x - -"),
        ("s://[1::]", "s - +[1::] -1  - -"),
        ("s://[1:2:3:4:5:6:7:8]", "s - +[1:2:3:4:5:6:7:8] -1  - -"),
        ("s://[1:2:3:4:5:6:7::]", "s - +[1:2:3:4:5:6:7::] -1  - -"),
        ("s://[::2:3:4:5:6:7:8]", "s - +[::2:3:4:5:6:7:8] -1  - -"),
        ("s://[1:2:3::6:7:8]", "s - +[1:2:3::6:7:8] -1  - -"),
        (
            "s://[2001:DB8:0:0:8:800:200C:417A]",
            "s - +[2001:DB8:0:0:8:800:200C:417A] -1  - -",
        ),
        (
            "s://[::ffff:192.0.2.128]",
            "s - +[::ffff:192.0.2.128] -1  - -",
        ),
        (
            "s://[1:2:3:4:5:6:255.255.255.255]",
            "s - +[1:2:3:4:5:6:255.255.255.255] -1  - -",
        ),
        ("s://[64:ff9b::0.0.0.0]", "s - +[64:ff9b::0.0.0.0] -1  - -"),
        ("s://u@[::1]:1", "s +u +[::1] 1  - -"),
        ("s://[v1.x]", "s - +[v1.x] -1  - -"),
        ("s://[vAf.a:b!$]", "s - +[vAf.a:b!$] -1  - -"),
        ("s://[]", "E.InvalidHost(4)"),
        ("s://[:]", "E.InvalidHost(4)"),
        ("s://[:::]", "E.InvalidHost(4)"),
        ("s://[1]", "E.InvalidHost(4)"),
        ("s://[1:2:3:4:5:6:7]", "E.InvalidHost(4)"),
        ("s://[1:2:3:4:5:6:7:8:9]", "E.InvalidHost(4)"),
        ("s://[1:2:3:4:5:6:7::8]", "E.InvalidHost(4)"),
        ("s://[1::2::3]", "E.InvalidHost(4)"),
        ("s://[12345::]", "E.InvalidHost(4)"),
        ("s://[g::]", "E.InvalidHost(4)"),
        ("s://[1:]", "E.InvalidHost(4)"),
        ("s://[:1]", "E.InvalidHost(4)"),
        ("s://[::1.2.3]", "E.InvalidHost(4)"),
        ("s://[::1.2.3.256]", "E.InvalidHost(4)"),
        ("s://[::1.2.3.04]", "E.InvalidHost(4)"),
        ("s://[::1.2.3.4:5]", "E.InvalidHost(4)"),
        ("s://[1.2.3.4]", "E.InvalidHost(4)"),
        ("s://[1:2:3:4:5:6:7:1.2.3.4]", "E.InvalidHost(4)"),
        // RFC 6874 zone identifiers are not accepted.
        ("s://[fe80::1%25eth0]", "E.InvalidHost(4)"),
        ("s://[v.x]", "E.InvalidHost(4)"),
        ("s://[v1.]", "E.InvalidHost(4)"),
        ("s://[v1x]", "E.InvalidHost(4)"),
        ("s://[v1.a/b]", "E.InvalidHost(4)"),
        ("s://[::1", "E.InvalidHost(4)"),
        ("s://[::1]x", "E.InvalidHost(9)"),
        ("s://[::1]]", "E.InvalidHost(9)"),
        ("s://x[::1]", "E.InvalidPort(7)"),
        ("s://::1", "E.InvalidPort(5)"),
    ]);
}

#[test]
fn ports_are_canonical_decimal_numbers() {
    fixtures(&[
        ("s://h:0", "s - +h 0  - -"),
        ("s://h:65535/", "s - +h 65535 / - -"),
        ("s://h:65536/", "E.InvalidPort(6)"),
        ("s://h:/", "E.InvalidPort(6)"),
        ("s://h:", "E.InvalidPort(6)"),
        ("s://h:080", "E.InvalidPort(6)"),
        ("s://h:00", "E.InvalidPort(6)"),
        ("s://h:8a", "E.InvalidPort(6)"),
        ("s://h:-1", "E.InvalidPort(6)"),
        ("s://h:+1", "E.InvalidPort(6)"),
        ("s://h:123456", "E.InvalidPort(6)"),
        ("s://h:1:2", "E.InvalidPort(6)"),
        ("s://h:%38", "E.InvalidPort(6)"),
    ]);
}

#[test]
fn reserved_delimiters_are_accepted_only_where_the_grammar_allows() {
    fixtures(&[
        (
            "s://a!$&'()*+,;=:b@h!$&'()*+,;=/p!$&'()*+,;=:@/?q!$&'()*+,;=:@/?#f!$&'()*+,;=:@/?",
            "s +a!$&'()*+,;=:b +h!$&'()*+,;= -1 /p!$&'()*+,;=:@/ +q!$&'()*+,;=:@/? +f!$&'()*+,;=:@/?",
        ),
        ("s://h/a?b?c#d?e", "s - +h -1 /a +b?c +d?e"),
        ("s:a:b@c", "s - - -1 a:b@c - -"),
        // A second `@` is not userinfo: it is an error, never a host choice.
        ("s://a@b@c/", "E.InvalidByte(7)"),
        ("s://a@b:1@c/", "E.InvalidPort(8)"),
        ("s://h/a#b#c", "E.InvalidByte(9)"),
        ("s://h/[x]", "E.InvalidByte(6)"),
        ("s://h/?[x]", "E.InvalidByte(7)"),
        ("s://h/a b", "E.InvalidByte(7)"),
        ("s://h/a\tb", "E.InvalidByte(7)"),
        ("s://h/a\nb", "E.InvalidByte(7)"),
        ("s://h/a\u{0}b", "E.InvalidByte(7)"),
        ("s://h/a\u{7f}", "E.InvalidByte(7)"),
        ("s://h/\"", "E.InvalidByte(6)"),
        ("s://h/<", "E.InvalidByte(6)"),
        ("s://h/>", "E.InvalidByte(6)"),
        ("s://h/\\", "E.InvalidByte(6)"),
        ("s://h/^", "E.InvalidByte(6)"),
        ("s://h/`", "E.InvalidByte(6)"),
        ("s://h/{", "E.InvalidByte(6)"),
        ("s://h/|", "E.InvalidByte(6)"),
        ("s://h/}", "E.InvalidByte(6)"),
        ("s://h\\x/", "E.InvalidByte(5)"),
        ("s://u<@h", "E.InvalidByte(5)"),
        (" s://h", "E.InvalidByte(0)"),
        ("s://h ", "E.InvalidByte(5)"),
        ("1s://h", "E.InvalidByte(0)"),
        ("s_x://h", "E.InvalidByte(1)"),
        ("", "E.MissingScheme"),
        (":", "E.MissingScheme"),
        ("://h", "E.MissingScheme"),
        ("h", "E.MissingScheme"),
        ("//h/p", "E.MissingScheme"),
        ("/a:b", "E.MissingScheme"),
        ("a/b:c", "E.MissingScheme"),
        ("?a:b", "E.MissingScheme"),
        ("#a:b", "E.MissingScheme"),
        ("../x", "E.MissingScheme"),
    ]);
}

#[test]
fn malformed_escapes_are_rejected_in_every_component() {
    fixtures(&[
        (
            "s://%41%7a%2F@%68/%2f?%3F#%23",
            "s +%41%7a%2F +%68 -1 /%2f +%3F +%23",
        ),
        ("s://h/%00%ff%FF", "s - +h -1 /%00%ff%FF - -"),
        ("s://h/%", "E.InvalidEscape(6)"),
        ("s://h/%4", "E.InvalidEscape(6)"),
        ("s://h/%4g", "E.InvalidEscape(6)"),
        ("s://h/%g4", "E.InvalidEscape(6)"),
        ("s://h/%%41", "E.InvalidEscape(6)"),
        ("s://h/a%4/b", "E.InvalidEscape(7)"),
        ("s://h/a%4?b", "E.InvalidEscape(7)"),
        ("s://u%@h", "E.InvalidEscape(5)"),
        ("s://u%4@h", "E.InvalidEscape(5)"),
        ("s://h%/", "E.InvalidEscape(5)"),
        ("s://h%2", "E.InvalidEscape(5)"),
        ("s://h?%x1", "E.InvalidEscape(6)"),
        ("s://h?a%2", "E.InvalidEscape(7)"),
        ("s://h#%", "E.InvalidEscape(6)"),
        ("s://h#a%-1", "E.InvalidEscape(7)"),
        ("%73://h", "E.InvalidByte(0)"),
    ]);
}

#[test]
fn unicode_must_be_percent_encoded_or_punycode() {
    fixtures(&[
        ("http://bücher.example/", "E.InvalidByte(8)"),
        (
            "http://xn--bcher-kva.example/",
            "http - +xn--bcher-kva.example -1 / - -",
        ),
        (
            "http://b%C3%BCcher.example/",
            "http - +b%C3%BCcher.example -1 / - -",
        ),
        ("http://h/ü", "E.InvalidByte(9)"),
        ("http://h/?q=日本", "E.InvalidByte(12)"),
        (
            "http://h/?q=%E6%97%A5%E6%9C%AC",
            "http - +h -1 / +q=%E6%97%A5%E6%9C%AC -",
        ),
        ("http://h/#\u{1F600}", "E.InvalidByte(10)"),
        ("http://ü@h/", "E.InvalidByte(7)"),
        ("httpü://h/", "E.InvalidByte(4)"),
        // Full-width and ideographic dots are not treated as `.`.
        ("http://a\u{3002}b/", "E.InvalidByte(8)"),
    ]);
}

#[test]
fn every_byte_is_escaped_or_kept_by_component() {
    // The characters each component leaves unescaped, besides letters,
    // digits, and `-._~`.
    let sets = [
        ("Userinfo", "!$&'()*+,;="),
        ("Segment", "!$&'()*+,;=:@"),
        ("Path", "!$&'()*+,;=:@/"),
        ("Query", "!$&'()*+,;=:@/?"),
        ("QueryPart", "!$'()*,;:@/?"),
        ("Fragment", "!$&'()*+,;=:@/?"),
    ];
    let mut body = String::from(
        "mut all = std.bytes.empty()?\nfor value in range(256):\n    all.push(u8(value))?\n",
    );
    let mut expected = String::new();
    for (name, kept) in sets {
        body.push_str(&format!(
            "coded{name} = std.url.encode_bytes(&all, 0, 256, Component.{name})?\n\
             print(coded{name}.to_text()?)?\n\
             print(std.url.decode_bytes(&coded{name}, 0, coded{name}.len())? == all, coded{name}.capacity() == coded{name}.len())?\n"
        ));
        for byte in 0..=255u8 {
            let letter = byte as char;
            if letter.is_ascii_alphanumeric()
                || UNRESERVED.contains(letter)
                || kept.contains(letter)
            {
                expected.push(letter);
            } else {
                expected.push_str(&format!("%{byte:02X}"));
            }
        }
        expected.push_str("\nTrue True\n");
    }
    native(&body, &expected);
}

#[test]
fn text_encoding_and_decoding_are_strict() {
    native(
        r#"
print(std.url.encode("a b/c?d=e&f#g%", Component.Segment)?)?
print(std.url.encode("a b/c?d=e&f#g%", Component.Path)?)?
print(std.url.encode("a b/c?d=e&f#g%", Component.Query)?)?
print(std.url.encode("a b/c?d=e&f#g%", Component.QueryPart)?)?
print(std.url.encode("user:name@x", Component.Userinfo)?)?
print(std.url.encode("ü 日本 😀", Component.Fragment)?)?
print("[", std.url.encode("", Component.Path)?, "]")?
print(std.url.encode("%41", Component.Path)?, std.url.decode("%2541")?)?
print(std.url.decode("a%20b%2fc%2Fd+e")?)?
print(std.url.decode("%C3%BC %e6%97%a5ü")?)?
print(std.url.decode("")? == "", std.url.decode("%00")? == "\0")?
print(str.repr(std.url.decode("%"))?)?
print(str.repr(std.url.decode("ab%4"))?)?
print(str.repr(std.url.decode("ü%zz"))?)?
print(str.repr(std.url.decode("%ff"))?)?
print(str.repr(std.url.decode("%C3"))?)?
print(str.repr(std.url.decode("%ED%A0%80"))?)?
raw = std.bytes.from_text("%ff%C3")?
print(std.encoding.hex_text(&std.url.decode_bytes(&raw, 0, 6)?, 0, 2)?)?
print(std.url.decode_bytes(&raw, 3, 6)?.len(), std.url.decode_bytes(&raw, 6, 6)?.len())?
print(str.repr(std.url.decode_bytes(&raw, 0, 2))?)?
print(str.repr(std.url.decode_bytes(&raw, 0, 5))?)?
print(str.repr(std.url.decode_bytes(&raw, 0, 7))?)?
print(str.repr(std.url.decode_bytes(&raw, -1, 3))?)?
print(str.repr(std.url.decode_bytes(&raw, 4, 3))?)?
print(str.repr(std.url.encode_bytes(&raw, 0, 7, Component.Path))?)?
print(str.repr(std.url.encode_bytes(&raw, 2, 1, Component.Path))?)?
"#,
        r#"
a%20b%2Fc%3Fd=e&f%23g%25
a%20b/c%3Fd=e&f%23g%25
a%20b/c?d=e&f%23g%25
a%20b/c?d%3De%26f%23g%25
user%3Aname%40x
%C3%BC%20%E6%97%A5%E6%9C%AC%20%F0%9F%98%80
[  ]
%2541 %41
a b/c/d+e
ü 日ü
True True
Result[str, E].Err(E.InvalidEscape(0))
Result[str, E].Err(E.InvalidEscape(2))
Result[str, E].Err(E.InvalidEscape(2))
Result[str, E].Err(E.InvalidUtf8)
Result[str, E].Err(E.InvalidUtf8)
Result[str, E].Err(E.InvalidUtf8)
ffc3
1 0
Result[Bytes, E].Err(E.InvalidEscape(0))
Result[Bytes, E].Err(E.InvalidEscape(3))
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
"#,
    );
}

#[test]
fn query_pairs_split_decode_and_rebuild() {
    native(
        r#"
pairs = std.url.query_pairs("a=1&b=x%26y%3Dz&c&=v&&q=a=b&sp=a+b%20c&%C3%BC=%E6%97%A5")?
for pair in &pairs:
    print("[", pair.key, "] [", pair.value, "]")?
print(std.url.query_text(&pairs)?)?
print(len(std.url.query_pairs("")?), len(std.url.query_pairs("&")?), len(std.url.query_pairs("a")?))?
print(std.url.query_text(&std.url.query_pairs("")?)? == "")?
print(str.repr(std.url.query_pairs("a=1&b=%4"))?)?
print(str.repr(std.url.query_pairs("a=1&%zz=2"))?)?
print(str.repr(std.url.query_pairs("a=%ff"))?)?
mut built = list[Pair]()?
built.append(Pair("name", "Ferret & Co"))?
built.append(Pair("k=1", "100%+/?#"))?
built.append(Pair("", ""))?
text = std.url.query_text(&built)?
print(text)?
url = Url("https", Nothing, some("example.com")?, Nothing, "/search", some(&text)?, Nothing)
print(url.format()?)?
again = std.url.query_pairs(&text)?
print(len(again), again[0].value, again[1].key, again[1].value)?
"#,
        r#"
[ a ] [ 1 ]
[ b ] [ x&y=z ]
[ c ] [  ]
[  ] [ v ]
[  ] [  ]
[ q ] [ a=b ]
[ sp ] [ a+b c ]
[ ü ] [ 日 ]
a=1&b=x%26y%3Dz&c=&=v&=&q=a%3Db&sp=a%2Bb%20c&%C3%BC=%E6%97%A5
0 2 1
True
Result[list[Pair], E].Err(E.InvalidEscape(6))
Result[list[Pair], E].Err(E.InvalidEscape(4))
Result[list[Pair], E].Err(E.InvalidUtf8)
name=Ferret%20%26%20Co&k%3D1=100%25%2B/?%23&=
https://example.com/search?name=Ferret%20%26%20Co&k%3D1=100%25%2B/?%23&=
3 Ferret & Co k=1 100%+/?#
"#,
    );
}

#[test]
fn format_rejects_components_that_would_change_meaning() {
    let cases = [
        // scheme, userinfo, host, port, path, query, fragment
        (
            r#""https", some("u:p")?, some("h")?, Some(8080), "/p", some("q")?, some("f")?"#,
            "https://u:p@h:8080/p?q#f True",
        ),
        (
            r#""mailto", Nothing, Nothing, Nothing, "a@b", Nothing, Nothing"#,
            "mailto:a@b True",
        ),
        (
            r#""s", Nothing, some("")?, Nothing, "", some("")?, some("")?"#,
            "s://?# True",
        ),
        (
            r#""s", Nothing, some("[::1]")?, Some(0), "", Nothing, Nothing"#,
            "s://[::1]:0 True",
        ),
        (
            r#""", Nothing, some("h")?, Nothing, "", Nothing, Nothing"#,
            "E.MissingScheme",
        ),
        (
            r#""1s", Nothing, some("h")?, Nothing, "", Nothing, Nothing"#,
            "E.InvalidByte(0)",
        ),
        (
            r#""s:x", Nothing, some("h")?, Nothing, "", Nothing, Nothing"#,
            "E.InvalidByte(1)",
        ),
        (
            r#""s", some("u")?, Nothing, Nothing, "", Nothing, Nothing"#,
            "E.MissingHost",
        ),
        (
            r#""s", Nothing, Nothing, Some(80), "", Nothing, Nothing"#,
            "E.MissingHost",
        ),
        (
            r#""s", some("a@b")?, some("h")?, Nothing, "", Nothing, Nothing"#,
            "E.InvalidByte(5)",
        ),
        (
            r#""s", Nothing, some("evil.example/x")?, Nothing, "", Nothing, Nothing"#,
            "E.InvalidByte(16)",
        ),
        (
            r#""s", Nothing, some("h:80")?, Nothing, "", Nothing, Nothing"#,
            "E.InvalidByte(5)",
        ),
        (
            r#""s", Nothing, some("a@b")?, Nothing, "", Nothing, Nothing"#,
            "E.InvalidByte(5)",
        ),
        (
            r#""s", Nothing, some("::1")?, Nothing, "", Nothing, Nothing"#,
            "E.InvalidByte(4)",
        ),
        (
            r#""s", Nothing, some("[::1")?, Nothing, "", Nothing, Nothing"#,
            "E.InvalidHost(4)",
        ),
        (
            r#""s", Nothing, some("h")?, Some(65536), "", Nothing, Nothing"#,
            "E.InvalidPort(6)",
        ),
        (
            r#""s", Nothing, some("h")?, Some(-1), "", Nothing, Nothing"#,
            "E.InvalidPort(6)",
        ),
        (
            r#""s", Nothing, some("h")?, Nothing, "p", Nothing, Nothing"#,
            "E.InvalidByte(5)",
        ),
        (
            r#""s", Nothing, Nothing, Nothing, "//h/p", Nothing, Nothing"#,
            "E.InvalidByte(3)",
        ),
        (
            r#""s", Nothing, Nothing, Nothing, "/a?b", Nothing, Nothing"#,
            "E.InvalidByte(4)",
        ),
        (
            r#""s", Nothing, Nothing, Nothing, "a b", Nothing, Nothing"#,
            "E.InvalidByte(3)",
        ),
        (
            r#""s", Nothing, Nothing, Nothing, "a%2", Nothing, Nothing"#,
            "E.InvalidEscape(3)",
        ),
        (
            r#""s", Nothing, Nothing, Nothing, "", some("a#b")?, Nothing"#,
            "E.InvalidByte(4)",
        ),
        (
            r#""s", Nothing, Nothing, Nothing, "", Nothing, some("a#b")?"#,
            "E.InvalidByte(4)",
        ),
        (
            r#""s", Nothing, Nothing, Nothing, "", Nothing, some("ü")?"#,
            "E.InvalidByte(3)",
        ),
    ];
    let mut body = String::new();
    let mut expected = String::new();
    for (arguments, line) in cases {
        body.push_str(&format!("attempt({arguments})?\n"));
        expected.push_str(line);
        expected.push('\n');
    }
    let source = format!(
        "{HEADER}{HELPERS}{}\ndef main() -> Result[(), Failure]:\n{}    Ok(())\n",
        r#"
def attempt(scheme: &str, userinfo: Option[str], host: Option[str], port: Option[i64], path: &str, query: Option[str], fragment: Option[str]) -> Result[(), Failure]:
    url = Url("".concat(scheme)?, userinfo, host, port, "".concat(path)?, query, fragment)
    match url.format():
        case Ok(text):
            print(text, std.url.parse(&text, 200)?.format()? == text)?
        case Err(error):
            print(str.repr(&error)?)?
    Ok(())
"#,
        body.lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>()
    );
    assert_eq!(output(&source), expected.trim_end());
}

#[test]
fn the_length_limit_is_checked_before_parsing() {
    native(
        r#"
show("s://h/abc", 9)?
show("s://h/abc", 8)?
show("s://h/abc", 0)?
show("s://h/abc", -1)?
show("", 0)?
show("s:", 2)?
show("s:", 1)?
show("s:é", 2)?
show("s:é", 3)?
show("s:é", 4)?
show("not a url at all", 3)?
"#,
        r#"
ok True s - +h -1 /abc - -
err E.LimitExceeded
err E.LimitExceeded
err E.InvalidLimit
err E.MissingScheme
ok True s - - -1  - -
err E.LimitExceeded
err E.LimitExceeded
err E.LimitExceeded
err E.InvalidByte(2)
err E.LimitExceeded
"#,
    );
}

#[test]
fn long_urls_roundtrip() {
    let path = "/segment%20x".repeat(200);
    let query = "k=v&".repeat(150);
    let url = format!("https://user@host.example:8443{path}?{query}#end");
    let lines = parse_all(std::slice::from_ref(&url));
    assert_eq!(lines[0], reference(&url).unwrap());
    let over = format!("s:{}", "a".repeat(4095));
    assert_eq!(parse_all(&[over])[0], "err E.LimitExceeded");
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn pick<'a>(&mut self, choices: &[&'a str]) -> &'a str {
        choices[self.below(choices.len())]
    }

    fn text(&mut self, alphabet: &[char], longest: usize) -> String {
        let count = self.below(longest + 1);
        (0..count)
            .map(|_| alphabet[self.below(alphabet.len())])
            .collect()
    }
}

/// Mostly well-formed URLs, some with random edits, so both acceptance and
/// every rejection path are exercised.
fn fuzz_inputs(seed: u64, count: usize) -> Vec<String> {
    let mut random = Random(seed);
    let noise: Vec<char> = ":/?#[]@%!$&'()*+,;=-._~ a1FgZ9\"<>\\^`{|}é\u{0}\t\u{7f}日"
        .chars()
        .collect();
    let address: Vec<char> = "0123456789abcdefF:::...".chars().collect();
    let component: Vec<char> = "abzAZ059-._~!$&'()*+,;=:@/?%41%7e%2F".chars().collect();
    let mut inputs = Vec::new();
    for _ in 0..count {
        let mut url = String::new();
        url.push_str(random.pick(&["http", "s", "a+b-c.d", "X9", "urn", "file", "", "1a"]));
        url.push_str(random.pick(&[":", ":", ":", ":", ":", ":", ":", ""]));
        if random.below(4) > 0 {
            url.push_str(random.pick(&["//", "//", "//", "/", ""]));
            if random.below(3) == 0 {
                url.push_str(&random.text(&component, 6).replace(['/', '?'], ""));
                url.push_str(random.pick(&["@", "@", "@", "@@", ""]));
            }
            match random.below(8) {
                0 => url.push_str(&format!("[{}]", random.text(&address, 24))),
                1 => {
                    let groups = random.below(10);
                    let mut parts: Vec<String> = (0..groups)
                        .map(|_| format!("{:x}", random.below(0x1_0000)))
                        .collect();
                    if random.below(2) == 0 && !parts.is_empty() {
                        let at = random.below(parts.len());
                        parts[at] = String::new();
                    }
                    if random.below(4) == 0 {
                        parts.push(format!(
                            "{}.{}.{}.{}",
                            random.below(300),
                            random.below(260),
                            random.pick(&["0", "00", "7", "255", "256"]),
                            random.below(256)
                        ));
                    }
                    url.push_str(&format!(
                        "[{}{}{}]",
                        random.pick(&["", "", ":", "::"]),
                        parts.join(":"),
                        random.pick(&["", "", ":", "::"])
                    ));
                }
                2 => url.push_str(random.pick(&[
                    "[v1.x]",
                    "[vF.a:b]",
                    "[v.x]",
                    "[v1.]",
                    "[V7.-]",
                    "[vg.x]",
                    "[v1.a%41]",
                    "[::1",
                    "::1]",
                    "[]",
                ])),
                _ => url.push_str(&random.text(&component, 10).replace(['/', '?', '@'], "")),
            }
            if random.below(3) == 0 {
                url.push(':');
                url.push_str(random.pick(&[
                    "80", "0", "65535", "65536", "", "080", "8a", "99999", "100000", "1",
                ]));
            }
        }
        if random.below(4) > 0 {
            url.push_str(random.pick(&["/", "/", "", "//"]));
            url.push_str(&random.text(&component, 12).replace('?', ""));
        }
        if random.below(3) == 0 {
            url.push('?');
            url.push_str(&random.text(&component, 10));
        }
        if random.below(3) == 0 {
            url.push('#');
            url.push_str(&random.text(&component, 10));
        }
        for _ in 0..[0, 0, 0, 0, 0, 0, 1, 2, 5][random.below(9)] {
            let mut letters: Vec<char> = url.chars().collect();
            let at = random.below(letters.len() + 1);
            match random.below(3) {
                0 => letters.insert(at, noise[random.below(noise.len())]),
                1 if at < letters.len() => {
                    letters.remove(at);
                }
                _ if at < letters.len() => letters[at] = noise[random.below(noise.len())],
                _ => {}
            }
            url = letters.into_iter().collect();
        }
        inputs.push(url);
    }
    inputs
}

#[test]
fn fuzzed_urls_agree_with_an_independent_reference() {
    let inputs = fuzz_inputs(0x9E37_79B9_7F4A_7C15, 6000);
    let lines = parse_all(&inputs);
    let mut accepted = 0;
    for (input, line) in inputs.iter().zip(&lines) {
        match reference(input) {
            Some(expected) => {
                accepted += 1;
                assert_eq!(*line, expected, "{input:?}");
            }
            None => assert!(line.starts_with("err E."), "{input:?} => {line}"),
        }
    }
    // The generator must keep exercising both outcomes.
    assert!(accepted > 600 && accepted < 5400, "{accepted}");
}

#[test]
fn arbitrary_text_never_traps_the_parser_or_decoder() {
    let mut random = Random(0x0123_4567_89AB_CDEF);
    let noise: Vec<char> = ":/?#[]@%%%0aF.s \u{0}é日\u{10FFFF}".chars().collect();
    let inputs: Vec<String> = (0..2000).map(|_| random.text(&noise, 24)).collect();
    let lines = parse_all(&inputs);
    for (input, line) in inputs.iter().zip(&lines) {
        assert_eq!(
            line.starts_with("ok"),
            reference(input).is_some(),
            "{input:?}"
        );
    }
    let mut blob = String::new();
    let mut expected = String::new();
    for input in &inputs {
        blob.push_str(&hex(input.as_bytes()));
        blob.push_str("ff");
        expected.push_str(
            if written(
                &input.replace(
                    |letter: char| letter != '%' && !letter.is_ascii_hexdigit(),
                    "x",
                ),
                "",
            ) {
                "True\n"
            } else {
                "False\n"
            },
        );
    }
    // Decoding succeeds exactly when every `%` starts a well-formed escape,
    // and re-encoding the decoded bytes then decodes to the same bytes.
    let body = format!(
        r#"
blob = std.encoding.hex_from_text("{blob}")?
mut start = 0
while start < blob.len():
    stop = blob.find(255u8, start, blob.len())?.unwrap()
    match std.url.decode_bytes(&blob, start, stop):
        case Ok(decoded):
            coded = std.url.encode_bytes(&decoded, 0, decoded.len(), Component.QueryPart)?
            print(std.url.decode_bytes(&coded, 0, coded.len())? == decoded)?
        case Err(_):
            print(False)?
    start = stop + 1
"#
    );
    native(&body, &expected);
}

#[test]
fn a_url_borrow_blocks_mutating_its_owner() {
    let error = rejected(
        r#"
import std.url

def main() -> Result[(), Failure]:
    mut url = std.url.parse("s://h/p", 64)?
    path = &url.path
    url.path = "/other"
    print(path)?
    Ok(())
"#,
    );
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn urls_parse_in_worker_threads() {
    let source = format!(
        "{HEADER}{}",
        r#"
def work(text: str) -> ():
    url = std.url.parse(&text, 256).unwrap()
    print(url.format().unwrap() == text, std.url.encode("a b", Component.Segment).unwrap()).unwrap()

def main() -> Result[(), Failure]:
    with spawn(work, "https://example.com/a?b=c")?:
        pass
    Ok(())
"#
    );
    assert_eq!(output(&source), "True a%20b");
}

/// Runs `call` with the allocator failing at each successive allocation
/// until it succeeds. Every earlier attempt must report `Allocation`, and
/// the exit-time balance check proves each failure released what it held.
#[cfg(feature = "runtime-checks")]
fn fails_cleanly_at_every_allocation(setup: &str, call: &str) {
    let attempts = 80;
    let mut body = String::from(setup.trim_matches('\n'));
    body.push('\n');
    for attempt in 0..attempts {
        body.push_str(&format!(
            "print(\"__test_fail_allocations_after_{attempt}__\")?\n\
             r{attempt} = {call}\n\
             print(\"__test_restore_allocations__\")?\n\
             print(tag(&r{attempt}))?\n"
        ));
    }
    let result = output(&program(&body));
    let tags: Vec<&str> = result.lines().collect();
    assert_eq!(tags.len(), attempts, "{call}");
    let failures = tags.iter().take_while(|tag| **tag == "alloc").count();
    assert!(failures > 0 && failures < attempts, "{call}: {tags:?}");
    assert!(
        tags[failures..].iter().all(|tag| *tag == "ok"),
        "{call}: {tags:?}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_at_every_point_without_leaking() {
    let setup = r#"
text = "https://a-user-name-too-long-for-inline-storage:and-a-password@a-host-name-too-long-for-inline-storage.example:8443/a/path/too/long/for/inline/string/storage?a-query-too-long-for-inline-string-storage=1#a-fragment-too-long-for-inline-string-storage"
url = std.url.parse(&text, 4096)?
query = "first-key-too-long-for-inline-storage=first%20value-too-long-for-inline-storage&second-key-too-long-for-inline-storage=second%20value-too-long-for-inline-storage"
pairs = std.url.query_pairs(&query)?
raw = std.bytes.from_text(&query)?
"#;
    for call in [
        "std.url.parse(&text, 4096)",
        "url.format()",
        "std.url.encode(&query, Component.QueryPart)",
        "std.url.decode(&query)",
        "std.url.encode_bytes(&raw, 0, raw.len(), Component.Path)",
        "std.url.decode_bytes(&raw, 0, raw.len())",
        "std.url.query_pairs(&query)",
        "std.url.query_text(&pairs)",
    ] {
        fails_cleanly_at_every_allocation(setup, call);
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn rejected_input_releases_everything_it_allocated() {
    // Exit-time allocation accounting fails the program on a leak.
    native(
        r#"
a = std.url.parse("https://a-host-name-too-long-for-inline-storage.example/a/path/too/long/for/inline/storage#bad#fragment", 4096)
b = std.url.parse("https://a-host-name-too-long-for-inline-storage.example:99999/", 4096)
c = std.url.query_pairs("first-key-too-long-for-inline-storage=value-too-long-for-inline-storage&second-key-too-long-for-inline-storage=%zz")
d = std.url.decode("a-text-too-long-for-inline-string-storage%ff")
print(tag(&a), tag(&b), tag(&c), tag(&d))?
"#,
        "other other other other",
    );
}
