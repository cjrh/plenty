//! Named binding preserves expression order; omitted defaults belong to the callee.
mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}

fn reject(source: &str, expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "expected {expected:?}: {source}\n{error}"
    );
}

#[test]
fn reference_alias_defaults_and_generators_preserve_types() {
    native(
        r#"
type Scratch = &mut list[i64]
def count[T](tag: T, scratch: Scratch = []?) -> Result[i64, AllocError]:
    scratch.append(1)?
    Ok(len(scratch))
def numbers(start: i64 = 4, end: i64 = 6) -> Generator[i64]:
    for n in range(start, end):
        yield n
def main() -> ():
    print(count(tag=True).unwrap()).unwrap()
    for n in numbers(end=7, start=5):
        print(n).unwrap()
    for n in numbers():
        print(n).unwrap()
"#,
        "1\n5\n6\n4\n5\n",
    );
}

#[test]
fn defaults_do_not_double_the_parameter_slot_budget() {
    let parameters = (0..130)
        .map(|i| format!("a{i}: i64 = {i}"))
        .collect::<Vec<_>>()
        .join(", ");
    native(
        &format!(
            "def f({parameters}) -> i64:\n    a0 + a129\ndef main() -> ():\n    print(f()).unwrap()\n"
        ),
        "129\n",
    );
    let arguments = (0..130)
        .rev()
        .map(|i| format!("a{i}={i}"))
        .collect::<Vec<_>>()
        .join(", ");
    native(
        &format!(
            "def f({parameters}) -> i64:\n    a0 + a129\ndef main() -> ():\n    print(f({arguments})).unwrap()\n"
        ),
        "129\n",
    );
}

#[test]
fn generic_constructors_and_receiver_reservations_keep_their_contracts() {
    native(
        r#"
class Cell[T]:
    value: T
    extra: i64
    def __init__(self, value: T, extra: i64 = 3) -> ():
        self.value = value
        self.extra = extra
    def add(self: &mut Cell[T], first: i64, second: i64 = 4) -> ():
        self.extra = self.extra + first + second
    def get(self) -> i64:
        self.extra
def main() -> ():
    mut cell = Cell(value=True)
    cell.add(second=cell.get(), first=cell.get())
    print(cell.extra).unwrap()
    other = Cell[u8](extra=6, value=2)
    print(other.extra).unwrap()
"#,
        "9\n6\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn wrappers_allocate_nothing_and_default_allocation_failure_is_returned() {
    let output = support::run(
        r#"
def combine(first: i64, second: i64 = 2) -> i64:
    first + second
def work(values: list[i64] = [1, 2, 3]?) -> Result[i64, AllocError]:
    Ok(len(values))
def main() -> ():
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    value = combine(second=4, first=3)
    fallback = combine(first=5)
    print("__test_end_no_allocations__").unwrap()
    result = work()
    print("__test_restore_allocations__").unwrap()
    print(value + fallback).unwrap()
    match result:
        case Ok(_):
            print("unexpected success").unwrap()
        case Err(_):
            print("allocation failure returned").unwrap()
    print(work().unwrap()).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let actual = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual, "14\nallocation failure returned\n3");
}

#[test]
fn direct_calls_bind_names_and_contextual_types() {
    native(
        r#"
def combine(left: u8, right: u8, offset: u8 = 1) -> u8:
    left * 10u8 + right + offset
def main() -> ():
    print(combine(right=2, left=3)).unwrap()
    print(combine(3, offset=4, right=2)).unwrap()
    print(combine(3, 2, 0)).unwrap()
"#,
        "33\n36\n32\n",
    );
}

#[test]
fn named_supplied_arguments_run_before_defaults_in_written_order() {
    native(
        r#"
def mark(value: i64) -> i64:
    print(value).unwrap()
    value
def combine(a: i64, b: i64 = mark(2), c: i64 = mark(3), d: i64 = mark(4)) -> i64:
    a * 1000 + b * 100 + c * 10 + d
def main() -> ():
    print(combine(d=mark(9), a=mark(1))).unwrap()
"#,
        "9\n1\n2\n3\n1239\n",
    );
}

#[test]
fn generated_and_explicit_constructors_and_methods_use_names() {
    native(
        r#"
class Point:
    x: i64
    y: i64
    def shifted(self, dx: i64 = 1, dy: i64 = 2) -> i64:
        self.x + dx + self.y + dy
class Pair:
    x: i64
    y: i64
    def __init__(self, first: i64, second: i64 = 8) -> ():
        self.x = first
        self.y = second
def make() -> Point:
    print("receiver").unwrap()
    Point(y=4, x=3)
def mark(value: i64) -> i64:
    print(value).unwrap()
    value
def main() -> ():
    print(make().shifted(dy=mark(9), dx=mark(5))).unwrap()
    pair = Pair(first=6)
    print(pair.x + pair.y).unwrap()
"#,
        "receiver\n9\n5\n21\n14\n",
    );
}

#[test]
fn generic_names_work_with_inference_and_explicit_types() {
    native(
        r#"
def select[T](first: T, second: T, choose_first: bool = True) -> T:
    if choose_first:
        return first
    second
def main() -> ():
    print(select(second=2, first=1)).unwrap()
    print(select[u8](choose_first=False, second=9, first=3)).unwrap()
"#,
        "1\n9\n",
    );
}

#[test]
fn imported_defaults_resolve_in_the_defining_module() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("settings.plenty"),
        r#"
def seed() -> i64:
    7
pub def value(base: i64 = seed(), scale: i64 = 2) -> i64:
    base * scale
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
import settings
from settings import value as imported
def seed() -> i64:
    99
def main() -> ():
    print(settings.value(scale=3)).unwrap()
    print(imported(base=4)).unwrap()
"#,
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "21\n8\n");
}

