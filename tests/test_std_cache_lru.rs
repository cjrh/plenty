//! `std.cache.lru` least-recently-used caches.

const HEADER: &str = "from std.cache.lru import LruCache\n";

const DECLARATIONS: &str = r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()

# Printing one short string allocates nothing, so this cleanup can run while
# allocation is failing.
class Quiet:
    n: i64
    def __del__(self) -> ():
        print("dropped").unwrap()

def quiet_number(quiet: &Quiet) -> i64:
    quiet.n

def number(guard: &Guard) -> i64:
    guard.n

def same(n: &i64) -> i64:
    *n

def length(values: &list[i64]) -> i64:
    len(values)

def show(result: Option[(i64, i64)]) -> Result[(), IoError]:
    match result:
        case Some(entry):
            key, value = entry
            print(key, value)
        case Nothing:
            print("-")

def show_guard(result: Option[(i64, Guard)]) -> Result[(), IoError]:
    match result:
        case Some(entry):
            key, guard = entry
            print("out", key, guard.n)
        case Nothing:
            print("-")
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}{DECLARATIONS}\ndef main() -> Result[(), AllocError | IoError]:\n{body}    Ok(())\n"
    )
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
fn put_evicts_the_least_recently_used_entry() {
    native(
        r#"
mut cache = LruCache[str, i64](2)?
print(cache.capacity(), cache.len(), cache.is_empty())?
print(cache.put("a", 1)?, cache.put("b", 2)?)?
print(cache.get_with("a", &same))?
print(cache.put("c", 3)?)?
print(cache.contains("a"), cache.contains("b"), cache.contains("c"), cache.len())?
print(cache.put("a", 10)?)?
print(cache.put("d", 4)?)?
print(cache.get_with("missing", &same))?
"#,
        r#"
2 0 True
Option[tuple[str, i64]].Nothing Option[tuple[str, i64]].Nothing
Option[i64].Some(1)
Option[tuple[str, i64]].Some(("b", 2))
True False True 2
Option[tuple[str, i64]].Some(("a", 1))
Option[tuple[str, i64]].Some(("c", 3))
Option[i64].Nothing
"#,
    );
}

#[test]
fn peek_and_contains_leave_recency_unchanged() {
    native(
        r#"
mut cache = LruCache[i64, i64](2)?
show(cache.put(1, 10)?)?
show(cache.put(2, 20)?)?
print(cache.peek_with(&1, &same), cache.contains(&1))?
slot = cache.peek_mut(&1)
*slot += 1
show(cache.put(3, 30)?)?
print(cache.promote(&2), cache.promote(&1))?
show(cache.put(4, 40)?)?
show(cache.pop_lru())?
show(cache.pop_lru())?
show(cache.pop_lru())?
"#,
        r#"
-
-
Option[i64].Some(10) True
1 11
True False
3 30
2 20
4 40
-
"#,
    );
}

#[test]
fn get_promotes_and_peek_copies_without_promoting() {
    native(
        r#"
mut cache = LruCache[i64, i64](2)?
show(cache.put(1, 10)?)?
show(cache.put(2, 20)?)?
print(cache.peek(&1), cache.peek(&9), cache.get(&9))?
show(cache.put(3, 30)?)?
print(cache.get(&2))?
show(cache.put(4, 40)?)?
print(cache.peek(&3), cache.len())?
"#,
        r#"
-
-
Option[i64].Some(10) Option[i64].Nothing Option[i64].Nothing
1 10
Option[i64].Some(20)
3 30
Option[i64].Nothing 2
"#,
    );
}

#[test]
fn get_copies_strings_and_tuples() {
    native(
        r#"
mut names = LruCache[str, str](2)?
drop(names.put("a", "Ada")?)
name = names.get("a")
drop(names.remove("a"))
print(name, names.len())?
mut pairs = LruCache[i64, (i64, str)](1)?
drop(pairs.put(1, (7, "seven"))?)
print(pairs.peek(&1), pairs.get(&1))?
"#,
        r#"
Option[str].Some("Ada") 0
Option[tuple[i64, str]].Some((7, "seven")) Option[tuple[i64, str]].Some((7, "seven"))
"#,
    );
}

