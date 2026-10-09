mod support;

fn run(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn generic_records_embed_distinct_owned_closure_layouts() {
    run(
        r#"
class Handler[F]:
    callback: F
    def call(self: &mut Handler[F], n: i64) -> i64:
        self.callback(n)
def counter(total: i64) -> Closure[[i64], i64]:
    def [mut total](n: i64) -> i64:
        total = total + n
        total
mut first = Handler(counter(10)).unwrap()
mut second = Handler(counter(100)).unwrap()
bias = 1000
subtract = def [bias](n: i64) -> i64:
    bias - n
third = Handler(subtract).unwrap()
print(first.call(3)).unwrap()
print(second.call(7)).unwrap()
print(first.callback(2)).unwrap()
print(third.callback(4)).unwrap()
"#,
        "13\n107\n15\n996\n",
    );
}

#[test]
fn tuple_closures_preserve_distinct_layouts_and_transfer_on_unpack() {
    run(
        r#"
def add(n: i64) -> Closure[[i64], i64]:
    def [n](x: i64) -> i64:
        n + x
def subtract(n: i64) -> Closure[[i64], i64]:
    span = range(n, n + 3)
    def [span](x: i64) -> i64:
        span[1] - x
def pack[A, B](a: A, b: B) -> Result[tuple[A, B], AllocError]:
    (a, b)
pair = pack(add(10), subtract(30)).unwrap()
print(pair[0](2)).unwrap()
print(pair[1](3)).unwrap()
first, second = pair
print(first(4)).unwrap()
print(second(5)).unwrap()
"#,
        "12\n28\n14\n26\n",
    );
}

#[test]
fn tuple_callback_loans_are_disjoint_and_do_not_allow_owner_invalidation() {
    run(
        r#"
def counter(n: i64) -> Closure[[], i64]:
    def [mut n]() -> i64:
        n = n + 1
        n
mut pair = (counter(1), counter(10)).unwrap()
first = &mut pair[0]
second = &mut pair[1]
print(first()).unwrap()
print(second()).unwrap()
print(pair[0]()).unwrap()
"#,
        "2\n11\n3\n",
    );
    for tail in [
        "drop(pair)\nprint(callback()).unwrap()",
        "print(pair[0]()).unwrap()\nprint(callback()).unwrap()",
    ] {
        let source = format!("def counter(n: i64) -> Closure[[], i64]:\n    def [mut n]() -> i64:\n        n = n + 1\n        n\nmut pair = (counter(1), counter(2)).unwrap()\ncallback = &mut pair[0]\n{tail}\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
}

#[test]
fn closure_lists_relocate_nested_environments_during_growth_reverse_and_pop() {
    run(
        r#"
def counter(n: i64) -> Closure[[], i64]:
    span = range(n, n + 3)
    inner = def [span]() -> i64:
        span[1]
    total = 0
    def [inner, mut total]() -> i64:
        total = total + 1
        inner() + total
mut callbacks = [counter(1)].unwrap()
for n in range(2, 60):
    callbacks.append(counter(n)).unwrap()
callbacks.reverse()
print(callbacks[0]()).unwrap()
mut removed = callbacks.pop(0).unwrap()
print(removed()).unwrap()
print(callbacks[0]()).unwrap()
for callback in &mut callbacks:
    callback()
print(callbacks[0]()).unwrap()
more = [counter(100), counter(200)].unwrap()
callbacks.extend(more).unwrap()
print(callbacks[-1]()).unwrap()
"#,
        "61\n62\n60\n62\n202\n",
    );
}

#[test]
fn closure_comprehensions_and_consuming_iteration_transfer_environments() {
    run(
        r#"
def make(n: i64) -> Closure[[], i64]:
    def [n]() -> i64:
        n
def wrap[F](callback: F) -> Result[list[F], AllocError]:
    [callback]
callbacks = [make(n) for n in range(3)].unwrap()
for callback in callbacks:
    print(callback()).unwrap()
owned = wrap(make(8)).unwrap()
for callback in owned:
    print(callback()).unwrap()
"#,
        "0\n1\n2\n8\n",
    );
    for tail in [
        "callbacks.append(make(3)).unwrap()\nprint(borrowed()).unwrap()",
        "callbacks.pop(0)\nprint(borrowed()).unwrap()",
    ] {
        let source = format!("def make(n: i64) -> Closure[[], i64]:\n    def [n]() -> i64:\n        n\nmut callbacks = [make(1)].unwrap()\nborrowed = &callbacks[0]\n{tail}\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn pop_append_and_call_of_inline_list_callbacks_reuse_capacity() {
    run(r#"
def make(n: i64) -> Closure[[], i64]:
    span = range(n, n + 3)
    def [span]() -> i64:
        span[1]
mut callbacks = [make(10), make(20)].unwrap()
next_callback = make(30)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
removed = callbacks.pop(0).unwrap()
callbacks.append(next_callback).unwrap()
result = removed() + callbacks[0]() + callbacks[1]()
drop(callbacks)
drop(removed)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(result).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n63\n");
}

#[test]
fn generic_record_closure_resources_drop_on_replacement() {
    run(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
class Handler[F]:
    callback: F
def make(id: i64) -> Result[Closure[[], i64], AllocError]:
    resource = Guard(id)?
    callback = def [resource]() -> i64:
        resource.id
    Ok(callback)
mut handler = Handler(make(1).unwrap()).unwrap()
handler.callback = make(2).unwrap()
print(handler.callback()).unwrap()
drop(handler)
"#,
        "1\n2\n2\n",
    );
}

#[test]
fn stored_closures_retain_ownership_borrowing_and_comparison_limits() {
    for (source, expected) in [
        ("class Holder[T]:\n    item: T\nn = 1\nf = def [&n]() -> i64:\n    n\nh = Holder(f).unwrap()\n", "reference"),
        ("class Holder[T]:\n    item: T\nn = 1\nf = def [mut n]() -> i64:\n    n = n + 1\n    n\nh = Holder(f).unwrap()\nh.item()\n", "immutable"),
        ("class Holder[T]:\n    item: T\nn = 1\nf = def [n]() -> i64:\n    n\nh = Holder(f).unwrap()\nprint(h == h).unwrap()\n", "containing closures do not support equality"),
        ("class Holder[T]:\n    item: T\nn = 1\nf = def [n]() -> i64:\n    n\nh = Holder(f).unwrap()\ncopy(h)\n", "cannot be copied"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_record_allocation_cleans_owned_environment_and_calls_do_not_allocate() {
    run(r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
class Holder[T]:
    callback: T
def make(id: i64) -> Result[Closure[[], i64], AllocError]:
    guard = Guard(id)?
    callback = def [guard]() -> i64:
        guard.id
    Ok(callback)
callback = make(9).unwrap()
holder = Holder(make(3).unwrap()).unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
value = holder.callback()
drop(holder)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(value).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
match Holder(callback):
    case Ok(value):
        drop(value)
    case Err(_):
        write_stdout("failed\n").unwrap()
        pass
print("__test_restore_allocations__").unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\ndrop\n__test_restore_allocations__\n__test_end_no_allocations__\n3\n__test_fail_allocations_after_0__\ndrop\nfailed\n__test_restore_allocations__\n");
}
