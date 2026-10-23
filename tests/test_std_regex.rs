//! `std.regex`: a Pike VM in Plenty, checked against the `regex` crate on a
//! shared syntax subset, plus syntax errors, limits, linear-time bounds,
//! bytes input, worker threads, and allocation failure.
mod support;
use support::build_file as build;

const HEADER: &str = r#"import std.bytes
import std.regex
from std.bytes import Bytes
from std.regex import Regex, RegexError, Limits, Limit, Match

def span(start: i64, end: i64) -> Result[str, AllocError | IoError]:
    parts = [str.repr(start)?, str.repr(end)?]?
    Ok("-".join(parts)?)

def spans(found: &list[Match]) -> Result[str, AllocError | IoError]:
    mut parts = list[str].new()?
    for index in range(len(found)):
        parts.append(span(found[index].start, found[index].end)?)?
    Ok(",".join(parts)?)

# Every match, then the groups of the first match.
def report(regex: &Regex, text: &str) -> Result[str, AllocError | IoError | std.regex.RegexError]:
    found = regex.find_all(text, 1000)?
    mut groups = list[str].new()?
    match regex.captures(text)?:
        case Some(caps):
            for index in range(caps.len()):
                match caps.get(index):
                    case Some(item):
                        groups.append(span(item.start, item.end)?)?
                    case Nothing:
                        groups.append("_")?
        case Nothing:
            groups.append("none")?
    parts = [spans(&found)?, " ".join(groups)?]?
    Ok(" | ".join(parts)?)

def check(pattern: &str, text: &str) -> Result[str, AllocError | IoError | std.regex.RegexError]:
    limits = std.regex.limits()
    match std.regex.compile(pattern, &limits):
        case Ok(regex):
            report(&regex, text)
        case Err(error):
            Ok(str.repr(error)?)

def shown[T](result: Result[T, RegexError]) -> Result[str, AllocError | IoError]:
    match result:
        case Ok(_):
            Ok("ok")
        case Err(error):
            Ok(str.repr(error)?)

def repeated(text: &str, count: i64) -> Result[str, AllocError | IoError]:
    mut parts = list[str].new()?
    for _ in range(count):
        parts.append(copy(text)?)?
    Ok("".join(parts)?)
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}def main() -> Result[(), AllocError | IoError | std.regex.RegexError | std.bytes.BytesError]:\n{body}    Ok(())\n"
    )
}

fn output(source: &str) -> String {
    let (_dir, executable) = build(source);
    run_output(&executable, &[], source)
}

