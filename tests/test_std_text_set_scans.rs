//! `std.text.find_in` and `find_not_in` scan text for a byte set in one native
//! call, reporting scalar positions; only ASCII scalars can be members
//! (issue #383).
mod support;

/// Every ASCII scalar in order, then non-ASCII scalars of each encoded width.
const NON_ASCII: &[char] = &['é', '€', '😀', '\u{80}', '\u{ff}'];

fn sweep_text() -> Vec<char> {
    (0..128u8)
        .map(char::from)
        .chain(NON_ASCII.iter().copied())
        .collect()
}

/// The scalar position of the first scalar whose membership is `member`.
fn model(text: &[char], set: &[u8], member: bool) -> String {
    let found = text
        .iter()
        .position(|&c| (c.is_ascii() && set.contains(&(c as u8))) == member);
    match found {
        Some(index) => format!("Option[i64].Some({index})"),
        None => "Option[i64].Nothing".into(),
    }
}

const SWEEP: &str = r#"
import std.bytes
import std.text
from std.bytes import Bytes, BytesError

def report(text: &str, set: &Bytes) -> Result[(), AllocError | IoError]:
    print(std.text.find_in(text, set), std.text.find_not_in(text, set))?
    Ok(())

def main() -> Result[(), AllocError | IoError | BytesError]:
    mut ascii = std.bytes.empty()?
    for code in range(128):
        ascii.push(u8(code))?
    ascii.extend(&std.bytes.from_text("é€😀")?)?
    # U+0080, the first two-byte scalar.
    ascii.push(194u8)?
    ascii.push(128u8)?
    ascii.extend(&std.bytes.from_text("ÿ")?)?
    text = ascii.to_text()?
    prefixed = "é€😀".concat(&text)?
    for code in range(256):
        mut single = std.bytes.empty()?
        single.push(u8(code))?
        mut others = std.bytes.empty()?
        for other in range(256):
            if other != code:
                others.push(u8(other))?
        report(&text, &single)?
        report(&text, &others)?
        report(&prefixed, &single)?
    Ok(())
"#;

/// Each byte value alone and its complement, over every ASCII scalar and
/// non-ASCII scalars of two, three, and four bytes, with and without a
/// non-ASCII prefix: every class boundary, and set bytes from 0x80 never match.
#[test]
fn scans_match_a_scalar_model_at_every_byte_boundary() {
    let text = sweep_text();
    let prefixed: Vec<char> = "é€😀".chars().chain(text.iter().copied()).collect();
    let mut expected = String::new();
    for code in 0..=255u8 {
        let single = [code];
        let others: Vec<u8> = (0..=255u8).filter(|&b| b != code).collect();
        for (text, set) in [
            (&text, &single[..]),
            (&text, &others[..]),
            (&prefixed, &single[..]),
        ] {
            expected.push_str(&format!(
                "{} {}\n",
                model(text, set, true),
                model(text, set, false)
            ));
        }
    }
    let stdout = support::run_stdout(SWEEP);
    for (index, (found, wanted)) in stdout.lines().zip(expected.lines()).enumerate() {
        assert_eq!(found, wanted, "byte {} case {}", index / 3, index % 3);
    }
    assert_eq!(stdout.lines().count(), expected.lines().count());
}

/// Literal sets, empty text and sets, inline and heap text, and non-ASCII set
/// bytes that spell a scalar's encoding.
#[test]
fn literal_sets_and_edge_cases() {
    support::assert_stdout(
        r#"
import std.bytes
import std.text
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    empty = std.bytes.empty()?
    print(std.text.find_in("", b"abc"), std.text.find_not_in("", b"abc"))?
    print(std.text.find_in("abc", b""), std.text.find_not_in("abc", b""))?
    print(std.text.find_in("abc", &empty), std.text.find_not_in("abc", &empty))?
    print(std.text.find_in("ab;c", b";"), std.text.find_not_in("ab;c", b"abc"))?
    print(std.text.find_in("é;", b"\xc3\xa9;"), std.text.find_not_in("é", b"\xc3\xa9"))?
    long = "a heap string, long enough to leave the inline form; then €;"
    print(std.text.find_in(long, b";"), std.text.find_not_in(long, b"abcdefghijklmnopqrstuvwxyz ,"))?
    print(std.text.find_in("€€x", b"x"), std.text.find_not_in("xx€", b"x"))?
    Ok(())
"#,
        "Option[i64].Nothing Option[i64].Nothing\n\
         Option[i64].Nothing Option[i64].Some(0)\n\
         Option[i64].Nothing Option[i64].Some(0)\n\
         Option[i64].Some(2) Option[i64].Some(2)\n\
         Option[i64].Some(1) Option[i64].Some(0)\n\
         Option[i64].Some(51) Option[i64].Some(51)\n\
         Option[i64].Some(2) Option[i64].Some(2)\n",
    );
}

/// A set must be a borrowable place, as for the `Bytes` scans.
#[test]
fn set_operand_must_be_a_byte_source_place() {
    let error = support::check_error(
        "import std.text\ndef main() -> ():\n    std.text.find_in(\"a\", 3)\n    pass\n",
    );
    assert!(error.contains("ByteSource"), "{error}");
}

