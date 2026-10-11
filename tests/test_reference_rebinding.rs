//! A `mut` reference binding may be assigned only a reference borrowed from the
//! binding itself, so a cursor walks owned data in a loop (issue 1).

mod support;

const CHAIN: &str = r#"
class Node:
    value: i64
    next: Option[Box[Node]]
def build(n: i64) -> Result[Node, AllocError]:
    mut head = Node(1, Nothing)
    for i in range(2, n + 1):
        head = Node(i, Some(Box(head)?))
    Ok(head)
def total(head: &Node) -> i64:
    mut sum = 0
    mut cur = head
    while True:
        sum = sum + cur.value
        match &cur.next:
            case Some(n):
                cur = n
            case Nothing:
                break
    sum
def bump(head: &mut Node) -> ():
    mut cur = head
    while True:
        cur.value = cur.value + 1
        match &mut cur.next:
            case Some(n):
                cur = n
            case Nothing:
                break
"#;

fn runs(source: &str, expected: &str) {
    let output = support::run(&format!("{CHAIN}{source}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn rejects(source: &str, expected: &str) {
    let error = support::check_source(&format!("{CHAIN}{source}"))
        .unwrap_err()
        .to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn shared_cursor_walks_a_class_chain() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    head = build(10)?
    print(total(&head))?
    print(total(&head))?
    Ok(())
"#,
        "55\n55\n",
    );
}

#[test]
fn exclusive_cursor_modifies_every_node() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    mut head = build(10)?
    bump(&mut head)
    print(total(&head))?
    print(head.value)?
    Ok(())
"#,
        "65\n11\n",
    );
}

#[test]
fn a_function_returns_its_cursor() {
    runs(
        r#"
def last(head: &Node) -> &Node:
    mut cur = head
    while True:
        match &cur.next:
            case Some(n):
                cur = n
            case Nothing:
                break
    cur
def last_mut(head: &mut Node) -> &mut Node:
    mut cur = head
    while True:
        match &mut cur.next:
            case Some(n):
                cur = n
            case Nothing:
                break
    cur
def main() -> Result[(), Failure]:
    mut head = build(4)?
    print(last(&head).value)?
    tail = last_mut(&mut head)
    tail.value = 7
    print(last(&head).value)?
    print(head.value)?
    Ok(())
"#,
        "1\n7\n4\n",
    );
}

#[test]
fn cursor_over_a_local_owner_ends_at_its_last_use() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    mut head = build(4)?
    mut cur = &head
    mut steps = 0
    while True:
        match &cur.next:
            case Some(n):
                cur = n
                steps = steps + 1
            case Nothing:
                break
    print(steps)?
    print(cur.value)?
    head = build(2)?
    print(head.value)?
    Ok(())
"#,
        "3\n1\n2\n",
    );
}

#[test]
fn cursor_advances_through_fields_and_list_elements() {
    runs(
        r#"
class Tree:
    value: i64
    children: list[Tree]
def leftmost(root: &Tree) -> i64:
    mut cur = root
    while len(cur.children) > 0:
        cur = &cur.children[0]
    cur.value
def clear_left_spine(root: &mut Tree) -> ():
    mut cur = root
    while len(cur.children) > 0:
        cur.value = cur.value + 10
        cur = &mut cur.children[0]
    cur.value = 0
def main() -> Result[(), Failure]:
    leaf = Tree(3, []?)
    mut kids: list[Tree] = []?
    kids.append(leaf)?
    mid = Tree(2, kids)
    mut top: list[Tree] = []?
    top.append(mid)?
    mut root = Tree(1, top)
    print(leftmost(&root))?
    clear_left_spine(&mut root)
    print(root.value)?
    print(leftmost(&root))?
    Ok(())
"#,
        "3\n11\n0\n",
    );
}

#[test]
fn cursor_advances_through_enum_payloads() {
    runs(
        r#"
enum Chain:
    End
    Link(i64, Box[Chain])
def sum(chain: &Chain) -> i64:
    mut result = 0
    mut cur = chain
    while True:
        match cur:
            case Chain.End:
                break
            case Chain.Link(value, rest):
                result = result + *value
                cur = rest
    result
def main() -> Result[(), Failure]:
    chain = Chain.Link(3, Box(Chain.Link(7, Box(Chain.End)?))?)
    print(sum(&chain))?
    Ok(())
"#,
        "10\n",
    );
}

