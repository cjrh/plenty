mod support;
use support::{check_source, run};

#[test]
fn pending_arguments_and_entered_bodies_drop_captures_on_error() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def fail() -> Result[i64, AllocError]:
    Err(AllocError.OutOfMemory)
def make() -> Result[OnceClosure[[Resource, i64], Result[i64, AllocError]], AllocError]:
    first = Resource(1)
    second = Resource(2)
    callback = def once [first, second](argument: Resource, value: i64) -> Result[i64, AllocError]:
        print("entered").unwrap()
        fail()?
        Ok(first.id + second.id + argument.id + value)
    Ok(callback)
def before_entry() -> Result[(), AllocError]:
    make()?(Resource(3), fail()?)?
    Ok(())
def after_entry() -> Result[(), AllocError]:
    make()?(Resource(3), 0)?
    Ok(())
def main() -> Result[(), Failure]:
    print(before_entry())?
    print(after_entry())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "3\n2\n1\nResult[(), AllocError].Err(AllocError.OutOfMemory)\nentered\n3\n2\n1\nResult[(), AllocError].Err(AllocError.OutOfMemory)\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn owned_and_borrowed_consuming_lifecycles_need_no_allocation() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
def make(values: list[i64], resource: Resource) -> OnceClosure[[], list[i64]]:
    def once [values, resource]() -> list[i64]:
        values
def invoke[T, F: OnceCallable[[], T]](callback: F) -> T:
    callback()
def main() -> ():
    values = [7, 8].unwrap()
    resource = Resource(1)
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    wrapped = Some(make(values, resource))
    result = invoke(wrapped.unwrap())
    mut count = 2
    take = def once [result, &mut count]() -> list[i64]:
        count = count + 1
        result
    output = take()
    answer = output[0] + count
    drop(output)
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(answer).unwrap()
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\ndrop\n__test_restore_allocations__\n__test_end_no_allocations__\n10\n");
}

#[test]
fn move_checking_covers_branches_and_repeated_loop_calls() {
    for statement in [
        "if True:\n        f()\n    f()",
        "for n in range(2):\n        f()",
    ] {
        let source = format!("def main() -> ():\n    f = def once () -> i64:\n        1\n    {statement}\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("moved"), "{error}");
    }
}

#[test]
fn uncalled_environments_drop_on_loop_exits() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def main() -> Result[(), Failure]:
    for n in range(3):
        resource = Resource(n)
        callback = def once [resource]() -> Resource:
            resource
        if n == 0:
            continue
        break
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0\n1\n");
}

#[test]
fn callable_bounds_validate_shape_and_explain_storage_constraints() {
    for bound in ["Callable[[()], i64]", "OnceCallable[[()], i64]"] {
        let source =
            format!("def unused[F: {bound}](f: F) -> ():\n    pass\ndef main() -> ():\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("parameters cannot have unit"), "{error}");
    }
    let error = check_source("def invalid(callback: OnceCallable[[], i64]) -> ():\n    pass\ndef main() -> ():\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("generic constraint"), "{error}");
    for constraint in ["Callable", "OnceCallable"] {
        let source = format!("class Private:\n    value: i64\npub def exposed[T, F: {constraint}[[Private], T]](f: F) -> ():\n    pass\ndef main() -> ():\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains("public signature exposes private type"),
            "{error}"
        );
    }
}

#[test]
fn captured_generator_dependencies_have_a_bounded_lowering_depth() {
    let mut source = String::new();
    for i in 0..70 {
        let next = i + 1;
        source.push_str(&format!("def gen{i}() -> Generator[i64]:\n    source = gen{next}()\n    callback = def once [mut source]() -> i64:\n        next(source).unwrap()\n    yield callback()\n"));
    }
    source.push_str("def gen70() -> Generator[i64]:\n    yield 1\ndef main() -> ():\n    pass\n");
    let error = check_source(&source).unwrap_err().to_string();
    assert!(error.contains("nesting exceeds"), "{error}");
}
