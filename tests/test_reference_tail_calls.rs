//! A tail call passing references releases its caller's frame only when every
//! reference was borrowed through a reference parameter of the caller, so none
//! can point into that frame (issue 3). Any other reference argument keeps the
//! frame and its cleanup until the callee returns.

mod support;

use rstest::rstest;

const PRELUDE: &str = r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
    def length(self) -> i64:
        print("length").unwrap()
        len(self.name)
    def relay(self) -> i64:
        local = Guard("local")
        self.length()
    def view(self) -> &str:
        print("view").unwrap()
        &self.name
class Pair:
    left: Guard
    right: Guard
def make(name: str) -> Guard:
    Guard(name)
def read(g: &Guard, n: i64) -> i64:
    print("read").unwrap()
    n
def both(a: &Guard, b: &Guard) -> i64:
    print("both").unwrap()
    0
def keep(g: &Guard, item: Guard) -> i64:
    print("keep").unwrap()
    0
"#;

fn trace(source: &str, expected: &str) {
    let source = format!("{PRELUDE}{source}");
    let output = support::run(&source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}

/// The caller's local and owned parameter are released before the callee runs.
#[rstest]
#[case("read(g, 1)", "local\nowned\nread\n1\n")]
#[case("return read(g, 1)", "local\nowned\nread\n1\n")]
#[case("same = &g\n    read(same, 1)", "local\nowned\nread\n1\n")]
#[case("operation = read\n    operation(g, 1)", "local\nowned\nread\n1\n")]
#[case("g.length()", "local\nowned\nlength\n5\n")]
#[case("both(g, g)", "local\nowned\nboth\n0\n")]
#[case("mut cursor = g\n    read(cursor, 1)", "local\nowned\nread\n1\n")]
// The owned inline argument keeps the frame, but not its locals.
#[case("keep(g, owned)", "local\nkeep\nowned\n0\n")]
#[case(
    "read(g, make('temp').length())",
    "length\ntemp\nlocal\nowned\nread\n4\n"
)]
fn forwarded_parameters_are_passed_after_cleanup(#[case] tail: &str, #[case] expected: &str) {
    trace(
        &format!(
            r#"
def caller(g: &Guard, owned: Guard) -> i64:
    local = Guard("local")
    {tail}
owner = make("owner")
print(caller(&owner, make("owned"))).unwrap()
drop(owner)
"#
        ),
        &format!("{expected}owner\n"),
    );
}

#[test]
fn projections_of_parameters_are_passed_after_cleanup() {
    trace(
        r#"
def field(p: &Pair) -> i64:
    local = Guard("field")
    read(&p.right, 1)
def payload(o: &Option[Guard]) -> i64:
    local = Guard("payload")
    match &o:
        case Some(g):
            read(g, 2)
        case Nothing:
            0
def exclusive(o: &mut Option[Guard]) -> i64:
    local = Guard("exclusive")
    match &mut o:
        case Some(g):
            g.name = "renamed"
            read(g, 3)
        case Nothing:
            0
def element(items: &list[Guard]) -> i64:
    local = Guard("element")
    read(&items[1], 4)
def each(items: &list[Guard]) -> i64:
    local = Guard("each")
    for g in &items:
        return read(g, 5)
    0
def method(p: &Pair) -> i64:
    local = Guard("method")
    p.left.length()
def result(g: &Guard) -> &str:
    local = Guard("result")
    return g.view()
def main() -> Result[(), Failure]:
    pair = Pair(make("left"), make("right"))
    print(field(&pair))?
    mut some = Some(make("some"))
    print(payload(&some))?
    print(exclusive(&mut some))?
    items = [make("first"), make("second")].unwrap()
    print(element(&items))?
    print(each(&items))?
    print(method(&pair))?
    print(result(&pair.left))?
    print("end")?
    Ok(())
"#,
        "field\nread\n1\npayload\nread\n2\nexclusive\nread\n3\nelement\nread\n4\neach\nread\n5\n\
         method\nlength\n4\nresult\nview\nleft\nend\nfirst\nsecond\nrenamed\nleft\nright\n",
    );
}

#[test]
fn closure_bodies_forward_their_captures_after_cleanup() {
    trace(
        r#"
def caller() -> i64:
    held = Guard("held")
    job = def [held]() -> i64:
        local = Guard("local")
        read(held, 1)
    job()
print(caller()).unwrap()
"#,
        "local\nread\nheld\n1\n",
    );
}

