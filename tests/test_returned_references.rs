mod support;
use std::process::Command;

#[test]
fn indexed_borrows_keep_the_projection_origin_not_the_index_origin() {
    for (parameters, expression) in [
        ("xs: &[i64], other: &[i64]", "&other[index(xs)]"),
        ("xs: &i64, other: &[i64]", "&other[xs]"),
        ("xs: &list[i64], other: &list[i64]", "&other[index(xs)]"),
        ("xs: &mut [i64], other: &mut [i64]", "&mut other[index(xs)]"),
        ("xs: &[i64], other: &dict[i64, i64]", "&other[index(xs)]"),
        (
            "xs: &[i64], other: &list[list[i64]]",
            "&other[index(xs)][0]",
        ),
    ] {
        let result = if expression.starts_with("&mut") {
            "&mut i64"
        } else {
            "&i64"
        };
        reject_views(
            &format!(
                "def index(xs: &[i64]) -> i64:\n    0\ndef wrong({parameters}) -> {result}:\n    {expression}\n"
            ),
            "must originate",
        );
        reject_views(
            &format!(
                "def index(xs: &[i64]) -> i64:\n    0\ndef wrong({parameters}) -> Option[{result}]:\n    Some({expression})\n"
            ),
            "must originate",
        );
    }
    reject_views(
        "def index(xs: &[i64]) -> i64:\n    0\ndef forward(xs: &[i64]) -> &[i64]:\n    xs\ndef wrong(xs: &[i64], other: &[i64]) -> &i64:\n    &forward(other)[index(xs)]",
        "must originate",
    );
    // The original wrong-owner-drop reproduction must fail in the callee,
    // before the caller can observe storage freed under its returned reference.
    reject_views(
        "def index(xs: &[i64]) -> i64:\n    0\ndef wrong(xs: &[i64], other: &[i64]) -> &i64:\n    &other[index(xs)]\na = [1, 2].unwrap()\nb = [7, 8].unwrap()\nr = wrong(&a, &b)\ndrop(b)\nprint(*r).unwrap()",
        "must originate",
    );
    reject_views(
        "def index(xs: &[i64]) -> i64:\n    0\ndef local(xs: &[i64]) -> &i64:\n    other = [7].unwrap()\n    &other[index(xs)]",
        "must originate",
    );
}

#[test]
fn indexed_borrows_do_not_keep_the_index_arguments_borrowed() {
    run_views(
        r#"
def index(xs: &[i64]) -> i64:
    print(xs[0]).unwrap()
    0
def first(xs: &[i64], indices: &[i64]) -> &i64:
    &xs[index(indices)]
def first_mut(xs: &mut [i64], indices: &[i64]) -> &mut i64:
    &mut xs[index(indices)]
xs = [7, 8].unwrap()
indices = [1].unwrap()
r = first(&xs, &indices)
drop(indices)
print(*r).unwrap()
mut ys = [9].unwrap()
indices2 = [2].unwrap()
m = first_mut(&mut ys, &indices2)
drop(indices2)
*m = 10
print(*m).unwrap()
"#,
        "1\n7\n2\n10\n",
    );
    run_views(
        "def forward(xs: &[i64]) -> &[i64]:\n    print(0).unwrap()\n    xs\ndef index(xs: &[i64]) -> i64:\n    print(1).unwrap()\n    0\ndef first(xs: &[i64], indices: &[i64]) -> &i64:\n    &forward(xs)[index(indices)]\nxs = [7].unwrap()\nindices = [1].unwrap()\nr = first(&xs, &indices)\ndrop(indices)\nprint(*r).unwrap()",
        "0\n1\n7\n",
    );
    reject_views(
        "def index(xs: &[i64]) -> i64:\n    0\ndef first(xs: &[i64], indices: &[i64]) -> &i64:\n    &xs[index(indices)]\nxs = [7].unwrap()\nindices = [1].unwrap()\nr = first(&xs, &indices)\ndrop(xs)\nprint(*r).unwrap()",
        "use of moved binding",
    );
    // A call-produced view is evaluated first, so its existing address must
    // keep the owner borrowed through the index call rather than move later.
    reject_views(
        "def forward(xs: &[i64]) -> &[i64]:\n    xs\ndef index(xs: &mut list[i64]) -> i64:\n    xs.append(9).unwrap()\n    0\nmut xs = [7].unwrap()\nr = &forward(&xs)[index(&mut xs)]\nprint(*r).unwrap()",
        "conflicting borrow",
    );
    // Evaluate the index before taking an element address, exactly once even
    // when user code resizes the owner. Nested indices keep source order.
    run_views(
        "def index(xs: &mut list[i64]) -> i64:\n    xs.append(9).unwrap()\n    print(len(xs)).unwrap()\n    1\nmut xs = [7].unwrap()\nr = &xs[index(&mut xs)]\nprint(*r).unwrap()\ndef key(n: i64) -> i64:\n    print(n).unwrap()\n    0\nrows = [[5].unwrap()].unwrap()\np = &rows[key(1)][key(2)]\nprint(*p).unwrap()",
        "2\n9\n1\n2\n5\n",
    );
    run_views(
        "def index(xs: &mut list[str]) -> i64:\n    xs.append(\"b\").unwrap()\n    1\nmut xs = [\"a\"].unwrap()\nr = &xs[index(&mut xs)]\nprint(*r).unwrap()",
        "b\n",
    );
}

