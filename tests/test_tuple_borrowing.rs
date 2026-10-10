mod support;

#[test]
fn captured_tuple_elements_support_observation_and_value_reads() {
    let output = support::run(
        r#"
pair = (13, [14].unwrap())
inspect = def [&pair]() -> i64:
    print(pair[1]).unwrap()
    pair[0]
print(inspect()).unwrap()

captured = (15, "sixteen")
inspect_owned = def [captured]() -> i64:
    print(captured[1]).unwrap()
    captured[0]
print(inspect_owned()).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "[14]\n13\nsixteen\n15\n"
    );
}

#[test]
fn disjoint_tuple_reads_preserve_borrows_and_project_through_references() {
    let output = support::run(
        r#"
class Holder:
    pair: (i64, list[i64])

mut pair = (1, [2].unwrap())
borrowed = &mut pair[1]
number = pair[0]
print(number).unwrap()
print(pair[0]).unwrap()
borrowed.append(3).unwrap()
print(pair[1]).unwrap()

mut nested = ((4, [5].unwrap()), [6].unwrap())
reference = &mut nested
child = &mut reference[0][1]
nested_number = reference[0][0]
print(nested_number).unwrap()
print(reference[0][0]).unwrap()
print(reference[1]).unwrap()
child.append(7).unwrap()
print(nested).unwrap()

mut holder = Holder((8, [9].unwrap()))
holder_items = &mut holder.pair[1]
holder_number = holder.pair[0]
print(holder_number).unwrap()
print(holder.pair[0]).unwrap()
holder_items.append(10).unwrap()
print(holder.pair[1]).unwrap()

mut unit_pair = ((), [11].unwrap())
unit_items = &mut unit_pair[1]
unit_pair[0]
unit_items.append(12).unwrap()
print(unit_pair[1]).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1\n1\n[2, 3]\n4\n4\n[6]\n((4, [5, 7]), [6])\n8\n8\n[9, 10]\n[11, 12]\n"
    );
}

#[rstest::rstest]
#[case::observed_element(
    "mut pair = (1, [2].unwrap())\nr = &mut pair[1]\nprint(pair[1]).unwrap()\nr.append(3).unwrap()",
    "cannot read or borrow `pair[1]` while it is exclusively borrowed"
)]
#[case::copied_element(
    "mut pair = (1, [2].unwrap())\nr = &mut pair[1]\ncopy(pair[1]).unwrap()\nr.append(3).unwrap()",
    "cannot read or borrow `pair[1]` while it is exclusively borrowed"
)]
#[case::scalar_value(
    "mut pair = (1, [2].unwrap())\nr = &mut pair[0]\nnumber = pair[0]\n*r = 3",
    "cannot read or borrow `pair[0]` while it is exclusively borrowed"
)]
#[case::whole_tuple(
    "mut pair = (1, [2].unwrap())\nr = &mut pair[1]\nprint(pair).unwrap()\nr.append(3).unwrap()",
    "cannot read or borrow `pair` while `pair[1]` is exclusively borrowed"
)]
#[case::nested_reference(
    "mut pair = ((1, [2].unwrap()), 3)\nr = &mut pair\ns = &mut r[0][1]\nprint(r[0][1]).unwrap()\ns.append(4).unwrap()",
    "cannot read or borrow `pair[0][1]` while it is exclusively borrowed"
)]
#[case::overlapping_parent(
    "mut pair = ((1, [2].unwrap()), 3)\nr = &mut pair[0]\nprint(pair[0][0]).unwrap()\nprint(r).unwrap()",
    "cannot read or borrow `pair[0][0]` while `pair[0]` is exclusively borrowed"
)]
#[case::mutable_reference_stays_conservative(
    "mut pair = (1, [2].unwrap())\nmut r = &mut pair\ns = &mut r[1]\nprint(r[0]).unwrap()\ns.append(3).unwrap()",
    "cannot read or borrow `pair` while it is exclusively borrowed"
)]
#[case::indexed_collection_stays_conservative(
    "mut pairs = [(1, [2].unwrap())].unwrap()\nr = &mut pairs[0][1]\nprint(pairs[0][0]).unwrap()\nr.append(3).unwrap()",
    "cannot read or borrow `pairs` while it is exclusively borrowed"
)]
fn overlapping_tuple_reads_are_rejected(#[case] source: &str, #[case] diagnostic: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    let (accessed, borrowed) = diagnostic.split_once(" while ").unwrap();
    assert!(error.contains(accessed), "{error}");
    assert!(error.contains(&format!("while {borrowed}")), "{error}");
}
