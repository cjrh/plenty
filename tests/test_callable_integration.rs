mod support;

fn expect(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn callable_result_types_contextualize_numeric_operands() {
    expect(
        r#"
def small(value: u8) -> u8:
    value
def factory() -> Callable[[u8], u8]:
    small
class Handler:
    callback: Callable[[u8], u8]
def main() -> Result[(), Failure]:
    f = small
    callbacks = [small]?
    index = 0
    handler = Handler(small)
    print(1 + f(2))?
    print(1 + factory()(3))?
    print(1 + callbacks[index](4))?
    print(1 + handler.callback(5))?
    print(small)?
    Ok(())
"#,
        "3\n4\n5\n6\n<function>\n",
    );
}

#[test]
fn indirect_calls_evaluate_callee_then_arguments_once() {
    expect(
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def select() -> Callable[[i64, i64], i64]:
    print("callee").unwrap()
    add
def argument(n: i64) -> i64:
    print(n).unwrap()
    n
def main() -> Result[(), Failure]:
    print(select()(argument(2), argument(3)))?
    Ok(())
"#,
        "callee\n2\n3\n5\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn callable_values_and_inline_results_need_no_heap_allocation() {
    expect(r#"
def make() -> Callable[[u64], Result[Option[range[u64]], AllocError]]:
    def(n: u64) -> Result[Option[range[u64]], AllocError]:
        Ok(Some(range[u64](n, n + 3)))
def forward(f: Callable[[u64], Result[Option[range[u64]], AllocError]], n: u64) -> Result[Option[range[u64]], AllocError]:
    f(n)
def main() -> ():
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    callback = make()
    saved = callback
    optional = Some(saved)
    result = forward(optional.unwrap(), 10).unwrap().unwrap()
    size = len(result)
    last = result[-1]
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(size).unwrap()
    print(last).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n3\n12\n");
}

#[test]
fn propagation_cleans_pending_owned_indirect_arguments() {
    expect(
        r#"
class Guard:
    def __del__(self) -> ():
        print("drop").unwrap()
def fail() -> Result[i64, AllocError]:
    Err(AllocError.OutOfMemory)
def consume(guard: Guard, n: i64) -> i64:
    n
def work() -> Result[i64, AllocError]:
    callback = consume
    Ok(callback(Guard(), fail()?))
def main() -> Result[(), Failure]:
    print(str.repr(work()).unwrap())?
    Ok(())
"#,
        "drop\nResult[i64, AllocError].Err(AllocError.OutOfMemory)\n",
    );
}
