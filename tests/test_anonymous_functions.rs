mod support;
use support::check_diagnostic as diagnostic;
use support::{check_source, run};

#[test]
fn multiline_functions_have_independent_control_flow_and_locals() {
    let output = run(r#"
def main() -> Result[(), IoError]:
    transform = def(value: i64) -> i64:
        mut result = 0
        for n in range(value):
            if n == 2:
                continue
            result = result + n
        if result > 10:
            return 10
        result
    print(transform(5))?
    print(transform(8))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "8\n10\n");
}

#[test]
fn anonymous_functions_escape_without_allocating_an_environment() {
    let output = run(r#"
def make[T]() -> Callable[[T], T]:
    return def(value: T) -> T:
        value
def nested() -> Callable[[], Callable[[i64], i64]]:
    def() -> Callable[[i64], i64]:
        def(value: i64) -> i64:
            value + 1
def main() -> Result[(), IoError]:
    print(make[u8]()(42))?
    print(make[str]()("hello"))?
    print(nested()()(5))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\nhello\n6\n");
}

#[test]
fn anonymous_functions_reject_implicit_capture_and_missing_types() {
    for (source, diagnostic) in [
        ("def main() -> ():\n    value = 1\n    f = def() -> i64:\n        value\n    pass", "cannot capture `value`"),
        ("def top(x: i64) -> i64:\n    x\ndef main() -> ():\n    top = 1\n    f = def(x: i64) -> i64:\n        top(x)\n    pass", "cannot capture `top`"),
        ("def main() -> ():\n    f = def(value) -> i64:\n        value\n    pass", "expected `:`"),
        ("def main() -> ():\n    f = def(value: i64):\n        value\n    pass", "expected `->`"),
    ] {
        let error = check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

fn output(source: &str) -> String {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const APPLY: &str = "def apply(f: Callable[[i64], i64], value: i64) -> i64:\n    f(value)\n";

#[test]
fn the_issue_27_examples_compile_and_run() {
    assert_eq!(
        output(&format!(
            "{APPLY}def main() -> Result[(), AllocError | IoError]:\n    print(apply(def(x: i64) -> i64: x * 2, 4))?\n    Ok(())\n"
        )),
        "8\n"
    );
    assert_eq!(
        output(
            r#"
def main() -> Result[(), SpawnError[OnceClosure[[], i64]] | IoError]:
    n = 5
    with spawn(def once [n]() -> i64: n)? as task:
        print(task.join())?
    Ok(())
"#
        ),
        "5\n"
    );
}

#[test]
fn an_inline_body_ends_at_the_comma_or_closing_bracket() {
    let source = format!(
        r#"{APPLY}
def both(f: Callable[[i64], i64], g: Callable[[i64], i64], value: i64) -> i64:
    g(f(value))

def adder() -> Callable[[i64], Callable[[i64], i64]]:
    (def(a: i64) -> Callable[[i64], i64]: def(b: i64) -> i64: b + 1)

def main() -> Result[(), AllocError | IoError]:
    print(apply(def(x: i64) -> i64: x * 2, 4))?
    print(apply(def(x: i64) -> i64: (x * 2), 4))?
    print(apply(def(x: i64) -> i64: apply(def(y: i64) -> i64: y + 1, x), 4))?
    print(apply(def(x: i64) -> i64: (x, x + 1)[1], 4))?
    print(apply(def(x: i64) -> i64: x if x > 10 else 0 - x, 4))?
    print(both(def(x: i64) -> i64: x + 1, def(x: i64) -> i64: x * 3, 4,))?
    print(both(
        def(x: i64) -> i64: x + 1,
        def(x: i64) -> i64:
            x
            * 3,
        4,
    ))?
    print(adder()(1)(41))?
    print((def(x: i64) -> i64: x + 100)(1))?
    Ok(())
"#
    );
    assert_eq!(output(&source), "8\n8\n5\n5\n-4\n15\n15\n42\n101\n");
}

#[test]
fn inline_functions_are_values_in_every_bracketed_position() {
    let source = r#"
class Holder[F: Callable[[i64], i64]]:
    callback: F

def main() -> Result[(), AllocError | IoError]:
    first, second = (def(x: i64) -> i64: x + 1, def(x: i64) -> i64: x + 2)
    print(first(1) + second(1))?
    handlers = [def(x: i64) -> i64: x + 1, def(x: i64) -> i64: x * 9]?
    print(handlers[1](3))?
    table = {"double": def(x: i64) -> i64: x * 2, "square": def(x: i64) -> i64: x * x}?
    print(table["square"](7))?
    scale = 3
    holder = Holder(def [scale](x: i64) -> i64: x * scale)
    print(holder.callback(5))?
    closures = [def [n]() -> i64: n * 10 for n in range(3)]?
    mut total = 0
    for closure in closures:
        total = total + closure()
    print(total)?
    Ok(())
"#;
    assert_eq!(output(source), "5\n27\n49\n15\n30\n");
}

#[test]
fn an_inline_string_body_is_the_value_not_documentation() {
    assert_eq!(
        output(
            "def main() -> Result[(), AllocError | IoError]:\n    print((def() -> str: \"hi\")())?\n    Ok(())\n"
        ),
        "hi\n"
    );
}

#[test]
fn inline_closures_follow_the_capture_and_once_rules() {
    let source = r#"
def twice[T, F: Callable[[T], T]](f: &F, value: T) -> T:
    f(f(value))

def owned[F: Callable[[i64], i64]](f: F, value: i64) -> i64:
    f(value)

def run[T, F: OnceCallable[[], T]](callback: F) -> T:
    mut job = callback
    job()

def identity[T](value: T) -> T:
    (def(inner: T) -> T: inner)(value)

def main() -> Result[(), AllocError | IoError]:
    offset = 10
    print(twice(&(def [offset](x: i64) -> i64: x + offset), 1))?
    print(twice(&(def(x: i64) -> i64: x + 1), 1))?
    print(owned(def [offset](x: i64) -> i64: x + offset, 1))?
    print(owned(def(x: i64) -> i64: x + 7, 1))?
    mut total = 1
    print(twice(&(def [&total](x: i64) -> i64: x + total), 4))?
    total = 5
    values = [1, 2, 3]?
    print(run(def once [values]() -> list[i64]: values))?
    print(run(def [total]() -> i64: total))?
    print(identity("generic"))?
    print(identity(7u8))?
    Ok(())
"#;
    assert_eq!(
        output(source),
        "21\n3\n11\n8\n6\n[1, 2, 3]\n5\ngeneric\n7\n"
    );
}

#[test]
fn inline_closures_keep_the_block_form_diagnostics() {
    let helpers = format!(
        "{APPLY}def run[T, F: OnceCallable[[], T]](callback: F) -> T:\n    mut job = callback\n    job()\ndef bump[F: Callable[[], i64]](f: &mut F) -> i64:\n    f() + f()\n"
    );
    for (body, expected) in [
        (
            "    offset = 3\n    apply(def(x: i64) -> i64: x + offset, 4)",
            "anonymous functions cannot capture `offset`",
        ),
        (
            "    offset = 3\n    apply(def [offset](x: i64) -> i64: x + offset, 4)",
            "use `F: Callable[[i64], i64]` with `callback: &F`",
        ),
        (
            "    apply(def once (x: i64) -> i64: x, 4)",
            "use `F: OnceCallable[[i64], i64]` with `callback: F`",
        ),
        (
            "    values = [1, 2].unwrap()\n    run(def once [values]() -> list[i64]: values)\n    print(values).unwrap()",
            "use of moved binding `values`",
        ),
        (
            "    values = [1, 2].unwrap()\n    f = (def [values](i: i64) -> i64: values[i])\n    print(values).unwrap()",
            "use of moved binding `values`",
        ),
        (
            "    apply(def(x: i64) -> i64: missing, 4)",
            "unknown binding `missing`",
        ),
        (
            "    apply(def(x: i64) -> i64: x * 2.5, 4)",
            "cannot combine i64 with f64; convert with `f64(x)`",
        ),
        (
            "    apply(def(x: i64, y: i64) -> i64: x * y, 4)",
            "expected Callable[[i64], i64], got Callable[[i64, i64], i64]",
        ),
        ("    apply(def(x) -> i64: x, 4)", "expected `:`"),
        ("    apply(def(x: i64): x, 4)", "expected `->`"),
        (
            "    apply(def [missing](x: i64) -> i64: x, 4)",
            "capture requires a local binding `missing`",
        ),
        // A temporary cannot be borrowed exclusively, whatever produced it.
        (
            "    count = 0\n    bump(&mut (def [mut count]() -> i64: count + 1))",
            "borrowing requires a named binding",
        ),
    ] {
        let error = diagnostic(&format!("{helpers}def main() -> ():\n{body}\n    pass\n"));
        assert!(error.contains(expected), "{body}\n{error}");
    }
}

#[test]
fn a_one_shot_inline_closure_cannot_run_twice() {
    let error = diagnostic(
        "def run_twice[F: OnceCallable[[], i64]](callback: F) -> i64:\n    mut job = callback\n    job()\n    job()\ndef main() -> ():\n    n = 1\n    run_twice(def once [n]() -> i64: n)\n    pass\n",
    );
    assert!(error.contains("use of moved binding `job`"), "{error}");
}

#[test]
fn an_inline_body_with_statements_must_be_bound_to_a_name() {
    const ONE_EXPRESSION: &str = "inside brackets an anonymous function body is one expression";
    const BIND: &str = "bind a function with statements to a name first";
    for (body, at, expected) in [
        (
            "    apply(def(x: i64) -> i64: return x * 2, 4)",
            "4:31",
            "remove `return`",
        ),
        ("    apply(def(x: i64) -> i64: y = x * 2, 4)", "4:31", BIND),
        ("    apply(def(x: i64) -> i64: mut y = x, 4)", "4:31", BIND),
        (
            "    apply(def(x: i64) -> i64: if x > 2: 1, 4)",
            "4:31",
            BIND,
        ),
        ("    apply(def(x: i64) -> i64: pass, 4)", "4:31", BIND),
        (
            "    apply(def(x: i64) -> i64:\n        y = x * 2\n        y, 4)",
            "5:9",
            BIND,
        ),
        (
            "    apply(def(x: i64) -> i64:\n        print(x)\n        x, 4)",
            "6:9",
            BIND,
        ),
        (
            "    values = [1].unwrap()\n    apply(def(x: i64) -> i64: values[0] = x, 4)",
            "5:41",
            BIND,
        ),
    ] {
        let error = diagnostic(&format!("{APPLY}def main() -> ():\n{body}\n    pass\n"));
        assert!(
            error.starts_with(&format!("{at}: {ONE_EXPRESSION}")),
            "{body}\n{error}"
        );
        assert!(error.contains(expected), "{body}\n{error}");
    }
    // A missing comma on one line stays the enclosing call's error.
    let error = diagnostic(&format!(
        "{APPLY}def main() -> ():\n    apply(def(x: i64) -> i64: x * 2 4)\n"
    ));
    assert!(error.contains("expected `,`"), "{error}");
}

#[test]
fn the_body_form_follows_the_position() {
    for (body, expected) in [
        (
            "    f = def(x: i64) -> i64: x * 2",
            "4:29: outside brackets an anonymous function needs an indented body on the next line",
        ),
        (
            "    return def(x: i64) -> i64: x * 2",
            "outside brackets an anonymous function needs an indented body on the next line",
        ),
        (
            "    twice(&def(x: i64) -> i64: x * 2, 4)",
            "4:12: an anonymous function must be a complete expression; wrap it in parentheses",
        ),
        (
            "    f = &def(x: i64) -> i64:\n        x",
            "an anonymous function must be a complete expression",
        ),
        (
            "    print(1 + def(x: i64) -> i64: x)",
            "an anonymous function must be a complete expression",
        ),
    ] {
        let error = diagnostic(&format!("{APPLY}def main() -> ():\n{body}\n    pass\n"));
        assert!(error.contains(expected), "{body}\n{error}");
    }
    // Named functions are never inside brackets, so they always take a block.
    let error = diagnostic("def double(x: i64) -> i64: x * 2\ndef main() -> ():\n    pass\n");
    assert!(error.contains("expected a newline after `:`"), "{error}");
    // Parentheses are brackets: the one-expression form can be bound.
    assert_eq!(
        output("def main() -> Result[(), IoError]:\n    f = (def(x: i64) -> i64: x * 2)\n    print(f(4))?\n    Ok(())\n"),
        "8\n"
    );
}

#[test]
fn lambda_names_the_plenty_spelling() {
    for (body, expected) in [
        (
            "    apply(lambda x: x * 2, 4)",
            "4:11: `lambda` is not supported; write an anonymous function as `def(name: T) -> U: expression`",
        ),
        (
            "    f = lambda x: x * 2",
            "4:9: `lambda` is not supported; write an anonymous function as `def(name: T) -> U:` with an indented body",
        ),
        ("    lambda = 3", "expected an identifier (keywords"),
    ] {
        let error = diagnostic(&format!("{APPLY}def main() -> ():\n{body}\n    pass\n"));
        assert!(error.contains(expected), "{body}\n{error}");
    }
}

#[test]
fn inline_closures_are_thread_and_pool_jobs() {
    let source = r#"
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

# B71: three jobs with distinct closure types cannot share one declared SpawnError, so main keeps `Failure`.
def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](2)?
    with spawn(def once [sender]() -> Result[(), SendError[i64]]: produce(sender))? as task:
        mut total = 0
        with receiver as inbox:
            while True:
                match inbox.recv():
                    case Ok(value):
                        total = total + value
                    case Err(_):
                        break
        task.join()?
        print(total)?
    values = [1, 2, 3]?
    with spawn(def [values]() -> i64: len(values))? as task:
        print(task.join())?
    with ThreadPoolExecutor(2, 2)? as pool:
        owned = [3, 6, 9]?
        future = pool.submit(def once [owned]() -> list[i64]: owned)?
        print(future.result()?)?
    Ok(())
"#;
    assert_eq!(output(source), "30\n3\n[3, 6, 9]\n");
    // Same rule and wording as a named closure moved into `spawn`.
    let error = diagnostic(
        "def main() -> Result[(), AllocError | ThreadError | IoError]:\n    values = [1]?\n    with spawn(def [&values]() -> i64: len(values))? as task:\n        print(task.join())?\n    Ok(())\n",
    );
    assert!(
        error.contains(
            "an owned worker must be a concrete zero-argument closure with wholly owned captures"
        ),
        "{error}"
    );
}

#[test]
fn inline_functions_are_tail_call_arguments() {
    let source = r#"
def countdown(n: i64, f: Callable[[i64], i64]) -> i64:
    if n == 0:
        return f(0)
    countdown(n - 1, def(x: i64) -> i64: x + 1)

def main() -> Result[(), IoError]:
    print(countdown(1000000, def(x: i64) -> i64: x))?
    Ok(())
"#;
    assert_eq!(output(source), "1\n");
}

#[test]
fn brackets_in_string_literals_do_not_select_the_inline_form() {
    assert_eq!(
        output(
            "def main() -> Result[(), IoError]:\n    text = \"(\"\n    f = def(x: i64) -> i64:\n        x + len(\"[{(\")\n    print(f(1))?\n    print(text)?\n    Ok(())\n"
        ),
        "4\n(\n"
    );
}
