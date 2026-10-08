mod support;
use support::{check_source, run};

#[test]
fn shared_environment_types_are_bounded_without_exponential_expansion() {
    let mut source = String::from(
        "def make0() -> Closure[[], i64]:\n    n = 1\n    def [n]() -> i64:\n        n\n",
    );
    for i in 1..28 {
        let previous = i - 1;
        source.push_str(&format!("def make{i}() -> Closure[[], i64]:\n    a = make{previous}()\n    b = make{previous}()\n    def [a, b]() -> i64:\n        a() + b()\n"));
    }
    source.push_str("def main() -> ():\n    pass\n");
    let error = check_source(&source).unwrap_err().to_string();
    assert!(error.contains("closure environment exceeds"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn suspended_owned_closures_relocate_with_generator_frames_without_allocating() {
    let output = run(r#"
def produce(start: i64) -> Generator[i64]:
    mut advance = def [mut start]() -> i64:
        start = start + 1
        start
    yield advance()
    yield advance()
def main() -> ():
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    mut source = produce(2)
    first = next(source).unwrap()
    mut moved = source
    second = next(moved).unwrap()
    drop(moved)
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(first + second).unwrap()
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n7\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn complete_environment_lifecycle_needs_no_heap_allocation() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
def identity[T](value: T) -> T:
    value
def build(values: list[i64], resource: Resource) -> Option[Closure[[i64], i64]]:
    span = range(3, 6)
    inner = def [values, resource, span]() -> i64:
        values[0] + resource.id + span[1]
    count = 0
    outer = def [inner, mut count](n: i64) -> i64:
        count = count + n
        inner() + count
    Some(outer)
def invoke[T](f: &mut Closure[[T], T], n: T) -> T:
    f(n)
def main() -> ():
    values = [10].unwrap()
    resource = Resource(2).unwrap()
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    mut callback = identity(build(values, resource).unwrap())
    first = invoke(&mut callback, 1)
    second = callback(2)
    mut external = 5
    mut borrowed = def [&mut external]() -> i64:
        external = external + 1
        external
    third = borrowed()
    drop(borrowed)
    drop(callback)
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(first).unwrap()
    print(second).unwrap()
    print(third).unwrap()
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\ndrop\n__test_restore_allocations__\n__test_end_no_allocations__\n17\n19\n6\n");
}

#[test]
fn captures_cannot_be_shadowed_by_loop_pattern_or_unpack_bindings() {
    for statement in [
        "for value in range(2):\n            pass",
        "pair = (1, 2)?\n        value, other = pair",
        "match Some(2):\n            case Some(value):\n                pass\n            case Nothing:\n                pass",
        "items = [value for value in range(2)]?",
    ] {
        let source = format!("def main() -> ():\n    value = 1\n    f = def [value]() -> Result[(), AllocError]:\n        {statement}\n        Ok(())\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("cannot shadow closure capture"), "{error}");
    }
}

#[test]
fn closure_cleanup_follows_loop_exits_and_callable_captures_remain_usable() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def double(n: i64) -> i64:
    n * 2
def invoke(f: &Callable[[i64], i64], n: i64) -> i64:
    f(n)
def main() -> Result[(), Failure]:
    for n in range(3):
        resource = Resource(n)?
        f = def [resource]() -> i64:
            resource.id
        if n == 0:
            continue
        break
    function = double
    wrapped = def [function](n: i64) -> i64:
        invoke(&function, n)
    print(wrapped(4))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0\n1\n8\n");
}
