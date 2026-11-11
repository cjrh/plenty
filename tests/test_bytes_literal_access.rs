//! `len` and indexing of a `bytes` literal are lowered inline: the length is
//! a constant where the literal is known, and a loop that reads a literal
//! still writes its `Bytes` buffer through the cached length and rows.
use std::path::Path;
use std::process::Command;

mod support;

fn plenty(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

/// Standard output and error of `source`, which may exit with a failure.
fn run(source: &str, flags: &[&str]) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let mut args = flags.to_vec();
    args.push("main.plenty");
    let output = plenty(dir.path(), &args);
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// The `--release` CLIF of function `name` in `source`.
fn clif(source: &str, name: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif-opt",
            "main.plenty",
            "-o",
            "out",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = std::fs::read_to_string(dir.path().join("out/clif-opt.txt")).unwrap();
    let start = dump
        .find(&format!("==== {name}"))
        .unwrap_or_else(|| panic!("no section {name} in:\n{dump}"));
    let rest = &dump[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

const SOURCE: &str = "\
import std.bytes

const CODES: bytes = b\"GATTACA\"


def at(i: i64) -> u8:
    CODES[i]


def pick(data: bytes, i: i64) -> u8:
    data[i]


def size(data: bytes) -> i64:
    len(data)


def fill(block: &mut std.bytes.Bytes, count: i64, start: i64) -> Result[i64, OverflowError[i64]]:
    mut position = start
    for i in range(count):
        block.set(i, CODES[position])
        position += 1
        if position == len(CODES):
            position = 0
    Ok(position)
";

#[test]
fn literal_access_keeps_results_and_traps() {
    let main = "
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    print(at(0), at(6), at(-1), at(-7), len(CODES), len(b\"\"), CODES.len(), len((CODES)))?
    print(pick(b\"xyz\", 1), pick(CODES, -2), size(b\"xyz\"), size(b\"\"))?
    mut block = std.bytes.with_capacity(10)?
    block.resize(10, 0u8)?
    print(fill(&mut block, 10, 5)?, block)?
    print(fill(&mut block, 0, 6)?, fill(&mut block, 1, 6)?)?
    print(REPLACED)?
    Ok(())
";
    for (index, message) in [
        ("at(7)", "7:5: index out of bounds: index 7, length 7"),
        ("at(-8)", "7:5: index out of bounds: index -8, length 7"),
        (
            "pick(b\"\", 0)",
            "11:5: index out of bounds: index 0, length 0",
        ),
        (
            "fill(&mut block, 1, 7)?",
            "21:22: index out of bounds: index 7, length 7",
        ),
    ] {
        let source = format!("{SOURCE}\n{}", main.replace("REPLACED", index));
        for flags in [&["--release"][..], &[]] {
            let (out, err) = run(&source, flags);
            assert_eq!(
                out, "71 65 65 71 7 0 7 7\n121 67 3 0\n1 b\"CAGATTACAG\"\n6 0\n",
                "{index} {flags:?}"
            );
            assert_eq!(
                err,
                format!("error: main.plenty:{message}\n"),
                "{index} {flags:?}"
            );
        }
    }
}

/// No runtime call reads a literal; a known literal's length is a constant;
/// and the step after the literal's index check has no overflow check, also
/// past the buffer's own guard.
#[test]
fn literal_access_is_inline() {
    let source = format!("{SOURCE}\n\ndef main() -> ():\n    ()\n");
    for function in ["at", "pick", "size", "fill"] {
        let body = clif(&source, function);
        assert!(!body.contains("plenty_byte_str_item"), "{body}");
        assert!(!body.contains("plenty_text_byte_len"), "{body}");
    }
    let at = clif(&source, "at");
    assert!(at.contains("iconst.i64 7"), "{at}");
    let fill = clif(&source, "fill");
    // One call each for the hoisted length and rows; the trapping `set`
    // is a call of the method, which makes its own length query.
    assert_eq!(fill.matches("plenty_bytes_len").count(), 1, "{fill}");
    assert_eq!(fill.matches("plenty_byte_view_rows").count(), 1, "{fill}");
    assert!(!fill.contains("sadd_overflow"), "{fill}");
}

/// Iterating `bytes` reads the length once: a literal's is a constant, so
/// the index needs no overflow check, and a parameter's is loaded before the
/// loop.
#[test]
fn iteration_reads_the_length_once() {
    let source = "\
const WORD: bytes = b\"GATTACA\"


def literal() -> i64:
    mut total = 0
    for byte in WORD:
        total = total ^ i64(byte)
    total


def parameter(text: bytes) -> i64:
    mut total = 0
    for byte in text:
        total = total ^ i64(byte)
    total


def main() -> Result[(), IoError]:
    print(literal(), parameter(WORD), parameter(b\"\"))?
    Ok(())
";
    for flags in [&[][..], &["--release"][..]] {
        let (stdout, stderr) = run(source, flags);
        assert_eq!(stdout, "69 69 0\n", "{stderr}");
    }
    let literal = clif(source, "literal");
    assert!(literal.contains("iconst.i64 7"), "{literal}");
    assert!(!literal.contains("load.i64"), "{literal}");
    assert!(!literal.contains("sadd_overflow"), "{literal}");
    let parameter = clif(source, "parameter");
    assert_eq!(parameter.matches("load.i64").count(), 1, "{parameter}");
    assert!(!parameter.contains("sadd_overflow"), "{parameter}");
}
