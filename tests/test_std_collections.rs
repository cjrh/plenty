//! `std.collections` heaps and `std.iter` generator adapters.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

const HEADER: &str = "from std.collections import Heap, HeapBy, drain, drain_by\n\
from std.iter import map, filter, take, fold, try_fold\n";

const DECLARATIONS: &str = r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()

def guards(stop: i64) -> Generator[Guard]:
    for n in range(stop):
        print("make", n).unwrap()
        yield Guard(n)

def naturals() -> Generator[i64]:
    mut n = 0
    while True:
        yield n
        n += 1

def larger(a: &Guard, b: &Guard) -> bool:
    a.n > b.n

# Printing one short string allocates nothing, so this cleanup can run while
# allocation is failing.
class Quiet:
    n: i64
    def __del__(self) -> ():
        print("dropped").unwrap()

def quieter(a: &Quiet, b: &Quiet) -> bool:
    a.n > b.n

def number(guard: Guard) -> i64:
    guard.n

def odd(guard: &Guard) -> bool:
    guard.n % 2 == 1

def even(n: &i64) -> bool:
    *n % 2 == 0

def square(n: i64) -> i64:
    n * n

def add(total: i64, n: i64) -> i64:
    total + n

def counted(a: &i64, b: &i64) -> bool:
    print("compare").unwrap()
    *a < *b

def keep(items: list[i64], n: i64) -> Result[list[i64], AllocError]:
    mut out = items
    out.append(n)?
    Ok(out)

def below_three(total: i64, guard: Guard) -> Result[i64, str]:
    if guard.n >= 3:
        return Err("too large")
    Ok(total + guard.n)
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}{DECLARATIONS}\ndef main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

fn execute(body: &str) -> std::process::Output {
    let source = program(body);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, &source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    std::process::Command::new(executable).output().unwrap()
}

