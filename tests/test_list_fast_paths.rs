//! Narrow list calls must preserve the language's bits, bounds, and loans.
mod support;
use rstest::rstest;

#[rstest]
#[case("i8", "-7i8", "5i8")]
#[case("i16", "-700i16", "500i16")]
#[case("i32", "-70000i32", "50000i32")]
#[case("i64", "-7000000000i64", "5000000000i64")]
#[case("u8", "250u8", "5u8")]
#[case("u16", "65000u16", "500u16")]
#[case("u32", "4000000000u32", "50000u32")]
#[case("u64", "18446744073709551615u64", "5000000000u64")]
#[case("f32", "-1.25f32", "3.5f32")]
#[case("f64", "-1.25f64", "3.5f64")]
#[case("bool", "True", "False")]
fn packed_scalar_lists_preserve_last_element_references_and_copies(
    #[case] ty: &str,
    #[case] first: &str,
    #[case] second: &str,
) {
    support::assert_visible_stdout(
        &format!(
            r#"
def assign(value: &mut {ty}, replacement: {ty}) -> ():
    *value = replacement
def observe(value: &{ty}) -> {ty}:
    *value
def last(values: &list[{ty}]) -> Option[&{ty}]:
    Some(&values[-1])
mut values = [{first}, {first}, {first}].unwrap()
assign(&mut values[-1], {second})
print(observe(&values[0]) == {first}, observe(&values[-1]) == {second}).unwrap()
for item in &mut values:
    *item = {second}
backup = copy(values).unwrap()
print(backup == values, values.count({second}), values.get(-1) == Some({second})).unwrap()
match last(&values):
    case Some(item):
        print(*item == {second}).unwrap()
    case Nothing:
        print(False).unwrap()
print(values.pop() == Some({second}), len(values)).unwrap()
values.append({first}).unwrap()
print(values[-1] == {first}, values[0] == {second}).unwrap()
values.reverse()
values.swap(0, 2)
print(values == [{second}, {second}, {first}].unwrap()).unwrap()
print(values.slice(1, 3).unwrap() == [{second}, {first}].unwrap()).unwrap()
"#
        ),
        "True True\nTrue 3 True\nTrue\nTrue 2\nTrue True\nTrue\nTrue",
    );
}

#[test]
fn narrow_reference_writes_ignore_padding_and_preserve_sum_tags() {
    support::assert_visible_stdout(
        r#"
def assign(value: &mut i8) -> ():
    *value = 5i8
mut wrapped: Option[i8] = Some(-1i8)
match &mut wrapped:
    case Some(value):
        assign(value)
    case Nothing:
        pass
print(wrapped, wrapped == Some(5i8)).unwrap()
mut pair = (-1i8, 9)
assign(&mut pair[0])
print(pair, pair == (5i8, 9)).unwrap()
keys = {pair: 7}.unwrap()
print(keys[(5i8, 9)]).unwrap()
"#,
        "Option[i8].Some(5) True\n(5, 9) True\n7",
    );
}

#[test]
fn formatting_optional_references_reads_only_the_packed_scalar() {
    support::assert_visible_stdout(
        r#"
def last(values: &list[i8]) -> Option[&i8]:
    Some(&values[-1])
values = [127i8, -7i8].unwrap()
print(last(&values)).unwrap()
"#,
        "Option[&i8].Some(-7)",
    );
}

