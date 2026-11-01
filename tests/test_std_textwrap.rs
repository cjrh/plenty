//! CPython compatibility cases plus Plenty's bounded-allocation contracts.
mod support;

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{{{:x}}}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => quote(s),
        serde_json::Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        serde_json::Value::Null => "Nothing".into(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(xs) if xs.is_empty() => "list[str].new()?".into(),
        serde_json::Value::Array(xs) => format!(
            "[{}]?",
            xs.iter().map(self::value).collect::<Vec<_>>().join(", ")
        ),
        other => panic!("unexpected fixture value: {other}"),
    }
}

fn run(body: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = format!(
        "import std.text\nimport std.test\ndef main() -> Result[(), IoError | AllocError | std.text.TextError | std.test.TestError]:\n{}\n    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let path = support::write_main(dir.path(), &source);
    let executable = dir.path().join("program");
    let options = plenty::CompileOptions {
        test_hooks: cfg!(feature = "runtime-checks"),
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&path, &executable, None, &options).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn cpython_textwrap_reference_cases() {
    // Recorded from the sibling CPython checkout's test_textwrap.py at
    // b3714a5fb7a, using Python 3.14.8. Includes all successful public wrap
    // calls, including fill/shorten's delegated calls, plus dedent and default
    // indent. The adaptation and fixture carry CPython's PSF license notice.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/textwrap-cpython.json")).unwrap();
    for (batch, cases) in fixture["cases"].as_array().unwrap().chunks(8).enumerate() {
        let mut body = String::new();
        let mut expected = String::new();
        for (index, case) in cases.iter().enumerate() {
            let text = value(&case["text"]);
            let expression = match case["operation"].as_str().unwrap() {
                "wrap" => {
                    body.push_str(&format!("mut wrapper_{index} = std.text.text_wrapper()\n"));
                    for (name, val) in case["options"].as_object().unwrap() {
                        let val = if name == "max_lines" && !val.is_null() {
                            format!("Some({})", value(val))
                        } else {
                            value(val)
                        };
                        body.push_str(&format!("wrapper_{index}.{name} = {val}\n"));
                    }
                    format!("wrapper_{index}.wrap({text})?")
                }
                "dedent" => format!("std.text.dedent({text})?"),
                "indent" => format!("std.text.indent({text}, {})?", value(&case["prefix"])),
                other => panic!("unexpected operation: {other}"),
            };
            // Python accepts max_lines=0 as effectively one line. Plenty
            // rejects nonpositive limits rather than silently changing them.
            if case["options"]["max_lines"] == 0 {
                body.push_str(&format!(
                    "print({index}, wrapper_{index}.wrap({text}).is_err())?\n"
                ));
            } else {
                body.push_str(&format!(
                    "print({index}, {expression} == {})?\n",
                    value(&case["expected"])
                ));
            }
            expected.push_str(&format!("{index} True\n"));
        }
        assert_eq!(run(&body), expected, "CPython batch {batch}");
    }
}

#[test]
fn bounded_textwrap_and_public_helpers() {
    let output = run(r#"
print(std.text.word_wrap("one two three", 7)? == ["one two", "three"]?)?
print(std.text.fill("one two three", 7)? == "one two\nthree")?
print(std.text.shorten("Hello  world!", 12)? == "Hello world!")?
print(std.text.shorten("Hello  world!", 11)? == "Hello [...]")?
print(std.text.shorten("Hello world", 10, "...")? == "Hello...")?
print(std.text.shorten("  a\u{a0} b\t\n c  ", 5, "...")? == "a b c")?
print(std.text.dedent("\t x\n\t y\n \t\n")? == "x\ny\n\n")?
print(std.text.dedent("  x\n\ty")? == "  x\n\ty")?
print(std.text.indent_with("a\r\n\r\nb", ">", def(line: &str) -> bool: True)? == ">a\r\n>\r\n>b")?
print(std.text.indent_with("a\nb\nc", ">", def(line: &str) -> bool: line.startswith("b"))? == "a\n>b\nc")?
print(std.text.fill("é🙂x", 2, 8)? == "é🙂\nx")?
print(std.text.fill("é🙂x", 2, 7).is_err())?
print(std.text.word_wrap("a b", 1, 2)? == ["a", "b"]?)?
print(std.text.fill("a b", 1, 2).is_err())?
print(std.text.fill("a", 0).is_err(), std.text.dedent("x", -1).is_err())?
print(std.text.fill("1234", 3, 100, 3).is_err())?
print(std.text.indent("x", "abc", 3).is_err())?
print(std.text.dedent("  é", 2)? == "é")?
print(std.text.dedent("  é", 1).is_err())?
mut wrapper = std.text.text_wrapper(4)
wrapper.tabsize = i64.MAX
print(wrapper.wrap("\t").is_err())?
wrapper.tabsize = -1
print(wrapper.wrap("").is_err())?
wrapper.tabsize = 0
print(wrapper.wrap("a\tb")? == ["ab"]?)?
wrapper.max_lines = Some(0)
print(wrapper.wrap("").is_err())?
wrapper.max_lines = Some(1)
wrapper.placeholder = "too long"
print(wrapper.wrap("x").is_err())?
wrapper.max_lines = Nothing
wrapper.initial_indent = "12345"
print(wrapper.wrap("abc")? == ["12345a", "bc"]?)?
wrapper.initial_indent = ""
wrapper.width = i64.MAX
print(wrapper.wrap("one two")? == ["one two"]?)?
print(std.text.fill("", 4, 0, 0)? == "")?
print(std.text.word_wrap("ab\u{a0}\u{a0}", 2)? == ["ab"]?)?
long_word = "é".repeat(4096)?
lines = std.text.word_wrap(&long_word, 1)?
print(len(lines) == 4096, std.text.join(&lines, "", 8192)? == long_word)?
dash_word = "-".repeat(8192)?
dash_lines = std.text.word_wrap(&dash_word, 7)?
print(std.text.join(&dash_lines, "", 8192)? == dash_word)?
margin = " ".repeat(8192)?
indented = (margin + "text")?
print(std.text.dedent(&indented)? == "text")?
"#);
    assert!(!output.contains("False"), "{output}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn textwrap_allocation_failures_are_recoverable() {
    let output = run(r#"
mut wrapper = std.text.text_wrapper(16)
wrapper.initial_indent = "* "
wrapper.subsequent_indent = "  "
text = "A reusable wrapper handles words and é🙂 text with a bounded result."
expected = [wrapper.fill(&text)?, std.text.dedent("    first line\n      second line")?, std.text.indent("first line\n\nsecond line", "> ")?, std.text.shorten(&text, 24)?]?
for mode in range(4):
    mut successes = 0
    mut failures = 0
    for budget in range(2048):
        std.test.fail_allocations_after(u32(budget))?
        result = match mode:
            case 0:
                wrapper.fill(&text)
            case 1:
                std.text.dedent("    first line\n      second line")
            case 2:
                std.text.indent("first line\n\nsecond line", "> ")
            case _:
                std.text.shorten(&text, 24)
        std.test.restore_allocations()?
        match result:
            case Ok(text):
                print(text == expected[mode])?
                successes += 1
                break
            case Err(error):
                failures += 1
    print(successes > 0, failures > 0)?
print(wrapper.fill("still usable")? == "* still usable")?
"#);
    assert_eq!(output, format!("{}True\n", "True\nTrue True\n".repeat(4)));
}