fn output(body: &str) -> String {
    let output = execute(body);
    assert!(
        output.status.success(),
        "{body}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn native(body: &str, expected: &str) {
    assert_eq!(output(body), expected.trim_matches('\n'), "{body}");
}

fn rejected(body: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, program(body)).unwrap();
    match plenty::check_file(&path, None) {
        Ok(()) => panic!("expected a rejection:\n{body}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn heap_removes_the_least_element_and_reports_empty() {
    native(
        r#"
mut heap = Heap[i64]()?
print(heap.len(), heap.is_empty(), heap.pop())?
for n in [5, 3, 8, 1, 9, 2, 3]?:
    heap.push(n)?
print(heap.len(), *heap.peek())?
while not heap.is_empty():
    print(heap.pop().unwrap())?
print(heap.pop())?
heap.push(4)?
heap.clear()
print(heap.len(), heap.pop())?
mut words = Heap[str]()?
words.push("pear")?
words.push("apple")?
words.push("fig")?
print(words.pop(), words.pop(), words.pop(), words.pop())?
mut reals = Heap[f64]()?
reals.push(2.5)?
reals.push(-1.0)?
print(reals.pop(), reals.pop())?
"#,
        r#"
0 True Option[i64].Nothing
7 1
1
2
3
3
5
8
9
Option[i64].Nothing
0 Option[i64].Nothing
Option[str].Some("apple") Option[str].Some("fig") Option[str].Some("pear") Option[str].Nothing
Option[f64].Some(-1.0) Option[f64].Some(2.5)
"#,
    );
}

#[test]
fn heap_agrees_with_an_independent_model_through_mixed_operations() {
    let mut reference = BinaryHeap::new();
    let mut state = 23u64;
    let mut actions = Vec::new();
    let mut expected = Vec::new();
    for _ in 0..600 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let value = ((state >> 40) % 50) as i64 - 25;
        if (state >> 32).is_multiple_of(3) {
            actions.push("(1, 0)".to_owned());
            expected.push(reference.pop().map_or_else(
                || "Option[i64].Nothing".to_owned(),
                |Reverse(n)| format!("Option[i64].Some({n})"),
            ));
        } else {
            actions.push(format!("(0, {value})"));
            reference.push(Reverse(value));
        }
    }
    expected.push(reference.len().to_string());
    while let Some(Reverse(value)) = reference.pop() {
        expected.push(value.to_string());
    }
    native(
        &format!(
            r#"
mut heap = Heap[i64]()?
for action, value in [{}]?:
    if action == 0:
        heap.push(value)?
    else:
        print(heap.pop())?
print(heap.len())?
for value in drain(heap):
    print(value)?
"#,
            actions.join(", ")
        ),
        &expected.join("\n"),
    );
}

#[test]
fn comparator_heap_orders_ties_by_the_comparator_alone() {
    native(
        r#"
earlier = (def(a: &(i64, i64, str), b: &(i64, i64, str)) -> bool: a[0] < b[0] or (a[0] == b[0] and a[1] < b[1]))
mut heap = HeapBy(earlier)?
mut sequence = 0
for priority, name in [(2, "b"), (1, "first"), (2, "a"), (1, "second"), (1, "third")]?:
    heap.push((priority, sequence, name))?
    sequence += 1
for priority, _, name in drain_by(heap):
    print(priority, name)?
mut same = HeapBy(def(a: &(i64, str), b: &(i64, str)) -> bool: a[0] < b[0])?
same.push((1, "x"))?
same.push((1, "y"))?
same.push((0, "z"))?
print(same.pop().unwrap()[1], same.len())?
mut steps = 0
mut greatest = HeapBy(def [mut steps](a: &i64, b: &i64) -> bool: *a > *b)?
for n in [4, 9, 7]?:
    greatest.push(n)?
print(greatest.pop(), *greatest.peek())?
"#,
        r#"
1 first
1 second
1 third
2 b
2 a
z 2
Option[i64].Some(9) 7
"#,
    );
}

#[test]
fn heaps_move_owned_elements_and_drop_the_rest_once() {
    let text = output(
        r#"
mut heap = HeapBy(larger)?
for n in [2, 7, 4, 9, 1]?:
    heap.push(Guard(n))?
print("peek", heap.peek().n)?
taken = heap.pop().unwrap()
print("took", taken.n)?
drop(taken)
print("drain")?
for value in take(map(drain_by(heap), number), 2):
    print("value", value)?
print("cleared")?
mut again = HeapBy(larger)?
again.push(Guard(30))?
again.push(Guard(40))?
again.clear()
print("empty", again.len())?
again.push(Guard(50))?
"#,
    );
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(
        lines[..9],
        [
            "peek 9", "took 9", "drop 9", "drain", "drop 7", "value 7", "drop 4", "value 4",
            // Ending the adapter chain drops the heap still owned by the drain frame.
            "drop 2",
        ]
    );
    assert_eq!(lines[9..11], ["drop 1", "cleared"]);
    let mut cleared = lines[11..13].to_vec();
    cleared.sort_unstable();
    assert_eq!(cleared, ["drop 30", "drop 40"]);
    assert_eq!(lines[13..], ["empty 0", "drop 50"]);
}

#[test]
fn heap_operations_take_logarithmic_comparisons() {
    const COUNT: usize = 512;
    let text = output(&format!(
        r#"
mut heap = HeapBy(counted)?
for n in range({COUNT}):
    heap.push(({COUNT} - n) * 7 % 101)?
print("pushed")?
while not heap.is_empty():
    drop(heap.pop())
"#
    ));
    let (pushes, pops) = text.split_once("pushed").unwrap();
    let count = |part: &str| part.lines().filter(|line| *line == "compare").count();
    // A push compares once per level; a removal compares with both children.
    let levels = COUNT.ilog2() as usize;
    assert!(count(pushes) <= COUNT * levels, "{}", count(pushes));
    assert!(count(pops) <= 2 * COUNT * levels, "{}", count(pops));
    assert!(count(pops) >= COUNT, "{}", count(pops));
}

#[test]
fn empty_peek_traps_like_a_missing_index() {
    let output = execute("heap = Heap[i64]()?\nprint(*heap.peek())?");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("index out of bounds"), "{stderr}");
}

#[test]
fn adapters_compose_lazily_and_keep_callback_state() {
    native(
        r#"
print(fold(map(filter(take(naturals(), 10), even), square), 0, add))?
offset = 100
shifted = map((n for n in [1, 2, 3]?), def [offset](n: i64) -> i64: n + offset)
print([n for n in shifted]?)?
mut seen = 0
numbered = def [mut seen](n: i64) -> (i64, i64):
    seen += 1
    (seen, n)
for pair in take(map(naturals(), numbered), 2):
    print(pair)?
print(try_fold(take(naturals(), 4), list[i64].new()?, keep)?)?
print(fold(take(naturals(), 0), 7, add), fold(take(naturals(), -3), 7, add))?
mut heap = Heap[i64]()?
for n in [6, 1, 4, 3]?:
    heap.push(n)?
print(fold(filter(drain(heap), even), 0, add))?
"#,
        r#"
120
[101, 102, 103]
(1, 0)
(2, 1)
[0, 1, 2, 3]
7 7
10
"#,
    );
}

