mod support;

fn rejects(source: &str, expected: &[&str]) {
    let error = support::check_source(source).unwrap_err().to_string();
    for text in expected {
        assert!(error.contains(text), "missing {text:?} in {error}");
    }
}

#[test]
fn local_reference_recursion_reports_call_and_binding() {
    rejects(
        "def walk(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(*value)\n    child = n - 1\n    return walk(child, n - 1)\n",
        &["5:12:", "recursive call to `walk`", "local `child`"],
    );
    rejects(
        "def walk(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(*value)\n    walk(n - 1, n - 1)\n",
        &["recursive call to `walk`", "temporary"],
    );
}

#[test]
fn mutual_cycles_include_non_tail_back_edges() {
    rejects(
        "def first(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    child = n\n    second(child, n)\ndef second(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(*value)\n    Ok(1 + first(value, n - 1)?)\n",
        &[
            "recursive call to `second`",
            "cycle as `first`",
            "local `child`",
        ],
    );
    rejects(
        "def first(value: &i64) -> i64:\n    child = 1\n    second(child)\ndef second(value: &i64) -> i64:\n    third(value)\ndef third(value: &i64) -> i64:\n    first(value)\n",
        &["recursive call to `second`", "local `child`"],
    );
}

#[test]
fn candidates_are_validated_among_functions_without_any() {
    // Only a function with a call in source tail position records candidates
    // (issue #57). The first offender in source order is still the one reported.
    let walk = |name: &str| {
        format!(
            "def {name}(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(*value)\n    child = n - 1\n    return {name}(child, n - 1)\n"
        )
    };
    let quiet = "def quiet(n: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(n + 1)\n";
    let source = format!(
        "{quiet}{}def other(n: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(1 + quiet(n)?)\n{}",
        walk("early"),
        walk("late")
    );
    rejects(
        &source,
        &["7:12:", "recursive call to `early`", "local `child`"],
    );
    // A cycle closed only through a call outside tail position still counts.
    rejects(
        &format!(
            "{quiet}def first(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    child = n\n    second(child, n)\ndef second(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(*value)\n    Ok(1 + first(value, n - 1)?)\n"
        ),
        &[
            "recursive call to `second`",
            "cycle as `first`",
            "local `child`",
        ],
    );
    support::check_source(&format!(
        "{quiet}def other(n: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(1 + quiet(n)?)\n"
    ))
    .unwrap();
}

#[test]
fn nested_branches_and_conditional_expressions_retain_candidates() {
    for tail in [
        "if n > 1:\n        if n > 2:\n            return walk(child, n - 1)\n        return Ok(0)\n    return Ok(0)",
        "match Some(n):\n        case Some(_):\n            walk(child, n - 1)\n        case Nothing:\n            Ok(0)",
        "walk(child, n - 1) if n > 0 else Ok(0)",
    ] {
        rejects(
            &format!(
                "def walk(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    child = n\n    {tail}\n"
            ),
            &["recursive call to `walk`", "local `child`"],
        );
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
def walk(n: i64) -> Result[i64, OverflowError[i64]]:
    if n == 0:
        return Ok(0)
    with Manager():
        return walk(n - 1)
"#,
        &[
            "recursive call to `walk`",
            "the `with` context opened at 9:10 runs its exit after the call returns; \
             bind the call's result to a local and return the local instead",
        ],
    );
}

