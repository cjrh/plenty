mod support;

#[test]
fn unicode_order_and_codepoint_round_trips() {
    let output = support::run(
        r#"
def main() -> ():
    print("a" < "b").unwrap()
    print("a" <= "a").unwrap()
    print("é" > "z").unwrap()
    print("é" >= "é").unwrap()
    print("a\0" > "a").unwrap()
    print("" < "a").unwrap()
    print(ord("é").unwrap()).unwrap()
    print(chr(128578).unwrap()).unwrap()
    for character in "aé🙂":
        print(chr(ord(character).unwrap()).unwrap()).unwrap()
    print(str.repr(ord("")).unwrap()).unwrap()
    print(str.repr(ord("ab")).unwrap()).unwrap()
    print(str.repr(chr(55296)).unwrap()).unwrap()
    print(str.repr(chr(1114112)).unwrap()).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "True\nTrue\nTrue\nTrue\nTrue\nTrue\n233\n🙂\na\né\n🙂\nResult[u32, ParseError].Err(ParseError.Invalid)\nResult[u32, ParseError].Err(ParseError.Invalid)\nResult[str, ParseError].Err(ParseError.OutOfRange)\nResult[str, ParseError].Err(ParseError.OutOfRange)\n");
}

#[test]
fn classification_case_mapping_and_whitespace_split() {
    let output = support::run(
        r#"
def main() -> ():
    text = "AbC"
    print(text.isalpha()).unwrap()
    print("09".isdigit()).unwrap()
    print("".isdigit()).unwrap()
    print("".isalpha()).unwrap()
    print("é".isalpha()).unwrap()
    print("１２".isdigit()).unwrap()
    print("a2".isalpha()).unwrap()
    print((&text).lower().unwrap()).unwrap()
    print("Straße".upper().unwrap()).unwrap()
    print("İ".lower().unwrap()).unwrap()
    print("  alpha\t\n beta  ".split().unwrap()).unwrap()
    print("\t\n".split().unwrap()).unwrap()
    print("a　b".split().unwrap()).unwrap()
    print("a,,b".split(",").unwrap()).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "True\nTrue\nFalse\nFalse\nFalse\nFalse\nFalse\nabc\nSTRASSE\ni̇\n[\"alpha\", \"beta\"]\n[]\n[\"a\", \"b\"]\n[\"a\", \"\", \"b\"]\n");
}

#[test]
fn invalid_character_calls_are_static_errors() {
    for source in [
        "chr(-1)",
        "chr(1u64)",
        "ord(1)",
        "ord(\"a\", \"b\")",
        "\"a\".lower(1)",
        "\"a\".isalpha(1)",
        "\"a\" < 1",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn character_operations_allocate_nothing_and_builders_recover() {
    let output = support::run(
        r#"
def main() -> ():
    print("__test_begin_no_allocations__").unwrap()
    code = ord("🙂").unwrap()
    character = chr(128578).unwrap()
    ordered = "a" < "é"
    alpha = "abc".isalpha()
    digits = "09".isdigit()
    lower = "ABC".lower().unwrap()
    upper = "ß".upper().unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(code).unwrap()
    print(character).unwrap()
    print(ordered and alpha and digits).unwrap()
    print(lower).unwrap()
    print(upper).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for budget in 0..=5 {
        let output = support::run(&format!(
            r#"
def main() -> ():
    source = "alphabet betagamma deltaepsilon"
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = source.split()
    print("__test_restore_allocations__").unwrap()
    match result:
        case Ok(parts):
            print(len(parts)).unwrap()
        case Err(error):
            print("failed").unwrap()
    print(source == "alphabet betagamma deltaepsilon").unwrap()
    print(source.split().unwrap()).unwrap()
"#
        ));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let visible = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n");
        let first = if budget < 5 { "failed" } else { "3" };
        assert_eq!(
            visible,
            format!("{first}\nTrue\n[\"alphabet\", \"betagamma\", \"deltaepsilon\"]")
        );
    }
    for method in ["lower", "upper"] {
        let output = support::run(&format!(
            r#"
def main() -> ():
    source = "AlphabetBetagamma"
    print("__test_fail_allocations_after_0__").unwrap()
    result = source.{method}()
    print("__test_restore_allocations__").unwrap()
    match result:
        case Ok(text):
            print("unexpected").unwrap()
        case Err(error):
            print("failed").unwrap()
    print(source == "AlphabetBetagamma").unwrap()
    print(source.{method}().unwrap()).unwrap()
"#
        ));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let visible = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n");
        let transformed = if method == "lower" {
            "alphabetbetagamma"
        } else {
            "ALPHABETBETAGAMMA"
        };
        assert_eq!(visible, format!("failed\nTrue\n{transformed}"));
    }
}
