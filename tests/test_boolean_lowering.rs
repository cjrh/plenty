//! Boolean short circuits and boolean `match` lower to direct branches on the
//! `bool` value, with no re-test of an already decided comparison and no
//! impossible-case trap after the last boolean arm (issue #362).
mod support;

const PROGRAM: &str = r#"import std.fmt
class Log:
    pub calls: i64

def note(log: &mut Log, value: bool) -> bool:
    log.calls += 1
    value

def digit(code: i64) -> bool:
    code >= 48 and code <= 57

def token_byte(code: i64) -> bool:
    if digit(code) or (code >= 65 and code <= 90) or (code >= 97 and code <= 122):
        return True
    code == 33 or (code >= 35 and code <= 39) or code == 42 or code == 43 or code == 45 or code == 46 or code == 94 or code == 95 or code == 96 or code == 124 or code == 126

def name(value: bool) -> str:
    match value:
        case True:
            "yes"
        case False:
            "no"

def flipped(value: bool) -> i64:
    match value:
        case False:
            return 0
        case True:
            return 1

def wild_true(value: bool) -> i64:
    match value:
        case True:
            10
        case _:
            20

def wild_false(value: bool) -> i64:
    match value:
        case False:
            30
        case _:
            40

def either(value: bool) -> i64:
    match value:
        case True | False:
            50

def safe_div(a: i64, b: i64) -> bool:
    b != 0 and a // b > 1

def first_long(words: &list[str]) -> bool:
    len(words) > 0 and len(words[0]) > 3

def consume(words: list[str]) -> bool:
    len(words) > 1

def early(a: bool, b: bool) -> i64:
    if a and not b:
        return 1
    if not a or b:
        return 2
    3

def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:
    mut log = Log(calls=0)
    for a in [False, True]?:
        for b in [False, True]?:
            for c in [False, True]?:
                and_or = (note(&mut log, a) and note(&mut log, b)) or note(&mut log, c)
                or_and = note(&mut log, a) or (note(&mut log, b) and note(&mut log, c))
                negated = not (a and b) or not c
                print(f"{a} {b} {c}: {and_or} {or_and} {negated} {early(a, b)} calls={log.calls}"?)?
    for code in [-9223372036854775807 - 1, -1, 0, 47, 48, 57, 58, 255, 9223372036854775807]?:
        print(f"{code} {digit(code)}"?)?
    for value in [False, True]?:
        print(f"{name(value)} {flipped(value)} {wild_true(value)} {wild_false(value)} {either(value)}"?)?
    print(f"{safe_div(7, 0)} {safe_div(7, 2)} {safe_div(1, 2)}"?)?
    words = ["plenty", "of", "bools"]?
    print(f"{first_long(&words)}"?)?
    empty: list[str] = []?
    print(f"{first_long(&empty)}"?)?
    print(f"{digit(50) and consume(words)}"?)?
    print(f"{token_byte(33)} {token_byte(34)} {token_byte(48)} {token_byte(122)} {token_byte(127)}"?)?
    Ok(())
"#;

const EXPECTED: &str = "False False False: False False True 2 calls=4
False False True: True False True 2 calls=8
False True False: False False True 2 calls=13
False True True: True True True 2 calls=18
True False False: False True True 1 calls=22
True False True: True True True 1 calls=26
True True False: True True True 2 calls=29
True True True: True True False 2 calls=32
-9223372036854775808 False
-1 False
0 False
47 False
48 True
57 True
58 False
255 False
9223372036854775807 False
no 0 20 30 50
yes 1 10 40 50
False True False
True
False
True
True False True True False
";

fn write_program() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bools.plenty"), PROGRAM).unwrap();
    dir
}

fn plenty(dir: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn boolean_short_circuits_keep_their_semantics() {
    // Truth tables of nested `and`/`or`/`not` with effectful operands (the
    // call count shows which ran), digit boundaries, every boolean `match`
    // arm order, a guarded division, borrowed and moved operands, and early
    // returns: in both build modes.
    let dir = write_program();
    assert_eq!(plenty(dir.path(), &["bools.plenty"]), EXPECTED);
    assert_eq!(plenty(dir.path(), &["--release", "bools.plenty"]), EXPECTED);
}

#[test]
fn boolean_matches_have_no_impossible_case_trap() {
    let dir = write_program();
    plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif,clif-opt",
            "bools.plenty",
            "-o",
            "out",
        ],
    );
    let section = |stage: &str, name: &str| {
        let text = std::fs::read_to_string(dir.path().join("out").join(stage)).unwrap();
        let start = text
            .find(&format!("==== {name} "))
            .unwrap_or_else(|| panic!("no `{name}` in {stage}"));
        let rest = &text[start + 5..];
        rest[..rest
            .find(
                "
==== ",
            )
            .unwrap_or(rest.len())]
            .to_string()
    };
    // User trap 1 is the exhaustive-match fallback; a two-valued domain needs
    // none, before or after Cranelift's passes.
    for stage in ["clif.txt", "clif-opt.txt"] {
        for name in [
            "digit",
            "token_byte",
            "name",
            "flipped",
            "wild_true",
            "wild_false",
            "either",
            "early",
        ] {
            let body = section(stage, name);
            assert!(
                !body.contains("user1"),
                "{stage} {name}:
{body}"
            );
        }
    }
    // `code >= 48 and code <= 57`: one compare per operand, with no second
    // test of `code < 48` on the false edge of the first.
    let digit = section("clif-opt.txt", "digit");
    assert_eq!(digit.matches(" icmp").count(), 2, "{digit}");
    // A decided operand of the `if` chain jumps to the `return True` arm, and
    // the trailing chain's operands meet only at the returned value: one
    // `bool` block parameter, no join tested again (issue #377).
    let token_byte = section("clif-opt.txt", "token_byte");
    assert_eq!(token_byte.matches(": i8)").count(), 1, "{token_byte}");
}
