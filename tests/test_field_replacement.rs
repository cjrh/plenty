//! Exchange initialized fields without cloning or partially moving their owner.
mod support;

fn expect(source: &str, expected: &str) {
    let output = support::run(source);
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
        "expected {expected:?}, got {error}"
    );
}

const NODE: &str = r#"
class Node:
    value: i64
    next: Option[Box[Node]]
"#;

#[test]
fn reverse_and_consume_a_recursive_class_chain() {
    expect(
        &format!(
            r#"{NODE}
def reverse(head: Option[Box[Node]]) -> Option[Box[Node]]:
    mut remaining = head
    mut reversed: Option[Box[Node]] = Nothing
    while True:
        match remaining:
            case Some(link):
                mut node = link
                remaining = replace(node.next, reversed)
                reversed = Some(node)
            case Nothing:
                break
    reversed
mut chain: Option[Box[Node]] = Nothing
for value in range(1, 4):
    chain = Some(Box(Node(value, chain)).unwrap())
chain = reverse(chain)
while True:
    match chain:
        case Some(link):
            mut node = link
            print(node.value).unwrap()
            chain = replace(node.next, Nothing)
        case Nothing:
            break
"#
        ),
        "1\n2\n3\n",
    );
}

#[test]
fn old_inline_owners_move_and_destruct_once() {
    expect(
        r#"
class Item:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
class Pair:
    left: Item
    right: Item
class Wrapper:
    pair: Pair
    def __del__(self) -> ():
        print(self.pair.left.label).unwrap()
mut owner = Wrapper(Pair(Item("old left"), Item("old right")))
old = replace(owner.pair, Pair(Item("new left"), Item("new right")))
print("replaced").unwrap()
drop(old)
drop(owner)
"#,
        "replaced\nold left\nold right\nnew left\nnew left\nnew right\n",
    );
}

#[test]
fn replacement_precedes_indices_and_resolves_addresses_after_user_code() {
    expect(
        r#"
class Row:
    value: i64
def fresh(rows: &mut list[Row]) -> i64:
    rows.clear()
    rows.append(Row(7)).unwrap()
    print("value").unwrap()
    9
def index(rows: &mut list[Row]) -> i64:
    rows.reserve(100).unwrap()
    print("index").unwrap()
    0
mut rows = [Row(1)].unwrap()
print(replace(rows[index(&mut rows)].value, fresh(&mut rows))).unwrap()
print(rows[0].value).unwrap()
"#,
        "value\nindex\n7\n9\n",
    );
}

#[test]
fn failed_index_drops_pending_replacement_and_preserves_old_field() {
    expect(
        r#"
class Item:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
class Row:
    item: Item
def missing() -> Result[i64, str]:
    Err("missing")
def update(rows: &mut list[Row]) -> Result[(), str]:
    drop(replace(rows[missing()?].item, Item("pending")))
    Ok(())
mut rows = [Row(Item("old"))].unwrap()
print(str.repr(update(&mut rows)).unwrap()).unwrap()
print(rows[0].item.label).unwrap()
"#,
        "pending\nResult[(), str].Err(\"missing\")\nold\nold\n",
    );
}

#[test]
fn field_exchange_obeys_loans_and_initialization() {
    for (source, diagnostic) in [
        (
            "class C:\n    n: i64\nc = C(1)\nreplace(c.n, 2)",
            "mut binding",
        ),
        (
            "class C:\n    n: i64\nmut c = C(1)\nr = &c\nreplace(r.n, 2)",
            "shared reference as mutable",
        ),
        (
            "class C:\n    n: i64\nmut c = C(1)\nr = &c.n\nreplace(c.n, 2)\nprint(r).unwrap()",
            "conflicting borrow",
        ),
        (
            "class C:\n    n: i64\nmut c = C(1)\nreplace(c.n, True)",
            "expected i64",
        ),
        ("class C:\n    n: i64\nreplace(C(1).n, 2)", "named owner"),
        ("mut n = 1\nreplace(n, 2)", "mutable class field"),
        ("replace()", "takes a mutable class field"),
        (
            "class C:\n    n: i64\n    def __init__(self) -> ():\n        replace(self.n, 2)",
            "not initialized",
        ),
    ] {
        reject(source, diagnostic);
    }
    expect(
        r#"
class C:
    left: i64
    right: i64
def change(c: &mut C) -> i64:
    replace(c.left, 5)
mut c = C(1, 2)
other = &c.right
print(replace(c.left, 3)).unwrap()
print(other).unwrap()
print(change(&mut c)).unwrap()
print(c.left).unwrap()
"#,
        "1\n2\n3\n5\n",
    );
}

#[test]
fn recursive_field_diagnostic_offers_supported_operations() {
    let source = format!("{NODE}\nmut n = Node(1, Nothing)\ntail = n.next");
    let dir = tempfile::tempdir().unwrap();
    let error = support::compile_source_to_executable(&source, &dir.path().join("program"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("match &value.field"), "{error}");
    assert!(
        error.contains("replace(value.field, replacement)"),
        "{error}"
    );
    assert!(!error.contains("copy"), "{error}");
}

#[test]
fn replacement_cannot_move_the_owner_and_can_exchange_siblings() {
    reject(
        r#"
class C:
    value: i64
def consume(c: C) -> i64:
    c.value
mut c = C(1)
replace(c.value, consume(c))
"#,
        "moved",
    );
    reject(
        r#"
class C:
    value: i64
mut c = C(1)
r = &mut c.value
replace(c.value, 2)
print(r).unwrap()
"#,
        "conflicting borrow",
    );
    expect(
        r#"
class Item:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
class Pair:
    left: Item
    right: Item
mut pair = Pair(Item("left"), Item("right"))
old = replace(pair.left, replace(pair.right, Item("new")))
print(pair.left.label).unwrap()
print(pair.right.label).unwrap()
drop(old)
drop(pair)
"#,
        "right\nnew\nleft\nright\nnew\n",
    );
}

#[test]
fn imported_code_resolves_replacement_and_keeps_fields_private() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("cell.plenty"),
        r#"
pub class Cell:
    value: i64
pub def exchange(cell: &mut Cell, value: i64) -> i64:
    replace(cell.value, value)
pub def create() -> Cell:
    Cell(1)
"#,
    )
    .unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(
        &source,
        r#"
import cell
def main() -> ():
    mut c = cell.create()
    print(cell.exchange(&mut c, 2)).unwrap()
    print(cell.exchange(&mut c, 3)).unwrap()
"#,
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &executable, Some(dir.path())).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"1\n2\n");
    std::fs::write(
        &source,
        r#"
import cell
def main() -> ():
    mut c = cell.create()
    replace(c.value, 2)
"#,
    )
    .unwrap();
    let error = plenty::check_file(&source, Some(dir.path()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("private"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn exchange_allocates_nothing_and_keeps_inline_sum_payloads() {
    expect(r#"
class C:
    value: Option[range[i64]]
mut c = C(Some(range(1, 4)))
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
old = replace(c.value, Some(range(5, 8)))
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(old).unwrap()
print(c.value).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\nOption[range].Some(range(1, 4, 1))\nOption[range].Some(range(5, 8, 1))\n");
}
