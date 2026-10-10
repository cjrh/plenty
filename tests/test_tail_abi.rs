mod support;

#[cfg(target_os = "linux")]
#[test]
fn native_inline_result_recursion_emits_a_jump_instead_of_a_call() {
    let workspace = tempfile::tempdir().unwrap();
    let object = workspace.path().join("tail.o");
    plenty::compile_source_to_object(
        r#"
def countdown(n: i64) -> range[i64]:
    if n == 0:
        return range(4)
    countdown(n - 1)
def main() -> ():
    pass
"#,
        &object,
    )
    .unwrap();
    let output = std::process::Command::new("objdump")
        .arg("-dr")
        .arg(&object)
        .output()
        .unwrap();
    assert!(output.status.success());
    let assembly = String::from_utf8(output.stdout).unwrap();
    let body = assembly
        .split("<__plenty_fn_countdown>:")
        .nth(1)
        .unwrap()
        .split("\n\n")
        .next()
        .unwrap();
    let lines: Vec<_> = body.lines().collect();
    let jump = lines
        .windows(2)
        .any(|pair| pair[0].contains("jmp") && pair[1].contains("__plenty_fn_countdown"));
    assert!(jump, "expected native tail jump to countdown:\n{body}");
    assert!(
        !lines
            .windows(2)
            .any(|pair| pair[0].contains("call") && pair[1].contains("__plenty_fn_countdown")),
        "recursive call retained frame:\n{body}"
    );
}

#[cfg(target_os = "linux")]
fn bounded(source: &str, expected: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("tail-abi");
    support::compile_source_to_executable(source, &executable).unwrap();
    let output = std::process::Command::new("sh")
        .args(["-c", "ulimit -s 256; exec \"$1\"", "tail-abi-test"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[cfg(target_os = "linux")]
#[test]
fn direct_and_indirect_inline_results_forward_the_incoming_area() {
    bounded(
        r#"
def direct(n: i64) -> range[i64]:
    if n == 0:
        return range(7, 10)
    direct(n - 1)
def indirect(n: i64) -> Option[range[i64]]:
    if n == 0:
        return Some(range(20, 23))
    call = indirect
    call(n - 1)
def main() -> Result[(), Failure]:
    print(direct(300000))?
    print(indirect(300000))?
    Ok(())
"#,
        "range(7, 10, 1)\nOption[range].Some(range(20, 23, 1))\n",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn mutually_recursive_argument_permutations_and_different_arity_relocate_nested_results() {
    bounded(
        r#"
class Parcel:
    prefix: i64
    values: Option[range[i64]]
    suffix: i64
def short(n: i64, a: i64, b: i64) -> Result[Parcel, i64]:
    if n == 0:
        return Ok(Parcel(a, Some(range(a, b)), b))
    long(n - 1, b, a, 1, 2, 3, 4, 5, 6, 7, 8)
def long(n: i64, b: i64, a: i64, x: i64, y: i64, z: i64, p: i64, q: i64, r: i64, s: i64, t: i64) -> Result[Parcel, i64]:
    short(n, a, b)
def main() -> Result[(), Failure]:
    value = short(300000, 4, 9).unwrap()
    print(value.prefix)?
    print(value.values)?
    print(value.suffix)?
    Ok(())
"#,
        "4\nOption[range].Some(range(4, 9, 1))\n9\n",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn returned_generator_preserves_its_frame_and_cleanup() {
    bounded(
        r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
def values() -> Generator[i64]:
    guard = Guard("drop")
    yield 42
def factory() -> Generator[i64]:
    values()
def outer_factory() -> Generator[i64]:
    factory()
def main() -> Result[(), Failure]:
    mut generator = outer_factory()
    print(next(generator))?
    drop(generator)
    Ok(())
"#,
        "Option[i64].Some(42)\ndrop\n",
    );
}

#[test]
fn returned_closure_relocates_nested_captures_and_drops_them_once() {
    let output = support::run(
        r#"
class Guard:
    value: str
    def __del__(self) -> ():
        print(self.value).unwrap()
def make() -> Closure[[], i64]:
    guard = Guard("closure cleanup")
    values = Some(range(3, 8))
    def [guard, values]() -> i64:
        len(values.unwrap()) + len(guard.value)
def forward() -> Closure[[], i64]:
    make()
def outer() -> Closure[[], i64]:
    forward()
callback = outer()
print(callback()).unwrap()
drop(callback)
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "20\nclosure cleanup\n"
    );
}

#[cfg(all(target_os = "linux", feature = "runtime-checks"))]
#[test]
fn inline_result_transfer_needs_no_heap_allocation() {
    bounded(r#"
def recurse(n: i64) -> Option[range[i64]]:
    if n == 0:
        return Some(range(4, 7))
    recurse(n - 1)
def main() -> Result[(), Failure]:
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    result = recurse(300000)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(result)?
    Ok(())
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\nOption[range].Some(range(4, 7, 1))\n");
}

#[test]
fn owned_inline_arguments_use_explicit_ordinary_fallback_without_losing_storage() {
    let output = support::run(
        r#"
class Guard:
    value: str
    def __del__(self) -> ():
        print(self.value).unwrap()
class Parcel:
    values: range[i64]
    guard: Guard
def receive(p: Parcel, r: range[i64]) -> i64:
    print(p.values).unwrap()
    print(r).unwrap()
    42
def direct(p: Parcel) -> i64:
    guard = Guard("direct cleanup")
    receive(p, range(6, 8))
def indirect(p: Parcel) -> i64:
    guard = Guard("indirect cleanup")
    f = receive
    f(p, range(6, 8))
print(direct(Parcel(range(2, 5), Guard("argument")))).unwrap()
print(indirect(Parcel(range(2, 5), Guard("argument")))).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "direct cleanup\nrange(2, 5, 1)\nrange(6, 8, 1)\nargument\n42\nindirect cleanup\nrange(2, 5, 1)\nrange(6, 8, 1)\nargument\n42\n");
}
