mod support;
use support::assert_stdout as expect;

#[test]
fn ranges_cross_calls_and_standard_sum_returns_by_value() {
    expect(
        r#"
import std.math as math

def make(n: i64) -> Result[range[i64], OverflowError[i64]]:
    Ok(range(n, n + 4))
def forward(r: range[i64]) -> range[i64]:
    r
def maybe(n: i64) -> Option[range[i64]]:
    if n < 0:
        return Nothing
    # Only maybe(3) reaches here; retain an Option of the inline range.
    Some(make(n).unwrap())
def checked(n: i64) -> Result[range[i64], range[i64]]:
    if n < 0:
        # Fixed 10 + 4 fits; keep range itself as the error payload.
        return Err(make(10).unwrap())
    # The sole nonnegative call is propagate(7): 7 + 4 fits.
    Ok(make(n).unwrap())
def propagate(n: i64) -> Result[range[i64], range[i64]]:
    value = checked(n)?
    Ok(forward(value))
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut original = make(2)?
    saved = original
    original = make(20)?
    print(list(saved)?)?
    print(list(forward(original))?)?
    print(maybe(3))?
    print(maybe(-1))?
    print(str.repr(propagate(-1))?)?
    print(str.repr(propagate(7))?)?
    print(math.sum_iter(&saved, 0)?)?
    Ok(())
"#,
        "[2, 3, 4, 5]\n[20, 21, 22, 23]\nOption[range].Some(range(3, 7, 1))\nOption[range].Nothing\nResult[range, range].Err(range(10, 14, 1))\nResult[range, range].Ok(range(7, 11, 1))\n14\n",
    );
}

#[test]
fn ranges_survive_collection_reallocation_removal_and_copy() {
    expect(
        r#"
def build() -> Result[list[range[i64]], AllocError | OverflowError[i64]]:
    mut values: list[range[i64]] = []?
    for n in range(20):
        values.append(range(n, n + 2))?
    Ok(values)
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut values = build()?
    first = values[0]
    values.reverse()
    removed = values.pop(0).unwrap()
    values.reserve(100)?
    print(first)?
    print(removed)?
    print(values[0])?
    dup = copy(values)?
    values.clear()
    print(dup[-1])?
    mut mapping = {"a": Some(range(2)), "b": Option[range[i64]].Nothing}?
    mapping.insert("b", Some(range(5)))?
    mapping.insert("c", Some(range(8)))?
    snapshot = mapping.values_snapshot()?
    removed_value = mapping.pop("b").unwrap()
    mapping.clear()
    print(snapshot)?
    print(removed_value)?
    Ok(())
"#,
        "range(0, 2, 1)\nrange(19, 21, 1)\nrange(18, 20, 1)\nrange(0, 2, 1)\n[Option[range].Some(range(0, 2, 1)), Option[range].Some(range(0, 5, 1)), Option[range].Some(range(0, 8, 1))]\nOption[range].Some(range(0, 5, 1))\n",
    );
}