#[cfg(target_arch = "x86_64")]
#[test]
fn indexed_column_hot_loop_has_no_runtime_calls() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("columns.plenty");
    std::fs::write(
        &source,
        r#"
def total(xs: &list[i64], ys: &list[i64]) -> i64:
    mut total = 0
    for i in range(len(xs)):
        total += xs[i] * ys[i]
    total
def main() -> ():
    print(total(&[1, 2].unwrap(), &[3, 4].unwrap())).unwrap()
"#,
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(["--release", "--dump", "asm", "--dump-filter", "total"])
        .arg(source)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = String::from_utf8(output.stdout).unwrap();
    let instruction = regex::Regex::new(r"^\s*([0-9a-f]+):\s+(.*)$").unwrap();
    let instructions: Vec<_> = asm
        .lines()
        .filter_map(|line| {
            let captures = instruction.captures(line)?;
            Some((
                u64::from_str_radix(&captures[1], 16).unwrap(),
                captures[2].to_owned(),
            ))
        })
        .collect();
    let (end, target) = instructions
        .iter()
        .find_map(|(offset, text)| {
            let target = text.strip_prefix("jmp")?.trim().strip_prefix("0x")?;
            let target = u64::from_str_radix(target, 16).ok()?;
            (target < *offset).then_some((*offset, target))
        })
        .expect("counted loop backedge");
    assert!(!instructions.iter().any(|(offset, text)| (*offset >= target && *offset <= end) && text.starts_with("call")), "{asm}");
    assert_eq!(asm.matches("# plenty_list_rows").count(), 2, "{asm}");
    assert_eq!(asm.matches("# plenty_list_row_bytes").count(), 2, "{asm}");
    // Only the out-of-bounds cold blocks call out, to report the index.
    assert_eq!(asm.matches("# plenty_trap_list_index").count(), 2, "{asm}");
    assert!(!asm.contains("plenty_list_scalar_get"), "{asm}");
}

#[test]
fn indexed_shared_columns_keep_bounds_bits_and_loop_control() {
    support::assert_visible_stdout(
        r#"
def total(xs: &list[i64], ys: &list[i64]) -> i64:
    mut total = 0
    for i in range(len(xs)):
        if i == 1:
            continue
        total += xs[i] * ys[-i - 1]
    total
def floats(xs: &list[f32], flags: &list[bool], wide: &list[u64]) -> ():
    for i in range(len(xs)):
        print(xs[i], flags[i], wide[i]).unwrap()
def local_columns(xs: &list[i64]) -> i64:
    column = xs
    mut total = 0
    for i in range(2):
        for j in range(len(column)):
            total += column[j]
    total
xs = [1, 2, 3].unwrap()
ys = [4, 5, 6].unwrap()
print(total(&xs, &ys), local_columns(&xs)).unwrap()
floats(&[-1.25f32].unwrap(), &[True].unwrap(), &[18446744073709551615u64].unwrap())
"#,
        "18 12\n-1.25 True 18446744073709551615",
    );
}

#[rstest]
#[case("0", "list[i64]().unwrap()")]
#[case("-2", "[1].unwrap()")]
#[case("-9223372036854775808", "[1].unwrap()")]
#[case("9223372036854775807", "[1].unwrap()")]
fn indexed_shared_columns_trap_at_each_columns_own_bound(
    #[case] index: &str,
    #[case] values: &str,
) {
    let source = format!(
        "def read(xs: &list[i64], ys: &list[i64]) -> i64:\n    mut result = 0\n    for i in range(len(xs)):\n        result += xs[i] + ys[{index}]\n    result\nprint(read(&[1].unwrap(), &{values})).unwrap()"
    );
    let output = support::run(&source);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("index out of bounds"));
}

#[test]
fn indexed_loops_do_not_cache_mutable_or_owned_sources() {
    support::assert_visible_stdout(
        r#"
def changing(xs: &mut list[i64]) -> ():
    for i in range(3):
        xs.append(i).unwrap()
        print(xs[-1]).unwrap()
mut xs = [9].unwrap()
changing(&mut xs)
first = [[1].unwrap(), [2].unwrap()].unwrap()
mut second = [2].unwrap()
mut view = &first[0]
for i in range(2):
    view = &first[i]
    print(view[0]).unwrap()
for row in &first:
    print(row[0]).unwrap()
for i in range(3):
    second.append(i).unwrap()
    print(second[-1]).unwrap()
"#,
        "0\n1\n2\n1\n2\n1\n2\n0\n1\n2",
    );
}

