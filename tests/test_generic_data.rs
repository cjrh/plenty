mod support;
use support::{check_source, run};

#[test]
fn integer_constraints_apply_to_class_and_enum_arguments() {
    let output = run(
        "class Counter[T: IntType]:\n    value: T\nenum Integer[T: IntType]:\n    Value(T)\ndef main() -> ():\n    print(Counter(3u8)).unwrap()\n    print(Integer[u16].Value(4)).unwrap()\n",
    );
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Counter[u8](value=3)\nInteger[u16].Value(4)\n"
    );
    for declaration in [
        "class Value[T: IntType]:\n    item: T\n",
        "enum Value[T: IntType]:\n    Item(T)\n",
    ] {
        let error = check_source(&format!(
            "{declaration}type Bad = Value[str]\ndef main() -> ():\n    pass\n"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("does not satisfy IntType"), "{error}");
    }
}

#[test]
fn constructors_infer_from_fields_and_explicit_initializers() {
    let output = run(r#"
class Pair[A, B]:
    a: A
    b: B
class Sized[T]:
    values: list[T]
    def __init__(self, value: T, count: u8) -> Result[(), AllocError]:
        self.values = [value for n in range(count)]?
        Ok(())
def main() -> Result[(), AllocError | IoError]:
    pair = Pair(7u8, "seven")
    print(pair)?
    sized = Sized(9u16, 2)?
    print(sized)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Pair[u8, str](a=7, b=\"seven\")\nSized[u16](values=[9, 9])\n"
    );
}

#[test]
fn constructor_inference_requires_all_parameters_and_matching_evidence() {
    for (declaration, call, expected) in [
        (
            "class Pair[T]:\n    a: T\n    b: T\n",
            "Pair(1u8, 2u16)",
            "conflicting types",
        ),
        (
            "class Marker[T]:\n    n: i64\n",
            "Marker(1)",
            "cannot infer type parameter",
        ),
    ] {
        let error = check_source(&format!(
            "{declaration}def main() -> ():\n    value = {call}.unwrap()\n"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn function_inference_reads_nominal_data_arguments() {
    let output = run(r#"
class Pair[K, V]:
    key: K
    value: V
enum Choice[T]:
    Value(T)
def key[K, V](pair: &Pair[K, V]) -> K:
    pair.key
def unpack[T](choice: Choice[T]) -> T:
    match choice:
        case Choice.Value(value):
            value
def same[T](left: &Pair[T, T], right: T) -> T:
    right
def main() -> Result[(), AllocError | IoError]:
    pair = Pair[u8, str](4, "four")
    print(key(&pair))?
    choice = Choice[list[i64]].Value([2, 3]?)
    print(unpack(choice))?
    equal = Pair[u16, u16](1, 2)
    print(same(&equal, 7u16))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n[2, 3]\n7\n");
}

#[test]
fn inference_rejects_conflicting_and_different_nominal_arguments() {
    for (actual, expected) in [
        ("Pair[u8, u16](1, 2)", "conflicting types"),
        ("Other[u8, u8](1, 2)", "does not match"),
    ] {
        let source = format!(
            "class Pair[A, B]:\n    a: A\n    b: B\nclass Other[A, B]:\n    a: A\n    b: B\ndef require[T](value: &Pair[T, T]) -> ():\n    pass\ndef main() -> ():\n    value = {actual}\n    require(&value)\n"
        );
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn generic_classes_specialize_fields_methods_and_lifecycle() {
    let output = run(r#"
class Cell[T]:
    value: T
    def get(self) -> T:
        self.value
    def replace(self: &mut Cell[T], value: T) -> ():
        self.value = value
class Logged[T]:
    value: T
    def __init__(self, value: T) -> ():
        self.value = value
    def __del__(self) -> ():
        print(self.value).unwrap()
def wrap[T](value: T) -> Result[Cell[T], AllocError]:
    Ok(Cell[T](value))
def main() -> Result[(), AllocError | IoError]:
    mut cell = Cell[u8](5)
    cell.replace(9)
    print(cell.get())?
    text = wrap[str]("hello")?
    print(text.get())?
    logged = Logged[list[i64]]([1, 2]?)
    drop(logged)
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "9\nhello\n[1, 2]\n"
    );
}

#[test]
fn concrete_generic_enums_construct_match_and_nest() {
    let output = run(r#"
enum Choice[T]:
    Empty
    Value(T)
type ByteChoice = Choice[u8]
enum Envelope:
    Item(Choice[list[i64]])
def main() -> Result[(), AllocError | IoError]:
    value = ByteChoice.Value(7)
    match value:
        case Choice.Empty:
            print(0)?
        case Choice.Value(n):
            print(n)?
    nested = Envelope.Item(Choice[list[i64]].Value([2, 4]?))
    print(nested)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "7\nEnvelope.Item(Choice[list[i64]].Value([2, 4]))\n"
    );
}

#[test]
fn generic_enum_instances_remain_nominal_and_owned() {
    for (body, expected) in [
        ("value: Choice[u8] = Choice[i64].Value(1)", "expected"),
        (
            "value: Choice = Choice[u8].Value(1)",
            "requires 1 type arguments",
        ),
        (
            "value: Choice[&i64] = Choice[&i64].Empty",
            "references cannot be stored in generic data",
        ),
        (
            "value = Choice[list[i64]].Value([1].unwrap())\n    drop(value)\n    drop(value)",
            "moved",
        ),
    ] {
        let source = format!(
            "enum Choice[T]:\n    Empty\n    Value(T)\ndef main() -> ():\n    {body}\n    pass\n"
        );
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn class_instance_method_errors_are_reported_at_the_requiring_use() {
    let cell = "class Cell[T]:
    value: T
    def get(self) -> T:
        self.value
";
    let note = "\n  4:9: note: in instantiation `Cell[list[i64]]`: cannot move out of a field; \
        use copy(value.field).unwrap(), &value.field (or &mut value.field), or \
        replace(value.field, replacement) on a mutable owner";
    // All methods of an instance are checked when the instance is first
    // required, whether or not the failing one is called.
    for (code, at) in [
        (
            "def main() -> Result[(), AllocError]:\n    a = Cell(1)\n    b = Cell([1, 2]?)\n    Ok(())\n",
            "8:9",
        ),
        (
            "def main() -> Result[(), AllocError]:\n    b = Cell[list[i64]]([1, 2]?)\n    b.get()\n    Ok(())\n",
            "7:9",
        ),
        (
            "def size(cell: &Cell[list[i64]]) -> i64:\n    1\n\ndef main() -> ():\n    pass\n",
            "6:17",
        ),
    ] {
        let error = plenty::check_source(&format!("{cell}\n{code}"))
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            format!("{at}: cannot instantiate `Cell[list[i64]]`: its definition is rejected{note}")
        );
    }
    // An instance required by another declaration is attributed to the use
    // that required the outer one.
    let error = plenty::check_source(&format!(
        "{cell}
class Pair[T]:
    left: Cell[T]
    right: Cell[T]

def wrap[T](left: T, right: T) -> Pair[T]:
    Pair(Cell(left), Cell(right))

def main() -> Result[(), AllocError]:
    a = wrap(1, 2)
    b = wrap([1]?, [2]?)
    Ok(())
"
    ))
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        format!("15:9: cannot instantiate `wrap[list[i64]]`: its definition is rejected{note}")
    );
}
