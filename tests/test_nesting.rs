//! An `if` holds its `elif` arms as a list, so a chain of any length compiles.
//! Blocks and expressions nest to 64 levels in total; deeper source is
//! rejected with a diagnostic. Both once overflowed the compiler's stack
//! (issue #70).

mod support;
use support::run_stdout as stdout;

use rstest::rstest;
use std::process::Command;

const LIMIT: &str = "block and expression nesting exceeds the implementation limit of 64";

fn chain(arms: usize) -> String {
    let mut source = String::from("def pick(x: i64) -> i64:\n    if x == 0:\n        0\n");
    for i in 1..arms {
        source.push_str(&format!("    elif x == {i}:\n        {}\n", i * 3));
    }
    source.push_str("    else:\n        -1\n");
    source
}

#[rstest]
#[case(4_000)]
#[case(20_000)]
fn long_elif_chain_selects_each_arm(#[case] arms: usize) {
    let last = arms - 1;
    let source = format!(
        "{}
def main() -> Result[(), Failure]:
    print(pick(0))?
    print(pick(7))?
    print(pick({last}))?
    print(pick({arms}))?
    Ok(())
",
        chain(arms)
    );
    assert_eq!(stdout(&source), format!("0\n21\n{}\n-1\n", last * 3));
}

#[test]
fn long_elif_chain_without_else_assigns_returns_and_falls_through() {
    const ARMS: usize = 4_000;
    let mut source =
        String::from("def pick(x: i64) -> i64:\n    mut r = 5\n    if x == 0:\n        r = 0\n");
    for i in 1..ARMS {
        source.push_str(&format!("    elif x == {i}:\n"));
        source.push_str(&match i % 3 {
            0 => format!("        r = {i}\n"),
            1 => format!("        return {i}\n"),
            _ => format!("        r = r + {i}\n"),
        });
    }
    source.push_str(
        "    r + 1

def main() -> Result[(), Failure]:
    print(pick(3000))?
    print(pick(3001))?
    print(pick(3002))?
    print(pick(-1))?
    Ok(())
",
    );
    assert_eq!(stdout(&source), "3001\n3001\n3008\n6\n");
}

#[test]
fn elif_arms_keep_the_rules_of_nested_conditionals() {
    assert_eq!(
        stdout(
            r#"
class Pair:
    x: i64
    y: i64
    def __init__(self, x: i64, kind: i64) -> ():
        if kind == 0:
            self.x = x
        elif kind == 1:
            self.x = -x
        elif kind == 2:
            self.x = 0
        else:
            self.x = 9
        self.y = self.x + 1

def count(n: i64, total: i64) -> i64:
    if n == 0:
        total
    elif n % 2 == 0:
        count(n - 1, total + 2)
    elif n % 3 == 0:
        count(n - 1, total + 3)
    else:
        count(n - 1, total + 1)

def probe(label: i64, answer: bool) -> bool:
    print(label).unwrap()
    answer

def loops(stop: i64) -> i64:
    mut total = 0
    mut i = 0
    while i < 10:
        i = i + 1
        if i == 1:
            continue
        elif i == stop:
            break
        elif i == 4:
            total = total + 100
        elif i == 9:
            return -total
        total = total + i
    total

def keep(name: str, kind: i64) -> Result[i64, Failure]:
    if kind == 0:
        drop(name)
        return Ok(0)
    elif kind == 1:
        print(&name)?
    elif name == "b":
        return Ok(2)
    Ok(len(&name))

def main() -> Result[(), Failure]:
    print(Pair(3, 1))?
    print(Pair(3, 5))?
    # A call in an arm is still a tail call: this depth would overflow otherwise.
    print(count(1000000, 0))?
    if probe(1, False):
        print(10)?
    elif probe(2, True):
        print(20)?
    elif probe(3, True):
        print(30)?
    else:
        print(40)?
    print(loops(6))?
    print(loops(20))?
    print(keep("abc", 0)?)?
    print(keep("abc", 1)?)?
    print(keep("b", 2)?)?
    print(keep("abcd", 2)?)?
    Ok(())
"#
        ),
        "Pair(x=-3, y=-2)\nPair(x=9, y=10)\n1833334\n1\n2\n20\n114\n-135\n0\nabc\n3\n2\n4\n"
    );
}

