//! Reverse traversal and boolean reductions share the ordinary ownership rules.
mod support;
use rstest::rstest;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim_end(), "{source}");
}

#[test]
fn reverse_lists_text_and_ranges_in_loops_and_comprehensions() {
    native(
        r#"
print([x for x in reversed([1, 2, 3].unwrap())].unwrap()).unwrap()
print([x for x in reversed(range(1, 8, 2))].unwrap()).unwrap()
print([x for x in reversed(range(8u8, 1u8, -3))].unwrap()).unwrap()
for x in reversed("aé中🙂"):
    print(x).unwrap()
for x in reversed(""):
    print("unreachable").unwrap()
print([x for x in reversed(range(0))].unwrap()).unwrap()
print([x for x in reversed(list[i64]().unwrap())].unwrap()).unwrap()
print([x for x in reversed(range(i64.MAX - 1, i64.MAX))].unwrap()).unwrap()
print([x for x in reversed(range(0u64, u64.MAX, i64.MAX))].unwrap()).unwrap()
"#,
        "[3, 2, 1]\n[7, 5, 3, 1]\n[2, 5, 8]\n🙂\n中\né\na\n[]\n[]\n[9223372036854775806]\n[18446744073709551614, 9223372036854775807, 0]",
    );
}

#[test]
fn reverse_borrows_preserve_elements_and_allow_exclusive_writes() {
    native(
        r#"
mut values = [1, 2, 3].unwrap()
for x in reversed(&mut values):
    *x += 10
for i, x in enumerate(reversed(&values)):
    print((i, *x)).unwrap()
print(values).unwrap()
text = "é🙂"
for x in reversed(&text):
    print(*x).unwrap()
print(text).unwrap()
for x in reversed(&range(3)):
    print(*x).unwrap()
"#,
        "(0, 13)\n(1, 12)\n(2, 11)\n[11, 12, 13]\n🙂\né\né🙂\n2\n1\n0",
    );
}

#[test]
fn reverse_adapters_keep_continue_break_and_unpacked_items_aligned() {
    native(
        r#"
for i, x in enumerate(reversed(range(5))):
    if x == 3:
        continue
    print((i, x)).unwrap()
    if x == 1:
        break
for a, b in zip(reversed(range(3)), reversed("xyz")):
    print((a, b)).unwrap()
for a, b in reversed([(1, 2), (3, 4)].unwrap()):
    print(a + b).unwrap()
print([x for x in reversed(range(6)) if x % 2 == 0].unwrap()).unwrap()
"#,
        "(0, 4)\n(2, 2)\n(3, 1)\n(2, \"z\")\n(1, \"y\")\n(0, \"x\")\n7\n3\n[4, 2, 0]",
    );
}

#[test]
fn owned_reverse_items_and_unvisited_items_drop_once() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
for item in reversed([Guard("first"), Guard("second"), Guard("last")].unwrap()):
    print(item.label).unwrap()
    break
print("done").unwrap()
"#,
        "last\nlast\nfirst\nsecond\ndone",
    );
}

#[test]
fn borrowed_temporaries_live_through_iteration_and_drop_at_scope_exit() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
for x in &[Guard("forward")].unwrap():
    print(x.label).unwrap()
for x in reversed(&[Guard("reverse")].unwrap()):
    print(x.label).unwrap()
    break
for x in &range(2):
    print(*x).unwrap()
print(any(&[False, True].unwrap())).unwrap()
print(all(reversed(&[True, False].unwrap()))).unwrap()
print("after").unwrap()
"#,
        "forward\nforward\nreverse\nreverse\n0\n1\nTrue\nFalse\nafter",
    );
}

#[test]
fn reverse_cleanup_runs_on_return_and_propagation() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def leave() -> Result[i64, str]:
    for item in reversed([Guard("a"), Guard("b")].unwrap()):
        return Err("leave")
    Ok(0)
def fail() -> Result[i64, str]:
    Err("propagate")
def propagate() -> Result[i64, str]:
    for item in reversed([Guard("c"), Guard("d")].unwrap()):
        fail()?
    Ok(0)
print(str.repr(leave()).unwrap()).unwrap()
print(str.repr(propagate()).unwrap()).unwrap()
"#,
        "b\na\nResult[i64, str].Err(\"leave\")\nd\nc\nResult[i64, str].Err(\"propagate\")",
    );
}

#[test]
fn intrinsics_resolve_in_imported_generics_and_generator_bodies() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("helpers.plenty"),
        r#"
pub def backwards[T](values: &list[T]) -> i64:
    mut count = 0
    for x in reversed(values):
        count += 1
    count
pub def reduce[T](unused: T, values: &list[bool]) -> bool:
    any(values) and not all(reversed(values))
pub def flags(values: list[bool]) -> Generator[bool]:
    for x in reversed(values):
        yield x
pub def summarize(values: list[bool]) -> Generator[bool]:
    yield any(&values)
    yield all(reversed(&values))
"#,
    )
    .unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(
        &source,
        r#"
import helpers
def main() -> ():
    values = [False, True].unwrap()
    print(helpers.backwards(&values)).unwrap()
    print(helpers.reduce(0, &values)).unwrap()
    print(any(helpers.flags([False, True].unwrap()))).unwrap()
    for x in helpers.summarize(values):
        print(x).unwrap()
"#,
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &executable, Some(dir.path())).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "2\nTrue\nTrue\nTrue\nFalse\n"
    );
}