#[test]
fn cursor_advances_through_a_reference_returning_call() {
    runs(
        r#"
def step(node: &Node) -> &Node:
    match &node.next:
        case Some(n):
            n
        case Nothing:
            node
def pick(label: str, node: &Node) -> &Node:
    node
def main() -> Result[(), Failure]:
    head = build(4)?
    mut cur = &head
    cur = step(cur)
    cur = pick("x", cur)
    seen = cur
    cur = step(cur)
    print(seen.value + cur.value)?
    Ok(())
"#,
        "5\n",
    );
}

#[test]
fn an_exclusive_cursor_is_usable_again_after_a_reborrow_ends() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    mut head = build(2)?
    mut cur = &mut head
    first = cur
    first.value = 8
    cur.value = cur.value + 1
    print(head.value)?
    Ok(())
"#,
        "9\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn walking_a_chain_does_not_allocate() {
    runs(
        r#"
def main() -> Result[(), Failure]:
    mut head = build(100)?
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    bump(&mut head)
    answer = total(&head)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(answer)?
    Ok(())
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n5150\n",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_million_node_chain_is_walked_and_dropped_on_a_small_stack() {
    let source = format!(
        r#"{CHAIN}
def main() -> Result[(), Failure]:
    mut head = build(1000000)?
    print(total(&head))?
    bump(&mut head)
    print(total(&head))?
    Ok(())
"#
    );
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("deep");
    support::compile_source_to_executable(&source, &executable).unwrap();
    let output = std::process::Command::new("sh")
        .args([
            "-c",
            "ulimit -s 256; exec \"$1\"",
            "reference-rebinding-test",
        ])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"500000500000\n500001500000\n");
}

#[test]
fn a_cursor_cannot_move_to_another_owner() {
    rejects(
        r#"
def main() -> Result[(), Failure]:
    head = build(4)?
    other = build(2)?
    mut cur = &head
    cur = &other
    print(cur.value)?
    Ok(())
"#,
        "reference binding `cur` can be reassigned only to a reference borrowed from `cur` itself",
    );
}

#[test]
fn a_cursor_cannot_move_to_another_reference_parameter() {
    rejects(
        r#"
def choose(head: &Node, other: &Node) -> i64:
    mut cur = head
    cur = other
    cur.value
"#,
        "reference binding `cur` can be reassigned only to a reference borrowed from `cur` itself",
    );
}

/// The rule has no same-owner exception: a borrow that does not go through
/// the cursor is rejected even when it reaches the same data.
#[test]
fn a_cursor_cannot_be_reset_through_its_owner() {
    rejects(
        r#"
def reset(head: &Node) -> i64:
    mut cur = head
    match &cur.next:
        case Some(n):
            cur = n
        case Nothing:
            pass
    cur = head
    cur.value
"#,
        "reference binding `cur` can be reassigned only to a reference borrowed from `cur` itself",
    );
    rejects(
        r#"
def skip(head: &Node) -> i64:
    mut cur = head
    match &head.next:
        case Some(n):
            cur = n
        case Nothing:
            pass
    cur.value
"#,
        "reference binding `cur` can be reassigned only to a reference borrowed from `cur` itself",
    );
}

/// A returned reference comes from the call's reference argument, whichever
/// other arguments read the cursor.
#[test]
fn a_call_result_follows_its_reference_argument() {
    for call in ["pick(other, depth(cur))", "pick_last(depth(cur), other)"] {
        rejects(
            &format!(
                r#"
def depth(node: &Node) -> i64:
    node.value
def pick(node: &Node, n: i64) -> &Node:
    node
def pick_last(n: i64, node: &Node) -> &Node:
    node
def choose(head: &Node, other: &Node) -> i64:
    mut cur = head
    cur = {call}
    cur.value
"#
            ),
            "reference binding `cur` can be reassigned only to a reference borrowed from `cur` itself",
        );
    }
}