#[test]
fn adapters_run_only_as_far_as_they_are_consumed_and_clean_up() {
    native(
        r#"
for n in map(filter(guards(5), odd), number):
    print("got", n)?
    break
print("stopped")?
print(str.repr(try_fold(guards(9), 0, below_three))?)?
print(fold(take(guards(9), 1), 10, def(total: i64, guard: Guard) -> i64: total + guard.n))?
unused = map(guards(3), number)
print("unused")?
"#,
        r#"
make 0
drop 0
make 1
drop 1
got 1
stopped
make 0
drop 0
make 1
drop 1
make 2
drop 2
make 3
drop 3
Result[i64, str].Err("too large")
make 0
drop 0
10
unused
"#,
    );
}

#[test]
fn heap_and_adapters_allocate_only_for_growth() {
    native(
        r#"
mut heap = Heap[i64]()?
heap.reserve(8)?
mut jobs = HeapBy(quieter)?
jobs.reserve(2)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
for n in range(8):
    heap.push(8 - n).unwrap()
least = *heap.peek()
first = heap.pop().unwrap()
jobs.push(Quiet(1)).unwrap()
jobs.push(Quiet(2)).unwrap()
top = jobs.pop().unwrap().n
total = fold(map(filter(take(drain(heap), 5), even), square), 0, add)
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(least, first, top, total)?
"#,
        "dropped\n1 1 2 56\ndropped",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn heap_growth_failure_preserves_contents_and_drops_the_value() {
    let text = output(
        r#"
mut heap = HeapBy(quieter)?
heap.push(Quiet(0))?
mut failed = False
while not failed:
    n = heap.len()
    print("__test_fail_allocations_after_0__")?
    result = heap.push(Quiet(n))
    print("__test_restore_allocations__")?
    match result:
        case Ok(_):
            pass
        case Err(error):
            print(error)?
            failed = True
print(heap.len(), heap.peek().n == heap.len() - 1)?
"#,
    );
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines[..2], ["dropped", "AllocError.OutOfMemory"]);
    let (count, ordered) = lines[2].split_once(' ').unwrap();
    assert_eq!(ordered, "True");
    // The rejected value dropped once above; the survivors drop with the heap.
    let count = count.parse::<usize>().unwrap();
    assert!(count >= 1);
    assert_eq!(lines[3..], vec!["dropped"; count]);
}

#[test]
fn unsupported_orders_sources_and_escaping_borrows_are_rejected() {
    let unordered = rejected("mut heap = Heap[Guard]()?\nheap.push(Guard(1))?");
    assert!(
        unordered.contains("operator `<` does not accept Guard"),
        "{unordered}"
    );
    let tuple = rejected("mut heap = Heap[(i64, bool)]()?");
    assert!(tuple.contains("operator `<` does not accept"), "{tuple}");
    let reference = rejected("mut heap = Heap[&i64]()?");
    assert!(
        reference.contains("references cannot be stored in generic data"),
        "{reference}"
    );
    let loan = rejected(
        "mut heap = Heap[i64]()?\nheap.push(1)?\nleast = heap.peek()\nheap.push(2)?\nprint(*least)?",
    );
    assert!(loan.contains("conflicting borrow"), "{loan}");
    let stored = rejected(
        "limit = [1]?\nmut heap = HeapBy(def [&limit](a: &i64, b: &i64) -> bool: *a + limit[0] < *b)?",
    );
    assert!(
        stored.contains("closures with borrowed captures cannot be stored in generic data"),
        "{stored}"
    );
    let borrowed = rejected(
        "base = 3\nshift = (def [&base](n: i64) -> i64: n + base)\nfor n in map(naturals(), shift):\n    print(n)?",
    );
    assert!(
        borrowed.contains("pass a borrowing closure by reference"),
        "{borrowed}"
    );
    let list = rejected("values = [1, 2]?\nprint(fold(values, 0, add))?");
    assert!(
        list.contains("argument type list[i64] does not match"),
        "{list}"
    );
    let view = rejected("values = [1, 2]?\nprint(fold((n for n in &values), 0, add))?");
    assert!(
        view.contains("generator expressions require owned inputs"),
        "{view}"
    );
}

#[test]
fn heap_orders_tuple_priorities_lexicographically() {
    native(
        r#"
mut heap = Heap[(i64, str)]()?
for value in [(2, "a"), (1, "z"), (1, "a"), (1, "a")]?:
    heap.push(value)?
print(*heap.peek())?
for value in drain(heap):
    print(value)?
"#,
        "(1, \"a\")\n(1, \"a\")\n(1, \"a\")\n(1, \"z\")\n(2, \"a\")",
    );
}
