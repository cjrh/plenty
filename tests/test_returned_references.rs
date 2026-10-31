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

#[test]
fn assignment_writes_through_a_returned_reference() {
    // Issue #598: `*f(...) = v` and `*f(...) op= v` need no named binding.
    run(
        r#"
def first(xs: &mut list[i64]) -> &mut i64:
    &mut xs[0]
def head(xs: &mut [i64]) -> &mut i64:
    &mut xs[0]
def name(xs: &mut list[str]) -> &mut str:
    &mut xs[1]
def bump(xs: &mut list[i64]) -> ():
    *first(xs) += 100
class Counter:
    calls: i64
    n: i64
    def slot(self: &mut Counter) -> &mut i64:
        self.calls += 1
        &mut self.n
mut xs = [1, 2, 3].unwrap()
*first(&mut xs) = 9
*first(&mut xs) += 1
*head(&mut xs[1:]) = 7
*head(&mut xs[2:]) *= 4
*(head(&mut xs[:2])) -= 1
print(xs).unwrap()
bump(&mut xs)
print(xs).unwrap()
mut names = ["a", "b"].unwrap()
*name(&mut names) = "zed"
print(names).unwrap()
mut c = Counter(0, 1)
*c.slot() = 5
*c.slot() += 2
print(c.n, c.calls).unwrap()
"#,
        "[9, 7, 12]\n[109, 7, 12]\n[\"a\", \"zed\"]\n7 2\n",
    );
}

#[test]
fn a_returned_reference_keeps_its_argument_borrowed_through_the_assignment() {
    for op in ["=", "+="] {
        reject(
            &format!(
                "def first(xs: &mut list[i64]) -> &mut i64:\n    &mut xs[0]\nmut xs = [1, 2].unwrap()\n*first(&mut xs) {op} xs[1]\n"
            ),
            "conflicting borrow",
        );
        reject(
            &format!(
                "def first(xs: &list[i64]) -> &i64:\n    &xs[0]\nxs = [1, 2].unwrap()\n*first(&xs) {op} 3\n"
            ),
            "assignment requires an exclusive reference",
        );
    }
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
        "borrow from the first reference parameter",
    );
    reject(
        "def bad(input: &i64) -> &i64:\n    value = 1\n    &value\n",
        "must originate",
    );
    reject(
        "def bad(a: &i64) -> &mut i64:\n    &mut a\n",
        "which must be `&mut`",
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

#[test]
fn the_first_reference_parameter_is_the_origin_of_a_returned_reference() {
    run(
        r#"
class Entry:
    name: str
    score: i64
class Table:
    entries: list[Entry]
    def score_of(self: &mut Table, name: &str) -> &mut i64:
        mut i = 0
        while i < len(self.entries) - 1:
            if self.entries[i].name == name:
                return &mut self.entries[i].score
            i = i + 1
        &mut self.entries[len(self.entries) - 1].score
def lookup(scores: &dict[str, i64], key: &str) -> &i64:
    &scores[*key]
mut table = Table([Entry("ada", 3), Entry("bob", 5)].unwrap())
score = table.score_of("ada")
*score = *score + 10
print(table.entries[0].score).unwrap()
scores = {"x": 4}.unwrap()
mut name = "x"
value = lookup(&scores, &name)
name = "changed"
print(value).unwrap()
print(name).unwrap()
"#,
        "13\n4\nchanged\n",
    );
}

#[test]
fn later_reference_arguments_are_released_after_the_call() {
    run(
        r#"
def add_into(target: &mut i64, amount: &i64) -> &mut i64:
    *target = *target + *amount
    target
mut total = 1
mut step = 2
result = add_into(&mut total, &step)
step = 7
*result = *result * 10
print(total).unwrap()
print(step).unwrap()
"#,
        "30\n7\n",
    );
    // A temporary later argument does not prevent binding the result.
    run(
        r#"
def first(items: &list[i64], label: &str) -> &i64:
    &items[0]
items = [5, 6].unwrap()
value = first(&items, "label")
print(value).unwrap()
"#,
        "5\n",
    );
}

#[test]
fn a_returned_reference_cannot_originate_in_a_later_reference_parameter() {
    reject(
        "def pick(a: &i64, b: &i64) -> &i64:\n    b\n",
        "must originate from the first reference parameter",
    );
    reject(
        "def pick(a: &i64, b: &i64) -> &i64:\n    if *a > *b:\n        return a\n    b\n",
        "must originate from the first reference parameter",
    );
    reject(
        "def other(a: &i64, b: &i64) -> &i64:\n    a\ndef pick(a: &i64, b: &i64) -> &i64:\n    other(b, a)\n",
        "must originate from the first reference parameter",
    );
    reject(
        "class Pair:\n    x: i64\ndef pick(a: &Pair, b: &Pair) -> &i64:\n    &b.x\n",
        "must originate from the first reference parameter",
    );
    // `b` is exclusive, but the result must come from `a`, which is shared.
    reject(
        "def pick(a: &i64, b: &mut i64) -> &mut i64:\n    b\n",
        "which must be `&mut`",
    );
}

#[test]
fn the_origin_argument_stays_borrowed_and_temporary_origins_cannot_escape() {
    reject(
        "def pick(a: &i64, b: &i64) -> &i64:\n    a\nmut x = 1\ny = 2\nr = pick(&x, &y)\nx = 5\nprint(r).unwrap()\n",
        "conflicting borrow",
    );
    reject(
        "def pick(a: &mut i64, b: &i64) -> &mut i64:\n    a\nmut x = 1\nr = pick(&mut x, &x)\nprint(r).unwrap()\n",
        "conflicting borrow",
    );
    reject(
        "def view(a: &str, b: &str) -> &str:\n    a\nr = view(\"temporary\", \"other\")\nprint(r).unwrap()\n",
        "temporary cannot be stored",
    );
}

#[test]
fn recursive_lookups_with_a_key_parameter_reuse_the_frame() {
    run(
        r#"
class Node:
    value: i64
    next: Option[Box[Node]]
def find(node: &mut Node, key: &i64) -> &mut i64:
    if node.value == *key:
        return &mut node.value
    match &mut node.next:
        case Some(next):
            return find(next, key)
        case Nothing:
            return &mut node.value
mut head = Node(0, Nothing)
mut i = 1
while i < 200000:
    head = Node(i, Some(Box(head).unwrap()))
    i = i + 1
key = 5
found = find(&mut head, &key)
*found = 99
print(*find(&mut head, &99)).unwrap()
"#,
        "99\n",
    );
}

#[test]
fn generic_and_callable_signatures_use_the_same_origin_rule() {
    run(
        r#"
def first_of[T](a: &list[T], b: &list[T]) -> &list[T]:
    a
def apply(f: Callable[[&list[i64], &list[i64]], &list[i64]], x: &list[i64], y: &list[i64]) -> i64:
    len(f(x, y))
xs = [1, 2, 3].unwrap()
mut ys = [4].unwrap()
chosen = first_of(&xs, &ys)
ys = [5, 6].unwrap()
print(len(chosen)).unwrap()
print(apply(first_of, &xs, &ys)).unwrap()
"#,
        "3\n3\n",
    );
}