#[test]
fn boolean_reductions_handle_empty_owned_borrowed_and_hash_sources() {
    native(
        r#"
print(any(list[bool]().unwrap())).unwrap()
print(all(list[bool]().unwrap())).unwrap()
print(any([False, True, False].unwrap())).unwrap()
print(all([True, False, True].unwrap())).unwrap()
print(all([True, True].unwrap())).unwrap()
mut flags = [False, True].unwrap()
print(any(&flags)).unwrap()
print(all(&mut flags)).unwrap()
flags.append(True).unwrap()
print(flags).unwrap()
print(any(reversed(&flags))).unwrap()
print(all(reversed([True, True].unwrap()))).unwrap()
print(any({False, True}.unwrap())).unwrap()
print(all({True: 1}.unwrap())).unwrap()
"#,
        "False\nTrue\nTrue\nFalse\nTrue\nTrue\nFalse\n[False, True, True]\nTrue\nTrue\nTrue\nTrue",
    );
}

#[test]
fn reductions_short_circuit_generators_and_release_their_frames() {
    native(
        r#"
class Guard:
    def __del__(self) -> ():
        print("drop frame").unwrap()
def flags(guard: Guard, value: bool) -> Generator[bool]:
    print("first").unwrap()
    yield value
    print("second").unwrap()
    yield not value
    print("third").unwrap()
print(any(flags(Guard(), True))).unwrap()
print(all(flags(Guard(), False))).unwrap()
print(any(flags(Guard(), False))).unwrap()
print(all(flags(Guard(), True))).unwrap()
def empty() -> Generator[bool]:
    if False:
        yield True
print(any(empty())).unwrap()
print(all(empty())).unwrap()
"#,
        "first\ndrop frame\nTrue\nfirst\ndrop frame\nFalse\nfirst\nsecond\ndrop frame\nTrue\nfirst\nsecond\ndrop frame\nFalse\nFalse\nTrue",
    );
}

#[test]
fn sources_evaluate_once_and_user_callables_keep_their_names() {
    native(
        r#"
def source() -> list[bool]:
    print("source").unwrap()
    [False, True].unwrap()
print(any(source())).unwrap()
print(all(source())).unwrap()
for x in reversed(source()):
    print(x).unwrap()
"#,
        "source\nTrue\nsource\nFalse\nsource\nTrue\nFalse",
    );
    native(
        r#"
def any(value: i64) -> i64:
    value + 1
def reversed(value: i64) -> range[i64]:
    range(value)
print(any(7)).unwrap()
all = any
print(all(9)).unwrap()
for x in reversed(2):
    print(x).unwrap()
"#,
        "8\n10\n0\n1",
    );
}

#[test]
fn intrinsic_names_do_not_hide_class_constructors_or_methods() {
    native(
        r#"
class any:
    value: i64
class all:
    value: i64
class reversed:
    value: i64
class Methods:
    def any(self) -> bool:
        True
    def all(self) -> bool:
        False
    def reversed(self) -> range[i64]:
        range(2)
print(any(1).value).unwrap()
print(all(2).value).unwrap()
print(reversed(3).value).unwrap()
methods = Methods()
print(methods.any()).unwrap()
print(methods.all()).unwrap()
for x in methods.reversed():
    print(x).unwrap()
"#,
        "1\n2\n3\nTrue\nFalse\n0\n1",
    );
}

#[rstest]
#[case("any()", "requires one boolean iterable")]
#[case(
    "all([True].unwrap(), [False].unwrap())",
    "requires one boolean iterable"
)]
#[case("any([1].unwrap())", "requires bool elements")]
#[case("all(\"abc\")", "requires bool elements")]
#[case("any(enumerate([True].unwrap()))", "requires bool elements")]
#[case("for x in reversed():\n    pass", "reversed requires one iterable")]
#[case(
    "for x in reversed({1, 2}.unwrap()):\n    pass",
    "requires a list, str, bytes, range, or Bytes"
)]
#[case(
    "for x in reversed({1: 2}.unwrap()):\n    pass",
    "requires a list, str, bytes, range, or Bytes"
)]
#[case(
    "def flags() -> Generator[bool]:\n    yield True\nfor x in reversed(flags()):\n    pass",
    "requires a list, str, bytes, range, or Bytes"
)]
#[case("saved = reversed(range(3))", "supported directly in for loops")]
#[case(
    "mut values = [1].unwrap()\nfor x in reversed(&values):\n    values.append(2).unwrap()\n    print(*x).unwrap()",
    "borrow"
)]
#[case(
    "mut values = [1].unwrap()\nfor x in reversed(&mut values):\n    print(values).unwrap()\n    *x = 2",
    "borrow"
)]
fn rejects_unsupported_sources_and_invalid_borrows(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reverse_and_reductions_allocate_nothing_after_inputs_exist() {
    native(
        r#"
values = [1, 2, 3].unwrap()
flags = [False, True].unwrap()
text = "long é🙂 text"
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
mut total = 0
for x in reversed(&values):
    total += *x
mut count = 0
for x in reversed(text):
    count += 1
yes = any(&flags)
no = all(reversed(&flags))
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(total).unwrap()
print(count).unwrap()
print(yes).unwrap()
print(no).unwrap()
"#,
        "6\n12\nTrue\nFalse",
    );
}
