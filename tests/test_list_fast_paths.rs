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
def total(xs: &list[i64], ys: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for i in range(len(xs)):
        total += xs[i] * ys[i]
    Ok(total)
def main() -> Result[(), OverflowError[i64]]:
    print(total(&[1, 2].unwrap(), &[3, 4].unwrap())?).unwrap()
    Ok(())
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
    // Scalar rows are dense: the setup reads the header without a call.
    assert!(!asm.contains("# plenty_list_rows"), "{asm}");
    assert!(!asm.contains("# plenty_list_row_bytes"), "{asm}");
    // Only the out-of-bounds cold block calls out, to report the index.
    // `xs[i]` needs no check: `i` is below `len(xs)` (issue #728).
    assert_eq!(asm.matches("# plenty_trap_list_index").count(), 1, "{asm}");
    assert!(!asm.contains("plenty_list_scalar_get"), "{asm}");
}

#[test]
fn indexed_shared_columns_keep_bounds_bits_and_loop_control() {
    support::assert_visible_stdout(
        r#"def total(xs: &list[i64], ys: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for i in range(len(xs)):
        if i == 1:
            continue
        total += xs[i] * ys[-i - 1]
    Ok(total)
def floats(xs: &list[f32], flags: &list[bool], wide: &list[u64]) -> ():
    for i in range(len(xs)):
        print(xs[i], flags[i], wide[i]).unwrap()
def local_columns(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    column = xs
    mut total = 0
    for i in range(2):
        for j in range(len(column)):
            total += column[j]
    Ok(total)
def main() -> Result[(), OverflowError[i64]]:
    xs = [1, 2, 3].unwrap()
    ys = [4, 5, 6].unwrap()
    print(total(&xs, &ys)?, local_columns(&xs)?).unwrap()
    floats(&[-1.25f32].unwrap(), &[True].unwrap(), &[18446744073709551615u64].unwrap())
    Ok(())
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
        "def read(xs: &list[i64], ys: &list[i64]) -> Result[i64, OverflowError[i64]]:\n    mut result = 0\n    for i in range(len(xs)):\n        result += xs[i] + ys[{index}]\n    Ok(result)\ndef main() -> Result[(), OverflowError[i64]]:\n    print(read(&[1].unwrap(), &{values})?).unwrap()\n    Ok(())\n"
    );
    let output = support::run(&source);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("index out of bounds"));
}