#[rstest]
#[case::condition_type(
    "def f(k: i64) -> i64:\n    if k == 0:\n        1\n    elif k:\n        2\n    else:\n        3",
    "4:10: expected bool, got i64"
)]
#[case::arm_type(
    "def f(k: i64) -> i64:\n    if k == 0:\n        1\n    elif k == 1:\n        True\n    else:\n        3",
    "5:9: expected i64, got bool"
)]
#[case::every_arm_exits(
    "def f(k: i64) -> i64:\n    if k == 0:\n        return 1\n    elif k == 1:\n        return 2\n    else:\n        return 3\n    4",
    "8:5: unreachable statement after a control-flow exit"
)]
#[case::move_in_one_arm(
    "def f(name: str, k: i64) -> i64:\n    if k == 0:\n        pass\n    elif k == 1:\n        drop(name)\n    elif k == 2:\n        pass\n    len(&name)",
    "8:10: use of possibly moved binding `name`\n  5:14: note: `name` is moved here on some paths"
)]
#[case::field_missing_in_one_arm(
    "class Pair:\n    x: i64\n    def __init__(self, k: i64) -> ():\n        if k == 0:\n            self.x = 1\n        elif k == 1:\n            pass\n        else:\n            self.x = 9",
    "3:5: fields not initialized: x"
)]
fn elif_arms_keep_their_diagnostics(#[case] declaration: &str, #[case] expected: &str) {
    let source = format!("{declaration}\ndef main() -> ():\n    pass\n");
    assert_eq!(
        plenty::check_source(&source).unwrap_err().to_string(),
        expected
    );
}

const MAIN: &str = "\ndef main() -> Result[(), Failure]:\n    print(pick(7))?\n    Ok(())\n";

/// `depth` suites inside one another around `r = 1`. `open` writes the lines
/// that begin a suite `step` indents deeper.
fn blocks(depth: usize, step: usize, open: impl Fn(&str) -> String) -> String {
    let indent = |level: usize| "    ".repeat(1 + level * step);
    let mut source = String::from("def pick(x: i64) -> i64:\n    mut r = 0\n    o = Some(x)\n");
    for level in 0..depth {
        source.push_str(&open(&indent(level)));
    }
    source.push_str(&format!("{}r = 1\n    r\n{MAIN}", indent(depth)));
    source
}

fn ifs(depth: usize) -> String {
    blocks(depth, 1, |i| format!("{i}if x == 7:\n"))
}

fn elses(depth: usize) -> String {
    blocks(depth, 1, |i| {
        format!("{i}if x == 0:\n{i}    r = 2\n{i}else:\n")
    })
}

fn whiles(depth: usize) -> String {
    blocks(depth, 1, |i| format!("{i}while r < 1:\n"))
}

fn fors(depth: usize) -> String {
    blocks(depth, 1, |i| format!("{i}for _ in range(1):\n"))
}

fn matches(depth: usize) -> String {
    blocks(depth, 2, |i| {
        format!("{i}match &o:\n{i}    case Nothing:\n{i}        r = 2\n{i}    case Some(_):\n")
    })
}

fn value(ty: &str, expression: &str) -> String {
    format!("def pick(x: i64) -> {ty}:\n    {expression}\n{MAIN}")
}

/// `pick` returns the value of a fallible `body`, which ends in `Ok(...)`.
fn fallible(body: &str) -> String {
    format!(
        "def same(x: i64) -> Result[i64, Failure]:\n    Ok(x)\n\ndef inner(x: i64) -> Result[i64, Failure]:\n{body}\ndef pick(x: i64) -> i64:\n    match inner(x):\n        case Ok(v):\n            v\n        case Err(_):\n            -1\n{MAIN}"
    )
}

fn sums(operators: usize) -> String {
    value("i64", &format!("x{}", " + 1".repeat(operators)))
}

fn conjunctions(operators: usize) -> String {
    value(
        "bool",
        &format!("x == 7{}", " and x == 7".repeat(operators)),
    )
}

fn disjunctions(operators: usize) -> String {
    value("bool", &format!("x == 0{}", " or x == 7".repeat(operators)))
}

fn parentheses(depth: usize) -> String {
    value(
        "i64",
        &format!("{}x{}", "(".repeat(depth), ")".repeat(depth)),
    )
}

fn negations(depth: usize) -> String {
    value("bool", &format!("{}x == 7", "not ".repeat(depth)))
}

fn conditionals(depth: usize) -> String {
    let arms: String = (0..depth)
        .map(|i| format!("{i} if x == {i} else "))
        .collect();
    value("i64", &format!("{arms}-1"))
}

fn calls(depth: usize) -> String {
    format!(
        "def f(x: i64) -> i64:\n    x\n{}",
        value(
            "i64",
            &format!("{}x{}", "f(".repeat(depth), ")".repeat(depth))
        )
    )
}

fn methods(calls: usize) -> String {
    value("i64", &format!("x{}", ".wrapping_add(1)".repeat(calls)))
}

fn indexes(depth: usize) -> String {
    fallible(&format!(
        "    xs = [0]?\n    y = {}0{}\n    Ok(y + x)\n",
        "xs[".repeat(depth),
        "]".repeat(depth)
    ))
}

fn propagations(depth: usize) -> String {
    fallible(&format!(
        "    y = {}x{}\n    Ok(y)\n",
        "same(".repeat(depth),
        ")?".repeat(depth)
    ))
}

