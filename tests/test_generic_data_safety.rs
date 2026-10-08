mod support;
use support::{check_source, run};

#[test]
fn concrete_nominal_names_do_not_become_method_parameters() {
    let output = run(r#"
class U:
    value: i64
class Wrapper[T]:
    value: T
    def borrowed[U](self, unused: U) -> &T:
        &self.value
class Factory[T]:
    def make[U](self, unused: U, value: i64) -> Result[T, AllocError]:
        T(value)
def main() -> Result[(), Failure]:
    wrapper = Wrapper(U(9)?)?
    reference = wrapper.borrowed(1u8)
    print(reference.value)?
    factory = Factory[U]()?
    print(factory.make(1u8, 10)?)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9\nU(value=10)\n");
}

#[test]
fn generic_c_handles_require_an_explicit_nongeneric_facade() {
    let error = check_source("class Cell[T]:\n    value: T\nexport def make(value: u8) -> Result[Cell[u8], AllocError] = \"make_cell\":\n    Cell(value)\ndef main() -> ():\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("nongeneric facade"), "{error}");
}

#[test]
fn validation_placeholders_do_not_instantiate_unused_method_bodies() {
    check_source(
        r#"
class Reader:
    def read(self) -> i64:
        1
class Wrapper[T]:
    value: T
    def read(self) -> i64:
        self.value.read()
protocol Source[T]:
    def make(self) -> Wrapper[T]:
        pass
def unused[T, F: Callable[[], Wrapper[T]]](f: F) -> ():
    pass
def main() -> ():
    wrapper = Wrapper(Reader().unwrap()).unwrap()
    print(wrapper.read()).unwrap()
"#,
    )
    .unwrap();
}

#[test]
fn return_only_instances_register_their_drop_methods() {
    let output = run("class Resource[T]:\n    value: T\n    def __del__(self) -> ():\n        write_stdout(\"drop\\n\").unwrap()\n        pass\ndef missing[T](value: T) -> Result[Resource[T], AllocError]:\n    Err(AllocError.OutOfMemory)\ndef main() -> ():\n    print(missing(1u8)).unwrap()\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Result[Resource[u8], AllocError].Err(AllocError.OutOfMemory)\n"
    );
}

#[test]
fn generic_initialization_failure_drops_only_live_fields_and_arguments() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def fail() -> Result[(), AllocError]:
    Err(AllocError.OutOfMemory)
class Pair[T]:
    first: T
    second: T
    def __init__(self, first: T, second: T) -> Result[(), AllocError]:
        self.first = first
        fail()?
        self.second = second
        Ok(())
    def __del__(self) -> ():
        write_stdout("pair drop\n").unwrap()
        pass
def main() -> Result[(), Failure]:
    result = Pair(Resource(1)?, Resource(2)?)
    print(result)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "2\n1\nResult[Pair[Resource], AllocError].Err(AllocError.OutOfMemory)\n"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn generic_dispatch_moves_generators_and_drop_need_no_extra_allocation() {
    let output = run(r#"
protocol Readable[T]:
    def read(self) -> T:
        pass
class Cell[T]:
    value: T
    def read(self) -> T:
        self.value
    def convert[U: IntType](self) -> U:
        U(self.value)
def read[T, R: Readable[T]](source: &R) -> T:
    source.read()
def values(cell: Cell[u8]) -> Generator[u8]:
    yield cell.read()
def main() -> ():
    cell = Cell(7u8).unwrap()
    print("__test_begin_no_allocations__").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    value = read(&cell)
    converted = cell.convert[u16]()
    wrapped = Some(cell)
    source = values(wrapped.unwrap())
    mut answer = u16(value) + converted
    for n in source:
        answer = answer + u16(n)
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    print(answer).unwrap()
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n21\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_generic_data_allocation_reclaims_transferred_arguments() {
    let output = run(r#"
class Resource:
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
class Holder[T]:
    value: T
enum Packet[T]:
    Value(T)
def main() -> ():
    first = Resource().unwrap()
    second = Resource().unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    a = Holder(first)
    b = Packet[Resource].Value(second)
    print("__test_restore_allocations__").unwrap()
    print(a).unwrap()
    print(b).unwrap()
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "__test_fail_allocations_after_0__\ndrop\ndrop\n__test_restore_allocations__\nResult[Holder[Resource], AllocError].Err(AllocError.OutOfMemory)\nResult[Packet[Resource], AllocError].Err(AllocError.OutOfMemory)\n");
}

#[test]
fn data_specialization_and_recursive_layouts_have_bounded_diagnostics() {
    let mut source = String::from("enum Marker[T]:\n    Empty\n");
    for n in 0..257 {
        let args = (0..9)
            .map(|bit| if n & (1 << bit) == 0 { "u8" } else { "u16" })
            .collect::<Vec<_>>()
            .join(", ");
        source.push_str(&format!("type M{n} = Marker[tuple[{args}]]\n"));
    }
    source.push_str("def main() -> ():\n    pass\n");
    let error = check_source(&source).unwrap_err().to_string();
    assert!(error.contains("specialization limit of 256"), "{error}");
    let error = check_source(
        "class Recursive[T]:\n    value: Recursive[list[T]]\ndef main() -> ():\n    pass\n",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("recursive"), "{error}");
}