/// Scans allocate nothing: they succeed with every allocation failing, and the
/// runtime checks the reference-count balance at exit.
#[cfg(feature = "runtime-checks")]
#[test]
fn scans_allocate_nothing() {
    support::assert_visible_stdout(
        r#"
import std.bytes
import std.text
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    set = std.bytes.from_text(";=")?
    long = "a heap string, long enough to leave the inline form; then €;".concat("=")?
    print("__test_fail_allocations_after_0__")?
    a = std.text.find_in(&long, &set)
    b = std.text.find_not_in(&long, b"abcdefghijklmnopqrstuvwxyz ,")
    c = std.text.find_in("é;", &set)
    print("__test_restore_allocations__")?
    print(a, b, c)?
    Ok(())
"#,
        "Option[i64].Some(51) Option[i64].Some(51) Option[i64].Some(1)",
    );
}

/// The scan is one call to the narrow runtime helper: no dispatcher and no
/// retain or release of the text or set.
#[test]
fn scans_call_only_the_narrow_helper() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.plenty"),
        "import std.http.cookie\nimport std.text\nfrom std.bytes import Bytes\n\
         def main() -> Result[(), AllocError | IoError | std.http.cookie.CookieError]:\n    \
         print(std.text.find_not_in(\"ab\", b\"a\"))?\n    \
         keys = list[Bytes].new()?\n    \
         print(std.http.cookie.verify(&keys, \"sid\", \"v\")?)?\n    \
         Ok(())\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args([
            "--release",
            "--dump",
            "clif",
            "--dump-std",
            "--dump-filter",
            "std.",
            "main.plenty",
            "-o",
            "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let clif = String::from_utf8(output.stdout).unwrap();
    let functions: Vec<&str> = clif.split("\n==== ").collect();
    let scans: Vec<&&str> = functions
        .iter()
        .filter(|f| f.starts_with("std.text.find_not_in["))
        .collect();
    assert!(!scans.is_empty(), "no find_not_in instance");
    for body in scans {
        let calls: Vec<&str> = body
            .lines()
            .filter(|line| line.trim_start().starts_with("fn") && line.contains(" = "))
            .filter_map(|line| line.rsplit("; ").next())
            .collect();
        assert_eq!(
            calls,
            ["plenty_text_find_in", "__plenty_fn_std.text.located"],
            "{body}"
        );
    }
    let all_bytes = functions
        .iter()
        .find(|f| f.starts_with("std.http.cookie.all_bytes "))
        .expect("all_bytes missing");
    for absent in ["plenty_op_", "plenty_retain", "isalpha", "token_byte"] {
        assert!(!all_bytes.contains(absent), "{all_bytes}");
    }
}

const VALIDATORS: &str = r#"
import std.http.cookie
import std.http.server

enum Route:
    Home

def mark[T, E](result: Result[T, E]) -> str:
    match result:
        case Ok(_):
            "+"
        case Err(_):
            "-"

def scalar(index: i64) -> str:
    codes = [233u32, 8364u32, 128512u32, 128u32, 255u32].unwrap()
    if index < 128:
        return chr(u32(index)).unwrap()
    chr(codes[index - 128]).unwrap()

def main() -> Result[(), std.http.cookie.CookieError | std.http.server.ServerError | AllocError | IoError]:
    mut names = ""
    mut values = ""
    mut paths = ""
    mut domains = ""
    mut methods = ""
    mut routes = ""
    for index in range(133):
        c = scalar(index)
        names = names.concat(mark(std.http.cookie.set_cookie(&"a".concat(&c)?.concat("b")?, "v")))?
        values = values.concat(mark(std.http.cookie.set_cookie("a", &"v".concat(&c)?.concat("w")?)))?
        mut cookie = std.http.cookie.set_cookie("a", "v")?
        cookie.path = "/".concat(&c)?
        paths = paths.concat(mark(cookie.format()))?
        cookie.path = "/"
        cookie.domain = "a".concat(&c)?.concat("b")?
        domains = domains.concat(mark(cookie.format()))?
        mut table = std.http.server.router[Route]()?
        methods = methods.concat(mark(table.add(&"G".concat(&c)?.concat("T")?, "/", Route.Home)))?
        routes = routes.concat(mark(table.add("GET", &"/{a".concat(&c)?.concat("b}")?, Route.Home)))?
    print(names)?
    print(values)?
    print(paths)?
    print(domains)?
    print(methods)?
    print(routes)?
    Ok(())
"#;

/// The std.http validators converted to set scans accept exactly their old
/// ASCII sets, and reject every non-ASCII scalar.
#[test]
fn http_validators_accept_exactly_their_sets() {
    let line = |accept: &dyn Fn(u8) -> bool| -> String {
        (0..128u8)
            .map(|b| if accept(b) { '+' } else { '-' })
            .chain("-----".chars())
            .collect()
    };
    let tchar = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b);
    let expected = [
        line(&tchar),
        line(&|b| (b'!'..=b'~').contains(&b) && !b"\",;\\".contains(&b)),
        line(&|b| (b' '..=b'~').contains(&b) && b != b';'),
        line(&|b| b.is_ascii_alphanumeric() || b"-.".contains(&b)),
        line(&|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)),
        line(&|b| b.is_ascii_alphanumeric() || b == b'_'),
    ]
    .join("\n");
    assert_eq!(support::run_stdout(VALIDATORS).trim_end(), expected);
}
