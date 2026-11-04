//! `std.collections` heaps, including heap drains as pipeline sources.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

const HEADER: &str = "from std.collections import Heap, HeapBy, drain, drain_by\n";

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
        if n == i64.MAX:
            return
        # The terminal value returned above; the nonnegative increment fits.
        unchecked:
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
    two: Positive[i64] = 2
    guard.n % two == 1

def even(n: &i64) -> bool:
    two: Positive[i64] = 2
    *n % two == 0

def square(n: i64) -> Result[i64, OverflowError[i64]]:
    Ok(n * n)

def add(total: i64, n: i64) -> Result[i64, OverflowError[i64]]:
    Ok(total + n)

def add_result(total: i64, n: Result[i64, OverflowError[i64]]) -> Result[i64, OverflowError[i64]]:
    Ok(total + n?)

def counted(a: &i64, b: &i64) -> bool:
    print("compare").unwrap()
    *a < *b

def keep(items: list[i64], n: i64) -> Result[list[i64], AllocError]:
    mut out = items
    out.append(n)?
    Ok(out)

def below_three(total: i64, guard: Guard) -> Result[i64, OverflowError[i64] | str]:
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
    format!(
        "{HEADER}{DECLARATIONS}\ndef main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:\n{body}    Ok(())\n"
    )
}

fn execute(body: &str) -> std::process::Output {
    execute_source(&program(body), false)
}

fn execute_source(source: &str, optimize: bool) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable_with_options(
        &path,
        &executable,
        None,
        &plenty::CompileOptions {
            optimize,
            ..Default::default()
        },
    )
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
print(heap.len(), heap.peek().unwrap())?
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
print(greatest.pop(), greatest.peek().unwrap())?
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
match heap.peek_ref():
    case Some(top):
        print("peek", top.n)?
    case Nothing:
        pass
taken = heap.pop().unwrap()
print("took", taken.n)?
drop(taken)
print("drain")?
for value in drain_by(heap).map(number).take(2):
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
            // Ending the pipeline drops the heap still owned by the drain frame.
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
fn peek_copies_or_borrows_the_first_element_and_reports_empty() {
    native(
        r#"
mut heap = Heap[i64]()?
print(heap.peek(), heap.peek_ref())?
heap.push(3)?
heap.push(1)?
print(heap.peek(), heap.peek_ref(), heap.len())?
mut guards = HeapBy(larger)?
print(guards.peek_ref().is_some())?
"#,
        "Option[i64].Nothing Option[&i64].Nothing\nOption[i64].Some(1) Option[&i64].Some(1) 2\nFalse",
    );
    // `peek` copies, so it is checked per use and rejected for affine elements.
    let affine =
        rejected("mut heap = HeapBy(larger)?\nheap.push(Guard(1))?\nprint(heap.peek().is_some())?");
    assert!(
        affine.contains("copies `T`, but `Guard` cannot be copied"),
        "{affine}"
    );
}

