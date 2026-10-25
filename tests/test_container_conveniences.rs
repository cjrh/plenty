mod support;

fn native(source: &str, expected: &str) {
    let (imports, body): (Vec<_>, Vec<_>) = source
        .lines()
        .partition(|line| line.starts_with("from ") || line.starts_with("import "));
    let program = format!(
        "{}\n{}",
        imports.join("\n"),
        support::program(&body.join("\n"))
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, &program).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{program}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
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
fn print_multiple_values_and_empty_line() {
    native(
        r#"
print("hello", 42, True, [1, 2].unwrap(), (3, "four")).unwrap()
print().unwrap()
mut values = [5, 6].unwrap()
print(values, values).unwrap()
values.append(7).unwrap()
print(values).unwrap()
"#,
        "hello 42 True [1, 2] (3, \"four\")\n\n[5, 6] [5, 6]\n[5, 6, 7]",
    );
}

#[test]
fn print_arguments_evaluate_once_before_output() {
    native(
        r#"
def argument(n: i64) -> i64:
    print(n).unwrap()
    n
print(argument(1), argument(2), argument(3)).unwrap()
"#,
        "1\n2\n3\n1 2 3",
    );
}

#[test]
fn print_observes_owned_values_without_destroying_them() {
    native(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
a = Guard(1)
b = Guard(2)
print(a, b).unwrap()
print(a.n, b.n).unwrap()
drop(a)
drop(b)
"#,
        "Guard(n=1) Guard(n=2)\n1 2\ndrop 1\ndrop 2",
    );
}

#[test]
fn list_unpack_moves_values_and_drops_ignored_items() {
    native(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
a, b = "hello world".split(" ").unwrap()
print(a, b).unwrap()
left, _, right = [Guard(1), Guard(2), Guard(3)].unwrap()
print(left.n, right.n).unwrap()
drop(left)
drop(right)
mut x = 0
mut y = 0
x, y = [4, 5].unwrap()
print(x, y).unwrap()
"#,
        "hello world\ndrop 2\n1 3\ndrop 1\ndrop 3\n4 5",
    );
}

#[test]
fn list_unpack_rejects_wrong_runtime_length_before_writing() {
    for value in ["[1].unwrap()", "[1, 2, 3].unwrap()"] {
        let output = support::run(&format!("a, b = {value}\n"));
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("unpacking requires exactly 2 list elements"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn tuple_keys_compare_by_fields_across_storage_and_rehash() {
    native(
        r#"
mut seen = set[(i64, str)].new().unwrap()
for i in range(100):
    seen.add((i, "grid")).unwrap()
for i in range(100):
    seen.add((i, "grid".repeat(1).unwrap())).unwrap()
print(len(seen), (99, "grid") in seen, (100, "grid") in seen).unwrap()
print(seen.discard((99, "grid")), len(seen)).unwrap()
mut table = {((1u8, False), "key"): [7].unwrap()}.unwrap()
table.insert(((1u8, False), "key".repeat(1).unwrap()), [9].unwrap()).unwrap()
print(len(table), table.pop(((1u8, False), "key")).unwrap()).unwrap()
"#,
        "100 True False\nTrue 99\n1 [9]",
    );
}

#[test]
fn dictionary_into_values_consumes_affine_payloads() {
    native(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
data = {1: Guard(1), 2: Guard(2)}.unwrap()
for value in data.into_values().unwrap():
    print(value.n).unwrap()
"#,
        "1\ndrop 1\n2\ndrop 2",
    );
}

#[test]
fn deque_wraps_grows_and_preserves_both_ends() {
    native(
        r#"
from std.deque import Deque
mut queue = Deque[i64]().unwrap()
for n in range(8):
    queue.append(n).unwrap()
for n in range(5):
    print(queue.popleft().unwrap()).unwrap()
for n in range(8, 25):
    queue.append(n).unwrap()
queue.appendleft(-1).unwrap()
queue.appendleft(-2).unwrap()
print(queue.len(), queue.pop().unwrap(), queue.popleft().unwrap()).unwrap()
while not queue.is_empty():
    print(queue.popleft().unwrap()).unwrap()
print(queue.pop(), queue.popleft()).unwrap()
"#,
        "0\n1\n2\n3\n4\n22 24 -2\n-1\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20\n21\n22\n23\nOption[i64].Nothing Option[i64].Nothing",
    );
}

#[test]
fn deque_moves_owned_values_and_clears_without_shifting() {
    native(
        r#"
from std.deque import Deque
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
mut queue = Deque[Guard]().unwrap()
for n in range(20):
    queue.append(Guard(n)).unwrap()
drop(queue.popleft().unwrap())
drop(queue.pop().unwrap())
queue.clear()
queue.appendleft(Guard(99)).unwrap()
print(queue.len()).unwrap()
drop(queue)
"#,
        "drop 0\ndrop 19\ndrop 18\ndrop 17\ndrop 16\ndrop 15\ndrop 14\ndrop 13\ndrop 12\ndrop 11\ndrop 10\ndrop 9\ndrop 8\ndrop 7\ndrop 6\ndrop 5\ndrop 4\ndrop 3\ndrop 2\ndrop 1\n1\ndrop 99",
    );
}

#[test]
fn generator_expressions_stream_to_calls_and_short_circuit() {
    native(
        r#"
def mark(n: i64) -> bool:
    print("visit", n).unwrap()
    n == 2
print((mark(n) for n in range(10)).any()).unwrap()
def total(source: Generator[i64]) -> i64:
    mut result = 0
    for value in source:
        result += value
    result
print(total(x * y for x in range(1, 4) for y in range(1, 3) if x != 2)).unwrap()
mut values = (n * n for n in range(4))
print(next(values).unwrap(), next(values).unwrap()).unwrap()
print([n for n in values].unwrap()).unwrap()
"#,
        "visit 0\nvisit 1\nvisit 2\nTrue\n12\n0 1\n[4, 9]",
    );
}

#[test]
fn generator_expressions_evaluate_outer_source_at_creation_and_capture_values() {
    native(
        r#"
def source() -> list[i64]:
    print("source").unwrap()
    [1, 2, 3].unwrap()
def item(n: i64, offset: i64) -> i64:
    print("item", n).unwrap()
    n + offset
mut offset = 10
values = (item(n, offset) for n in source())
offset = 20
print("created", offset).unwrap()
for value in values:
    print(value).unwrap()
"#,
        "source\ncreated 20\nitem 1\n11\nitem 2\n12\nitem 3\n13",
    );
}

#[test]
fn generator_expressions_release_owned_captures_on_early_exit() {
    native(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
guards = [Guard(1), Guard(2), Guard(3)].unwrap()
print((value.n == 1 for value in guards).any()).unwrap()
guard = Guard(4)
values = (guard.n + n for n in range(3))
drop(values)
"#,
        "drop 1\ndrop 2\ndrop 3\nTrue\ndrop 4",
    );
}

#[test]
fn generator_expressions_compose_adapters_and_generic_consumers() {
    native(
        r#"
def collect[T](source: Generator[T]) -> list[T]:
    list[T].from(source).unwrap()
print(collect(x * x for x in range[u8](4))).unwrap()
"#,
        "[0, 1, 4, 9]",
    );
}

#[test]
fn generator_expression_parenthesized_arguments_and_adapters() {
    native(
        r#"
def total[T: IntType](source: Generator[T], zero: T) -> T:
    mut result = zero
    for value in source:
        result += value
    result
print(total((x * x for x in range[u8](4)), 0u8)).unwrap()
print([i + x + y for i, pair in enumerate(zip(range(3), range(5)), 10) for x, y in [pair].unwrap()].unwrap()).unwrap()
for value in (i + x for i, x in enumerate(reversed(range(3)), 10)):
    print(value).unwrap()
"#,
        "14\n[10, 13, 16]\n12\n12\n12",
    );
}

#[test]
fn invalid_conveniences_have_compile_time_diagnostics() {
    for (source, diagnostic) in [
        (
            "result: Result[i64, str] = Ok(2)\nprint(1, result).unwrap()",
            "print cannot print a Result directly",
        ),
        (
            "mut xs = [1].unwrap()\nprint(xs, xs.pop().unwrap()).unwrap()",
            "conflicting borrow",
        ),
        (
            "xs = [1].unwrap()\ng = (x for x in &xs)",
            "references cannot be suspended",
        ),
        (
            "def f(xs: Generator[i64], n: i64) -> ():\n    pass\nf(x for x in range(3), 1)",
            "expected `)`",
        ),
        (
            "data = {1: [2].unwrap()}.unwrap()\nvalues = data.into_values().unwrap()\nprint(data).unwrap()",
            "moved",
        ),
        (
            "xs = [1, 2].unwrap()\na, b = xs\nprint(xs).unwrap()",
            "moved",
        ),
        ("values = {(1, 2.0)}.unwrap()", "set elements must"),
        (
            "values = ((n for n in range(2)) for _ in range(2))",
            "generator expression elements cannot",
        ),
        (
            "values = (&n for n in range(2))",
            "generator expression elements cannot",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn removal_unpacking_and_generator_expressions_need_no_allocation() {
    native(
        r#"
from std.deque import Deque
mut queue = Deque[i64]().unwrap()
queue.append(1).unwrap()
queue.append(2).unwrap()
items = [3, 4].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
a, b = items
x = queue.popleft().unwrap()
queue.appendleft(5).unwrap()
y = queue.pop().unwrap()
z = (n == 2 for n in range(10)).any()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a, b, x, y, z).unwrap()
"#,
        "3 4 1 2 True",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn deque_growth_failure_preserves_contents() {
    native(
        r#"
from std.deque import Deque
mut queue = Deque[i64]().unwrap()
for n in range(8):
    queue.append(n).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = queue.append(9)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(queue.len(), queue.popleft().unwrap(), queue.pop().unwrap()).unwrap()
"#,
        "Result[(), AllocError].Err(AllocError.OutOfMemory)\n8 0 7",
    );
}

#[test]
fn deque_agrees_with_independent_model_through_mixed_operations() {
    let mut reference = std::collections::VecDeque::new();
    let mut state = 17u64;
    let mut actions = Vec::new();
    let mut expected = Vec::new();
    for value in 0..500 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let action = (state >> 32) % 4;
        actions.push(format!("({action}, {value})"));
        let removed = match action {
            0 => {
                reference.push_back(value);
                continue;
            }
            1 => {
                reference.push_front(value);
                continue;
            }
            2 => reference.pop_back(),
            _ => reference.pop_front(),
        };
        expected.push(removed.map_or_else(
            || "Option[i64].Nothing".into(),
            |n| format!("Option[i64].Some({n})"),
        ));
    }
    expected.push(reference.len().to_string());
    for value in reference {
        expected.push(value.to_string());
    }
    native(
        &format!(
            r#"
from std.deque import Deque
mut queue = Deque[i64]().unwrap()
for action, value in [{}].unwrap():
    match action:
        case 0:
            queue.append(value).unwrap()
        case 1:
            queue.appendleft(value).unwrap()
        case 2:
            print(queue.pop()).unwrap()
        case _:
            print(queue.popleft()).unwrap()
print(queue.len()).unwrap()
while not queue.is_empty():
    print(queue.popleft().unwrap()).unwrap()
"#,
            actions.join(", ")
        ),
        &expected.join("\n"),
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn print_failure_writes_no_prefix_and_keeps_observed_owners() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
owner = Guard("destroyed")
print("__test_fail_allocations_after_0__").unwrap()
result = print("no prefix", owner)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(owner.label).unwrap()
drop(owner)
"#,
        "Result[(), IoError].Err(IoError.Data(DataError.Allocation(AllocError.OutOfMemory)))\ndestroyed\ndestroyed",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_dictionary_values_transfer_drops_remaining_owners_once() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
data = {1: Guard("first"), 2: Guard("second")}.unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = data.into_values()
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(values):
        print("unexpected success").unwrap()
    case Err(error):
        print(error).unwrap()
"#,
        "first\nsecond\nAllocError.OutOfMemory",
    );
}

#[test]
fn propagation_in_later_print_argument_releases_earlier_observations() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def fail() -> Result[i64, str]:
    Err("failure")
def report() -> Result[(), str]:
    owner = Guard("drop owner")
    print(owner, fail()?).unwrap()
    Ok(())
print(str.repr(report()).unwrap()).unwrap()
"#,
        "drop owner\nResult[(), str].Err(\"failure\")",
    );
}

#[test]
fn tuple_keys_are_independent_copies_and_support_single_components() {
    native(
        r#"
mut point = (1, 2)
keys = {point, (3, 4)}.unwrap()
coordinate = &mut point[0]
*coordinate = 9
print((1, 2) in keys, point in keys).unwrap()
single = {(u64.MAX,), (0u64,)}.unwrap()
print((u64.MAX,) in single, (1u64,) in single).unwrap()
"#,
        "True False\nTrue False",
    );
}

#[test]
fn named_and_temporary_class_methods_keep_into_values_dispatch() {
    native(
        r#"
class Value:
    n: i64
    def into_values(self, extra: i64) -> i64:
        self.n + extra
value = Value(1)
print(value.into_values(2), Value(3).into_values(4)).unwrap()
print(Box(Value(5)).unwrap().into_values(6)).unwrap()
"#,
        "3 7\n11",
    );
}

#[test]
fn indexed_replace_evaluates_rhs_before_index_and_transfers_owners() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def replacement() -> Guard:
    print("value").unwrap()
    Guard("new")
def index() -> i64:
    print("index").unwrap()
    0
mut values = [Guard("old")].unwrap()
old = replace(values[index()], replacement())
print("replaced").unwrap()
drop(old)
drop(values)
mut dictionary = {1: [4].unwrap()}.unwrap()
print(replace(dictionary[1], [5].unwrap()), dictionary).unwrap()
"#,
        "value\nindex\nreplaced\nold\nnew\n[4] {1: [5]}",
    );
}

#[test]
fn imported_generic_factories_can_return_generator_expressions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("helper.plenty"),
        r#"
pub def squares[T: IntType](stop: T) -> Generator[T]:
    (n * n for n in range[T](stop))
"#,
    )
    .unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(
        &path,
        r#"
from helper import squares
def main() -> Result[(), AllocError | IoError]:
    print(list[u8].from(squares(4u8))?)?
    Ok(())
"#,
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&path, &executable, None).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "[0, 1, 4, 9]\n");
}
