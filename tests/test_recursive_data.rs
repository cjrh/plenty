//! Recursive types reach themselves through `Box`, which owns a heap value.
mod support;

fn runs(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn recursive_class_constructs_moves_and_borrows_fields() {
    runs(
        r#"
class Node:
    value: i64
    next: Option[Box[Node]]
    def head(self) -> i64:
        self.value
mut chain = Node(1, Some(Box(Node(2, Nothing)).unwrap()))
print(chain.head()).unwrap()
chain.value = 4
print(chain.head()).unwrap()
"#,
        "1\n4\n",
    );
}

#[test]
fn recursive_enum_and_generic_class_have_finite_native_layouts() {
    runs(
        r#"
enum Tree:
    Leaf(i64)
    Branch(Box[Tree], Box[Tree])
def total(tree: Tree) -> i64:
    match tree:
        case Tree.Leaf(value):
            value
        case Tree.Branch(left, right):
            total(left) + total(right)
class Node[T]:
    value: T
    next: Option[Box[Node[T]]]
tree = Tree.Branch(Box(Tree.Leaf(3)).unwrap(), Box(Tree.Leaf(7)).unwrap())
print(total(tree)).unwrap()
node = Node[i64](42, Nothing)
print(node.value).unwrap()
"#,
        "10\n42\n",
    );
}

#[test]
fn aliases_and_mutually_recursive_classes_resolve_in_any_order() {
    runs(
        r#"
type Children = list[Branch]
class Root:
    children: Children
class Branch:
    value: i64
    children: list[Root]
root = Root([Branch(7, [].unwrap())].unwrap())
print(root.children[0].value).unwrap()
"#,
        "7\n",
    );
}

#[test]
fn consuming_matches_can_walk_deep_values_iteratively() {
    runs(
        r#"
enum Chain[T]:
    End
    Link(T, Box[Chain[T]])
mut chain = Chain[i64].End
for n in range(1000):
    chain = Chain[i64].Link(n, Box(chain).unwrap())
mut total = 0
while True:
    match chain:
        case Chain[i64].End:
            break
        case Chain[i64].Link(value, rest):
            total = total + value
            chain = rest
print(total).unwrap()
"#,
        "499500\n",
    );
}

#[test]
fn recursive_owners_preserve_borrow_and_move_rules() {
    for (body, diagnostic) in [
        ("other = node\nprint(node.value).unwrap()", "moved"),
        (
            "borrowed = &node\nnode.value = 2\nprint(borrowed.value).unwrap()",
            "borrow",
        ),
        ("node.next = Some(Box(node).unwrap())", "moved"),
    ] {
        let source = format!("class Node:\n    value: i64\n    next: Option[Box[Node]]\nmut node = Node(1, Nothing)\n{body}\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
    }
}

#[test]
fn unsupported_deep_operations_are_rejected_through_wrappers() {
    for body in [
        "copy(node)",
        "print(node)",
        "str.repr(node)",
        "node == node",
        "wrapped = Some(node)\nprint(wrapped)",
        "wrapped = [node].unwrap()\ncopy(wrapped)",
        "wrapped = [node].unwrap()\nwrapped == wrapped",
        "other = Node(2, Nothing)\nwrapped = [node].unwrap()\nother in wrapped",
    ] {
        let source = format!("class Node:\n    value: i64\n    next: Option[Box[Node]]\nnode = Node(1, Nothing)\n{body}\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains("not supported for recursive data"),
            "{source}\n{error}"
        );
    }
}

#[test]
fn recursive_names_do_not_bypass_inline_tag_or_finite_depth_limits() {
    let wrapped = format!("{}Node{}", "Option[".repeat(65), "]".repeat(65));
    for source in [
        format!("class Node:\n    next: Option[Box[Node]]\ntype TooDeep = {wrapped}"),
        format!("class Node:\n    next: {wrapped}"),
        (0..66)
            .map(|i| format!("class C{i}:\n    next: C{}\n", i + 1))
            .collect::<String>()
            + "class C66:\n    value: i64\n",
    ] {
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("type nesting exceeds"), "{error}");
    }
}

#[test]
fn finite_mutual_generic_instances_reuse_their_nominal_identities() {
    runs(
        r#"
class Link[A, B]:
    value: A
    next: Option[Box[Link[B, A]]]
type Head = Link[i64, u8]
tail = Link[u8, i64](7u8, Nothing)
head = Head(42, Some(Box(tail).unwrap()))
print(head.value).unwrap()
"#,
        "42\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_boxes_drop_the_values_they_would_own_exactly_once() {
    runs(r#"
class Node:
    next: Option[Box[Node]]
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
enum Tree:
    Leaf(Node)
    Pair(Box[Tree], Box[Tree])
child = Node(Nothing)
print("__test_fail_allocations_after_0__").unwrap()
failed = Box(child)
print("__test_restore_allocations__").unwrap()
match failed:
    case Ok(value):
        print("unexpected").unwrap()
    case Err(error):
        print(error).unwrap()
left = Tree.Leaf(Node(Nothing))
print("__test_fail_allocations_after_0__").unwrap()
failed_tree = Box(left)
print("__test_restore_allocations__").unwrap()
match failed_tree:
    case Ok(value):
        print("unexpected").unwrap()
    case Err(error):
        print(error).unwrap()
"#, "__test_fail_allocations_after_0__\ndrop\n__test_restore_allocations__\nAllocError.OutOfMemory\n__test_fail_allocations_after_0__\ndrop\n__test_restore_allocations__\nAllocError.OutOfMemory\n");
}

#[cfg(all(feature = "runtime-checks", target_os = "linux"))]
#[test]
fn deep_source_values_drop_on_a_small_native_stack_without_allocating() {
    let source = r#"
enum Chain:
    End
    Link(Box[Chain], Tail)
class Tail:
    value: i64
    def __del__(self) -> ():
        pass
class Node:
    next: Option[Box[Node]]
    tail: Tail
mut chain = Chain.End
mut nodes: Option[Box[Node]] = Nothing
for n in range(100000):
    chain = Chain.Link(Box(chain).unwrap(), Tail(n))
    nodes = Some(Box(Node(nodes, Tail(n))).unwrap())
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
drop(chain)
drop(nodes)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
"#;
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("deep");
    support::compile_source_to_executable(source, &executable).unwrap();
    let output = std::process::Command::new("sh")
        .args(["-c", "ulimit -s 256; exec \"$1\"", "recursive-drop-test"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn nested_recursive_field_updates_do_not_allocate_and_drop_replaced_values() {
    runs(r#"
class Node:
    value: i64
    children: list[Node]
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
mut root = Node(0, [Node(1, [].unwrap())].unwrap())
replacement = Node(2, [].unwrap())
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
root.children[0] = replacement
root.children[0].value = 3
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(root.children[0].value).unwrap()
drop(root)
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\ndrop\n__test_restore_allocations__\n__test_end_no_allocations__\n3\ndrop\ndrop\n");
}