/// Runs a built program with `args` and returns its stdout without test
/// markers. `source` only labels a failure.
fn run_output(executable: &std::path::Path, args: &[&str], source: &str) -> String {
    let output = std::process::Command::new(executable)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}\n{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.regex.RegexError.", "")
        .replace("std.regex.SyntaxError", "SyntaxError")
        .replace("std.regex.Limit.", "")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

/// A Plenty string literal.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Patterns with the `regex` crate spelling where it differs: Perl classes
/// and word boundaries are ASCII-only here, and properties use other names.
const PATTERNS: &[(&str, &str)] = &[
    ("a", ""),
    ("abc", ""),
    ("", ""),
    ("a|ab", ""),
    ("ab|a", ""),
    ("a*", ""),
    ("a+", ""),
    ("a?", ""),
    ("a*?", ""),
    ("a+?", ""),
    ("a??", ""),
    ("a{2}", ""),
    ("a{2,}", ""),
    ("a{2,3}", ""),
    ("a{2,3}?", ""),
    ("a{0}b", ""),
    ("x*", ""),
    ("(a)(b)?", ""),
    ("(a|b)*c", ""),
    ("(a*)*", ""),
    ("(a*)+b", ""),
    ("(a|ab)(c|bcd)(d*)", ""),
    ("((a)|(b))+", ""),
    ("(a+)(b+)?", ""),
    ("(?:ab)+", ""),
    ("(?:a|b)*?b", ""),
    ("(a?){3}a{3}", ""),
    ("^a", ""),
    ("a$", ""),
    ("^$", ""),
    ("^", ""),
    ("$", ""),
    ("(?m)^a", ""),
    ("(?m)c$", ""),
    ("(?m)^$", ""),
    ("(?m)^\\w+$", "(?m)^[0-9A-Za-z_]+$"),
    (".", ""),
    (".+", ""),
    ("(?s).+", ""),
    ("(?s)a.b", ""),
    ("..", ""),
    ("[a-c]+", ""),
    ("[^a-c]+", ""),
    ("[abc-]+", ""),
    ("[-a]", ""),
    ("[\\]x]+", ""),
    ("\\.", ""),
    ("\\x41|\\x{e9}", ""),
    ("[\\t\\n ]+", ""),
    ("(?i)abc", ""),
    ("(?i)σ", ""),
    ("(?i)k", ""),
    ("(?i)[a-z]+", ""),
    ("(?i)[^k]", ""),
    ("(?i:a)b", ""),
    ("a(?i)b", ""),
    ("(?i)a(?-i)b", ""),
    ("(?i)é", ""),
    ("\\d+", "[0-9]+"),
    ("\\w+", "[0-9A-Za-z_]+"),
    ("\\s+", "[\\t\\n\\x0B\\x0C\\r ]+"),
    ("\\D+", "[^0-9]+"),
    ("\\W+", "[^0-9A-Za-z_]+"),
    ("[\\d_]+", "[0-9_]+"),
    ("\\bfoo\\b", "(?-u:\\b)foo(?-u:\\b)"),
    ("\\Bo\\B", "(?-u:\\B)o(?-u:\\B)"),
    ("\\b", "(?-u:\\b)"),
    ("\\Afoo", ""),
    ("foo\\z", ""),
    ("(?P<word>[a-z]+)=(?P<num>[0-9]+)", ""),
    ("(?<x>a)|(?<y>b)", ""),
    ("(a)|b", ""),
    ("\\p{Lowercase}+", ""),
    ("\\p{Uppercase}+", ""),
    ("\\P{Alphabetic}+", ""),
    ("\\p{Numeric}", "\\p{N}"),
    ("[\\p{Uppercase}0-9]+", ""),
    ("\\p{White_Space}+", ""),
    ("\\p{Mark}", "\\p{M}"),
    ("\\p{Control}", "\\p{Cc}"),
    ("\\p{Any}", ""),
    ("é+", ""),
    ("[é-ü]+", ""),
    ("ü|é", ""),
];

const HAYSTACKS: &[&str] = &[
    "",
    "a",
    "ab",
    "abc",
    "aaa",
    "baaab",
    "xaxbxc",
    "foo bar foo",
    "aab abb",
    "AbC abc",
    "x=1 yy=22",
    "line1\nline2\nc\n",
    "\n",
    "café über",
    "ΣσςK k",
    "a.b-c]x",
    "12 ab_3\t!",
    "abcbcd",
    "Ⅻ ٣ 7",
    "e\u{301}\u{7}A",
];

fn expected(pattern: &str, text: &str) -> String {
    let regex = regex::Regex::new(pattern).unwrap_or_else(|error| panic!("{pattern}: {error}"));
    let spans = regex
        .find_iter(text)
        .map(|m| format!("{}-{}", m.start(), m.end()))
        .collect::<Vec<_>>()
        .join(",");
    let groups = match regex.captures(text) {
        Some(caps) => (0..caps.len())
            .map(|index| {
                caps.get(index)
                    .map_or("_".to_owned(), |m| format!("{}-{}", m.start(), m.end()))
            })
            .collect::<Vec<_>>()
            .join(" "),
        None => "none".to_owned(),
    };
    format!("{spans} | {groups}")
}

#[test]
fn matches_agree_with_the_regex_crate() {
    let patterns = PATTERNS
        .iter()
        .map(|(pattern, _)| quoted(pattern))
        .collect::<Vec<_>>()
        .join(", ");
    let haystacks = HAYSTACKS
        .iter()
        .map(|text| quoted(text))
        .collect::<Vec<_>>()
        .join(", ");
    let body = format!(
        "patterns = [{patterns}]?\n\
         haystacks = [{haystacks}]?\n\
         for p in range(len(patterns)):\n\
         \x20   for h in range(len(haystacks)):\n\
         \x20       print(check(&patterns[p], &haystacks[h])?)?\n"
    );
    let actual = output(&program(&body));
    let mut lines = actual.lines();
    let mut failures = Vec::new();
    for (pattern, rust) in PATTERNS {
        let rust = if rust.is_empty() { pattern } else { rust };
        for text in HAYSTACKS {
            let line = lines.next().unwrap_or("<missing>");
            let want = expected(rust, text);
            if line != want {
                failures.push(format!("{pattern:?} on {text:?}: got {line}, want {want}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn syntax_errors_report_the_byte_offset() {
    let cases = [
        "ab)",
        "(ab",
        "a**",
        "a{2}{3}",
        "*a",
        "a|+",
        "{2}",
        "a{3,2}",
        "a{x}",
        "a{2",
        "[]a]",
        "[a",
        "[[:alpha:]]",
        "[z-a]",
        "[\\d-z]",
        "[\\b]",
        "\\1",
        "\\q",
        "\\",
        "\\x4",
        "\\x{110000}",
        "\\x{d800}",
        "\\p{Greek}",
        "\\pL",
        "é(?=a)",
        "(?<!a)",
        "(?P<1x>a)",
        "(?<a>x)(?<a>y)",
        "(?x)a",
        "(?)",
        "(?i-m-s)",
        "(?i",
        "(?",
        "(?-",
    ];
    let body = cases
        .iter()
        .map(|pattern| format!("print(check({}, \"\")?)?\n", quoted(pattern)))
        .collect::<String>();
    let expected = r#"
Syntax(SyntaxError(offset=2, message="unopened group"))
Syntax(SyntaxError(offset=0, message="unclosed group"))
Syntax(SyntaxError(offset=2, message="repetition of a repetition; use a group"))
Syntax(SyntaxError(offset=4, message="repetition of a repetition; use a group"))
Syntax(SyntaxError(offset=0, message="repetition operator missing expression"))
Syntax(SyntaxError(offset=2, message="repetition operator missing expression"))
Syntax(SyntaxError(offset=0, message="repetition operator missing expression"))
Syntax(SyntaxError(offset=1, message="invalid repetition; the minimum exceeds the maximum"))
Syntax(SyntaxError(offset=1, message="invalid repetition; expected `{n}`, `{n,}`, or `{n,m}`"))
Syntax(SyntaxError(offset=1, message="invalid repetition; expected `{n}`, `{n,}`, or `{n,m}`"))
Syntax(SyntaxError(offset=1, message="empty class; write a literal `]` as `\\]`"))
Syntax(SyntaxError(offset=0, message="unclosed class"))
Syntax(SyntaxError(offset=1, message="nested class; write a literal `[` as `\\[`"))
Syntax(SyntaxError(offset=1, message="invalid range; the start exceeds the end"))
Syntax(SyntaxError(offset=3, message="invalid range; a range joins two scalars"))
Syntax(SyntaxError(offset=1, message="assertion inside a class"))
Syntax(SyntaxError(offset=0, message="backreferences are not supported"))
Syntax(SyntaxError(offset=0, message="unknown escape"))
Syntax(SyntaxError(offset=0, message="incomplete escape"))
Syntax(SyntaxError(offset=0, message="invalid hexadecimal escape; expected `\\xHH` or `\\x{H...}`"))
Syntax(SyntaxError(offset=0, message="escape is not a Unicode scalar value"))
Syntax(SyntaxError(offset=0, message="escape is not a Unicode scalar value"))
Syntax(SyntaxError(offset=0, message="unknown property; expected Alphabetic, Lowercase, Uppercase, White_Space, Numeric, Control, Mark, ASCII, or Any"))
Syntax(SyntaxError(offset=0, message="expected `{` after `\\p` or `\\P`"))
Syntax(SyntaxError(offset=2, message="look-around is not supported"))
Syntax(SyntaxError(offset=0, message="look-around is not supported"))
Syntax(SyntaxError(offset=0, message="invalid group name; expected `[A-Za-z_][A-Za-z0-9_]*`"))
Syntax(SyntaxError(offset=7, message="duplicate group name"))
Syntax(SyntaxError(offset=0, message="unknown flag; expected `i`, `m`, or `s`"))
Syntax(SyntaxError(offset=0, message="empty flag group"))
Syntax(SyntaxError(offset=0, message="repeated `-` in flags"))
Syntax(SyntaxError(offset=0, message="unclosed group"))
Syntax(SyntaxError(offset=0, message="unclosed group"))
Syntax(SyntaxError(offset=0, message="unclosed group"))
"#;
    native(&body, expected);
}

#[test]
fn captures_names_and_replacement_templates() {
    native(
        r#"
limits = std.regex.limits()
date = std.regex.compile("(?P<year>\\d{4})-(?P<month>\\d{2})(-(?P<day>\\d{2}))?", &limits)?
text = "from 2024-05 to 1999-12-31"
print(date.group_count(), date.group_index("month"), date.group_index("nope"))?
caps = date.captures(text)?.unwrap()
print(caps.len(), caps.name("year").unwrap().text(text)?, caps.name("day").is_some())?
later = date.captures_at(text, 8)?.unwrap()
print(later.name("day").unwrap().text(text)?, later.get(3).unwrap().text(text)?, later.get(9).is_some())?
print(date.replace(text, "${day}.${month}.$1", 100)?)?
print(date.replace_all(text, "<$0>$$${2}0", 100)?)?
print(shown(date.replace_all(text, "${month", 100))?)?
print(shown(date.replace_all(text, "$month", 100))?)?
print(shown(date.replace_all(text, "${week}", 100))?)?
print(shown(date.replace_all(text, "$5", 100))?)?
print(shown(date.replace_all(text, "$0$0", 42))?)?
print(date.replace_all(text, "$0$0", 43)?)?
words = std.regex.compile("\\s*,\\s*", &limits)?
parts = words.split("a , b,,c ,", 10)?
print(len(parts), "|".join(parts)?)?
print(shown(words.split("a,b,c", 2))?)?
print(len(words.split("", 1)?))?
empty = std.regex.compile("", &limits)?
print(empty.replace_all("añb", "-", 100)?)?
print("|".join(empty.split("añ", 10)?)?)?
"#,
        r#"
5 Option[i64].Some(2) Option[i64].Nothing
5 2024 False
31 -31 False
from .05.2024 to 1999-12-31
from <2024-05>$050 to <1999-12-31>$120
Syntax(SyntaxError(offset=0, message="unclosed `${`"))
Syntax(SyntaxError(offset=0, message="`$` must be followed by a group number, `{name}`, or `$`"))
Syntax(SyntaxError(offset=0, message="unknown group name"))
Syntax(SyntaxError(offset=0, message="unknown group"))
LimitExceeded(Output)
from 2024-052024-05 to 1999-12-311999-12-31
5 a|b||c|
LimitExceeded(Matches)
1
-a-ñ-b-
|a|ñ|
"#,
    );
}

#[test]
fn iteration_and_offsets() {
    native(
        r#"
limits = std.regex.limits()
text = "añ aa ñ"
any = std.regex.compile("a*", &limits)?
print(spans(&any.find_all(text, 100)?)?)?
mut cursor = any.cursor()?
mut count = 0
while True:
    match any.next_match(text, &mut cursor)?:
        case Some(item):
            count += 1
        case Nothing:
            break
print(count, any.next_match(text, &mut cursor)?.is_some())?
print(shown(any.find_all(text, 4))?)?
print(shown(any.find_at(text, 2))?)?
print(shown(any.find_at(text, 99))?)?
print(any.find_at(text, 3)?.unwrap().start)?
word = std.regex.compile("\\bñ", &limits)?
print(word.find(text)?.is_some())?
other = std.regex.compile("b", &limits)?
mut foreign = other.cursor()?
print(shown(any.next_match(text, &mut foreign))?)?
m = any.find_at(text, 4)?.unwrap()
print(m.start, m.end, m.len(), m.is_empty(), m.text(text)?)?
print(shown(Match(1, 2).text(text))?)?
print(shown(Match(0, 99).text(text))?)?
print(any.instructions() > 0)?
"#,
        r#"
0-1,3-3,4-6,7-7,9-9
5 False
LimitExceeded(Matches)
OutOfRange
OutOfRange
3
True
OutOfRange
4 6 2 False aa
OutOfRange
OutOfRange
True
"#,
    );
}

#[test]
fn unicode_properties_and_case_folding() {
    native(
        r#"
limits = std.regex.limits()
letters = std.regex.compile("\\p{Alphabetic}+", &limits)?
text = "Ωμέγα 42 ٣Ⅻx"
print(spans(&letters.find_all(text, 100)?)?)?
print(spans(&std.regex.compile("\\p{Numeric}+", &limits)?.find_all(text, 100)?)?)?
print(spans(&std.regex.compile("[\\p{Mark}\\p{ASCII}]+", &limits)?.find_all(text, 100)?)?)?
print(spans(&std.regex.compile("\\d+", &limits)?.find_all(text, 100)?)?)?
print(spans(&std.regex.compile("\\w+", &limits)?.find_all(text, 100)?)?)?
folded = std.regex.compile("(?i)ωμ[Έa]", &limits)?
print(folded.find("ΩΜέ")?.is_some(), folded.find("ωμΈ")?.is_some())?
print(std.regex.compile("(?i)ss", &limits)?.is_match("ß")?)?
print(std.regex.compile("(?i)\\x{212A}", &limits)?.is_match("k")?)?
"#,
        r#"
0-10,16-20
11-13,14-19
10-14,19-20
11-13
11-13,19-20
True True
False
True
"#,
    );
}

#[test]
fn bytes_input_decodes_strict_utf8() {
    native(
        r#"
limits = std.regex.limits()
mut data = std.bytes.from_text("aé")?
data.push(255u8)?
data.push(98u8)?
data.push(204u8)?
data.push(129u8)?
data.push(237u8)?
data.push(160u8)?
data.push(128u8)?
print(data.len())?
print(spans(&std.regex.compile(".", &limits)?.find_all_bytes(&data, 100)?)?)?
print(spans(&std.regex.compile("[^a]", &limits)?.find_all_bytes(&data, 100)?)?)?
print(spans(&std.regex.compile("\\p{Mark}", &limits)?.find_all_bytes(&data, 100)?)?)?
print(spans(&std.regex.compile("é\\P{Any}?b", &limits)?.find_all_bytes(&data, 100)?)?)?
e = std.regex.compile("é", &limits)?
print(e.is_match_bytes(&data)?, e.find_bytes(&data, 0)?.unwrap().start, e.find_bytes(&data, 3)?.is_some())?
print(shown(e.find_bytes(&data, 11))?)?
caps = std.regex.compile("(a)(é)", &limits)?.captures_bytes(&data, 0)?.unwrap()
print(caps.get(2).unwrap().bytes(&data)?.len())?
mut cursor = e.cursor()?
print(e.next_match_bytes(&data, &mut cursor)?.unwrap().end)?
"#,
        r#"
10
0-1,1-3,4-5,5-7
1-3,4-5,5-7
5-7

True 1 False
OutOfRange
2
3
"#,
    );
}

#[test]
fn limits_are_enforced() {
    native(
        r#"
base = std.regex.limits()
print(shown(std.regex.compile("abc", &Limits(2, 128, 1000, 100000, 67108864, 100000000)))?)?
print(shown(std.regex.compile("", &Limits(100, 0, 1000, 100000, 67108864, 100000000)))?)?
print(shown(std.regex.compile("a|b", &Limits(100, 0, 1000, 100000, 67108864, 100000000)))?)?
print(shown(std.regex.compile("(a)", &Limits(100, 0, 1000, 100000, 67108864, 100000000)))?)?
print(shown(std.regex.compile("(a)", &Limits(100, 1, 1000, 100000, 67108864, 100000000)))?)?
print(shown(std.regex.compile("((a))", &Limits(100, 1, 1000, 100000, 67108864, 100000000)))?)?
print(shown(std.regex.compile("((a))", &Limits(100, 2, 1000, 100000, 67108864, 100000000)))?)?
print(shown(std.regex.compile("a{1001}", &base))?)?
print(shown(std.regex.compile("a{99999999999999999999}", &base))?)?
print(shown(std.regex.compile("(a{1000}){1000}", &base))?)?
print(shown(std.regex.compile("abcdef", &Limits(100, 128, 1000, 8, 67108864, 100000000)))?)?
print(shown(std.regex.compile("[abc]", &Limits(100, 128, 1000, 2, 67108864, 100000000)))?)?
print(shown(std.regex.compile("(a)(b)(c)", &Limits(100, 128, 1000, 100, 1000, 100000000)))?)?
print(shown(std.regex.compile("a", &Limits(100, 1001, 1000, 100, 1000, 100000000)))?)?
print(shown(std.regex.compile("a", &Limits(-1, 128, 1000, 100, 1000, 100000000)))?)?
small = std.regex.compile("a+b", &Limits(100, 128, 1000, 100, 100000, 200))?
print(shown(small.find("aaaaaaaaaab"))?)?
print(shown(small.find(&repeated("a", 100)?))?)?
mut deep = list[str].new()?
for _ in range(1001):
    deep.append("(")?
print(shown(std.regex.compile(&"".join(deep)?, &base))?)?
"#,
        r#"
LimitExceeded(PatternBytes)
ok
ok
LimitExceeded(Nesting)
ok
LimitExceeded(Nesting)
ok
LimitExceeded(Repetition)
LimitExceeded(Repetition)
LimitExceeded(Instructions)
LimitExceeded(Instructions)
LimitExceeded(Instructions)
LimitExceeded(Memory)
InvalidLimits
InvalidLimits
ok
LimitExceeded(Work)
LimitExceeded(Nesting)
"#,
    );
}

/// Patterns that take exponential time in a backtracking matcher. Here the
/// work per input position is bounded by twice the program size (one set
/// insertion and one step per instruction), so a work limit of
/// `2 * instructions * (n + 1)` always suffices.
#[test]
fn pathological_patterns_run_in_linear_time() {
    let patterns = [
        "(a*)*b",
        "(x+x+)+y",
        "(a|aa)*c",
        "(a?){30}a{30}b",
        "(?:a*)*$x",
        "((a+)+)+!",
    ];
    let mut body = String::from(
        "for size in [10, 1000, 20000]?:\n    text = repeated(\"a\", size)?\n    x = repeated(\"x\", size)?\n",
    );
    for (i, pattern) in patterns.iter().enumerate() {
        body.push_str(&format!(
            "    probe{i} = std.regex.compile({p}, &std.regex.limits())?\n\
             \x20   work{i} = 2 * probe{i}.instructions() * (size + 1)\n\
             \x20   regex{i} = std.regex.compile({p}, &Limits(65536, 128, 1000, 100000, 67108864, work{i}))?\n\
             \x20   found{i} = regex{i}.find(&text)?.is_some() or regex{i}.find(&x)?.is_some()\n\
             \x20   print(found{i}, regex{i}.is_match(&text)?, regex{i}.captures(&text)?.is_some())?\n",
            p = quoted(pattern)
        ));
    }
    let started = std::time::Instant::now();
    let out = output(&program(&body));
    assert_eq!(out, ["False False False"; 18].join("\n"));
    // 20 000 bytes through six patterns, four searches each.
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}

#[test]
fn regexes_work_in_worker_threads() {
    let source = format!(
        r#"{HEADER}
def work(text: str) -> Result[(), AllocError | IoError | std.regex.RegexError]:
    regex = std.regex.compile("(?P<w>\\w+) (?P<x>\\w+)", &std.regex.limits())?
    print(regex.replace(&text, "${{x}} ${{w}}", 100)?)?
    Ok(())

def main() -> Result[(), AllocError | IoError | ThreadError]:
    with spawn(work, "thread worker")?:
        pass
    Ok(())
"#
    );
    assert_eq!(output(&source), "worker thread");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_without_leaks() {
    // Each allocation in compiling and matching fails in turn; every failure
    // is reported, and the exit-time balance check finds no leaks. One
    // program reads its budget marker from argv, so each budget is a run,
    // not a link (issue #409).
    let source = program(
        r#"
argv = args()?
limits = std.regex.limits()
text = "key=value; other=thing"
print(argv[1])?
result = run(&limits, text)
print("__test_restore_allocations__")?
print(result)?
"#,
    )
    .replace(
        "def main()",
        r#"def run(limits: &Limits, text: &str) -> str:
    match std.regex.compile("(?P<k>\\w+)=(?P<v>[a-z]+)", limits):
        case Ok(regex):
            match regex.replace_all(text, "${v}:${k}", 100):
                case Ok(out):
                    match regex.find_all(text, 10):
                        case Ok(found):
                            if len(found) == 2:
                                return out
                            return "wrong"
                        case Err(error):
                            return "find failed"
                case Err(error):
                    return "replace failed"
        case Err(error):
            return "compile failed"

def main()"#,
    );
    let (_dir, executable) = build(&source);
    let mut reported = 0;
    for after in 0..80 {
        let marker = format!("__test_fail_allocations_after_{after}__");
        let out = run_output(&executable, &[&marker], &source);
        if out == "value:key; thing:other" {
            assert!(after > 10, "{after}");
            break;
        }
        assert!(out.ends_with("failed"), "{after}: {out}");
        reported += 1;
    }
    assert!(reported > 10);
}

#[cfg(feature = "runtime-checks")]
#[test]
fn compile_allocation_failure_is_reported_as_allocation() {
    // Issue #258: the empty bytes buffer allocated last in compile reported
    // its allocation failure as `OutOfRange`. The budget comes from argv, as
    // above.
    let source = program(
        r#"
argv = args()?
limits = std.regex.limits()
print(argv[1])?
result = run(&limits)
print("__test_restore_allocations__")?
print(result)?
"#,
    )
    .replace(
        "def main()",
        r#"def run(limits: &Limits) -> str:
    match std.regex.compile("(?P<k>\\w+)=(?P<v>[a-z]+)", limits):
        case Ok(regex):
            return "compiled"
        case Err(error):
            match error:
                case RegexError.Allocation(cause):
                    return "allocation"
                case _:
                    return "other"

def main()"#,
    );
    let (_dir, executable) = build(&source);
    let mut reported = 0;
    for after in 0..80 {
        let marker = format!("__test_fail_allocations_after_{after}__");
        let out = run_output(&executable, &[&marker], &source);
        if out == "compiled" {
            break;
        }
        assert_eq!(out, "allocation", "{after}");
        reported += 1;
    }
    assert!(reported > 10);
}