#[test]
fn indexed_loops_that_resize_or_retarget_keep_ordinary_access() {
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
        r#"def index(value: i64) -> i64:
    print(value).unwrap()
    value
def total(xs: &list[i64], ys: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut result = 0
    for i in range(2):
        result += xs[index(i)] + ys[index(-i - 1)]
    Ok(result)
def main() -> Result[(), OverflowError[i64]]:
    print(total(&[1, 2].unwrap(), &[3, 4].unwrap())?).unwrap()
    Ok(())
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
def total() -> Result[i64, AllocError | OverflowError[i64]]:
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
    total += iter([1, 2, 3]?).map(def(n: i64) -> Result[i64, OverflowError[i64]]: Ok(n * 2)).try_fold(0, def(a: i64, b: Result[i64, OverflowError[i64]]) -> Result[i64, OverflowError[i64]]: Ok(a + b?))?
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
        r#"def receiver() -> list[i64]:
    print("receiver").unwrap()
    [42].unwrap()
def index() -> i64:
    print("index").unwrap()
    -1
def read(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    Ok(xs[index()] + len(xs))
def main() -> Result[(), OverflowError[i64]]:
    print(receiver()[index()]).unwrap()
    print(len(receiver())).unwrap()
    mut xs = [1].unwrap()
    print(read(&xs)?).unwrap()
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
    Ok(())
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
        "def read(xs: &list[i64], i: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(xs[i] + len(xs))\ndef main() -> Result[(), OverflowError[i64]]:\n    xs = [1].unwrap()\n    print(read(&xs, 0)?).unwrap()\n    Ok(())\n",
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

/// Owned lists and exclusive references read their header once per loop
/// when the loop cannot resize them, in `while` and `for` loops (issue #715).
#[test]
fn row_caches_cover_exclusive_owned_and_while_loops() {
    support::assert_visible_stdout(
        r#"
def reverse(xs: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    mut lo = 0
    mut hi = len(xs) - 1
    while lo < hi:
        t = xs[lo]
        xs[lo] = xs[hi]
        xs[hi] = t
        lo += 1
        hi -= 1
    Ok(())
def bump(xs: &mut list[u8], times: i64) -> Result[(), OverflowError[u8] | OverflowError[i64]]:
    for _ in range(times):
        for i in range(len(xs)):
            xs[-i - 1] += 1u8
    Ok(())
def scan(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut i = 0
    while xs[i] != 0:
        i += 1
    Ok(i)
def grow(xs: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    mut i = 0
    while i < 3:
        xs[0] = xs[-1] + 1
        xs.append(xs[0]).unwrap()
        i += 1
    Ok(())
def main() -> Result[(), OverflowError[u8] | OverflowError[i64]]:
    mut xs = [1, 2, 3, 4, 5].unwrap()
    reverse(&mut xs)?
    print(xs).unwrap()
    mut bytes = [1u8, 2u8].unwrap()
    bump(&mut bytes, 3)?
    print(bytes).unwrap()
    print(scan(&[3, 2, 0, 1].unwrap())?).unwrap()
    grow(&mut xs)?
    print(xs).unwrap()
    mut owned = [0.5, 1.5, 2.5].unwrap()
    mut j = 0
    while j < len(owned):
        owned[j] = owned[-j - 1] * 2.0
        j += 1
    print(owned).unwrap()
    Ok(())
"#,
        "[5, 4, 3, 2, 1]\n[4, 5]\n2\n[4, 4, 3, 2, 1, 2, 3, 4]\n[5.0, 3.0, 10.0]",
    );
}

#[rstest]
#[case("0", "list[i64]().unwrap()")]
#[case("1", "[1].unwrap()")]
#[case("-2", "[1].unwrap()")]
#[case("-9223372036854775808", "[1].unwrap()")]
#[case("9223372036854775807", "[1].unwrap()")]
fn cached_exclusive_and_owned_rows_trap_out_of_bounds(#[case] index: &str, #[case] values: &str) {
    for source in [
        format!(
            "def read(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:\n    mut result = 0\n    mut i = 0\n    while i < 2:\n        result += xs[{index}]\n        i += 1\n    Ok(result)\ndef main() -> Result[(), OverflowError[i64]]:\n    mut xs = {values}\n    print(read(&mut xs)?).unwrap()\n    Ok(())"
        ),
        format!(
            "def write(xs: &mut list[i64]) -> ():\n    for _ in range(2):\n        xs[{index}] = 1\nmut xs = {values}\nwrite(&mut xs)"
        ),
        format!(
            "def main() -> Result[(), OverflowError[i64]]:\n    mut xs = {values}\n    for _ in range(2):\n        xs[{index}] += 1\n    Ok(())"
        ),
        format!(
            "def size(xs: &[i64]) -> i64:\n    len(xs)\ndef main() -> Result[(), OverflowError[i64]]:\n    mut xs = {values}\n    mut n = 0\n    for _ in range(2):\n        n += size(&xs) + xs[{index}]\n    print(n).unwrap()\n    Ok(())"
        ),
    ] {
        let output = support::run(&source);
        assert!(!output.status.success(), "{source}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("index out of bounds"),
            "{source}"
        );
    }
}

fn dump(kind: &str, filter: &str, source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rows.plenty");
    std::fs::write(&path, source).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(["--release", "--dump", kind, "--dump-filter", filter])
        .arg(path)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

const REVERSE: &str = r#"
def reverse(xs: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    mut lo = 0
    mut hi = len(xs) - 1
    while lo < hi:
        t = xs[lo]
        xs[lo] = xs[hi]
        xs[hi] = t
        lo += 1
        hi -= 1
    Ok(())
def grow(xs: &mut list[i64]) -> ():
    for i in range(2):
        xs[i] = 1
        xs.append(i).unwrap()
def swap_bump(xs: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    for i in range(1):
        xs.swap(i, 1)
        xs[i] += 1
    Ok(())
def main() -> Result[(), OverflowError[i64]]:
    mut xs = [1, 2].unwrap()
    reverse(&mut xs)?
    grow(&mut xs)
    swap_bump(&mut xs)?
    Ok(())
"#;

/// A loop that cannot resize its list uses the cached accesses; one that can
/// keeps the accesses that read the header (issue #715).
#[test]
fn row_caches_fall_back_when_the_loop_can_resize_the_list() {
    let reverse = dump("ops", "reverse", REVERSE);
    assert_eq!(
        reverse.matches("ListScalarGetView(").count(),
        2,
        "{reverse}"
    );
    assert_eq!(
        reverse.matches("ListScalarSetView(").count(),
        2,
        "{reverse}"
    );
    assert!(!reverse.contains("ListScalarGetRef("), "{reverse}");
    assert!(!reverse.contains("ListScalarSetRef("), "{reverse}");
    let grow = dump("ops", "grow", REVERSE);
    assert!(grow.contains("ListScalarSetRef("), "{grow}");
    assert!(!grow.contains("View("), "{grow}");
    // A swap cannot resize the list, so it keeps the cache.
    let swap = dump("ops", "swap_bump", REVERSE);
    assert!(swap.contains("ListScalarSwapRef("), "{swap}");
    assert!(swap.contains("ListScalarSetView("), "{swap}");
    assert!(!swap.contains("ListScalarSetRef("), "{swap}");
}

/// The reversal loop reads the list's flag byte only for `len(xs)` and the
/// loop's setup, and
/// a nonnegative index takes one unsigned comparison: no `cmov`
/// normalization (issue #715).
#[cfg(target_arch = "x86_64")]
#[test]
fn cached_rows_read_the_header_once_and_skip_normalization() {
    let asm = dump("asm", "reverse", REVERSE);
    let removed = regex::Regex::new(r"movzb\S*\s+0x30\(").unwrap();
    assert_eq!(removed.find_iter(&asm).count(), 2, "{asm}");
    assert!(!asm.contains("cmov"), "{asm}");
}

/// An index failure stores its index and length to the stack for the
/// helper, so the hot loop keeps its values out of the call's argument
/// registers: the reversal loop has no register-to-register moves.
#[cfg(target_arch = "x86_64")]
#[test]
fn index_failures_add_no_moves_to_the_loop() {
    let asm = dump("asm", "reverse", REVERSE);
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
    let (end, start) = instructions
        .iter()
        .find_map(|(offset, text)| {
            let target = text.strip_prefix("jmp")?.trim().strip_prefix("0x")?;
            let target = u64::from_str_radix(target, 16).ok()?;
            (target < *offset).then_some((*offset, target))
        })
        .expect("reversal loop backedge");
    // The function's exit lies inside the address range, so the frame
    // pointer's restore is not a loop move.
    let register_move = regex::Regex::new(r"^mov\s+%(\w+),%(\w+)$").unwrap();
    let register_move = |text: &str| {
        register_move
            .captures(text)
            .is_some_and(|c| ![&c[1], &c[2]].iter().any(|r| *r == "rsp" || *r == "rbp"))
    };
    assert!(
        !instructions
            .iter()
            .any(|(offset, text)| (start..=end).contains(offset) && register_move(text)),
        "{asm}"
    );
    assert_eq!(asm.matches("# plenty_trap_list_index").count(), 2, "{asm}");
}

/// Release builds report the index as written and the length through the
/// stored pair.
#[rstest]
#[case("-4", "index -4, length 3")]
#[case("3", "index 3, length 3")]
fn release_index_failures_report_the_index_and_length(#[case] index: &str, #[case] message: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.plenty");
    std::fs::write(
        &path,
        format!(
            "def at(xs: &[i64], i: i64) -> i64:\n    xs[i]\n\
             def main() -> ():\n    xs = [1, 2, 3].unwrap()\n    print(at(&xs, {index})).unwrap()\n"
        ),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--release")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("index.plenty:2:5: index out of bounds: {message}")),
        "{stderr}"
    );
}

/// Loops whose calls take shared or view borrows, or an exclusive borrow of
/// another owner, keep the row cache, for locals and field lists alike; calls
/// that could resize the list through an exclusive list, owner or method
/// receiver fall back (issue #717).
const CALLS: &str = r#"
class Bag:
    items: list[i64]
    total: i64

    def add(self: &mut Bag, v: i64) -> ():
        self.items.append(v).unwrap()

def peek(xs: &[i64], i: i64) -> i64:
    xs[i]
def bump(xs: &mut [i64], i: i64) -> Result[(), OverflowError[i64]]:
    xs[i] += 1
    Ok(())
def first(xs: &list[i64]) -> i64:
    xs[0]
def extend(xs: &mut list[i64]) -> ():
    xs.append(xs[0]).unwrap()
def refill(bag: &mut Bag) -> ():
    bag.items.append(9).unwrap()
def viewed(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut s = 0
    for i in range(len(xs)):
        s += xs[i] + peek(xs, i) + first(xs)
        bump(xs, i)?
    Ok(s)
def others(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut zs = [5].unwrap()
    mut s = 0
    for i in range(len(xs)):
        s += xs[i]
        extend(&mut zs)
    Ok(s + len(zs))
def fields(bag: &mut Bag) -> Result[i64, OverflowError[i64]]:
    mut s = 0
    for i in range(len(bag.items)):
        s += bag.items[i]
        bag.items[i] = peek(&bag.items, i) * 2
        bag.total += 1
    Ok(s + bag.total)
def shared_fields(bag: &Bag) -> Result[i64, OverflowError[i64]]:
    mut s = 0
    for i in range(len(bag.items)):
        s += bag.items[i] + first(&bag.items)
    Ok(s)
def extended(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut s = 0
    for i in range(3):
        s += xs[i]
        extend(xs)
    Ok(s)
def refilled(bag: &mut Bag) -> Result[i64, OverflowError[i64]]:
    mut s = 0
    for i in range(3):
        s += bag.items[i]
        refill(bag)
    Ok(s)
def added(bag: &mut Bag) -> Result[i64, OverflowError[i64]]:
    mut s = 0
    mut i = 0
    while i < 3:
        s += bag.items[i]
        bag.add(i)
        i += 1
    Ok(s)
def nested(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut s = 0
    for i in range(3):
        s += xs[i]
        for _ in range(2):
            s += xs[0]
            extend(xs)
    Ok(s)
def owned() -> Result[i64, OverflowError[i64]]:
    mut xs = [1, 2, 3].unwrap()
    mut s = 0
    for i in range(3):
        s += xs[i]
        extend(&mut xs)
    Ok(s + len(xs))
def main() -> Result[(), OverflowError[i64]]:
    mut xs = [1, 2, 3].unwrap()
    print(viewed(&mut xs)?).unwrap()
    print(others(&mut xs)?).unwrap()
    mut bag = Bag([1, 2].unwrap(), 0)
    print(fields(&mut bag)?).unwrap()
    print(shared_fields(&bag)?).unwrap()
    mut ys = [4].unwrap()
    print(extended(&mut ys)?).unwrap()
    mut one = Bag([1].unwrap(), 0)
    print(refilled(&mut one)?).unwrap()
    mut four = Bag([4].unwrap(), 0)
    print(added(&mut four)?).unwrap()
    mut zs = [1].unwrap()
    print(nested(&mut zs)?).unwrap()
    print(owned()?).unwrap()
    Ok(())
"#;

#[test]
fn row_caches_survive_calls_that_cannot_resize_the_list() {
    support::assert_visible_stdout(CALLS, "17\n13\n5\n10\n12\n19\n5\n9\n12");
    let views = |name: &str| {
        let ops = dump("ops", &format!("__plenty_fn_{name}"), CALLS);
        let cached =
            ops.matches("ListScalarGetView(").count() + ops.matches("ListScalarSetView(").count();
        let ordinary =
            ops.matches("ListScalarGetRef(").count() + ops.matches("ListScalarSetRef(").count();
        (cached, ordinary)
    };
    for (name, accesses) in [
        ("viewed", 1),
        ("others", 1),
        ("fields", 2),
        ("shared_fields", 1),
    ] {
        assert_eq!(views(name), (accesses, 0), "{name}");
    }
    for (name, accesses) in [
        ("extended", 1),
        ("refilled", 1),
        ("added", 1),
        ("nested", 2),
        ("owned", 1),
    ] {
        assert_eq!(views(name), (0, accesses), "{name}");
    }
}

/// With a call in the loop, the list header is still read only before it
/// (issue #717): every flag-byte read precedes the loop's head, the target of
/// its back edge. `first` is recursive so that it stays a call.
#[cfg(target_arch = "x86_64")]
#[test]
fn cached_rows_across_a_call_read_the_header_before_the_loop() {
    let asm = dump(
        "asm",
        "summed",
        r#"
def first(xs: &list[i64], n: i64) -> Result[i64, OverflowError[i64]]:
    if n == 0:
        return Ok(xs[0])
    first(xs, n - 1)
def summed(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut s = 0
    for i in range(len(xs)):
        s += xs[i] + first(xs, 0)?
    Ok(s)
def main() -> Result[(), OverflowError[i64]]:
    mut xs = [1, 2, 3].unwrap()
    print(summed(&mut xs)?).unwrap()
    Ok(())
"#,
    );
    let line = regex::Regex::new(r"(?m)^\s*([0-9a-f]+):\s+(\S+)\s+(\S+)").unwrap();
    let mut header = Vec::new();
    let mut head = usize::MAX;
    for found in line.captures_iter(&asm) {
        let at = usize::from_str_radix(&found[1], 16).unwrap();
        if found[2].starts_with("movzb") && found[3].starts_with("0x30(") {
            header.push(at);
        } else if &found[2] == "jmp"
            && let Some(target) = found[3].strip_prefix("0x")
            && let Ok(target) = usize::from_str_radix(target, 16)
            && target < at
        {
            head = head.min(target);
        }
    }
    assert!(head != usize::MAX, "{asm}");
    assert!(
        !header.is_empty() && header.iter().all(|&at| at < head),
        "{asm}"
    );
}

/// Accesses of one row in a statement share its bounds check (issue #721),
/// and their results and traps match unshared checks: through lists and
/// views, with constant, negative and computed indices.
const REPEATED: &str = r#"
def swap_ends(xs: &mut [i64], i: i64) -> Result[(), OverflowError[i64]]:
    j = -1 - i
    t = xs[i]
    xs[i] = xs[j]
    xs[j] = t
    Ok(())
def double(xs: &mut [i64], i: i64) -> Result[(), OverflowError[i64]]:
    xs[i] = xs[i] + xs[i]
    Ok(())
def sum_twice(xs: &[i64], i: i64) -> Result[i64, OverflowError[i64]]:
    Ok(xs[i] + xs[i] + xs[-1] + xs[-1] + xs[0] + xs[0])
def main() -> Result[(), OverflowError[i64]]:
    mut xs = [1, 2, 3, 4, 5].unwrap()
    swap_ends(&mut xs, 0)?
    swap_ends(&mut xs, -4)?
    double(&mut xs, 2)?
    double(&mut xs, -1)?
    print(xs).unwrap()
    print(sum_twice(&xs, 1)?).unwrap()
    print(sum_twice(&xs, -5)?).unwrap()
    Ok(())
"#;

#[test]
fn repeated_row_accesses_keep_their_results() {
    support::assert_stdout(REPEATED, "[5, 4, 6, 2, 2]\n22\n24\n");
}

#[rstest]
#[case("swap_ends(&mut xs, 5)?", "index 5, length 5")]
#[case("swap_ends(&mut xs, -6)?", "index -6, length 5")]
#[case("double(&mut xs, 5)?", "index 5, length 5")]
#[case("double(&mut xs, -6)?", "index -6, length 5")]
#[case("print(sum_twice(&xs, 7)?).unwrap()", "index 7, length 5")]
#[case("print(sum_twice(&xs[5:], 0)?).unwrap()", "index 0, length 0")]
#[case("print(sum_twice(&xs[:1], -2)?).unwrap()", "index -2, length 1")]
fn repeated_row_accesses_trap_with_the_first_failing_index(
    #[case] call: &str,
    #[case] message: &str,
) {
    let source = REPEATED.replace(
        "    swap_ends(&mut xs, 0)?\n",
        &format!("    {call}\n    swap_ends(&mut xs, 0)?\n"),
    );
    let output = support::run(&source);
    assert!(!output.status.success(), "{source}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("index out of bounds: {message}")),
        "{stderr}"
    );
}

/// Each distinct row is checked once: `double` has one row and `sum_twice`
/// three, so the release build emits one trap call per row (issue #721).
#[test]
fn repeated_row_accesses_check_each_row_once() {
    for (name, rows) in [("double", 1), ("sum_twice", 3), ("swap_ends", 2)] {
        let clif = dump("clif-opt", &format!("__plenty_fn_{name}"), REPEATED);
        let traps = clif.matches("call fn").count();
        assert_eq!(traps, rows, "{name}\n{clif}");
    }
}

/// A `range` counter starting at a nonnegative constant with step 1 is below
/// the range's stop. When the stop is the length of a shared list reference
/// or a slice view, indexing that list or view with the counter cannot fail,
/// so it has no bounds check (issue #728). Other lists, other stops and
/// negative starts keep theirs.
const RANGE_BOUNDS: &str = r#"
def sum_index(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(len(xs)):
        t += xs[i]
    Ok(t)
def sum_from(xs: &list[u8], n: &list[u8]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(1, len(xs)):
        for j in range(len(n)):
            t += i64(xs[i]) * i64(n[j])
    Ok(t)
def sum_other(xs: &list[i64], ys: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(len(xs)):
        t += ys[i]
    Ok(t)
def sum_past(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(len(xs) + 1):
        t += xs[i]
    Ok(t)
def sum_wrapped(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(-1, len(xs)):
        t += xs[i]
    Ok(t)
def scale(s: &mut [i64]) -> ():
    for i in range(len(s)):
        s[i] = s[i] ^ 3
def copy_row(source: &[i64], target: &mut [i64]) -> ():
    for i in range(len(source)):
        target[i] = source[i]
def main() -> Result[(), OverflowError[i64]]:
    xs = [1, 2, 3, 4].unwrap()
    bytes = [5u8, 6u8, 7u8].unwrap()
    mut zs = [1, 2, 3, 4].unwrap()
    scale(&mut zs[1:3])
    copy_row(&xs[:2], &mut zs[2:])
    print(zs).unwrap()
    empty = list[i64].with_capacity(0).unwrap()
    short = [1, 2, 3].unwrap()
    print(sum_other(&short, &short)?).unwrap()
    print(sum_index(&xs)?, sum_index(&empty)?).unwrap()
    print(sum_from(&bytes, &bytes)?).unwrap()
    print(sum_other(&xs, &xs)?, sum_wrapped(&xs)?).unwrap()
    Ok(())
"#;

#[test]
fn range_counters_index_their_shared_lists() {
    support::assert_stdout(RANGE_BOUNDS, "[1, 1, 1, 2]\n6\n10 0\n234\n10 14\n");
}

#[rstest]
#[case("print(sum_other(&xs, &short)?).unwrap()", "index 3, length 3")]
#[case("print(sum_past(&xs)?).unwrap()", "index 4, length 4")]
#[case("copy_row(&xs[:], &mut zs[1:])", "index 3, length 3")]
fn range_counters_trap_on_other_bounds(#[case] call: &str, #[case] message: &str) {
    let source = RANGE_BOUNDS.replace(
        "    print(sum_other(&short, &short)?",
        &format!("    {call}\n    print(sum_other(&short, &short)?"),
    );
    let output = support::run(&source);
    assert!(!output.status.success(), "{source}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("index out of bounds: {message}")),
        "{stderr}"
    );
}

#[test]
fn range_counters_below_a_shared_length_skip_the_check() {
    for (name, checks) in [
        ("sum_index", 0),
        ("sum_from", 0),
        ("sum_other", 1),
        ("sum_past", 1),
        ("sum_wrapped", 1),
        ("scale", 0),
        ("copy_row", 1),
    ] {
        let clif = dump("clif-opt", &format!("__plenty_fn_{name}"), RANGE_BOUNDS);
        let traps = clif.matches("plenty_trap_list_index").count();
        assert_eq!(traps > 0, checks > 0, "{name}\n{clif}");
    }
}

/// A shared reborrow of an exclusive reference has the same native value as
/// its source. Once it ends, the source may clear or replace the list, so a
/// length read through the reborrow, or through an inlined shared-parameter
/// helper, says nothing about later accesses (issue #736).
const ENDED_LOANS: &str = r#"
def length(xs: &list[i64]) -> i64:
    len(xs)
def read(xs: &mut list[i64]) -> i64:
    shared = &xs
    n = len(shared)
    xs.clear()
    mut t = 0
    for i in range(n):
        t = t ^ xs[i]
    t
def write(xs: &mut list[i64]) -> ():
    shared = &xs
    n = len(shared)
    xs.clear()
    for i in range(n):
        xs[i] = 99
def replace(xs: &mut list[i64]) -> i64:
    shared = &xs
    n = len(shared)
    *xs = list[i64].with_capacity(0).unwrap()
    mut t = 0
    for i in range(n):
        t = t ^ xs[i]
    t
def inlined(xs: &mut list[i64]) -> i64:
    n = length(&xs)
    xs.clear()
    mut t = 0
    for i in range(n):
        t = t ^ xs[i]
    t
def main() -> ():
    mut xs = [10, 20, 30].unwrap()
    CALL
"#;

#[rstest]
fn range_counters_trap_after_their_length_loan_ends(
    #[values(
        "print(read(&mut xs)).unwrap()",
        "write(&mut xs)",
        "print(replace(&mut xs)).unwrap()",
        "print(inlined(&mut xs)).unwrap()"
    )]
    call: &str,
    #[values("--fast-compile", "--release")] mode: &str,
) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("loans.plenty");
    std::fs::write(&source, ENDED_LOANS.replace("CALL", call)).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .env("PLENTY_CACHE", "off")
        .arg(mode)
        .arg(&source)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{call} {mode}\n{stderr}");
    assert!(
        stderr.contains("index out of bounds: index 0, length 0"),
        "{call} {mode}\n{stderr}"
    );
}

/// `while lo < hi` loops whose body starts with an access indexed by `hi`
/// through an unchanging view, `hi` only decreasing (issue #728).
const RELATION: &str = r#"
def reverse(s: &mut [i64], first: i64) -> Result[(), OverflowError[i64]]:
    mut lo = 1
    mut hi = first - 1
    while lo < hi:
        s.swap(lo, hi)
        lo += 1
        hi -= 1
    Ok(())
def from_top(s: &[i64]) -> Result[i64, OverflowError[i64]]:
    low = 0
    mut hi = len(s) - 1
    mut t = 0
    while low < hi:
        t = s[hi] + t * 10
        hi -= 2
    Ok(t)
def from_param(s: &[i64], lo: i64) -> Result[i64, OverflowError[i64]]:
    mut low = lo
    mut hi = len(s) - 1
    mut t = 0
    while low < hi:
        t += s[hi]
        low += 1
    Ok(t)
def low_only(s: &[i64], hi: i64) -> Result[i64, OverflowError[i64]]:
    mut lo = 0
    mut high = hi
    mut t = 0
    while lo < high:
        t += s[lo]
        lo += 1
        high -= 1
    Ok(t)
def offset(s: &mut [i64], hi: i64) -> Result[(), OverflowError[i64]]:
    mut lo = 0
    mut high = hi
    while lo < high:
        s.swap(lo, high - 1)
        lo += 1
        high -= 1
    Ok(())
def grows(s: &[i64], hi: i64) -> Result[i64, OverflowError[i64]]:
    mut lo = 0
    mut high = hi
    mut t = 0
    while lo < high:
        t += s[high]
        lo += 2
        high += 1
    Ok(t)
def main() -> Result[(), OverflowError[i64]]:
    mut xs = [0, 1, 2, 3, 4, 5, 6].unwrap()
    reverse(&mut xs[:], 6)?
    print(xs).unwrap()
    reverse(&mut xs[2:], 1)?
    print(from_top(&xs[:])?, from_top(&xs[:1])?).unwrap()
    print(low_only(&xs[:], 3)?).unwrap()
    offset(&mut xs[:4], 4)?
    print(xs).unwrap()
    print(grows(&xs[:], 2)?).unwrap()
    Ok(())
"#;

#[test]
fn relational_counters_keep_their_results() {
    support::assert_stdout(
        RELATION,
        "[0, 5, 4, 3, 2, 1, 6]\n624 0\n5\n[3, 4, 5, 0, 2, 1, 6]\n5\n",
    );
}

#[rstest]
#[case("reverse(&mut xs[:3], 6)?", "index 5, length 3")]
#[case("reverse(&mut xs[:1], 6)?", "index 1, length 1")]
#[case("print(grows(&xs[:], 7)?).unwrap()", "index 7, length 7")]
fn relational_counters_trap_as_the_first_pass_would(#[case] call: &str, #[case] message: &str) {
    let source = RELATION.replace(
        "    print(xs).unwrap()\n    reverse(&mut xs[2:]",
        &format!("    {call}\n    print(xs).unwrap()\n    reverse(&mut xs[2:]"),
    );
    for mode in ["--fast-compile", "--release"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relation.plenty");
        std::fs::write(&path, &source).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
            .env("PLENTY_CACHE", "off")
            .arg(mode)
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{call} {mode}\n{stderr}");
        assert!(
            stderr.contains(&format!("index out of bounds: {message}")),
            "{call} {mode}\n{stderr}"
        );
    }
}

/// Calls to `plenty_trap_list_index` in or after the first loop header of
/// `clif`, a back-edge target: a check before the loop comes earlier.
fn loop_index_traps(clif: &str) -> usize {
    let lines: Vec<&str> = clif.lines().collect();
    let label = |line: &str| {
        let name = line.split(['(', ':']).next()?;
        (name.starts_with("block") && !line.starts_with(' ')).then(|| name.to_string())
    };
    let header = lines
        .iter()
        .enumerate()
        .filter_map(|(at, line)| Some((at, label(line)?)))
        .find(|(at, name)| {
            lines[at + 1..]
                .iter()
                .any(|line| line.trim_start().starts_with(&format!("jump {name}(")))
        })
        .map_or(lines.len(), |(at, _)| at);
    let traps: Vec<String> = lines
        .iter()
        .filter(|line| line.ends_with("; plenty_trap_list_index"))
        .filter_map(|line| Some(line.trim().split(' ').next()?.to_string()))
        .collect();
    lines[header..]
        .iter()
        .filter(|line| {
            traps
                .iter()
                .any(|name| line.contains(&format!("call {name}(")))
        })
        .count()
}

#[test]
fn relational_counters_check_before_the_loop() {
    for (name, in_loop) in [
        ("reverse", 0),
        ("from_top", 0),
        ("from_param", 1),
        ("low_only", 1),
        ("offset", 2),
        ("grows", 1),
    ] {
        let clif = dump("clif-opt", &format!("__plenty_fn_{name}"), RELATION);
        assert_eq!(loop_index_traps(&clif), in_loop, "{name}\n{clif}");
    }
}

/// `enumerate` over a fresh forward list or view cursor counts with the
/// cursor's own index (issue #727), so `xs[i]` on the same list needs no check
/// and the count needs no overflow check. Other sources keep their own count.
const ENUMERATE: &str = r#"
def sum_self(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in enumerate(xs):
        t += *x * xs[i]
    Ok(t)
def view_self(v: &[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in enumerate(v):
        t += *x * v[i]
    Ok(t)
def sum_other(xs: &list[i64], ys: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in enumerate(xs):
        t += *x * ys[i]
    Ok(t)
def with_start(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in enumerate(xs, 1):
        t += *x * xs[i]
    Ok(t)
def reversed_self(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in enumerate(reversed(xs)):
        t += *x * xs[i]
    Ok(t)
def filtered(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in enumerate(iter(xs).filter(def(x: &i64) -> bool: *x > 1)):
        t += *x * xs[i]
    Ok(t)
def owned(xs: list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in enumerate(xs):
        t += x * i
    Ok(t)
def method(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in iter(xs).enumerate():
        t += *x * xs[i]
    Ok(t)
def paired(xs: list[i64], ys: list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, p in enumerate(zip(xs, ys)):
        a, b = p
        t += i * (a + b)
    Ok(t)
def nested(xs: list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, p in enumerate(enumerate(xs, 5)):
        j, x = p
        t += i * j + x
    Ok(t)
def resumed(xs: list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    mut q = iter(xs)
    match next(&mut q):
        case Some(x):
            t += x
        case Nothing:
            pass
    for i, x in enumerate(q):
        t += i * 100 + x
    Ok(t)
def labels(xs: list[i64]) -> Generator[i64]:
    unchecked:
        for i, x in enumerate(xs):
            yield i * 10 + x
def main() -> Result[(), OverflowError[i64] | AllocError | IoError]:
    xs = [1, 2, 3]?
    ys = [4, 5, 6]?
    empty = list[i64].with_capacity(0)?
    print(sum_self(&xs)?, sum_self(&empty)?, view_self(&xs[1:])?)?
    print(sum_other(&xs, &ys)?, reversed_self(&xs)?, filtered(&xs)?)?
    print(owned(copy(xs)?)?, method(&xs)?, paired(copy(xs)?, copy(ys)?)?)?
    print(nested(copy(xs)?)?, resumed(copy(xs)?)?)?
    for v in labels(copy(ys)?):
        print(v)?
    Ok(())
"#;

#[test]
fn enumerate_counters_keep_their_results() {
    support::assert_stdout(ENUMERATE, "14 0 13\n32 10 8\n8 14 25\n26 106\n4\n15\n26\n");
}

#[rstest]
#[case("print(with_start(&xs)?)?", "index 3, length 3")]
#[case("print(sum_other(&xs, &empty)?)?", "index 0, length 0")]
fn enumerate_counters_trap_on_other_bounds(#[case] call: &str, #[case] message: &str) {
    let source = ENUMERATE.replace(
        "    print(sum_self(&xs)?",
        &format!("    {call}\n    print(sum_self(&xs)?"),
    );
    let output = support::run(&source);
    assert!(!output.status.success(), "{source}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("index out of bounds: {message}")),
        "{stderr}"
    );
}

#[test]
fn enumerate_counters_reuse_the_cursor_index() {
    // `sadd_overflow` counts the checked `t +=` plus any checked count.
    for (name, traps, adds) in [
        ("sum_self", 0, 1),
        ("view_self", 0, 1),
        ("method", 0, 1),
        ("sum_other", 1, 1),
        ("with_start", 1, 2),
        ("reversed_self", 1, 2),
        ("filtered", 1, 2),
    ] {
        let clif = dump("clif-opt", &format!("__plenty_fn_{name}"), ENUMERATE);
        assert_eq!(
            (
                clif.matches("plenty_trap_list_index").count() > 0,
                clif.matches("sadd_overflow").count()
            ),
            (traps > 0, adds),
            "{name}\n{clif}"
        );
    }
}

/// Owned lists and exclusive references whose loop cannot resize them: the
/// row cache takes `range(len(xs))`'s stop as its length, and an owned
/// `for x in xs` reads its hidden list's length and rows once (issue #728).
const OWNED_BOUNDS: &str = r#"
class Item:
    name: str
    hits: i64

def sum_owned(n: i64) -> Result[i64, OverflowError[i64] | AllocError]:
    mut xs = list[i64].with_capacity(n)?
    for i in range(n):
        xs.append(i)?
    mut t = 0
    for i in range(len(xs)):
        xs[i] = xs[i] * 2
        t += xs[i]
    Ok(t)
def sum_mut(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(len(xs)):
        xs[i] = xs[i] + 1
        t += xs[i]
    Ok(t)
def sum_stale(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    n = len(xs)
    xs.clear()
    mut t = 0
    for i in range(n):
        t += xs[i]
    Ok(t)
def sum_popped(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(len(xs)):
        t += xs[i]
        _ = xs.pop()
    Ok(t)
def sum_other(xs: &mut list[i64], ys: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(len(xs)):
        t += ys[i]
    Ok(t)
def cursor_sum(xs: list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for x in xs:
        t += x
    Ok(t)
def cursor_bytes(xs: list[u8]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i, x in enumerate(xs):
        t += i * i64(x)
    Ok(t)
def cursor_text(xs: list[str]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for s in xs:
        if s == "stop":
            break
        t += len(s)
    Ok(t)
def cursor_items(xs: list[Item]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for item in xs:
        if item.hits < 0:
            break
        t += item.hits + len(item.name)
    Ok(t)
def main() -> Result[(), OverflowError[i64] | AllocError | IoError]:
    mut xs = [1, 2, 3]?
    mut short = [1, 2]?
    print(sum_owned(4)?, sum_mut(&mut xs)?, xs)?
    print(cursor_sum([4, 5, 6]?)?, cursor_bytes([1u8, 2u8, 3u8]?)?)?
    print(cursor_text(["a", "bb", "stop", "dddd"]?)?)?
    items = [Item(name="ab", hits=1), Item(name="c", hits=-1), Item(name="d", hits=5)]?
    print(cursor_items(items)?)?
    Ok(())
"#;

#[test]
fn owned_range_counters_and_cursors_keep_their_results() {
    support::assert_stdout(OWNED_BOUNDS, "12 9 [2, 3, 4]\n15 8\n3\n3\n");
}

#[rstest]
#[case("print(sum_stale(&mut xs)?)?", "index 0, length 0")]
#[case("print(sum_popped(&mut xs)?)?", "index 2, length 1")]
#[case("print(sum_other(&mut xs, &mut short)?)?", "index 2, length 2")]
fn owned_range_counters_trap_on_other_lengths(#[case] call: &str, #[case] message: &str) {
    let source = OWNED_BOUNDS.replace(
        "    print(cursor_sum(",
        &format!("    {call}\n    print(cursor_sum("),
    );
    let output = support::run(&source);
    assert!(!output.status.success(), "{source}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("index out of bounds: {message}")),
        "{stderr}"
    );
}

#[test]
fn owned_range_counters_and_cursors_skip_checks() {
    // `sadd_overflow` counts the checked `t +=` plus any checked step.
    // `sum_stale` and `sum_other` are versioned: both copies add, and the
    // checked copy keeps the trap for a stop past the indexed length.
    for (name, traps, adds) in [
        ("sum_owned", 0, 1),
        ("sum_mut", 0, 2),
        ("sum_stale", 1, 2),
        ("sum_popped", 1, 1),
        ("sum_other", 1, 2),
        ("cursor_sum", 0, 1),
        ("cursor_bytes", 0, 1),
        ("cursor_text", 0, 1),
        ("cursor_items", 0, 2),
    ] {
        let clif = dump("clif-opt", &format!("__plenty_fn_{name}"), OWNED_BOUNDS);
        assert_eq!(
            (
                clif.matches("plenty_trap_list_index").count() > 0,
                clif.matches("sadd_overflow").count()
            ),
            (traps > 0, adds),
            "{name}\n{clif}"
        );
    }
}

#[test]
fn owned_cursor_takes_packed_atomic_rows() {
    // Atomic rows have their element's packed width; taking a 16-byte slot
    // would read the neighbouring rows.
    support::assert_stdout(
        r#"
from std.atomic import Atomic, Ordering
def main() -> Result[(), AllocError | IoError]:
    flags = [Atomic(False), Atomic(True), Atomic(False)]?
    for flag in flags:
        print(flag.load(Ordering.Acquire))?
    counts = [Atomic[u8](3), Atomic[u8](4)]?
    for count in counts:
        print(count.into_inner())?
    Ok(())
"#,
        "False\nTrue\nFalse\n3\n4\n",
    );
}

const SAVED_LENGTHS: &str = r#"
def saved(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    n = len(xs)
    mut t = 0
    for i in range(n):
        t += xs[i]
    Ok(t)

def saved_view(v: &[i64]) -> Result[i64, OverflowError[i64]]:
    n = len(v)
    mut t = 0
    for i in range(1, n):
        t += v[i]
    Ok(t)

def unchanged(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut n = len(xs)
    mut t = 0
    for i in range(n):
        t += xs[i]
    Ok(t)

def grown(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut n = len(xs)
    n += 1
    mut t = 0
    for i in range(n):
        t += xs[i]
    Ok(t)

def other(xs: &list[i64], ys: &list[i64]) -> Result[i64, OverflowError[i64]]:
    n = len(ys)
    mut t = 0
    for i in range(n):
        t += xs[i]
    Ok(t)

def owned(xs: list[i64]) -> Result[i64, OverflowError[i64]]:
    n = len(xs)
    mut t = 0
    for i in range(n):
        t += xs[i]
    Ok(t)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    xs = [1, 2, 3]?
    short = [1, 2]?
    print(saved(&xs)?, saved_view(&xs[0:3])?, unchanged(&xs)?, other(&xs, &short)?, owned(copy(xs)?)?)?
    print(grown(&xs)?)?
    Ok(())
"#;

#[test]
fn saved_lengths_of_borrowed_lists_bound_their_counters() {
    // A borrowed list's length cannot change, so a stop saved from it is the
    // hoisted length (issue #728). Owned lists keep the check until the
    // frontend proves nothing resizes them between the read and the loop.
    let output = support::run(SAVED_LENGTHS);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "6 5 6 3 6\n");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("index 3, length 3"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (name, checked) in [
        ("saved", false),
        ("saved_view", false),
        ("unchanged", false),
        ("grown", true),
        ("other", true),
        ("owned", true),
    ] {
        let clif = dump("clif-opt", &format!("__plenty_fn_{name}"), SAVED_LENGTHS);
        assert_eq!(
            clif.contains("plenty_trap_list_index"),
            checked,
            "{name}\n{clif}"
        );
    }
}

/// Range loops whose stop has no visible relation to the lengths they index
/// run an unchecked copy when one test before the loop shows every index is
/// in bounds, and an ordinary checked copy otherwise (issue #728).
const VERSIONED: &str = r#"
def copy_rows(n: i64, dst: &mut list[i64], src: &list[i64]) -> ():
    for i in range(n):
        dst[i] = src[i]

def shift(r: i64, xs: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    for i in range(r):
        xs[i] = xs[i + 1]
    Ok(())

def view_sum(n: i64, v: &[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(n):
        t += v[i]
    Ok(t)

def until_negative(n: i64, xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut t = 0
    for i in range(n):
        if xs[i] < 0:
            break
        t += xs[i]
    Ok(t)

def first_negative(n: i64, xs: &list[i64]) -> i64:
    for i in range(1, n):
        if xs[i] < 0:
            return i
    -1

def owned_copy(n: i64) -> Result[list[i64], AllocError]:
    mut a = list[i64].with_capacity(n)?
    mut b = list[i64].with_capacity(n)?
    for i in range(n):
        a.append(i)?
        b.append(0)?
    for i in range(n):
        b[i] = a[i]
    Ok(b)

def double(x: i64) -> Result[i64, OverflowError[i64]]:
    Ok(x * 2)

def called(n: i64, xs: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    for i in range(n):
        xs[i] = double(xs[i])?
    Ok(())

def own_length(xs: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    for i in range(len(xs)):
        xs[i] = xs[i] + 1
    Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut dst = [0, 0, 0]?
    src = [1, 2, 3]?
    copy_rows(3, &mut dst, &src)
    mut xs = [1, 2, 3, 4]?
    shift(3, &mut xs)?
    print(dst, xs, view_sum(3, &src[0:3])?, view_sum(2, &src[1:3])?)?
    signs = [1, 2, -1]?
    print(until_negative(10, &signs)?, first_negative(10, &signs), first_negative(2, &signs))?
    big = [9223372036854775807, 1]?
    match view_sum(5, &big[0:2]):
        case Ok(t):
            print(t)?
        case Err(_):
            print("overflow")?
    print(owned_copy(4)?)?
    called(3, &mut dst)?
    own_length(&mut dst)?
    print(dst)?
    Ok(())
"#;

#[test]
fn versioned_range_loops_keep_their_results() {
    // `until_negative`, `first_negative` and the overflowing `view_sum` leave
    // the loop before their stop passes the length: the checked copy runs
    // them without trapping.
    support::assert_stdout(
        VERSIONED,
        "[1, 2, 3] [2, 3, 4, 4] 6 5\n3 2 -1\noverflow\n[0, 1, 2, 3]\n[3, 5, 7]\n",
    );
}

#[rstest]
#[case("copy_rows(4, &mut dst, &src)", "index 3, length 3")]
#[case("shift(3, &mut dst)?", "index 3, length 3")]
#[case("print(view_sum(3, &src[1:3])?)?", "index 2, length 2")]
#[case("print(until_negative(4, &src)?)?", "index 3, length 3")]
#[case("print(first_negative(5, &src))?", "index 3, length 3")]
fn versioned_range_loops_trap_past_the_length(#[case] call: &str, #[case] message: &str) {
    let source = VERSIONED.replace(
        "    copy_rows(3, &mut dst, &src)",
        &format!("    copy_rows(3, &mut dst, &src)\n    {call}"),
    );
    let output = support::run(&source);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{call}\n{stderr}");
    assert!(
        stderr.contains(&format!("index out of bounds: {message}")),
        "{call}\n{stderr}"
    );
    assert!(output.stdout.is_empty(), "{call}");
}

/// The bounds-check traps inside each loop of `clif`, in layout order: a
/// loop runs from its header to the block that jumps back to it.
fn traps_per_loop(clif: &str) -> Vec<usize> {
    let lines: Vec<&str> = clif.lines().collect();
    let traps: Vec<String> = lines
        .iter()
        .filter(|line| line.ends_with("; plenty_trap_list_index"))
        .filter_map(|line| Some(line.trim().split(' ').next()?.to_string()))
        .collect();
    let mut loops = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let Some(name) = line.split(['(', ':']).next() else {
            continue;
        };
        if !name.starts_with("block") || line.starts_with(' ') {
            continue;
        }
        let back = lines[at + 1..].iter().position(|line| {
            let line = line.trim();
            line == format!("jump {name}") || line.starts_with(&format!("jump {name}("))
        });
        if let Some(back) = back {
            let count = lines[at..=at + 1 + back]
                .iter()
                .filter(|line| traps.iter().any(|t| line.contains(&format!("call {t}("))))
                .count();
            loops.push(count);
        }
    }
    loops
}

#[test]
fn versioned_range_loops_have_an_unchecked_copy() {
    for (name, loops) in [
        ("copy_rows", &[0, 2][..]),
        ("shift", &[0, 2]),
        ("view_sum", &[0, 1]),
        ("until_negative", &[0, 2]),
        ("first_negative", &[0, 1]),
        // The second loop; the first appends.
        ("owned_copy", &[0, 0, 2]),
        ("called", &[2]),
        ("own_length", &[0]),
    ] {
        let clif = dump("clif-opt", &format!("__plenty_fn_{name}"), VERSIONED);
        assert_eq!(traps_per_loop(&clif), loops, "{name}\n{clif}");
    }
}