#[test]
fn defaults_are_skipped_for_explicit_values_including_nothing() {
    native(
        r#"
def fallback() -> Option[i64]:
    print("default").unwrap()
    Some(4)
def work(value: Option[i64] = fallback()) -> i64:
    match value:
        case Some(n):
            n
        case Nothing:
            0
def main() -> ():
    print(work(value=Nothing)).unwrap()
    print(work()).unwrap()
    print(work(Some(8))).unwrap()
"#,
        "0\ndefault\n4\n8\n",
    );
}

#[test]
fn owned_mutable_defaults_are_fresh_on_every_call() {
    native(
        r#"
def work(initial: list[i64] = []?) -> Result[i64, AllocError]:
    mut values = initial
    values.append(9)?
    Ok(len(values))
def main() -> ():
    print(work().unwrap()).unwrap()
    print(work().unwrap()).unwrap()
    print(work(initial=[1, 2].unwrap()).unwrap()).unwrap()
"#,
        "1\n1\n3\n",
    );
}

#[test]
fn borrowed_mutable_defaults_lend_fresh_temporary_owners() {
    native(
        r#"
def work(values: &mut list[i64] = []?) -> Result[i64, AllocError]:
    values.append(9)?
    Ok(len(values))
def main() -> ():
    print(work().unwrap()).unwrap()
    print(work().unwrap()).unwrap()
    mut values = [1, 2].unwrap()
    print(work(values=&mut values).unwrap()).unwrap()
    print(len(values)).unwrap()
"#,
        "1\n1\n3\n3\n",
    );
}

#[test]
fn default_propagation_returns_from_the_call_not_the_caller() {
    native(
        r#"
enum Problem:
    Failed
def fail() -> Result[i64, Problem]:
    Err(Problem.Failed)
def work(value: i64 = fail()?) -> Result[i64, Problem]:
    print("body").unwrap()
    Ok(value)
def main() -> ():
    match work():
        case Ok(value):
            print(value).unwrap()
        case Err(_):
            print("failed").unwrap()
    print(work(value=3).unwrap()).unwrap()
    print("continued").unwrap()
"#,
        "failed\nbody\n3\ncontinued\n",
    );
}

#[test]
fn default_failure_drops_supplied_and_preceding_default_owners() {
    native(
        r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
enum Problem:
    Failed
def fail() -> Result[i64, Problem]:
    Err(Problem.Failed)
def work(given: Guard, first: Guard = Guard("default"), last: i64 = fail()?) -> Result[(), Problem]:
    print("unreachable").unwrap()
    Ok(())
def main() -> ():
    match work(given=Guard("supplied")):
        case Ok(_):
            pass
        case Err(_):
            print("failed").unwrap()
"#,
        "default\nsupplied\nfailed\n",
    );
}

