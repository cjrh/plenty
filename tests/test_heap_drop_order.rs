//! A heap child's complete subtree precedes its parent's later inline siblings.
mod support;

const LEAF: &str = r#"
class Leaf:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
"#;

fn trace(source: &str, expected: &str) {
    let output = support::run(&format!("{LEAF}\n{source}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn heap_fields_finish_before_inline_fields_and_later_list_elements() {
    trace(
        r#"
class Pair:
    first: list[Leaf]
    second: Leaf
pairs = [Pair([Leaf("a")].unwrap(), Leaf("b")), Pair([Leaf("c")].unwrap(), Leaf("d"))].unwrap()
drop(pairs)
"#,
        "a\nb\nc\nd\n",
    );
}

#[test]
fn boxed_tuples_and_enum_payloads_preserve_mixed_field_order() {
    trace(
        r#"
enum Mixed:
    Fields(list[Leaf], Leaf, Box[Leaf])
    Empty
value = Box(Mixed.Fields([Leaf("a")].unwrap(), Leaf("b"), Box(Leaf("c")).unwrap())).unwrap()
drop(value)
tuples = [([Leaf("d")].unwrap(), Leaf("e"), Box(Leaf("f")).unwrap())].unwrap()
drop(tuples)
"#,
        "a\nb\nc\nd\ne\nf\n",
    );
}

#[test]
fn dictionary_values_follow_insertion_order_after_removal_and_reuse() {
    trace(
        r#"
class Pair:
    first: Box[Leaf]
    second: Leaf
mut pairs = {1: Pair(Box(Leaf("a")).unwrap(), Leaf("b")), 2: Pair(Box(Leaf("c")).unwrap(), Leaf("d"))}.unwrap()
drop(pairs.pop(1))
pairs.insert(3, Pair(Box(Leaf("e")).unwrap(), Leaf("f"))).unwrap()
drop(pairs)
"#,
        "a\nb\nc\nd\ne\nf\n",
    );
}

#[test]
fn resumed_parent_runs_its_hook_once_and_isolates_nested_drops() {
    trace(
        r#"
class Parent:
    first: list[Leaf]
    second: Leaf
    def __del__(self) -> ():
        print("parent").unwrap()
        drop([Leaf("nested")].unwrap())
        print("after nested").unwrap()
parents = [Parent([Leaf("a")].unwrap(), Leaf("b")), Parent([Leaf("c")].unwrap(), Leaf("d"))].unwrap()
drop(parents)
"#,
        "parent\nnested\nafter nested\na\nb\nparent\nnested\nafter nested\nc\nd\n",
    );
}