#[test]
fn copying_lookups_reject_only_calls_with_affine_values() {
    // Generic class methods are checked when used (issue #515), so an
    // affine value type is accepted until `get` or `peek` is called.
    let accepted = "mut cache = LruCache[i64, list[i64]](1)?\ndrop(cache.put(1, [1]?)?)\nprint(cache.contains(&1), cache.peek_with(&1, &length))?";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, program(accepted)).unwrap();
    plenty::check_file(&path, None).unwrap();
    for method in ["get", "peek"] {
        let body = format!("mut cache = LruCache[i64, list[i64]](1)?\nvalue = cache.{method}(&1)");
        let call = program(&body)
            .lines()
            .position(|line| line.contains(&format!(".{method}(")))
            .unwrap()
            + 1;
        let error = rejected(&body);
        assert!(
            error.contains(&format!(
                "main.plenty:{call}:13: `std.cache.lru.LruCache[i64, list[i64]].{method}` copies \
                 `V`, but `list[i64]` cannot be copied"
            )),
            "{error}"
        );
        // The note describes the library line without advice to change it
        // (issue #518).
        assert!(
            error.ends_with(&format!(
                "note: in instantiation `std.cache.lru.LruCache[i64, list[i64]].{method}`: \
                 cannot move out of indexed storage"
            )),
            "{error}"
        );
    }
}

#[test]
fn check_all_methods_skips_standard_library_types() {
    // `--check-all-methods` checks only types the program declares (issue
    // #517). `get` copies `V`, which `list[i64]` cannot be, yet the program
    // never calls it, so the instance is accepted.
    let source = program("mut cache = LruCache[i64, list[i64]](1)?\ndrop(cache.put(1, [1]?)?)");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, &source).unwrap();
    let options = plenty::CheckOptions { all_methods: true };
    plenty::check_file_with_options(&path, None, true, &options).unwrap();
}

#[test]
fn get_ref_promotes_and_returns_a_borrow() {
    native(
        r#"
mut cache = LruCache[i64, list[i64]](2)?
drop(cache.put(1, [1, 2]?)?)
drop(cache.put(2, [3]?)?)
first = cache.get_ref(&1)
print(len(first), first)?
match cache.put(3, [4]?)?:
    case Some(entry):
        key, value = entry
        print(key, value)?
    case Nothing:
        print("-")?
print(cache.remove(&1), cache.remove(&1), cache.len())?
"#,
        r#"
2 [1, 2]
2 [3]
Option[list[i64]].Some([1, 2]) Option[list[i64]].Nothing 1
"#,
    );
}

#[test]
fn owned_values_transfer_out_or_drop_exactly_once() {
    native(
        r#"
mut cache = LruCache[i64, Guard](2)?
show_guard(cache.put(1, Guard(1))?)?
show_guard(cache.put(2, Guard(2))?)?
print(cache.get_with(&1, &number))?
show_guard(cache.put(3, Guard(3))?)?
show_guard(cache.put(3, Guard(33))?)?
print(cache.get_ref(&1).n, cache.peek_with(&3, &number))?
guard = cache.remove(&1)
print("removed", cache.len())?
drop(guard)
show_guard(cache.put(4, Guard(4))?)?
cache.clear()
print("cleared", cache.len())?
show_guard(cache.put(5, Guard(5))?)?
"#,
        r#"
-
-
Option[i64].Some(1)
out 2 2
drop 2
out 3 3
drop 3
1 Option[i64].Some(33)
removed 1
drop 1
-
drop 33
drop 4
cleared 0
-
drop 5
"#,
    );
}

#[test]
fn capacity_zero_stores_nothing_and_invalid_capacities_fail() {
    native(
        r#"
mut cache = LruCache[i64, Guard](0)?
show_guard(cache.put(1, Guard(1))?)?
print(cache.len(), cache.contains(&1), cache.pop_lru().is_some())?
match LruCache[i64, i64](-1):
    case Ok(_):
        print("built")?
    case Err(error):
        print(error)?
match LruCache[i64, i64](i64.MAX):
    case Ok(_):
        print("built")?
    case Err(error):
        print(error)?
"#,
        r#"
out 1 1
drop 1
0 False False
AllocError.CapacityOverflow
AllocError.CapacityOverflow
"#,
    );
}