#[test]
fn cached_column_reads_preserve_index_effects_and_owner_loans() {
    support::assert_visible_stdout(
        r#"
def index(value: i64) -> i64:
    print(value).unwrap()
    value
def total(xs: &list[i64], ys: &list[i64]) -> i64:
    mut result = 0
    for i in range(2):
        result += xs[index(i)] + ys[index(-i - 1)]
    result
print(total(&[1, 2].unwrap(), &[3, 4].unwrap())).unwrap()
"#,
        "0\n-1\n1\n-2\n10",
    );
    let error = support::check_source(
        r#"
mut values = [1, 2].unwrap()
column = &values
for i in range(2):
    values.append(3).unwrap()
    print(column[i]).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn immediate_literal_progressions_need_no_collection_storage() {
    support::assert_visible_stdout(
        r#"
def numbers() -> Generator[i64]:
    for n in [3, 2, 1].unwrap():
        yield n
def total() -> Result[i64, AllocError]:
    mut total = 0
    for n in [2, 4, 6]?:
        total += n
    for n in reversed([9, 6, 3]?):
        if n == 6:
            continue
        total += n
    for n in [7, 7, 7].unwrap():
        total += n
        break
    for n in [9223372036854775806, 9223372036854775807]?:
        total += n - 9223372036854775806
    for n in [-9223372036854775808]?:
        total += n - -9223372036854775808
    total += iter([1, 2, 3]?).map(def(n: i64) -> i64: n * 2).fold(0, def(a: i64, b: i64) -> i64: a + b)
    for n in numbers():
        total += n
    Ok(total)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
answer = total()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(answer.unwrap()).unwrap()
"#,
        "50",
    );
}

#[test]
fn immediate_lists_keep_eager_effects_and_ordinary_fallbacks() {
    support::assert_visible_stdout(
        r#"
def item(n: i64) -> i64:
    print(n).unwrap()
    n
for n in [item(1), item(2), item(3)].unwrap():
    print("body", n).unwrap()
    break
for n in [1, 4, 2].unwrap():
    print(n).unwrap()
for n in [254u8, 255u8].unwrap():
    print(n).unwrap()
for n in [-9223372036854775808, 9223372036854775807].unwrap():
    print(n).unwrap()
for n in &[1, 2, 3].unwrap():
    print(*n).unwrap()
"#,
        "1\n2\n3\nbody 1\n1\n4\n2\n254\n255\n-9223372036854775808\n9223372036854775807\n1\n2\n3",
    );
}

#[rstest]
#[case(
    "def main() -> ():\n    for n in [1, 2]?:\n        pass",
    "requires a function returning Result or Option"
)]
#[case(
    "def main() -> Result[(), IoError]:\n    for n in [1, 2]?:\n        pass\n    Ok(())",
    "cannot propagate as IoError"
)]
#[case("def main() -> ():\n    for n in [1, 2]:\n        pass", "iterable")]
#[case(
    "def main() -> ():\n    for n in [1i8, 2i64].unwrap():\n        pass",
    "expected list[i8]"
)]
#[case(
    "def numbers() -> Generator[i64]:\n    for n in [1, 2]?:\n        yield n\ndef main() -> ():\n    pass",
    "not supported in generators"
)]
fn literal_iteration_elision_keeps_source_checks(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

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
    // Issue #509: scalar reads and length are inline; only the
    // out-of-bounds report calls out.
    assert_eq!(
        calls
            .iter()
            .filter(|name| name.starts_with("plenty_"))
            .collect::<Vec<_>>(),
        ["plenty_trap_list_index"],
        "{calls:?}"
    );
    assert!(
        !calls.iter().any(|name| name.starts_with("plenty_op_")
            || matches!(name.as_str(), "plenty_retain" | "plenty_release")),
        "{calls:?}"
    );
}