/// A reference to a local, an owned parameter, or a temporary points into the
/// caller's frame. One such argument keeps the whole call ordinary.
#[rstest]
#[case("read(&local, 1)", "read\nlocal\nowned\n1\n")]
#[case("read(&owned, 1)", "read\nlocal\nowned\n1\n")]
#[case("read(make('temp'), 1)", "read\ntemp\nlocal\nowned\n1\n")]
#[case("both(g, &local)", "both\nlocal\nowned\n0\n")]
#[case("both(&owned, g)", "both\nlocal\nowned\n0\n")]
#[case("both(g, make('temp'))", "both\ntemp\nlocal\nowned\n0\n")]
#[case("same = &local\n    read(same, 1)", "read\nlocal\nowned\n1\n")]
#[case(
    "operation = both\n    operation(g, &local)",
    "both\nlocal\nowned\n0\n"
)]
#[case("local.length()", "length\nlocal\nowned\n5\n")]
#[case("mut cursor = &local\n    read(cursor, 1)", "read\nlocal\nowned\n1\n")]
#[case("keep(&local, owned)", "keep\nowned\nlocal\n0\n")]
fn references_into_the_caller_keep_its_frame_until_the_callee_returns(
    #[case] tail: &str,
    #[case] expected: &str,
) {
    trace(
        &format!(
            r#"
def caller(g: &Guard, owned: Guard) -> i64:
    local = Guard("local")
    {tail}
owner = make("owner")
print(caller(&owner, make("owned"))).unwrap()
drop(owner)
"#
        ),
        &format!("{expected}owner\n"),
    );
}

#[test]
fn a_borrowing_closure_argument_keeps_the_frame() {
    trace(
        r#"
def invoke(g: &Guard, f: &Closure[[], i64]) -> i64:
    print("invoke").unwrap()
    f()
def caller(g: &Guard) -> i64:
    local = Guard("local")
    job = def [&local]() -> i64:
        len(local.name)
    invoke(g, job)
owner = make("owner")
print(caller(&owner)).unwrap()
drop(owner)
"#,
        "invoke\nlocal\n5\nowner\n",
    );
}

/// Moving a forwarding call's loan uses ahead of it must not hide a conflict
/// between its arguments.
#[rstest]
#[case("node: &mut Node", "two(node, node)")]
#[case("node: &mut Node", "return pair(node, node)")]
#[case("node: &mut Node", "same = &mut node\n    pair(same, node)")]
#[case("node: &mut Node", "part(node, &node.items)")]
#[case("node: &mut Node", "operation = pair\n    operation(node, node)")]
#[case("node: &Node", "two(node, node)")]
#[case("node: &Node", "local = Node(1, [1].unwrap())\n    take(local, local)")]
fn conflicting_arguments_of_a_tail_call_are_rejected(#[case] parameter: &str, #[case] tail: &str) {
    let source = format!(
        r#"
class Node:
    value: i64
    items: list[i64]
def two(a: &mut Node, b: &Node) -> i64:
    a.value + b.value
def pair(a: &mut Node, b: &mut Node) -> i64:
    a.value + b.value
def part(a: &mut Node, b: &list[i64]) -> i64:
    a.value + len(b)
def take(a: &Node, b: Node) -> i64:
    a.value + b.value
def caller({parameter}) -> i64:
    {tail}
"#
    );
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(
        error.contains("conflicting borrow") || error.contains("cannot borrow shared reference"),
        "{source}\n{error}"
    );
}