#[test]
fn missing_key_in_a_reference_accessor_traps() {
    let output = execute(
        r#"
mut cache = LruCache[i64, i64](2)?
drop(cache.put(1, 1)?)
print("before")?
value = cache.get_ref(&2)
print(*value)?
"#,
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "before\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("std/cache/lru.plenty"), "{stderr}");
}

/// Keys, values and operations come from a linear congruential generator so
/// that the Plenty cache and this model see the same long mixed sequence.
#[test]
fn matches_a_reference_model_over_mixed_operations() {
    const CAPACITY: usize = 5;
    const STEPS: u64 = 400;
    let mut state = 7u64;
    let mut order: Vec<(i64, i64)> = Vec::new(); // most recent first
    let mut expected = Vec::new();
    for step in 0..STEPS {
        state = (state * 1_103_515_245 + 12_345) % 2_147_483_648;
        let key = ((state / 16) % 9) as i64;
        let value = step as i64;
        let position = order.iter().position(|(k, _)| *k == key);
        match state % 6 {
            0..=2 => {
                let line = if let Some(index) = position {
                    let (_, old) = order.remove(index);
                    order.insert(0, (key, value));
                    format!("put {key} {old}")
                } else {
                    order.insert(0, (key, value));
                    if order.len() > CAPACITY {
                        let (k, v) = order.pop().unwrap();
                        format!("put {k} {v}")
                    } else {
                        "put -".to_string()
                    }
                };
                expected.push(line);
            }
            3 => expected.push(match position {
                Some(index) => {
                    let entry = order.remove(index);
                    order.insert(0, entry);
                    format!("get {}", entry.1)
                }
                None => "get -".to_string(),
            }),
            4 => expected.push(match position {
                Some(index) => format!("peek {}", order[index].1),
                None => "peek -".to_string(),
            }),
            _ => expected.push(match position {
                Some(index) => format!("remove {}", order.remove(index).1),
                None => "remove -".to_string(),
            }),
        }
    }
    let keys = order.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
    expected.push(format!("order {}", keys.join(" ")));

    let body = format!(
        r#"
mut cache = LruCache[i64, i64]({CAPACITY})?
mut state = 7
for step in range({STEPS}):
    state = (state * 1103515245 + 12345) % 2147483648
    key = (state // 16) % 9
    op = state % 6
    if op <= 2:
        match cache.put(key, step)?:
            case Some(entry):
                k, v = entry
                print("put", k, v)?
            case Nothing:
                print("put -")?
    elif op == 3:
        match cache.get(&key):
            case Some(v):
                print("get", v)?
            case Nothing:
                print("get -")?
    elif op == 4:
        match cache.peek(&key):
            case Some(v):
                print("peek", v)?
            case Nothing:
                print("peek -")?
    else:
        match cache.remove(&key):
            case Some(v):
                print("remove", v)?
            case Nothing:
                print("remove -")?
while True:
    match cache.pop_lru():
        case Some(entry):
            k, _ = entry
            print("lru", k)?
        case Nothing:
            break
"#
    );
    let text = output(&body);
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    // The cache drains least recent first; the model lists most recent first.
    let mut drained = lines
        .iter()
        .filter_map(|line| line.strip_prefix("lru ").map(str::to_string))
        .collect::<Vec<_>>();
    lines.retain(|line| !line.starts_with("lru "));
    drained.reverse();
    lines.push(format!("order {}", drained.join(" ")));
    assert_eq!(lines, expected);
}

#[test]
fn operations_after_construction_allocate_nothing() {
    native(
        r#"
mut cache = LruCache[i64, Quiet](4)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
mut evicted = 0
for n in range(200):
    match cache.put(n % 7, Quiet(n)).unwrap():
        case Some(entry):
            k, guard = entry
            evicted += 1
            drop(k)
            drop(guard)
        case Nothing:
            pass
    drop(cache.get_with(&(n % 5), &quiet_number))
    drop(cache.remove(&(n % 11)))
    drop(cache.promote(&(n % 3)))
cache.clear()
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(evicted > 0, cache.len())?
"#,
        &format!("{}\nTrue 0", "dropped\n".repeat(200).trim_end()),
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn construction_failure_returns_alloc_error() {
    let text = output(
        r#"
mut failures = 0
mut built = False
mut limit = 0
while not built:
    marker = ("__test_fail_allocations_after_" + str.from(limit)?)?
    print((marker + "__")?)?
    result = LruCache[str, Guard](3)
    print("__test_restore_allocations__")?
    match result:
        case Ok(_):
            built = True
        case Err(error):
            print(error)?
            failures += 1
    limit += 1
print(failures)?
"#,
    );
    let lines = text.lines().collect::<Vec<_>>();
    let failures = lines.last().unwrap().parse::<usize>().unwrap();
    assert!(failures >= 5, "{text}");
    assert!(
        lines[..failures]
            .iter()
            .all(|line| *line == "AllocError.OutOfMemory"),
        "{text}"
    );
}

#[test]
fn references_and_unhashable_keys_are_rejected() {
    let escaping = rejected(
        "mut cache = LruCache[i64, i64](1)?\nvalue = cache.get_ref(&1)\ndrop(cache.put(2, 2)?)\nprint(*value)?",
    );
    assert!(escaping.contains("borrow"), "{escaping}");
    let float_keys = rejected("cache = LruCache[f64, i64](1)?");
    assert!(
        float_keys.contains("dictionary keys must be integers"),
        "{float_keys}"
    );
}