#[test]
fn nested_projection_views_keep_their_actual_owner() {
    reject_views(
        "def index(xs: &[i64]) -> i64:\n    0\ndef wrong(xs: &[i64], other: &list[list[i64]]) -> &[i64]:\n    &other[index(xs)][:]",
        "must originate",
    );
    run_views(
        "def index(xs: &[i64]) -> i64:\n    0\ndef first(xs: &list[list[i64]], indices: &[i64]) -> &[i64]:\n    &xs[index(indices)][:]\nxs = [[7].unwrap()].unwrap()\nindices = [1].unwrap()\nv = first(&xs, &indices)\ndrop(indices)\nprint(v[0]).unwrap()",
        "7\n",
    );
}

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
def bump(xs: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    *first(xs) += 100
    Ok(())
class Counter:
    calls: i64
    n: i64
    def slot(self: &mut Counter) -> &mut i64:
        # Fixture-only: main starts calls at 0 and invokes slot exactly twice.
        # Keep the returned-reference ABI; these calls advance it only to 2.
        unchecked:
            self.calls += 1
        &mut self.n
def main() -> Result[(), OverflowError[i64]]:
    mut xs = [1, 2, 3].unwrap()
    *first(&mut xs) = 9
    *first(&mut xs) += 1
    *head(&mut xs[1:]) = 7
    *head(&mut xs[2:]) *= 4
    *(head(&mut xs[:2])) -= 1
    print(xs).unwrap()
    bump(&mut xs)?
    print(xs).unwrap()
    mut names = ["a", "b"].unwrap()
    *name(&mut names) = "zed"
    print(names).unwrap()
    mut c = Counter(0, 1)
    *c.slot() = 5
    *c.slot() += 2
    print(c.n, c.calls).unwrap()
    Ok(())
"#,
        "[9, 7, 12]\n[109, 7, 12]\n[\"a\", \"zed\"]\n7 2\n",
    );
}

