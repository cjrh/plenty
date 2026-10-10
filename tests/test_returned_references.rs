mod support;
use std::process::Command;

#[test]
fn temporary_argument_references_cannot_escape_the_full_expression() {
    reject(
        "def view(s: &str) -> &str:\n    s\nr = view(\"temporary\")\nprint(r).unwrap()\n",
        "temporary cannot be stored",
    );
    reject(
        "def view(s: &str) -> &str:\n    s\ndef bad(s: &str) -> &str:\n    view(\"temporary\")\n",
        "must originate",
    );
    reject(
        "def worker(s: &str) -> i64:\n    len(s)\nwith spawn(worker, \"temporary\").unwrap() as task:\n    print(task.join().unwrap()).unwrap()\n",
        "temporary cannot be stored",
    );
}

#[test]
fn direct_returned_fields_allow_disjoint_access_without_losing_conflicts() {
    run(
        r#"
class Point:
    x: i64
    y: i64
class Pair:
    left: Point
    right: Point
    def left_ref(self: &mut Pair) -> &mut Point:
        &mut self.left
mut pair = Pair(Point(1, 2), Point(3, 4))
left = pair.left_ref()
x = &mut left.x
pair.right.x = 7
*x = 9
print(x).unwrap()
print(pair).unwrap()
"#,
        "9\nPair(left=Point(x=9, y=2), right=Point(x=7, y=4))\n",
    );
    reject(
        r#"
class Pair:
    x: i64
    y: i64
def select(p: &mut Pair, which: bool) -> &mut i64:
    if which:
        return &mut p.x
    &mut p.y
mut pair = Pair(1, 2)
r = select(&mut pair, True)
pair.y = 3
print(r).unwrap()
"#,
        "conflicting borrow",
    );
}

fn run(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
fn reject(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let error = support::compile_source_to_executable(source, &dir.path().join("program"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected}, got {error}"
    );
}

#[test]
fn functions_and_methods_return_borrows_of_their_parameter() {
    run(
        r#"
def identity(value: &mut i64) -> &mut i64:
    return &mut value
def view(value: &i64) -> &i64:
    value
class Pair:
    x: i64
    y: i64
    def x_ref(self: &mut Pair) -> &mut i64:
        &mut self.x
mut pair = Pair(2, 3)
x = pair.x_ref()
*x = 5
mut number = 7
alias = identity(&mut number)
*alias = *alias + 1
print(*view(&number)).unwrap()
print(pair.x).unwrap()
"#,
        "8\n5\n",
    );
}

#[test]
fn return_paths_can_select_different_fields_of_one_origin() {
    run(
        r#"
class Pair:
    x: i64
    y: i64
def select(pair: &mut Pair, left: bool) -> &mut i64:
    if left:
        return &mut pair.x
    &mut pair.y
mut p = Pair(1, 2)
r = select(&mut p, False)
*r = 8
print(p).unwrap()
"#,
        "Pair(x=1, y=8)\n",
    );
}

#[test]
fn returned_borrows_preserve_conflicts_and_cannot_escape_owned_locals() {
    reject(
        "def bad() -> &i64:\n    value = 1\n    &value\n",
        "exactly one reference parameter",
    );
    reject(
        "def bad(input: &i64) -> &i64:\n    value = 1\n    &value\n",
        "must originate",
    );
    reject(
        "def bad(a: &i64, b: &i64) -> &i64:\n    a\n",
        "exactly one reference parameter",
    );
    reject(
        "def bad(a: &i64) -> &mut i64:\n    &mut a\n",
        "mutable reference parameter",
    );
    reject(
        "def view(a: &i64) -> &i64:\n    a\nmut n = 1\nr = view(&n)\nn = 2\nprint(r).unwrap()\n",
        "conflicting borrow",
    );
    reject(
        "def view(a: &i64) -> &i64:\n    a\ndef bad(a: &i64) -> &i64:\n    n = 1\n    view(&n)\n",
        "must originate",
    );
}

#[test]
fn returned_projection_footprint_is_conservative_after_further_projection() {
    let source = r#"
class Point:
    x: i64
class Pair:
    left: Point
    right: Point
def right(pair: &mut Pair) -> &mut Point:
    &mut pair.right
mut pair = Pair(Point(1), Point(2))
r = right(&mut pair)
x = &mut r.x
pair.right.x = 9
print(x).unwrap()
"#;
    reject(source, "conflicting borrow");
}

#[test]
fn local_reference_bindings_can_be_returned_and_shared_returns_allow_reads() {
    run(
        r#"
def forward(a: &i64) -> &i64:
    r = &a
    return r
n = 9
r = forward(&n)
print(n).unwrap()
print(r).unwrap()
"#,
        "9\n9\n",
    );
}

#[test]
fn conditional_reference_values_do_not_guess_an_origin() {
    reject(
        r#"
def view(value: &i64) -> &i64:
    value
left = 1
right = 2
prior = &left
print(view(&left) if True else view(&right)).unwrap()
"#,
        "reference result requires",
    );
}

#[test]
fn projected_temporary_receivers_cannot_hide_returned_loan_origins() {
    reject(
        r#"
class Point:
    x: i64
    def view(self) -> &i64:
        &self.x
print(Point(1).view()).unwrap()
"#,
        "require a named receiver",
    );
    reject(
        r#"
class Point:
    x: i64
    def view(self) -> &i64:
        &self.x
print([Point(1)].unwrap()[0].view()).unwrap()
"#,
        "require a named receiver",
    );
}
