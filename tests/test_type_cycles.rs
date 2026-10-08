//! Dependency diagnostics distinguish transparent aliases from nominal recursion.
mod support;

#[test]
fn errors_show_the_actual_cycle_without_an_unrelated_prefix() {
    for (source, diagnostic) in [
        (
            "type Entry = A\ntype A = B\ntype B = A",
            "cyclic type alias: A -> B -> A",
        ),
        (
            "class Node:\n    next: Option[Node]",
            "recursive data declarations are not supported yet: Node -> Node",
        ),
        (
            "class Left:\n    child: Right\nclass Right:\n    children: list[Left]",
            "recursive data declarations are not supported yet: Left -> Right -> Left",
        ),
        (
            "type Link = Node\nenum Node:\n    End\n    Next(Link)",
            "recursive data declarations are not supported yet: Link -> Node -> Link",
        ),
        (
            "class Node[T]:\n    children: list[Node[T]]",
            "recursive data declarations are not supported yet: Node -> Node",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
    }
}

#[test]
fn long_alias_chains_use_a_worklist_and_bound_cycle_diagnostics() {
    let chain = (0..5000)
        .map(|i| format!("type A{i} = A{}\n", i + 1))
        .collect::<String>();
    support::check_source(&format!("{chain}type A5000 = i64")).unwrap();
    let error = support::check_source(&format!("{chain}type A5000 = A0"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("cyclic type alias: A0 -> A1"), "{error}");
    assert!(error.contains("declarations omitted"), "{error}");
    assert!(error.len() < 1024, "cycle diagnostic should remain bounded");
}

#[test]
fn diamond_dependencies_and_generic_parameter_shadows_are_not_cycles() {
    support::check_source("class Root:\n    left: Left\n    right: Right\nclass Left:\n    value: Leaf\nclass Right:\n    value: Leaf\nclass Leaf:\n    value: i64").unwrap();
    support::check_source(
        "class Item:\n    value: Wrapper[i64]\nclass Wrapper[Item]:\n    value: Item\nx = Wrapper[i64](3).unwrap()",
    )
    .unwrap();
}