#[test]
fn only_a_mut_reference_binding_is_reassigned() {
    rejects(
        r#"
def first(head: &Node) -> i64:
    cur = head
    cur = &cur
    cur.value
"#,
        "`cur` is immutable; declare it with `mut`",
    );
    rejects(
        r#"
def first(head: &Node) -> i64:
    match &head.next:
        case Some(n):
            n = &n
        case Nothing:
            pass
    head.value
"#,
        "`n` is immutable; declare it with `mut`",
    );
}

#[test]
fn a_cursor_keeps_its_mutability() {
    rejects(
        r#"
def first(head: &Node) -> i64:
    mut cur = head
    cur = &mut cur
    cur.value
"#,
        "cannot borrow shared reference as mutable",
    );
    rejects(
        r#"
def first(head: &mut Node) -> i64:
    mut cur = head
    cur = &cur
    cur.value
"#,
        "expected &mut Node, got &Node",
    );
}

#[test]
fn the_owner_stays_borrowed_while_the_cursor_is_live() {
    rejects(
        r#"
def main() -> Result[(), Failure]:
    mut head = build(4)?
    mut cur = &head
    match &cur.next:
        case Some(n):
            cur = n
        case Nothing:
            pass
    head = build(1)?
    print(cur.value)?
    Ok(())
"#,
        "cannot assign to `head` while it is borrowed",
    );
    rejects(
        r#"
def detach(head: &mut Node) -> i64:
    mut cur = head
    match &mut cur.next:
        case Some(n):
            cur = n
        case Nothing:
            pass
    head.next = Nothing
    cur.value
"#,
        "cannot modify or exclusively borrow `head.next` while `head` is exclusively borrowed",
    );
}

/// After `cur = n`, `cur` and `n` are the same exclusive reference. Using `cur`
/// while `n` is still live would alias it.
#[test]
fn an_exclusive_cursor_excludes_the_reference_it_was_assigned() {
    rejects(
        r#"
def walk(head: &mut Node) -> ():
    mut cur = head
    match &mut cur.next:
        case Some(n):
            cur = n
            value = &mut n.value
            cur.value = 2
            *value = 3
        case Nothing:
            pass
"#,
        "cannot modify or exclusively borrow `head` (through `cur`) while it is exclusively borrowed",
    );
}

/// `value` points into the node `rest` owns once `cur` has advanced, although
/// the two were borrowed through different fields of `cur`. A `mut` reference
/// binding therefore borrows its whole first target, not one field path.
#[test]
fn borrows_through_a_cursor_cover_its_whole_first_target() {
    rejects(
        r#"
def cut(head: &mut Node) -> ():
    mut cur = head
    rest = &mut cur.next
    match rest:
        case Some(n):
            cur = n
        case Nothing:
            pass
    value = &mut cur.value
    *rest = Nothing
    *value = 1
"#,
        "cannot modify or exclusively borrow `head` (through `cur`) while it is exclusively borrowed",
    );
    rejects(
        r#"
def split(head: &mut Node) -> ():
    mut cur = head
    value = &mut cur.value
    next = &mut cur.next
    *value = 5
    *next = Nothing
"#,
        "cannot modify or exclusively borrow `head` (through `cur`) while it is exclusively borrowed",
    );
}

#[test]
fn an_exclusive_cursor_excludes_its_live_reborrow() {
    rejects(
        r#"
def main() -> Result[(), Failure]:
    mut head = build(2)?
    mut cur = &mut head
    first = cur
    cur.value = 9
    first.value = 8
    Ok(())
"#,
        "cannot modify or exclusively borrow `head` (through `cur`) while it is exclusively borrowed",
    );
}

/// A binding's loan is the borrow of its place, not a borrow made while
/// computing an index into that place.
#[test]
fn a_reference_binding_holds_the_loan_of_its_place() {
    rejects(
        r#"
class Bag:
    items: list[i64]
def index(values: &list[i64]) -> i64:
    0
def main() -> Result[(), Failure]:
    mut bag = Bag([1, 2]?)
    other = [7, 8]?
    view = &bag.items[index(&other)]
    bag.items.append(4)?
    print(*view)?
    Ok(())
"#,
        "cannot modify or exclusively borrow `bag.items` while `bag` is borrowed",
    );
}
