mod support;

#[test]
fn mutable_components_have_independent_inline_storage_for_copyable_tuples() {
    let output = support::run(
        r#"
def change(pair: &mut (i64, i64)) -> ():
    first = &mut pair[0]
    second = &mut pair[1]
    *first = 5
    *second = 6
mut pair = (1, 2)
saved = pair
change(&mut pair)
print(pair).unwrap()
print(saved).unwrap()
mut nested = ((3, 4), 7)
saved_nested = nested
item = &mut nested[0][1]
*item = 8
print(nested).unwrap()
print(saved_nested).unwrap()
class Holder:
    pair: (i64, i64)
mut holder = Holder((9, 10))
field = &mut holder.pair[0]
*field = 11
print(holder.pair).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"(5, 6)\n(1, 2)\n((3, 8), 7)\n((3, 4), 7)\n(11, 10)\n"
    );
}

#[test]
fn mutable_enum_payload_match_works_inside_copyable_tuple() {
    let output = support::run(
        r#"
enum Shape:
    Circle(i64)
    Square(i64)
mut pair = (Shape.Circle(3), 1)
saved = pair
match &mut pair[0]:
    case Shape.Circle(radius):
        *radius += 1
    case Shape.Square(side):
        *side = 0
print(pair).unwrap()
print(saved).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"(Shape.Circle(4), 1)\n(Shape.Circle(3), 1)\n"
    );
}

#[test]
fn changing_a_copyable_string_component_preserves_the_saved_copy() {
    let output = support::run(
        r#"
mut words = (("first " + "long string").unwrap(), 1)
saved = words
word = &mut words[0]
*word = "replacement text"
print(words[0]).unwrap()
print(saved[0]).unwrap()
drop(words)
print(saved[0]).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"replacement text\nfirst long string\nfirst long string\n"
    );
}

#[rstest::rstest]
#[case("pair = (1, 2)\nitem = &mut pair[0]", "immutable")]
#[case(
    "mut pair = (1, 2)\nitem = &mut pair[0]\nprint(pair[0]).unwrap()\n*item = 5",
    "exclusively borrowed"
)]
#[case(
    "mut pair = (1, 2)\nitem = &mut pair[0]\nprint(pair).unwrap()\n*item = 5",
    "exclusively borrowed"
)]
#[case(
    "mut pair = (1, 2)\nitem = &mut pair[0]\nother = &mut pair[0]\n*item = 5\n*other = 6",
    "exclusively borrowed"
)]
#[case("mut pair = (1, 2)\nshared = &pair\nitem = &mut shared[0]", "shared")]
#[case(
    "mut pair = (1, 2)\npair[0] = 3",
    "indexed assignment requires list or dict"
)]
fn tuple_mutation_respects_loans_and_assignment_contract(
    #[case] source: &str,
    #[case] expected: &str,
) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