/// A borrower writes a lent pipeline's state back to the lender when it
/// returns, so forwarding the pipeline in tail position cannot reuse the
/// frame. The diagnostic names the source function or instance, never a
/// frame or method symbol, and gives the rewrite (issue #885).
#[test]
fn lent_pipeline_write_back_names_the_source_function_and_the_rewrite() {
    let reason = |name: &str, at: &str| {
        format!(
            "is in tail position but cannot be a tail call: the state of lent pipeline `{name}` \
             (declared at {at}) is written back to the lender after the call returns; \
             bind the call's result to a local and return the local instead"
        )
    };
    let cases = [
        (
            "def nested(p: &mut Iter[i64], again: bool) -> Option[i64]:
    if again:
        drop(next(p))
        return nested(p, False)
    next(p)
mut p = iter(range(10))
drop(nested(&mut p, True))
",
            "4:16: recursive call to `nested` ",
            "1:15",
        ),
        (
            "def tagged[T](p: &mut Iter[i64], label: T, again: bool) -> Option[i64]:
    if again:
        drop(next(p))
        return tagged(p, label, False)
    next(p)
mut p = iter(range(10))
drop(tagged(&mut p, \"x\", True))
",
            "in instantiation `tagged[str]`: recursive call to `tagged[str]` ",
            "1:18",
        ),
        (
            "def first[T](items: &mut Iter[i64], tag: T, again: bool) -> Option[i64]:
    if again:
        return second(items, tag)
    next(items)
def second[T](items: &mut Iter[i64], tag: T) -> Option[i64]:
    drop(next(items))
    first(items, tag, False)
mut p = iter(range(10))
drop(first(&mut p, 1u8, True))
",
            "recursive call to `second[u8]` (in the same direct recursion cycle as `first[u8]`) ",
            "1:21",
        ),
        (
            "class Walker:
    n: i64
    def walk(self, p: &mut Iter[i64], again: bool) -> Option[i64]:
        if again:
            drop(next(p))
            return self.walk(p, False)
        next(p)
w = Walker(0)
mut p = iter(range(10))
drop(w.walk(&mut p, True))
",
            "6:20: recursive call to `Walker.walk` ",
            "3:23",
        ),
        (
            "class Walker[T]:
    tag: T
    def walk(self, p: &mut Iter[i64], again: bool) -> Option[i64]:
        if again:
            drop(next(p))
            return self.walk(p, False)
        next(p)
w = Walker(1u8)
mut p = iter(range(10))
drop(w.walk(&mut p, True))
",
            "recursive call to `Walker[u8].walk` ",
            "3:23",
        ),
    ];
    for (source, call, at) in cases {
        let error = support::check_diagnostic(source);
        let name = if source.contains("items") {
            "items"
        } else {
            "p"
        };
        assert!(
            error.contains(&format!("{call}{}", reason(name, at))),
            "{error}"
        );
        assert!(!error.contains("__plenty"), "{error}");
    }
}

#[test]
fn lent_pipeline_recursion_with_a_bound_result_keeps_the_items() {
    assert_eq!(
        support::run_stdout(
            "def nested(p: &mut Iter[i64], again: bool) -> Option[i64]:
    if again:
        drop(next(p))
        found = nested(p, False)
        return found
    next(p)
mut p = iter(range(10))
print(nested(&mut p, True)).unwrap()
print(next(p)).unwrap()
"
        ),
        "Option[i64].Some(1)\nOption[i64].Some(2)\n"
    );
}

#[test]
fn owned_inline_argument_has_an_abi_reason() {
    rejects(
        "def walk(values: range, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(0)\n    walk(values, n - 1)\n",
        &["recursive call to `walk`", "inline"],
    );
}

#[test]
fn ordinary_nonrecursive_and_non_tail_calls_remain_legal() {
    for source in [
        "def done(value: &i64) -> i64:\n    *value\ndef start() -> i64:\n    child = 3\n    done(child)\n",
        "def walk(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(0)\n    child = n\n    Ok(1 + walk(child, n - 1)?)\n",
        "def walk(n: i64) -> Option[i64]:\n    if n == 0:\n        return Nothing\n    # Keep Option propagation as the non-tail ABI subject; only the primitive decrement traps.\n    remaining = unchecked:\n        n - 1\n    Some(walk(remaining)?)\n",
        "def walk(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(0)\n    child = n\n    callback = walk\n    callback(child, n - 1)\n",
    ] {
        support::check_source(source).unwrap_or_else(|error| panic!("{source}\n{error}"));
    }
}

#[test]
fn distinct_generic_instances_are_not_a_recursion_cycle() {
    support::check_source("def choose[T](value: T) -> i64:\n    child = 1\n    choose_ref[i64](child)\ndef choose_ref[T](value: &T) -> i64:\n    0\nchoose[bool](True)\nchoose[i64](1)\n").unwrap();
    rejects(
        "def walk[T](value: &T, n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        return Ok(0)\n    child = n\n    walk[i64](child, n - 1)\ndrop(walk[bool](True, 2))\n",
        &["recursive call to", "local `child`"],
    );
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
    def walk(self, n: i64) -> Result[i64, OverflowError[i64]]:
        if n == 0:
            return Ok(0)
        child = Walker()
        child.walk(n - 1)
"#,
        &["recursive call to `Walker.walk`", "local `child`"],
    );
}

#[test]
fn generator_construction_is_outside_the_synchronous_tail_guarantee() {
    support::check_source(
        "def values(n: i64) -> Generator[Result[i64, OverflowError[i64]]]:\n    yield Ok(n)\n    remaining = checked:\n        n - 1\n    match remaining:\n        case Ok(value):\n            values(value)\n        case Err(error):\n            yield Err(error)\n",
    )
    .unwrap();
}

#[test]
fn short_circuit_result_branches_are_tail_positions() {
    for expression in [
        "n == 0 or walk(child, previous(n))",
        "n > 0 and walk(child, previous(n))",
        "n > 0 and (n == 1 or walk(child, previous(n)))",
    ] {
        rejects(
            &format!(
                "def previous(n: i64) -> i64:\n    # Keep boolean short-circuit tail/non-tail ABI oracles; only the primitive decrement traps.\n    unchecked:\n        n - 1\ndef walk(value: &i64, n: i64) -> bool:\n    child = n\n    {expression}\n"
            ),
            &["recursive call to `walk`", "local `child`"],
        );
    }
    support::check_source("def previous(n: i64) -> i64:\n    # Keep boolean short-circuit tail/non-tail ABI oracles; only the primitive decrement traps.\n    unchecked:\n        n - 1\ndef walk(value: &i64, n: i64) -> bool:\n    if n == 0:\n        return True\n    child = n\n    walk(child, previous(n)) and n > 0\n").unwrap();
    support::check_source("def previous(n: i64) -> i64:\n    # Keep boolean short-circuit tail/non-tail ABI oracles; only the primitive decrement traps.\n    unchecked:\n        n - 1\ndef walk(n: i64) -> bool:\n    n == 0 or walk(previous(n))\n").unwrap();
}

#[test]
fn deep_scalar_and_external_reference_cycles_run_on_a_bounded_stack() {
    let source = r#"
def first(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:
    if n == 0:
        return Ok(*value)
    second(value, n - 1)
def second(value: &i64, n: i64) -> Result[i64, OverflowError[i64]]:
    first(value, n)
def main() -> Result[(), IoError | OverflowError[i64]]:
    value = 7
    print(first(value, 300000)?)?
    Ok(())
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
