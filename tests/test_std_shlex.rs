//! Shell-word parsing and POSIX quoting without a shell in the implementation.
mod support;

use std::process::Command;

fn literal(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn program(body: &str) -> String {
    format!(
        "import std.shlex\nfrom std.shlex import ShlexError\ndef main() -> Result[(), ShlexError | AllocError | IoError]:\n{}    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>()
    )
}

#[test]
fn quotes_escapes_whitespace_and_literal_shell_syntax() {
    let cases: &[(&str, &[&str])] = &[
        ("", &[]),
        (" \t\r\n", &[]),
        ("a '' \"\" b", &["a", "", "", "b"]),
        ("a' b'\" c\"d", &["a b cd"]),
        (r"a\ b \# \\", &["a b", "#", "\\"]),
        ("'a\\b\nc'", &["a\\b\nc"]),
        ("\"a\\$b\\`c\\\"d\\\\e\\q\"", &["a$b`c\"d\\e\\q"]),
        ("a\\\nb \\\n c \"d\\\ne\"", &["ab", "c", "de"]),
        ("\\\n", &[]),
        ("é🙂 'two words'\u{a0}end", &["é🙂", "two words\u{a0}end"]),
        (
            "$HOME *.txt a|b x;y $(id) ~ #literal",
            &["$HOME", "*.txt", "a|b", "x;y", "$(id)", "~", "#literal"],
        ),
    ];
    let mut body = String::new();
    for (n, (input, expected)) in cases.iter().enumerate() {
        let expected = expected
            .iter()
            .map(|x| literal(x))
            .collect::<Vec<_>>()
            .join(", ");
        body.push_str(&format!(
            "expected{n}: list[str] = [{expected}]?\nprint(std.shlex.split({})? == expected{n})?\n",
            literal(input),
        ));
    }
    assert_eq!(
        support::run_file_stdout(&program(&body)),
        vec!["True"; cases.len()].join("\n")
    );
}

#[test]
fn comments_are_opt_in_and_start_only_between_words() {
    let source = program(
        r##"
print(std.shlex.split("a # ignored\nb", comments=True)? == ["a", "b"]?)?
print(std.shlex.split("a#b '#' \\# ''#x", True)? == ["a#b", "#", "#", "#x"]?)?
print(std.shlex.split("# ignored 'unterminated \\\nnext", True)? == ["next"]?)?
print(std.shlex.split("a # b")? == ["a", "#", "b"]?)?
"##,
    );
    assert_eq!(support::run_file_stdout(&source), "True\nTrue\nTrue\nTrue");
}

#[test]
fn malformed_input_has_scalar_positions_and_no_partial_result() {
    let source = program(
        r##"
print(str.repr(std.shlex.split("é 'unfinished"))?)?
print(str.repr(std.shlex.split("a \"unfinished"))?)?
print(str.repr(std.shlex.split("é \\"))?)?
print(str.repr(std.shlex.split("é\0"))?)?
print(str.repr(std.shlex.quote("é\0"))?)?
print(str.repr(std.shlex.split("#\0", True))?)?
"##,
    );
    let output = support::run_file_stdout(&source);
    for (line, expected) in output.lines().zip([
        "UnclosedQuote(2)",
        "UnclosedQuote(2)",
        "TrailingEscape(2)",
        "Nul(1)",
        "Nul(1)",
        "Nul(1)",
    ]) {
        assert!(line.contains(expected), "{line}");
    }
    assert_eq!(output.lines().count(), 6);
}

#[test]
fn quote_roundtrips_arbitrary_text_through_parser_and_real_posix_shell() {
    let source = r#"import std.env
import std.shlex
def main() -> Result[(), IoError | AllocError | std.shlex.ShlexError]:
    input = std.env.get("PLENTY_SHLEX_INPUT")?.unwrap_or("")
    quoted = std.shlex.quote(input)?
    words = std.shlex.split(quoted)?
    if len(words) != 1 or words[0] != input:
        return Err(std.shlex.ShlexError.Nul(-1))
    print(quoted)?
    Ok(())
"#;
    let (_dir, executable) = support::build_file(source);
    let controls: String = (1..=127u8).map(char::from).collect();
    for input in [
        "",
        "simple/word-1.txt",
        "a'b",
        "two words",
        "$(printf BAD); `id` $HOME * ? [x]",
        "\n\t\r\\\"'",
        "é🙂\u{a0}",
        &controls,
    ] {
        let result = Command::new(&executable)
            .env("PLENTY_SHLEX_INPUT", input)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let quoted = String::from_utf8(result.stdout).unwrap();
        let quoted = quoted.strip_suffix('\n').unwrap();
        let shell = Command::new("/bin/sh")
            .args(["-c", &format!("printf '%s' {quoted}")])
            .output()
            .unwrap();
        assert!(
            shell.status.success(),
            "{}",
            String::from_utf8_lossy(&shell.stderr)
        );
        assert_eq!(shell.stdout, input.as_bytes(), "{quoted}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_cleans_partial_words_and_quoted_text() {
    let mut body = String::new();
    for budget in 0..24 {
        body.push_str(&format!(
            r#"
print("__test_fail_allocations_after_{budget}__")?
split{budget} = std.shlex.split("firstword 'second very long argument for allocation' third")
print("__test_restore_allocations__")?
print(str.repr(split{budget})?)?
print("__test_fail_allocations_after_{budget}__")?
quote{budget} = std.shlex.quote("a long shell argument with 'apostrophes' and spaces")
print("__test_restore_allocations__")?
print(str.repr(quote{budget})?)?
"#
        ));
    }
    let output = support::run_file_stdout(&program(&body));
    assert_eq!(output.lines().count(), 48);
    assert!(output.contains("OutOfMemory"));
    assert!(output.contains(".Ok("));
    for line in output.lines() {
        assert!(
            line.contains(".Ok(") || line.contains("OutOfMemory"),
            "{line}"
        );
    }
}