#[test]
fn a_forwarded_result_cannot_outlive_a_conflicting_borrow_or_a_local() {
    for (body, expected) in [
        (
            "alias = &node\n    out = pick(node)\n    print(alias.value).unwrap()\n    out",
            "conflicting borrow",
        ),
        (
            "mut local = Node(1)\n    pick(&mut local)",
            "must originate from the reference parameter",
        ),
    ] {
        let source = format!(
            "class Node:\n    value: i64\ndef pick(a: &mut Node) -> &mut Node:\n    a\ndef caller(node: &mut Node) -> &mut Node:\n    {body}\n"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

/// Runs `source` on a 256 KB stack, where a million ordinary calls overflow.
#[cfg(target_os = "linux")]
fn deep(source: &str, expected: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("deep");
    support::compile_source_to_executable(source, &executable).unwrap();
    let output = std::process::Command::new("sh")
        .args([
            "-c",
            "ulimit -s 256; exec \"$1\"",
            "reference-tail-call-test",
        ])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[cfg(target_os = "linux")]
#[test]
fn forwarded_parameters_recurse_on_a_small_stack() {
    deep(
        r#"
class Guard:
    id: i64
    def __del__(self: &mut Guard) -> ():
        self.id = 0
class Inner:
    hits: i64
class Node:
    value: i64
    inner: Inner
def shared(node: &Node, n: i64, acc: i64) -> i64:
    scratch = Guard(n)
    if n == 0:
        return acc
    shared(node, n - 1, acc + node.value)
def exclusive(node: &mut Node, n: i64) -> i64:
    scratch = [n].unwrap()
    if n == 0:
        return node.value
    node.value = node.value + 1
    return exclusive(node, n - 1)
def alias(node: &Node, n: i64, acc: i64) -> i64:
    if n == 0:
        return acc
    same = &node
    alias(same, n - 1, acc + same.value)
def two(a: &Node, b: &mut Inner, n: i64) -> i64:
    if n == 0:
        return b.hits
    b.hits = b.hits + a.value
    two(a, b, n - 1)
def even(node: &Node, n: i64) -> bool:
    if n == 0:
        return True
    odd(node, n - 1)
def odd(node: &Node, n: i64) -> bool:
    if n == 0:
        return False
    even(node, n - 1)
def generic[T](item: &T, n: i64) -> i64:
    if n == 0:
        return 0
    generic(item, n - 1)
def invoke(f: &Closure[[], i64], n: i64) -> i64:
    if n == 0:
        return f()
    invoke(f, n - 1)
def main() -> Result[(), Failure]:
    mut head = Node(1, Inner(0))
    print(shared(&head, 1000000, 0))?
    print(exclusive(&mut head, 1000000))?
    print(head.value)?
    print(alias(&head, 1000000, 0))?
    mut tally = Inner(0)
    print(two(&head, &mut tally, 1000000))?
    print(tally.hits)?
    print(even(&head, 1000001))?
    print(generic(&head, 1000000))?
    values = [7].unwrap()
    job = def [&values]() -> i64:
        values[0]
    print(invoke(&job, 1000000))?
    Ok(())
"#,
        "1000000\n1000001\n1000001\n1000001000000\n1000001000000\n1000001000000\nFalse\n0\n7\n",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn projections_of_parameters_recurse_on_a_small_stack() {
    deep(
        r#"
class Inner:
    hits: i64
class Node:
    value: i64
    inner: Inner
class Tree:
    child: Option[Box[Node]]
    items: list[Node]
def field(node: &mut Node, n: i64) -> i64:
    if n == 0:
        return node.inner.hits
    count(&mut node.inner, n)
    field(node, n - 1)
def count(inner: &mut Inner, n: i64) -> ():
    inner.hits = inner.hits + 1
def nested(inner: &mut Inner, n: i64) -> i64:
    if n == 0:
        return inner.hits
    inner.hits = inner.hits + 1
    nested(inner, n - 1)
def project(node: &mut Node, n: i64) -> i64:
    nested(&mut node.inner, n)
def payload(tree: &Tree, n: i64) -> i64:
    match &tree.child:
        case Some(child):
            back(child, tree, n - 1)
        case Nothing:
            0
def back(child: &Node, tree: &Tree, n: i64) -> i64:
    if n == 0:
        return child.value
    payload(tree, n)
def element(tree: &Tree, n: i64, acc: i64) -> i64:
    if n == 0:
        return acc
    item(&tree.items[n % 2], tree, n, acc)
def item(node: &Node, tree: &Tree, n: i64, acc: i64) -> i64:
    element(tree, n - 1, acc + node.value)
def each(tree: &Tree, n: i64, acc: i64) -> i64:
    if n == 0:
        return acc
    for node in &tree.items:
        if node.value > 0:
            return visit(node, tree, n, acc)
    acc
def visit(node: &Node, tree: &Tree, n: i64, acc: i64) -> i64:
    each(tree, n - 1, acc + node.value)
def main() -> Result[(), Failure]:
    mut head = Node(1, Inner(0))
    print(field(&mut head, 1000000))?
    print(project(&mut head, 1000000))?
    print(head.inner.hits)?
    tree = Tree(
        Some(Box(Node(7, Inner(0))).unwrap()),
        [Node(1, Inner(0)), Node(2, Inner(0))].unwrap(),
    )
    print(payload(&tree, 1000000))?
    print(element(&tree, 1000000, 0))?
    print(each(&tree, 1000000, 0))?
    Ok(())
"#,
        "1000000\n2000000\n2000000\n7\n1500000\n1000000\n",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn methods_indirect_calls_and_reference_results_recurse_on_a_small_stack() {
    deep(
        r#"
class Node:
    value: i64
    def spin(self, n: i64, acc: i64) -> i64:
        if n == 0:
            return acc
        self.spin(n - 1, acc + self.value)
    def bump(self: &mut Node, n: i64) -> ():
        if n == 0:
            return
        self.value = self.value + 1
        self.bump(n - 1)
def indirect(step: Callable[[&Node, i64, i64], i64], node: &Node, n: i64, acc: i64) -> i64:
    if n == 0:
        return acc
    step(node, n, acc)
def bounce(node: &Node, n: i64, acc: i64) -> i64:
    indirect(bounce, node, n - 1, acc + node.value)
def last(node: &Node, n: i64) -> &Node:
    if n == 0:
        return node
    return last(node, n - 1)
def last_expression(node: &mut Node, n: i64) -> &mut Node:
    if n == 0:
        return node
    last_expression(node, n - 1)
def main() -> Result[(), Failure]:
    mut head = Node(1)
    print(head.spin(1000000, 0))?
    head.bump(1000000)
    print(head.value)?
    print(bounce(&head, 500000, 0))?
    print(last(&head, 1000000).value)?
    found = last_expression(&mut head, 1000000)
    found.value = 5
    print(head.value)?
    Ok(())
"#,
        "1000000\n1000001\n500000500000\n1000001\n5\n",
    );
}

/// Each step borrows the next node out of the previous one, so every frame's
/// reference comes from a different payload of the same external chain.
#[cfg(target_os = "linux")]
#[test]
fn a_boxed_chain_is_walked_through_its_payloads_on_a_small_stack() {
    deep(
        r#"
class Node:
    value: i64
    next: Option[Box[Node]]
def increment(node: &mut Node) -> ():
    node.value = node.value + 1
    match &mut node.next:
        case Some(next):
            increment(next)
        case Nothing:
            pass
def last(node: &Node) -> i64:
    match &node.next:
        case Some(next):
            last(next)
        case Nothing:
            node.value
def main() -> Result[(), Failure]:
    mut head = Node(0, Nothing)
    for n in range(100000):
        head = Node(n + 1, Some(Box(head).unwrap()))
    increment(&mut head)
    print(head.value)?
    print(last(&head))?
    Ok(())
"#,
        "100001\n1\n",
    );
}

/// A `mut` reference binding keeps the loan it was declared with and is only
/// reassigned to references borrowed through it, so its origin never changes.
#[test]
fn a_reassigned_reference_cursor_keeps_its_origin() {
    trace(
        r#"
class Link:
    name: str
    next: Option[Box[Link]]
    def __del__(self) -> ():
        print(self.name).unwrap()
def show(link: &Link) -> i64:
    print("show").unwrap()
    len(link.name)
def through_parameter(head: &Link) -> i64:
    local = Guard("local")
    mut cursor = head
    match &cursor.next:
        case Some(next):
            cursor = next
        case Nothing:
            pass
    show(cursor)
def through_local() -> i64:
    local = Link("first", Some(Box(Link("second", Nothing)).unwrap()))
    mut cursor = &local
    match &cursor.next:
        case Some(next):
            cursor = next
        case Nothing:
            pass
    show(cursor)
chain = Link("a", Some(Box(Link("bb", Nothing)).unwrap()))
print(through_parameter(&chain)).unwrap()
print(through_local()).unwrap()
drop(chain)
"#,
        "local\nshow\n2\nshow\nfirst\nsecond\n6\na\nbb\n",
    );
}

/// The cursor advances along the chain before each call. `build` returns
/// inline storage through the result area its own caller supplied.
#[cfg(target_os = "linux")]
#[test]
fn a_reference_cursor_and_an_inline_result_recurse_on_a_small_stack() {
    deep(
        r#"
class Node:
    value: i64
    next: Option[Box[Node]]
class Sum:
    total: i64
    last: i64
def walk(node: &Node, n: i64) -> i64:
    mut cursor = node
    match &cursor.next:
        case Some(next):
            cursor = next
        case Nothing:
            pass
    if n == 0:
        return cursor.value
    walk(cursor, n - 1)
def scan(node: &Node, n: i64) -> i64:
    mut cursor = node
    while True:
        match &cursor.next:
            case Some(next):
                cursor = next
            case Nothing:
                break
    if n == 0:
        return cursor.value
    scan(node, n - 1)
def build(node: &Node, n: i64, total: i64) -> Sum:
    if n == 0:
        return Sum(total, node.value)
    build(node, n - 1, total + node.value)
def main() -> Result[(), Failure]:
    head = Node(3, Some(Box(Node(2, Some(Box(Node(1, Nothing)).unwrap()))).unwrap()))
    print(walk(&head, 1000000))?
    print(scan(&head, 1000000))?
    sum = build(&head, 1000000, 0)
    print(sum.total)?
    print(sum.last)?
    Ok(())
"#,
        "1\n1\n3000000\n3\n",
    );
}