#[test]
fn invalid_argument_bindings_have_specific_diagnostics() {
    for (call, diagnostic) in [
        ("f(a=1, 2)", "positional"),
        ("f(a=1, a=2)", "duplicate"),
        ("f(1, a=2, b=3)", "supplied"),
        ("f(a=1, unknown=2)", "unknown"),
        ("f(a=1)", "missing"),
    ] {
        reject(
            &format!("def f(a: i64, b: i64) -> ():\n    pass\ndef main() -> ():\n    {call}\n"),
            diagnostic,
        );
    }
}

#[test]
fn required_parameters_cannot_follow_defaults_or_be_default_dependencies() {
    reject(
        "def f(a: i64 = 1, b: i64) -> ():\n    pass\ndef main() -> ():\n    pass\n",
        "default",
    );
    reject(
        "def f(a: i64, b: i64 = a) -> ():\n    pass\ndef main() -> ():\n    pass\n",
        "parameter",
    );
}

#[test]
fn callable_values_do_not_acquire_names_or_optional_arity() {
    let prefix = "def f(value: i64 = 3) -> i64:\n    value\n";
    reject(
        &format!("{prefix}def main() -> ():\n    g = f\n    g(value=1)\n"),
        "named",
    );
    reject(
        &format!("{prefix}def main() -> ():\n    g = f\n    g()\n"),
        "argument",
    );
    native(
        &format!("{prefix}def main() -> ():\n    g = f\n    print(g(7)).unwrap()\n"),
        "7\n",
    );
}

#[test]
fn protocol_calls_cannot_use_implementation_parameter_names() {
    reject(
        r#"
protocol Add:
    def add(self, amount: i64) -> i64:
        pass
class Counter:
    value: i64
    def add(self, delta: i64) -> i64:
        self.value + delta
def work[T: Add](value: &T) -> i64:
    value.add(amount=1)
def main() -> ():
    counter = Counter(4)
    print(work(&counter)).unwrap()
"#,
        "named",
    );
}

#[test]
fn named_binding_preserves_moves_and_exclusive_borrows() {
    reject(
        r#"
class Owner:
    value: i64
def consume(first: Owner, second: Owner) -> ():
    pass
def main() -> ():
    value = Owner(1)
    consume(second=value, first=value)
"#,
        "moved",
    );
    reject(
        r#"
def mutate(first: &mut i64, second: &mut i64) -> ():
    pass
def main() -> ():
    mut value = 1
    mutate(second=&mut value, first=&mut value)
"#,
        "borrow",
    );
}

#[test]
fn references_to_default_temporary_owners_cannot_escape() {
    reject(
        r#"
def view(value: &i64 = 1) -> &i64:
    value
def main() -> ():
    borrowed = view()
    print(*borrowed).unwrap()
"#,
        "borrow",
    );
}

#[test]
fn defaults_cannot_hide_allocation_failure_or_use_a_different_error_type() {
    reject(
        "def work(values: list[i64] = []) -> Option[bool]:\n    Some(True)\ndef main() -> ():\n    pass\n",
        "allocation failure",
    );
    reject(
        "def work(values: list[i64] = []?) -> Option[bool]:\n    Some(True)\ndef main() -> ():\n    pass\n",
        "?",
    );
    reject(
        "def work(values: list[i64] = []?) -> Result[(), IoError]:\n    Ok(())\ndef main() -> ():\n    pass\n",
        "AllocError",
    );
}

#[test]
fn invalid_defaults_are_checked_even_when_every_call_supplies_the_argument() {
    reject(
        "def work(value: i64 = True) -> i64:\n    value\ndef main() -> ():\n    print(work(value=1)).unwrap()\n",
        "i64",
    );
}

#[test]
fn generic_owned_defaults_receive_the_specialized_parameter_type() {
    native(
        r#"
def count[T](values: list[T] = []?) -> Result[i64, AllocError]:
    Ok(len(values))
def main() -> ():
    print(count[u8]().unwrap()).unwrap()
    print(count[str]().unwrap()).unwrap()
    print(count[u8](values=[3u8, 4u8].unwrap()).unwrap()).unwrap()
"#,
        "0\n0\n2\n",
    );
}
