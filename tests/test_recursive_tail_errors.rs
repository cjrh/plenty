mod support;

fn rejects(source: &str, expected: &[&str]) {
    let error = support::check_source(source).unwrap_err().to_string();
    for text in expected {
        assert!(error.contains(text), "missing {text:?} in {error}");
    }
}

#[test]
fn local_reference_recursion_reports_call_and_binding() {
    rejects("def walk(value: &i64, n: i64) -> i64:\n    if n == 0:\n        return *value\n    child = n - 1\n    return walk(child, n - 1)\n", &["5:12:", "recursive call to `walk`", "local `child`"]);
    rejects("def walk(value: &i64, n: i64) -> i64:\n    if n == 0:\n        return *value\n    walk(n - 1, n - 1)\n", &["recursive call to `walk`", "temporary"]);
}

#[test]
fn mutual_cycles_include_non_tail_back_edges() {
    rejects("def first(value: &i64, n: i64) -> i64:\n    child = n\n    second(child, n)\ndef second(value: &i64, n: i64) -> i64:\n    if n == 0:\n        return *value\n    1 + first(value, n - 1)\n", &["recursive call to `second`", "cycle as `first`", "local `child`"]);
    rejects("def first(value: &i64) -> i64:\n    child = 1\n    second(child)\ndef second(value: &i64) -> i64:\n    third(value)\ndef third(value: &i64) -> i64:\n    first(value)\n", &["recursive call to `second`", "local `child`"]);
}

#[test]
fn nested_branches_and_conditional_expressions_retain_candidates() {
    for tail in [
        "if n > 1:\n        if n > 2:\n            return walk(child, n - 1)\n        return 0\n    return 0",
        "match Some(n):\n        case Some(_):\n            walk(child, n - 1)\n        case Nothing:\n            0",
        "walk(child, n - 1) if n > 0 else 0",
    ] {
        rejects(&format!("def walk(value: &i64, n: i64) -> i64:\n    child = n\n    {tail}\n"), &["recursive call to `walk`", "local `child`"]);
    }
}

#[test]
fn post_call_context_exit_has_a_specific_reason() {
    rejects(
        r#"
class Manager:
    def __enter__(self: &mut Manager) -> ():
        pass
    def __exit__(self: &mut Manager) -> ():
        pass
def walk(n: i64) -> i64:
    if n == 0:
        return 0
    with Manager():
        return walk(n - 1)
"#,
        &[
            "recursive call to `walk`",
            "context opened at",
            "post-call exit action",
        ],
    );
}

#[test]
fn owned_inline_argument_has_an_abi_reason() {
    rejects("def walk(values: range, n: i64) -> i64:\n    if n == 0:\n        return 0\n    walk(values, n - 1)\n", &["recursive call to `walk`", "inline"]);
}

#[test]
fn ordinary_nonrecursive_and_non_tail_calls_remain_legal() {
    for source in [
        "def done(value: &i64) -> i64:\n    *value\ndef start() -> i64:\n    child = 3\n    done(child)\n",
        "def walk(value: &i64, n: i64) -> i64:\n    if n == 0:\n        return 0\n    child = n\n    1 + walk(child, n - 1)\n",
        "def walk(n: i64) -> Option[i64]:\n    if n == 0:\n        return Nothing\n    Some(walk(n - 1)?)\n",
        "def walk(value: &i64, n: i64) -> i64:\n    if n == 0:\n        return 0\n    child = n\n    callback = walk\n    callback(child, n - 1)\n",
    ] {
        support::check_source(source).unwrap_or_else(|error| panic!("{source}\n{error}"));
    }
}

#[test]
fn distinct_generic_instances_are_not_a_recursion_cycle() {
    support::check_source("def choose[T](value: T) -> i64:\n    child = 1\n    choose_ref[i64](child)\ndef choose_ref[T](value: &T) -> i64:\n    0\nchoose[bool](True)\nchoose[i64](1)\n").unwrap();
    rejects("def walk[T](value: &T, n: i64) -> i64:\n    if n == 0:\n        return 0\n    child = n\n    walk[i64](child, n - 1)\nwalk[bool](True, 2)\n", &["recursive call to", "local `child`"]);
    support::check_source(
        r#"
class End:
    def finish(self) -> i64:
        0
class Begin:
    def finish(self) -> i64:
        relay[End](range(2), End())
def relay[T](values: range, value: T) -> i64:
    value.finish()
relay[Begin](range(3), Begin())
"#,
    )
    .unwrap();
}

#[test]
fn concrete_method_recursion_preserves_the_receiver_name() {
    rejects(
        r#"
class Walker:
    def walk(self, n: i64) -> i64:
        if n == 0:
            return 0
        child = Walker()
        child.walk(n - 1)
"#,
        &["recursive call to", "local `child`"],
    );
}

#[test]
fn generator_construction_is_outside_the_synchronous_tail_guarantee() {
    support::check_source(
        "def values(n: i64) -> Generator[i64]:\n    yield n\n    values(n - 1)\n",
    )
    .unwrap();
}

#[test]
fn short_circuit_result_branches_are_tail_positions() {
    for expression in [
        "n == 0 or walk(child, n - 1)",
        "n > 0 and walk(child, n - 1)",
        "n > 0 and (n == 1 or walk(child, n - 1))",
    ] {
        rejects(
            &format!("def walk(value: &i64, n: i64) -> bool:\n    child = n\n    {expression}\n"),
            &["recursive call to `walk`", "local `child`"],
        );
    }
    support::check_source("def walk(value: &i64, n: i64) -> bool:\n    if n == 0:\n        return True\n    child = n\n    walk(child, n - 1) and n > 0\n").unwrap();
    support::check_source("def walk(n: i64) -> bool:\n    n == 0 or walk(n - 1)\n").unwrap();
}

#[test]
fn deep_scalar_and_external_reference_cycles_run_on_a_bounded_stack() {
    let source = r#"
def first(value: &i64, n: i64) -> i64:
    if n == 0:
        return *value
    second(value, n - 1)
def second(value: &i64, n: i64) -> i64:
    first(value, n)
def main() -> ():
    value = 7
    print(first(value, 300000)).unwrap()
"#;
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("program");
    support::compile_source_to_executable(source, &executable).unwrap();
    let output = std::process::Command::new("sh")
        .args(["-c", "ulimit -s 256; exec \"$1\"", "sh"])
        .arg(executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
}
