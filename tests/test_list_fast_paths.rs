//! Narrow list calls must preserve the language's bits, bounds, and loans.
mod support;
use rstest::rstest;

#[rstest]
#[case("-128i8", "-128")]
#[case("-32768i16", "-32768")]
#[case("-2147483648i32", "-2147483648")]
#[case("-9223372036854775808i64", "-9223372036854775808")]
#[case("255u8", "255")]
#[case("65535u16", "65535")]
#[case("4294967295u32", "4294967295")]
#[case("18446744073709551615u64", "18446744073709551615")]
#[case("-1.25f32", "-1.25")]
#[case("-1.25f64", "-1.25")]
#[case("True", "True")]
#[case("False", "False")]
fn scalar_bits_in_owned_borrowed_and_iterated_reads(#[case] value: &str, #[case] expected: &str) {
    let source = format!(
        "xs = [{value}].unwrap()\nprint(xs[0]).unwrap()\nprint(xs[-1]).unwrap()\nprint([{value}].unwrap()[-1]).unwrap()\nfor x in xs:\n    print(x).unwrap()"
    );
    let output = support::run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{expected}\n").repeat(4)
    );
}

#[rstest]
#[case("[1].unwrap()", "1")]
#[case("[1].unwrap()", "-2")]
#[case("[1].unwrap()", "-9223372036854775808")]
#[case("[1].unwrap()", "9223372036854775807")]
#[case("list[i64]().unwrap()", "0")]
fn invalid_indices_keep_the_diagnostic(#[case] receiver: &str, #[case] index: &str) {
    for expression in [format!("{receiver}[{index}]"), format!("xs[{index}]")] {
        let output = support::run(&format!("xs = {receiver}\nprint({expression}).unwrap()"));
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("index out of bounds"));
    }
}

#[test]
fn evaluation_order_growth_and_managed_fallbacks() {
    let output = support::run(
        r#"
def receiver() -> list[i64]:
    print("receiver").unwrap()
    [42].unwrap()
def index() -> i64:
    print("index").unwrap()
    -1
def read(xs: &list[i64]) -> i64:
    xs[index()] + len(xs)
print(receiver()[index()]).unwrap()
print(len(receiver())).unwrap()
mut xs = [1].unwrap()
print(read(&xs)).unwrap()
for i in range(100):
    xs.append(i).unwrap()
    if xs[-1] != i or len(xs) != i + 2:
        print("wrong growth").unwrap()
xs[0] = 9
print(xs[0]).unwrap()
print(xs.pop(-1).unwrap()).unwrap()
print(len(xs)).unwrap()
print(xs[-1]).unwrap()
mut nested = [["a long managed string"].unwrap()].unwrap()
print(len(nested[0])).unwrap()
print(nested[0][0]).unwrap()
nested[0].append("another managed string").unwrap()
print(len(nested[0])).unwrap()
drop(nested)
drop(xs)
print("__test_small_live_heap__").unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with(
        "receiver\nindex\n42\nreceiver\n1\nindex\n2\n9\n99\n100\n98\n1\na long managed string\n2\n"
    ));
}

#[test]
fn index_cannot_mutate_a_borrowed_receiver() {
    let error = support::check_source(
        "def index(xs: &mut list[i64]) -> i64:\n    xs.append(2).unwrap()\n    0\nmut xs = [1].unwrap()\nprint(xs[index(&mut xs)]).unwrap()",
    ).unwrap_err().to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn borrowed_access_has_only_narrow_runtime_relocations() {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("access.o");
    plenty::compile_source_to_object(
        "def read(xs: &list[i64], i: i64) -> i64:\n    xs[i] + len(xs)\ndef main() -> ():\n    xs = [1].unwrap()\n    print(read(&xs, 0)).unwrap()\n",
        &path,
    ).unwrap();
    let bytes = std::fs::read(path).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let function = file
        .symbols()
        .find(|s| s.name().is_ok_and(|name| name.ends_with("read")))
        .unwrap();
    let section = file
        .section_by_index(function.section_index().unwrap())
        .unwrap();
    let calls: Vec<_> = section
        .relocations()
        .filter(|(offset, _)| {
            *offset >= function.address() && *offset < function.address() + function.size()
        })
        .filter_map(|(_, relocation)| match relocation.target() {
            RelocationTarget::Symbol(index) => Some(
                file.symbol_by_index(index)
                    .unwrap()
                    .name()
                    .unwrap()
                    .to_owned(),
            ),
            _ => None,
        })
        .collect();
    assert!(
        calls.iter().any(|name| name == "plenty_list_scalar_get"),
        "{calls:?}"
    );
    assert!(
        calls.iter().any(|name| name == "plenty_list_len"),
        "{calls:?}"
    );
    assert!(
        !calls.iter().any(|name| matches!(
            name.as_str(),
            "plenty_collection" | "plenty_retain" | "plenty_release"
        )),
        "{calls:?}"
    );
}