fn tuples(depth: usize) -> String {
    format!(
        "def pick(x: i64) -> i64:\n    t = {}x{}\n    x\n{MAIN}",
        "(".repeat(depth),
        ", 1)".repeat(depth)
    )
}

fn managers(count: usize) -> String {
    format!(
        "class Scope:\n    v: i64\n    def __enter__(self: &mut Scope) -> ():\n        ()\n    def __exit__(self: &mut Scope) -> ():\n        ()\n\n{}",
        fallible(&format!(
            "    mut y = 0\n    with {}:\n        y = x\n    Ok(y)\n",
            vec!["Scope(x)"; count].join(", ")
        ))
    )
}

fn clauses(conditions: usize) -> String {
    fallible(&format!(
        "    xs = [v for v in range(8){}]?\n    Ok(xs[0])\n",
        " if v == x".repeat(conditions)
    ))
}

/// The compiler as a process: on a test thread's 2 MB stack an unoptimized
/// compiler cannot reach the limit that its 8 MB main thread can.
fn compile_and_run(source: &str) -> (bool, String, String) {
    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().join("nesting.plenty");
    std::fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg(&path)
        .output()
        .unwrap();
    let prefix = format!("error: {}:", path.display());
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr)
            .unwrap()
            .replace(&prefix, ""),
    )
}

/// A function body is level 1 and a statement's expression is one level
/// below its block, so `most` differs by shape. `at` is the token that
/// reaches level 65 with one more.
#[rstest]
#[case::ifs(ifs, 62, "1\n", "66:258")]
#[case::elses(elses, 62, "1\n", "190:258")]
#[case::whiles(whiles, 62, "1\n", "66:261")]
#[case::fors(fors, 62, "1\n", "66:268")]
#[case::matches(matches, 62, "1\n", "252:508")]
#[case::sums(sums, 62, "69\n", "2:255")]
#[case::conjunctions(conjunctions, 61, "True\n", "2:683")]
#[case::disjunctions(disjunctions, 61, "True\n", "2:622")]
#[case::parentheses(parentheses, 62, "7\n", "2:68")]
#[case::negations(negations, 61, "False\n", "2:255")]
#[case::conditionals(conditionals, 61, "7\n", "2:1152")]
#[case::calls(calls, 62, "7\n", "4:131")]
#[case::methods(methods, 62, "69\n", "2:998")]
#[case::indexes(indexes, 62, "7\n", "6:197")]
#[case::propagations(propagations, 31, "7\n", "5:231")]
#[case::tuples(tuples, 62, "7\n", "2:72")]
#[case::managers(managers, 62, "7\n", "14:13")]
#[case::clauses(clauses, 59, "7\n", "5:631")]
fn nesting_is_accepted_to_the_limit_and_rejected_beyond_it(
    #[case] shape: fn(usize) -> String,
    #[case] most: usize,
    #[case] printed: &str,
    #[case] at: &str,
) {
    let (succeeded, stdout, stderr) = compile_and_run(&shape(most));
    assert!(succeeded, "{stderr}");
    assert_eq!(stdout, printed);
    let (succeeded, _, stderr) = compile_and_run(&shape(most + 1));
    assert!(!succeeded);
    assert_eq!(stderr, format!("{at}: {LIMIT}\n"));
}

/// `ident[...](...)` is also tried as an index expression. That discarded
/// reading must not count: types have their own limit of 64.
#[test]
fn type_arguments_are_not_expression_nesting() {
    let argument = format!("{}i64{}", "Option[".repeat(63), "]".repeat(63));
    let (succeeded, stdout, stderr) = compile_and_run(&format!(
        "def ident[T](x: T) -> T:\n    x\n\ndef main() -> Result[(), Failure]:\n    v = ident[{argument}](Nothing)\n    print(7)?\n    Ok(())\n"
    ));
    assert!(succeeded, "{stderr}");
    assert_eq!(stdout, "7\n");
}

/// The parser must stop at the limit however deep the source goes on.
#[rstest]
#[case::ifs(ifs, 1_000)]
#[case::matches(matches, 300)]
#[case::sums(sums, 100_000)]
#[case::disjunctions(disjunctions, 100_000)]
#[case::parentheses(parentheses, 100_000)]
#[case::negations(negations, 100_000)]
#[case::conditionals(conditionals, 100_000)]
#[case::calls(calls, 100_000)]
#[case::methods(methods, 100_000)]
#[case::propagations(propagations, 100_000)]
#[case::managers(managers, 100_000)]
#[case::clauses(clauses, 100_000)]
fn far_deeper_nesting_is_rejected_without_recursing(
    #[case] shape: fn(usize) -> String,
    #[case] depth: usize,
) {
    let error = plenty::check_source(&shape(depth)).unwrap_err().to_string();
    assert!(error.ends_with(LIMIT), "{error}");
}