#[test]
fn a_returned_reference_keeps_its_argument_borrowed_through_the_assignment() {
    for op in ["=", "+="] {
        reject(
            &format!(
                "def first(xs: &mut list[i64]) -> &mut i64:\n    &mut xs[0]\ndef main() -> Result[(), OverflowError[i64]]:\n    mut xs = [1, 2].unwrap()\n    *first(&mut xs) {op} xs[1]\n    Ok(())\n"
            ),
            "conflicting borrow",
        );
        reject(
            &format!(
                "def first(xs: &list[i64]) -> &i64:\n    &xs[0]\ndef main() -> Result[(), OverflowError[i64]]:\n    xs = [1, 2].unwrap()\n    *first(&xs) {op} 3\n    Ok(())\n"
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
def main() -> Result[(), OverflowError[i64]]:
    mut pair = Pair(2, 3)
    x = pair.x_ref()
    *x = 5
    mut number = 7
    alias = identity(&mut number)
    *alias = *alias + 1
    print(*view(&number)).unwrap()
    print(pair.x).unwrap()
    Ok(())
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

/// A conditional's reference values join both owners, as an `if` block's
/// arms do (issue #897); a borrow in the condition is not the value's origin.
#[test]
fn conditional_reference_values_join_their_owners() {
    run(
        r#"
def view(value: &i64) -> &i64:
    value
left = 1
right = 2
print(*(view(&left) if False else view(&right))).unwrap()
"#,
        "2\n",
    );
    reject(
        r#"
mut a = [1].unwrap()
b = [2].unwrap()
r = &a[0] if len(b) > 5 else &b[0]
a.clear()
print(*r).unwrap()
"#,
        "an `if` or `match` value keeps the value of every arm borrowed",
    );
}

#[test]
fn temporary_receivers_lend_returned_references_for_their_full_expression() {
    // Issue #597: the temporary receiver lives to the end of the statement.
    run(
        r#"
class Point:
    x: i64
    name: str
    def view(self) -> &i64:
        &self.x
    def label(self) -> &str:
        &self.name
    def slot(self: &mut Point) -> &mut i64:
        &mut self.x
    def maybe(self) -> Option[&i64]:
        Some(&self.x)
    def __del__(self: &mut Point) -> ():
        print("drop", self.name).unwrap()
def make(n: i64, name: str) -> Point:
    Point(n, name)
def main() -> Result[(), OverflowError[i64]]:
    print("value", *Point(1, "a").view()).unwrap()
    print("after").unwrap()
    total = *make(2, "b").view() + len(make(3, "cd").label())
    print(total).unwrap()
    print(make(4, "e").label()).unwrap()
    print(*Point(5, "f").slot()).unwrap()
    print(Point(6, "g").maybe().is_some()).unwrap()
    Ok(())
"#,
        "value 1\ndrop a\nafter\ndrop cd\ndrop b\n4\ne\ndrop e\n5\ndrop f\nTrue\ndrop g\n",
    );
    // A receiver viewing a temporary collection lends its result too, and
    // the collection still destroys the element (issue #599).
    run(
        r#"
class Point:
    x: i64
    name: str
    def view(self) -> &i64:
        &self.x
    def __del__(self: &mut Point) -> ():
        print("drop", self.name).unwrap()
print(*[Point(7, "h")].unwrap()[0].view()).unwrap()
print("after").unwrap()
"#,
        "7\ndrop h\nafter\n",
    );
}

#[test]
fn references_from_temporary_receivers_cannot_outlive_the_statement() {
    let class = "class Point:\n    x: i64\n    def view(self) -> &i64:\n        &self.x\n    def maybe(self) -> Option[&i64]:\n        Some(&self.x)\n";
    for (body, expected) in [
        (
            "r = Point(1).view()\nprint(*r).unwrap()\n",
            "temporary cannot be stored",
        ),
        (
            "r = Point(1).maybe()\nprint(r.is_some()).unwrap()\n",
            "temporary cannot be stored",
        ),
        (
            "def bad(p: &Point) -> &i64:\n    Point(1).view()\n",
            "must originate",
        ),
        // The receiver views `a`; its loan would end before the result's use.
        (
            "def pick(p: &Point) -> &Point:\n    p\na = Point(1)\nprint(*pick(&a).view()).unwrap()\n",
            "bind the receiver to a name",
        ),
    ] {
        reject(&format!("{class}{body}"), expected);
    }
}

#[test]
fn the_first_reference_parameter_is_the_origin_of_a_returned_reference() {
    run(
        r#"
class Entry:
    name: str
    score: i64
def last_index(length: i64) -> i64:
    # Called only at the two score_of sites below, each with len(entries).
    # 0 <= length <= i64.MAX, so length - 1 lies in -1..i64.MAX-1.
    unchecked:
        length - 1
class Table:
    entries: list[Entry]
    def score_of(self: &mut Table, name: &str) -> &mut i64:
        mut i = 0
        while i < last_index(len(self.entries)):
            if self.entries[i].name == name:
                return &mut self.entries[i].score
            # i starts at 0; i < len(entries)-1 <= i64.MAX-1 bounds the step.
            unchecked:
                i = i + 1
        &mut self.entries[last_index(len(self.entries))].score
def lookup(scores: &dict[str, i64], key: &str) -> &i64:
    &scores[*key]
def main() -> Result[(), OverflowError[i64]]:
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
    Ok(())
"#,
        "13\n4\nchanged\n",
    );
}

#[test]
fn later_reference_arguments_are_released_after_the_call() {
    run(
        r#"
def add_into(target: &mut i64, amount: &i64) -> &mut i64:
    # Fixture-only: the sole call below has target=1 and amount=2, giving 3.
    # Preserve the returned-reference ABI, not a general arbitrary-input API.
    unchecked:
        *target = *target + *amount
    target
def main() -> Result[(), OverflowError[i64]]:
    mut total = 1
    mut step = 2
    result = add_into(&mut total, &step)
    step = 7
    *result = *result * 10
    print(total).unwrap()
    print(step).unwrap()
    Ok(())
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
def main() -> Result[(), OverflowError[i64]]:
    mut head = Node(0, Nothing)
    mut i = 1
    while i < 200000:
        head = Node(i, Some(Box(head).unwrap()))
        i = i + 1
    key = 5
    found = find(&mut head, &key)
    *found = 99
    print(*find(&mut head, &99)).unwrap()
    Ok(())
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

// Issue #628: the diagnostic names the origin and the borrowed parameter.
#[test]
fn a_wrong_origin_names_both_parameters() {
    reject(
        "def pick(key: &str, items: &list[str]) -> &str:\n    &items[0]\n",
        "returned reference must originate from the first reference parameter, `key`; this one borrows `items`: make `items` the first reference parameter",
    );
    run(
        "def pick(items: &list[str], key: &str) -> &str:\n    &items[0]\nwords = [\"a\"].unwrap()\nprint(pick(&words, \"k\")).unwrap()\n",
        "a\n",
    );
}

#[test]
fn a_returned_conditional_reference_checks_each_origin() {
    run(
        "def pick(items: &list[str], c: bool) -> &str:\n    &items[0] if c else &items[1]\nwords = [\"a\", \"b\"].unwrap()\nprint(pick(&words, False)).unwrap()\n",
        "b\n",
    );
    reject(
        "def pick(items: &list[str], other: &list[str], c: bool) -> &str:\n    &items[0] if c else &other[0]\n",
        "returned reference must originate from the first reference parameter, `items`; this one borrows `other`",
    );
    run(
        "def pick(items: &list[str], c: bool) -> &str:\n    if c:\n        return &items[0]\n    &items[1]\nwords = [\"a\", \"b\"].unwrap()\nprint(pick(&words, False)).unwrap()\n",
        "b\n",
    );
}

#[test]
fn returned_views_preserve_rows_text_and_mutability() {
    run_views(
        r#"
def tail[T](xs: &[T]) -> &[T]:
    &xs[1:]
def forward(xs: &[i64]) -> &[i64]:
    tail(xs)
def mutable_tail(xs: &mut [i64]) -> &mut [i64]:
    &mut xs[1:]
def text_tail(text: &strview) -> &strview:
    &text[1:]
def bytes_of(text: &str) -> &[u8]:
    text.as_bytes()
mut xs = [1, 2, 3].unwrap()
v = forward(&xs)
print(len(v), v[0], v[-1]).unwrap()
print(len(tail(tail(&xs)))).unwrap()
w = mutable_tail(&mut xs)
w[0] = 9
print(xs).unwrap()
text = "aé日"
s = text_tail(text)
print(s, len(s)).unwrap()
b = bytes_of(&text)
print(len(b), b[0]).unwrap()
"#,
        "2 2 3\n1\n[1, 9, 3]\né日 2\n6 97\n",
    );
}

#[test]
fn optional_and_fallible_views_keep_the_origin() {
    run_views(
        r#"
def maybe(xs: &[i64], present: bool) -> Option[&[i64]]:
    if present:
        return Some(&xs[1:])
    Nothing
def fallible(xs: &mut [i64]) -> Result[&mut [i64], str]:
    Ok(&mut xs[1:])
def text(s: &strview) -> Option[&strview]:
    Some(&s[1:])
mut xs = [1, 2, 3].unwrap()
v = maybe(&xs, True).unwrap()
print(len(v), v[0], maybe(&xs, False).is_none()).unwrap()
w = fallible(&mut xs).unwrap()
w[0] = 7
print(xs).unwrap()
s = "abc"
print(text(s).unwrap()).unwrap()
"#,
        "2 2 True\n[1, 7, 3]\nbc\n",
    );
}

#[test]
fn returned_views_reject_escaping_and_conflicting_loans() {
    for source in [
        "def bad(xs: &[i64], other: &[i64]) -> &[i64]:\n    &other[:]",
        "def bad(xs: &[i64]) -> &[i64]:\n    local = [1].unwrap()\n    &local[:]",
        "def bad(xs: &[i64]) -> Option[&[i64]]:\n    local = [1].unwrap()\n    Some(&local[:])",
        "def bad(s: &strview) -> &strview:\n    local = \"short\"\n    &local[:]",
    ] {
        reject(source, "must originate");
    }
    reject(
        "def bad() -> &[i64]:\n    &([1].unwrap())[:]",
        "needs a reference parameter",
    );
    reject(
        "def bad(xs: &[i64]) -> &mut [i64]:\n    &mut xs[:]",
        "to be `&mut`",
    );
    let shared = "def tail(xs: &[i64]) -> &[i64]:\n    &xs[1:]\n";
    for action in ["xs.append(4).unwrap()", "xs = [4].unwrap()", "owned = xs"] {
        reject(
            &format!(
                "{shared}mut xs = [1, 2, 3].unwrap()\nv = tail(&xs)\n{action}\nprint(len(v)).unwrap()"
            ),
            "borrow",
        );
    }
    reject(
        &format!("{shared}v = tail([1, 2].unwrap())\nprint(len(v)).unwrap()"),
        "temporary cannot be stored",
    );
    reject(
        "def tail(xs: &mut [i64]) -> &mut [i64]:\n    &mut xs[1:]\nmut xs = [1, 2, 3].unwrap()\na = tail(&mut xs)\nb = tail(&mut xs)\na[0] = b[0]",
        "conflicting borrow",
    );
    reject(
        "def tail(s: &strview) -> &strview:\n    &s[1:]\nmut s = \"inline\"\nv = tail(s)\ns = \"moved\"\nprint(v).unwrap()",
        "borrowed",
    );
}

#[test]
fn returned_view_methods_callables_and_branch_results() {
    run_views(
        r#"
class Data:
    values: list[i64]
    def tail(self) -> &[i64]:
        &self.values[1:]
def choose(xs: &[i64], flag: bool) -> &[i64]:
    if flag:
        &xs[1:]
    else:
        &xs[2:]
def apply(f: Callable[[&[i64], bool], &[i64]], xs: &[i64]) -> &[i64]:
    f(xs, True)
def get(xs: &[i64]) -> Option[&[i64]]:
    Some(choose(xs, False))
def propagate(xs: &[i64]) -> Option[&[i64]]:
    v = get(xs)?
    Some(v)
def extract(xs: Option[&[i64]]) -> &[i64]:
    xs.unwrap()
def select(xs: &[i64], n: i64) -> &[i64]:
    match n:
        case 0:
            &xs[:1]
        case _:
            &xs[1:]
xs = [1, 2, 3].unwrap()
v = apply(choose, &xs)
print(v[0], len(choose(&xs, False))).unwrap()
match propagate(&xs):
    case Some(w):
        print(w[0], len(w)).unwrap()
    case Nothing:
        pass
data = Data([4, 5, 6].unwrap())
w = data.tail()
print(w[0], len(data.tail())).unwrap()
held = Some(&xs[:])
whole = extract(held)
piece = select(&xs, 1)
fallback = get(&xs).unwrap_or(&xs[:])
print(len(whole), piece[0], fallback[0]).unwrap()
"#,
        "2 1\n3 1\n5 2\n3 2 3\n",
    );
}

#[test]
fn view_wrappers_do_not_enable_storage_or_extra_origins() {
    for source in [
        "def bad(xs: &[i64]) -> Option[Option[&[i64]]]:\n    Some(Some(xs))",
        "xs = [1, 2].unwrap()\nv = Some(Some(&xs[:]))",
        "class Holder:\n    values: Option[&[i64]]",
        "def bad(xs: &[i64]) -> Result[(), &[i64]]:\n    Err(xs)",
    ] {
        reject(source, "cannot");
    }
    reject(
        "def get(xs: &[i64], ys: &[i64]) -> Option[&[i64]]:\n    Some(&ys[:])",
        "must originate",
    );
    reject(
        "def get(xs: &[i64]) -> Option[&[i64]]:\n    Some(xs)\nv = get([1].unwrap())\nprint(v.unwrap()[0]).unwrap()",
        "temporary cannot be stored",
    );
    reject(
        "def get(xs: &mut [i64]) -> Option[&mut [i64]]:\n    Some(xs)\nmut xs = [1, 2].unwrap()\nv = get(&mut xs)\nxs.append(3).unwrap()\nprint(len(v.unwrap())).unwrap()",
        "borrow",
    );
    reject(
        "def get(xs: &[i64], ys: &[i64], flag: bool) -> &[i64]:\n    if flag:\n        &xs[:]\n    else:\n        &ys[:]",
        "must originate",
    );
    reject(
        "def tail(xs: &[i64]) -> &[i64]:\n    &xs[1:]\ndef inspect(xs: &[i64], n: i64) -> ():\n    pass\ndef change(xs: &mut list[i64]) -> i64:\n    xs.append(4).unwrap()\n    0\nmut xs = [1, 2].unwrap()\ninspect(tail(&xs), change(&mut xs))",
        "borrow",
    );
    reject(
        "def tail(xs: &[i64]) -> &[i64]:\n    &xs[1:]\ndef change(xs: &mut list[i64]) -> i64:\n    xs.append(4).unwrap()\n    0\nmut xs = [1, 2].unwrap()\nprint(tail(&xs), change(&mut xs)).unwrap()",
        "borrow",
    );
}

#[test]
fn returned_views_retain_bounds_checks() {
    let source = support::program(
        "def tail(xs: &[i64]) -> Option[&[i64]]:\n    Some(&xs[1:])\nxs = [1, 2, 3].unwrap()\nv = tail(&xs).unwrap()\nprint(v[2]).unwrap()",
    );
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("program");
        plenty::compile_source_to_executable_with_options(
            &source,
            &executable,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap();
        let output = Command::new(executable).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "optimize={optimize}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("index out of bounds: index 2, length 2"),
            "optimize={optimize}: {error}"
        );
    }
}

#[test]
fn capturing_closures_cannot_return_loan_carrying_values() {
    for capture in ["[xs]", "[&xs]", "once [xs]", "once [&xs]", "once "] {
        for (output, body) in [
            ("&[i64]", "&xs[:]"),
            ("Option[&[i64]]", "Some(&xs[:])"),
            ("Result[&[i64], AllocError]", "Ok(&xs[:])"),
        ] {
            let body = if capture == "once " {
                body.replace("xs", "unused")
            } else {
                body.to_owned()
            };
            reject_views(
                &format!(
                    "xs = [11, 22, 33].unwrap()\nf = def {capture}(unused: &[i64]) -> {output}:\n    {body}\ndummy = [1].unwrap()\nv = f(&dummy)\ndrop(f)\nprint(v).unwrap()"
                ),
                "capturing or one-shot closures cannot return references or yield yet",
            );
        }
    }
    // Inline pipeline callbacks have a separate environment-splicing path.
    reject_views(
        "xs = [1, 2].unwrap()\np = iter(&xs).map(def [&xs](n: &i64) -> Option[&[i64]]: Some(&xs[:]))\ndrop(p)",
        "capturing or one-shot closures cannot return references or yield yet",
    );
    run_views(
        "xs = [1, 2].unwrap()\nf = def (xs: &[i64]) -> &[i64]:\n    xs\nprint(f(&xs)).unwrap()",
        "[1, 2]\n",
    );
}

#[test]
fn returned_views_are_observed_and_iterated_after_type_inference() {
    run_views(
        r#"
def tail[T](xs: &[T]) -> &[T]:
    &xs[1:]
def maybe(xs: &[i64]) -> Option[&[i64]]:
    Some(xs)
def result(xs: &[i64]) -> Result[&[i64], AllocError]:
    Ok(xs)
def text[T](s: &str, unused: T) -> Option[&strview]:
    Some(&s[1:])
xs = [1, 2, 3].unwrap()
print(&tail(&xs)).unwrap()
for x in tail(&xs):
    print(*x).unwrap()
for x in &tail(&xs):
    print(*x).unwrap()
for x in tail(&[4, 5, 6].unwrap()):
    print(*x).unwrap()
for x in maybe(&xs).unwrap():
    print(*x).unwrap()
for x in result(&xs).unwrap():
    print(*x).unwrap()
s = "aéλ"
for c in text(&s, 0).unwrap():
    print(c).unwrap()
for c in &text(&s, 0).unwrap():
    print(c).unwrap()
"#,
        "[2, 3]\n2\n3\n2\n3\n5\n6\n1\n2\n3\n1\n2\n3\né\nλ\né\nλ\n",
    );
    for method in ["keys", "values", "items"] {
        reject_views(
            &format!(
                "def tail[T](xs: &[T]) -> &[T]:\n    &xs[1:]\nxs = [1, 2].unwrap()\ndrop(tail(&xs).{method}())"
            ),
            "requires a dictionary",
        );
        let names = if method == "items" { "k, v" } else { "v" };
        reject_views(
            &format!(
                "def tail[T](xs: &[T]) -> &[T]:\n    &xs[1:]\nxs = [1, 2].unwrap()\nfor {names} in tail(&xs).{method}():\n    pass"
            ),
            "requires a dictionary",
        );
    }
    // Retrying an inferred binary operand saves its descriptor in a hidden
    // binding; it must save the loan as well, even for unsupported operators.
    for operator in ["<", "+"] {
        reject_views(
            &format!(
                "def tail[T](xs: &[T]) -> &[T]:\n    &xs[1:]\nxs = [1, 2].unwrap()\nprint(tail(&xs) {operator} tail(&xs)).unwrap()"
            ),
            "slice views support",
        );
    }
}

#[test]
fn named_exclusive_view_returns_preserve_descriptor_and_iteration_loans() {
    run_views(
        r#"
def forward[T](xs: &mut [T]) -> &mut [T]:
    xs
def maybe(xs: &mut [i64]) -> Option[&mut [i64]]:
    Some(xs)
def result(xs: &mut [i64]) -> Result[&mut [i64], AllocError]:
    Ok(xs)
mut xs = [1, 2, 3].unwrap()
v = forward(&mut xs)
v[0] = 4
for x in forward(&mut xs):
    *x = 5
for x in &mut forward(&mut xs):
    *x = 6
for x in maybe(&mut xs).unwrap():
    *x = 7
for x in result(&mut xs).unwrap():
    *x = 8
print(xs).unwrap()
for x in &forward(&mut xs):
    print(*x).unwrap()
xs.append(9).unwrap()
print(xs).unwrap()
"#,
        "[8, 8, 8]\n8\n8\n8\n[8, 8, 8, 9]\n",
    );
    for call in [
        "forward(&mut xs)",
        "maybe(&mut xs).unwrap()",
        "result(&mut xs).unwrap()",
    ] {
        for conflict in [
            "xs.append(4).unwrap()",
            "drop(xs)",
            "other = forward(&mut xs)\n    print(len(other)).unwrap()",
        ] {
            reject_views(
                &format!(
                    "def forward[T](xs: &mut [T]) -> &mut [T]:\n    xs\ndef maybe(xs: &mut [i64]) -> Option[&mut [i64]]:\n    Some(xs)\ndef result(xs: &mut [i64]) -> Result[&mut [i64], AllocError]:\n    Ok(xs)\nmut xs = [1, 2, 3].unwrap()\nfor x in {call}:\n    {conflict}\n    *x = 9"
                ),
                if conflict == "drop(xs)" {
                    "moved"
                } else {
                    "borrow"
                },
            );
        }
    }
    reject_views(
        "def tail[T](xs: &[T]) -> &[T]:\n    &xs[1:]\nmut xs = [1, 2].unwrap()\nfor x in tail(&xs):\n    xs.append(3).unwrap()\n    print(*x).unwrap()",
        "borrow",
    );
    reject_views(
        "def forward[T](xs: &mut [T]) -> &mut [T]:\n    xs\nmut xs = [1, 2].unwrap()\nfor x in &forward(&mut xs):\n    *x = 9",
        "exclusive reference",
    );
}

fn reject_views(source: &str, expected: &str) {
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let error = plenty::compile_source_to_executable_with_options(
            &support::program(source),
            &dir.path().join("program"),
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains(expected),
            "optimize={optimize}: {source}\nexpected {expected}, got {error}"
        );
    }
}

fn run_views(source: &str, expected: &str) {
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("program");
        plenty::compile_source_to_executable_with_options(
            &support::program(source),
            &executable,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("optimize={optimize}: {source}\n{error}"));
        let output = Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "optimize={optimize}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "optimize={optimize}"
        );
    }
}