#[test]
fn range_fields_references_and_generator_captures_keep_their_storage() {
    expect(
        r#"
class Crate:
    before: i64
    values: range[i64]
    optional: Option[range[i64]]
    after: i64
def replace(target: &mut range[i64]) -> ():
    *target = range(6, 9)
def replace_optional(target: &mut Option[range[i64]]) -> ():
    *target = Some(range(10, 12))
def produce(r: range[i64]) -> Generator[range[i64]]:
    saved = r
    yield saved
    yield range(7, 9)
def make_generator() -> Generator[range[i64]]:
    produce(range(3))
mut b = Crate(11, range(2), Nothing, 22)
replace(&mut b.values)
replace_optional(&mut b.optional)
print(b.before).unwrap()
print(b.values).unwrap()
print(b.optional).unwrap()
print(b.after).unwrap()
mut g = make_generator()
first = next(g).unwrap()
second = next(g).unwrap()
drop(g)
print(first).unwrap()
print(second).unwrap()
"#,
        "11\nrange(6, 9, 1)\nOption[range].Some(range(10, 12, 1))\n22\nrange(0, 3, 1)\nrange(7, 9, 1)\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn range_creation_iteration_and_returns_need_no_heap_allocation() {
    expect(
        r#"
def make() -> Result[range[u64], AllocError]:
    Ok(range[u64](18446744073709551612, 18446744073709551615))
def main() -> Result[(), IoError | AllocError | OverflowError[u64]]:
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    values = make()?
    mut total = 0u64
    for value in values:
        total = total + (value - 18446744073709551612u64)
    for value in values:
        total = total + (value - 18446744073709551612u64)
    size = len(values)
    last = values[-1]
    contained = 18446744073709551614u64 in values
    same = values == range[u64](18446744073709551612, 18446744073709551615)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(total)?
    print(size)?
    print(last)?
    print(contained)?
    print(same)?
    Ok(())
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n6\n3\n18446744073709551614\nTrue\nTrue\n",
    );
}

/// Compile first so a frontend rejection can never satisfy a native trap test.
fn native_range_output(source: &str, release: bool) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ranges.plenty");
    let executable = dir.path().join("ranges");
    std::fs::write(&path, source).unwrap();
    let mut command = std::process::Command::new(support::plenty_bin());
    command.env("PLENTY_CACHE", "off");
    if release {
        command.arg("--release");
    }
    let compiled = command
        .arg("--compile")
        .arg(&path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "release={release}: {}\n{source}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    std::process::Command::new(executable).output().unwrap()
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn oversized_ranges_trap_before_iteration_or_storage(#[case] release: bool) {
    for arguments in ["i64.MIN, i64.MAX", "0u64, u64.MAX"] {
        for body in [
            format!("    values = range({arguments})\n    print(len(values))?\n"),
            // Breaking immediately must not bypass validation of the full count.
            format!("    for value in range({arguments}):\n        print(value)?\n        break\n"),
        ] {
            let source = format!("def main() -> Result[(), IoError]:\n{body}    Ok(())\n");
            let output = native_range_output(&source, release);
            assert_eq!(output.status.code(), Some(1), "{source}");
            assert!(output.stdout.is_empty(), "{source}");
            let column = if body.contains("values =") { 14 } else { 18 };
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("error: ranges.plenty:2:{column}: range length exceeds i64\n"),
                "release={release}\n{source}"
            );
        }
    }
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn minimum_steps_and_maximum_lengths_keep_native_range_semantics(#[case] release: bool) {
    let output = native_range_output(
        r#"def make(start: i64, stop: i64, step: NonZero[i64]) -> range[i64]:
    range(start, stop, step)
def produce(values: range[i64]) -> Generator[i64]:
    for value in values:
        yield value
def main() -> Result[(), AllocError | IoError]:
    # Construction validates a representable maximum count without allocating it.
    print(len(make(0, i64.MAX, 1)))?
    for value in range(0, i64.MAX):
        print(value)?
        break
    values = make(i64.MAX, i64.MIN, -9223372036854775808)
    print(list(values)?)?
    mut fast: list[i64] = []?
    for value in range(i64.MAX, i64.MIN, -9223372036854775808):
        fast.append(value)?
    print(fast)?
    print(list(make(10, -10, -9223372036854775808))?)?
    print(list(make(i64.MIN, i64.MAX, -9223372036854775808))?)?
    print(list(range[u64](u64.MAX, 0, -9223372036854775808))?)?
    mut g = produce(values)
    print(next(g), next(g), next(g), next(g))?
    mut early = produce(values)
    print(next(early))?
    drop(early)
    print(values[-1])?
    Ok(())
"#,
        release,
    );
    assert!(
        output.status.success(),
        "release={release}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "9223372036854775807\n0\n[9223372036854775807, -1]\n[9223372036854775807, -1]\n[10]\n[]\n[18446744073709551615, 9223372036854775807]\nOption[i64].Some(9223372036854775807) Option[i64].Some(-1) Option[i64].Nothing Option[i64].Nothing\nOption[i64].Some(9223372036854775807)\n-1\n"
    );
}