#[test]
fn generator_pipelines_compose_lazily_and_keep_callback_state() {
    native(
        r#"
print(naturals().take(10).filter(even).map(square).try_fold(0, add_result)?)?
offset = 100
shifted = (n for n in [1, 2, 3]?).map(def [offset](n: i64) -> Result[i64, OverflowError[i64]]: Ok(n + offset))
print([n? for n in shifted]?)?
mut seen = 0
numbered = def [mut seen](n: i64) -> Result[(i64, i64), OverflowError[i64]]:
    seen += 1
    Ok((seen, n))
for pair in naturals().map(numbered).take(2):
    print(pair?)?
print(naturals().take(4).try_fold(list[i64].new()?, keep)?)?
print(naturals().take(0).try_fold(7, add)?, naturals().take(-3).try_fold(7, add)?)?
mut heap = Heap[i64]()?
for n in [6, 1, 4, 3]?:
    heap.push(n)?
print(drain(heap).filter(even).try_fold(0, add)?)?
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
fn generator_pipelines_run_only_as_far_as_they_are_consumed_and_clean_up() {
    native(
        r#"
for n in guards(5).filter(odd).map(number):
    print("got", n)?
    break
print("stopped")?
print(str.repr(guards(9).try_fold(0, below_three))?)?
print(guards(9).take(1).try_fold(10, def(total: i64, guard: Guard) -> Result[i64, OverflowError[i64]]: Ok(total + guard.n))?)?
unused = guards(3).map(number)
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
Result[i64, OverflowError[i64] | str].Err("too large")
make 0
drop 0
10
unused
"#,
    );
}

#[test]
fn heap_and_pipelines_allocate_only_for_growth() {
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
least = heap.peek().unwrap()
first = heap.pop().unwrap()
jobs.push(Quiet(1)).unwrap()
jobs.push(Quiet(2)).unwrap()
top = jobs.pop().unwrap().n
total = drain(heap).take(5).filter(even).map(square).try_fold(0, add_result)?
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
match heap.peek_ref():
    case Some(top):
        print(heap.len(), top.n == heap.len() - 1)?
    case Nothing:
        pass
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
    let tuple = rejected("mut heap = Heap[(i64, bool)]()?\nheap.push((1, True))?");
    assert!(tuple.contains("operator `<` does not accept"), "{tuple}");
    let reference = rejected("mut heap = Heap[&i64]()?");
    assert!(
        reference.contains("references cannot be stored in generic data"),
        "{reference}"
    );
    let loan = rejected(
        "mut heap = Heap[i64]()?\nheap.push(1)?\nleast = heap.peek_ref()\nheap.push(2)?\nprint(least)?",
    );
    assert!(loan.contains("conflicting borrow"), "{loan}");
    let stored = rejected(
        "limit = [1]?\nmut heap = HeapBy(def [&limit](a: &i64, b: &i64) -> bool: *a < *b and limit[0] == 1)?",
    );
    assert!(
        stored.contains("closures with borrowed captures cannot be stored in generic data"),
        "{stored}"
    );
    let view = rejected("values = [1, 2]?\nprint((n for n in &values).fold(0, add))?");
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
print(heap.peek().unwrap())?
for value in drain(heap):
    print(value)?
"#,
        "(1, \"a\")\n(1, \"a\")\n(1, \"a\")\n(1, \"z\")\n(2, \"a\")",
    );
}

/// Exercise the exact private arithmetic used by production without allocating
/// impossible containers. Only the pre-class helper declarations are included.
#[test]
fn production_ring_heap_and_cache_extent_boundaries_in_both_modes() {
    let helpers = [
        include_str!("../stdlib/std/deque.plenty"),
        include_str!("../stdlib/std/collections.plenty"),
        include_str!("../stdlib/std/cache/lru.plenty"),
    ]
    .map(|source| source.split_once("pub class").unwrap().0)
    .join("\n");
    let source = format!(
        r#"{helpers}
def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    print(grown_capacity(0)?, grown_capacity(8)?, grown_capacity(i64.MAX // 2)?)?
    print(grown_capacity(-1).is_err(), grown_capacity(i64.MAX // 2 + 1).is_err())?
    print(ring_position(i64.MAX, i64.MAX - 1, 0), ring_position(i64.MAX, i64.MAX - 1, 1), ring_position(i64.MAX, i64.MAX - 1, i64.MAX - 1))?
    print(first_child(0, 0), first_child(0, 1), first_child(0, 2))?
    print(first_child(i64.MAX // 2 - 1, i64.MAX), first_child(i64.MAX // 2, i64.MAX))?
    print(cache_slots(0)?, cache_slots(i64.MAX - 1)?, cache_slots(-1).is_err(), cache_slots(i64.MAX).is_err())?
    Ok(())
"#
    );
    let expected = "8 16 9223372036854775806\nTrue True\n9223372036854775806 0 9223372036854775805\nOption[i64].Nothing Option[i64].Nothing Option[i64].Some(1)\nOption[i64].Some(9223372036854775805) Option[i64].Nothing\n1 9223372036854775807 True True\n";
    for optimize in [false, true] {
        let output = execute_source(&source, optimize);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn generic_containers_keep_their_public_contracts_in_both_modes() {
    let source = r#"
from std.deque import Deque
from std.collections import Heap, HeapBy, drain, drain_by
from std.graph import DiGraph
from std.cache.lru import LruCache

def main() -> Result[(), AllocError | IoError]:
    mut queue = Deque[i64]()?
    for n in range(40):
        queue.append(n)?
    for n in range(20):
        drop(queue.popleft())
    for n in range(40, 80):
        queue.append(n)?
    queue.appendleft(99)?
    print(queue.popleft(), queue.pop(), queue.len())?
    mut heap = Heap[i64]()?
    mut reverse = HeapBy(def(a: &i64, b: &i64) -> bool: *a > *b)?
    for n in [3, 1, 2]?:
        heap.push(n)?
        reverse.push(n)?
    print(list(drain(heap))?, list(drain_by(reverse))?)?
    mut graph = DiGraph[i64, f64]()?
    a = graph.add_node(1)?
    b = graph.add_node(2)?
    drop(graph.add_edge(a, b, 1.0)?)
    print(graph.bfs(a)?, graph.toposort().is_ok())?
    drop(graph.remove_node(b))
    print(graph.node_count(), graph.edge_count(), graph.add_node(3)?)?
    mut cache = LruCache[i64, i64](2)?
    drop(cache.put(1, 10)?)
    drop(cache.put(2, 20)?)
    drop(cache.get(&1))
    print(cache.put(3, 30)?, cache.pop_lru(), cache.pop_lru())?
    Ok(())
"#;
    for optimize in [false, true] {
        let output = execute_source(source, optimize);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "Option[i64].Some(99) Option[i64].Some(79) 59\n[1, 2, 3] [3, 2, 1]\n[0, 1] True\n1 0 2\nOption[tuple[i64, i64]].Some((2, 20)) Option[tuple[i64, i64]].Some((1, 10)) Option[tuple[i64, i64]].Some((3, 30))\n"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn deque_wrapped_growth_failure_preserves_owned_rows_and_allows_reuse() {
    for optimize in [false, true] {
        // Use the existing affine fixture; marker output is filtered below.
        let body = r#"
mut failures = 0
mut successes = 0
for limit in range(3):
    mut queue = Deque[Quiet]()?
    for n in range(8):
        queue.append(Quiet(n))?
    for n in range(3):
        drop(queue.popleft())
    for n in range(8, 11):
        queue.append(Quiet(n))?
    marker = ("__test_fail_allocations_after_" + str.from(limit)?)?
    print((marker + "__")?)?
    result = queue.appendleft(Quiet(99))
    print("__test_restore_allocations__")?
    match result:
        case Err(error):
            print(error, queue.len(), queue.popleft().unwrap().n, queue.pop().unwrap().n)?
            failures += 1
        case Ok(_):
            print(queue.len(), queue.popleft().unwrap().n, queue.pop().unwrap().n)?
            successes += 1
    queue.clear()
    queue.append(Quiet(100))?
    print(queue.pop().unwrap().n, queue.is_empty())?
print(failures > 0, successes > 0)?
"#;
        let source = format!("from std.deque import Deque\n{}", program(body));
        let output = execute_source(&source, optimize);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        let lines = text
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect::<Vec<_>>();
        assert_eq!(
            lines.iter().filter(|line| **line == "dropped").count(),
            39,
            "{text}"
        );
        let states = lines
            .into_iter()
            .filter(|line| *line != "dropped")
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            [
                "AllocError.OutOfMemory 8 3 10",
                "100 True",
                "AllocError.OutOfMemory 8 3 10",
                "100 True",
                "9 99 10",
                "100 True",
                "True True"
            ]
        );
    }
}

#[test]
fn fallible_fixture_callbacks_keep_overflow_and_drain_cleanup_observable() {
    native(
        r#"
print(square(i64.MAX).is_err(), add(i64.MAX, 1).is_err())?
mut heap = HeapBy(def(a: &Guard, b: &Guard) -> bool: a.n < b.n)?
for n in [4, 2, 1, 3]?:
    heap.push(Guard(n))?
print(str.repr(drain_by(heap).try_fold(0, below_three))?)?
"#,
        "True True\ndrop 1\ndrop 2\ndrop 3\ndrop 4\nResult[i64, OverflowError[i64] | str].Err(\"too large\")",
    );
}
