//! Diagnostics for Python statements, operators, parameters and named
//! arguments that Plenty rejects: each names the Plenty form (issue #627).
mod support;
use support::check_diagnostic as diagnostic;

#[test]
fn python_statement_keywords_name_the_plenty_form() {
    for (statement, expected) in [
        (
            "assert x",
            "3:5: Plenty has no run-time `assert` statement; return an error with `if not condition: return Err(...)`, or use `std.test.check(condition, message)?` in tests; a module-scope `assert` checks constants at compile time",
        ),
        (
            "del x",
            "3:5: `del` is not supported; remove a list element with `items.pop(index)` or a dictionary entry with `table.pop(key)`, and handle the returned Option",
        ),
        (
            "global x",
            "3:5: Plenty has no global variables; pass the value as a parameter, or declare a `const` at module scope",
        ),
        (
            "nonlocal x",
            "3:5: Plenty has no global variables; pass the value as a parameter, or declare a `const` at module scope",
        ),
    ] {
        let source = format!("def main() -> ():\n    x = True\n    {statement}\n");
        assert_eq!(diagnostic(&source), expected, "{statement}");
    }
}

#[test]
fn python_statement_words_remain_ordinary_names() {
    support::assert_stdout(
        "def assert(x: bool) -> bool:\n    x\ndef main() -> Result[(), IoError]:\n    del = 1\n    global = assert(True)\n    print(del, global)?\n    Ok(())\n",
        "1 True\n",
    );
}

#[test]
fn is_names_the_option_tests() {
    for test in ["x is None", "x is not None"] {
        let source = format!(
            "def f(x: Option[i64]) -> bool:\n    if {test}:\n        return True\n    False\ndef main() -> ():\n    b = f(Nothing)\n"
        );
        assert_eq!(
            diagnostic(&source),
            "2:10: Plenty has no `is` operator; test an Option with `.is_none()` or `.is_some()`, or `match` it, and compare values with `==`",
            "{test}"
        );
    }
}

#[test]
fn string_repetition_names_repeat() {
    for (expression, column) in [("\"-\" * 3", 9), ("3 * \"-\"", 13), ("s * n", 9)] {
        let source = format!("def main() -> ():\n    s = \"-\"\n    n = 3\n    t = {expression}\n");
        assert_eq!(
            diagnostic(&source),
            format!("4:{column}: strings do not support `*` repetition; use `text.repeat(count)?`"),
            "{expression}"
        );
    }
    support::assert_stdout(
        "def main() -> Result[(), AllocError | IoError]:\n    print(\"-\".repeat(3)?)?\n    Ok(())\n",
        "---\n",
    );
}

#[test]
fn variadic_parameters_name_a_list_parameter() {
    for parameter in ["*args: i64", "**kwargs: i64", "n: i64, *rest: i64"] {
        let source = format!("def f({parameter}) -> ():\n    pass\ndef main() -> ():\n    pass\n");
        let error = diagnostic(&source);
        assert!(
            error.ends_with(": variadic parameters (`*args`, `**kwargs`) are not supported; take a list parameter, such as `values: &list[i64]`"),
            "{parameter}: {error}"
        );
    }
}

#[test]
fn unused_keywords_name_the_plenty_form() {
    const ASYNC: &str = "Plenty has no `async`/`await`; write an ordinary `def` and run concurrent work explicitly, such as on a `std.concurrent.ThreadPoolExecutor`";
    for (source, expected) in [
        (
            "async def f() -> ():\n    pass\ndef main() -> ():\n    pass\n".to_owned(),
            format!("1:1: {ASYNC}"),
        ),
        (
            "def f() -> i64:\n    1\ndef main() -> ():\n    n = await f()\n".to_owned(),
            format!("4:9: {ASYNC}"),
        ),
        (
            "struct P:\n    x: i64\ndef main() -> ():\n    pass\n".to_owned(),
            "1:1: Plenty has no `struct`; declare a `class` with typed fields".to_owned(),
        ),
        (
            "trait Shape:\n    def area(self) -> f64\ndef main() -> ():\n    pass\n".to_owned(),
            "1:1: Plenty has no `trait`; declare a `protocol`".to_owned(),
        ),
    ] {
        assert_eq!(diagnostic(&source), expected, "{source}");
    }
}

#[test]
fn print_named_arguments_name_the_line_rule() {
    for argument in ["end=\"\"", "sep=\",\""] {
        let source = format!(
            "def main() -> Result[(), IoError]:\n    print(\"a\", {argument})?\n    Ok(())\n"
        );
        assert_eq!(
            diagnostic(&source),
            "2:16: `print` takes no named arguments such as `end` or `sep`: it separates values with one space and ends the line; build the line first, such as with an f-string, and print it once",
            "{argument}"
        );
    }
}

#[test]
fn sort_reverse_names_the_positional_flag() {
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), AllocError]:\n    mut xs = [2, 1]?\n    xs.sort(reverse=True)\n    Ok(())\n"
        ),
        "3:13: `sort` takes the descending flag positionally: `items.sort(True)`"
    );
    assert_eq!(
        diagnostic(
            "def key(x: &i64) -> i64:\n    *x\ndef order(xs: &mut list[i64]) -> ():\n    xs.sort_by_key(key, reverse=True)\ndef main() -> ():\n    pass\n"
        ),
        "4:25: `sort_by_key` takes the descending flag positionally: `items.sort_by_key(key, True)`"
    );
    support::assert_stdout(
        "def main() -> Result[(), AllocError | IoError]:\n    mut xs = [1, 3, 2]?\n    xs.sort(True)\n    print(xs)?\n    Ok(())\n",
        "[3, 2, 1]\n",
    );
}

#[test]
fn a_user_sort_keeps_its_named_arguments() {
    support::assert_stdout(
        "class Crate:\n    n: i64\n    def sort(self, reverse: bool) -> bool:\n        reverse\ndef main() -> Result[(), IoError]:\n    b = Crate(1)\n    print(b.sort(reverse=True))?\n    Ok(())\n",
        "True\n",
    );
}

#[test]
fn tuple_component_assignment_names_the_component_reference() {
    assert_eq!(
        diagnostic("def main() -> ():\n    mut q = (1, 2)\n    q[0] = 5\n"),
        "3:5: tuples have no indexed assignment; assign through a component reference: `item = &mut pair[0]`, then `*item = value`"
    );
    support::assert_stdout(
        "def main() -> Result[(), IoError]:\n    mut q = (1, 2)\n    item = &mut q[0]\n    *item = 5\n    print(q)?\n    Ok(())\n",
        "(5, 2)\n",
    );
}

#[test]
fn get_of_owned_values_names_borrowing_lookups() {
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), AllocError]:\n    d = {\"a\": [1]?}?\n    v = d.get(\"a\")\n    Ok(())\n"
        ),
        "3:9: get cannot return owned dictionary values; borrow with `&items[key]`, which traps when absent, `get_mut(key)` for an optional exclusive borrow, copy with `copy(&items[key])?`, or remove and take ownership with `pop`"
    );
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), AllocError]:\n    xs = [[1]?]?\n    v = xs.get(0)\n    Ok(())\n"
        ),
        "3:9: get cannot return owned list values; borrow with `&items[index]`, which traps when absent, copy with `copy(&items[index])?`, or remove and take ownership with `pop`"
    );
    support::assert_stdout(
        "def main() -> Result[(), AllocError | IoError]:\n    d = {\"a\": [1]?}?\n    r = &d[\"a\"]\n    c = copy(&d[\"a\"])?\n    print(len(r), c)?\n    Ok(())\n",
        "1 [1]\n",
    );
}
