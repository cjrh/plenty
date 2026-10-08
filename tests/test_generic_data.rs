mod support;
use support::{check_source, run};

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
        case Choice[T].Value(value):
            value
def same[T](left: &Pair[T, T], right: T) -> T:
    right
def main() -> Result[(), Failure]:
    pair = Pair[u8, str](4, "four")?
    print(key(&pair))?
    choice = Choice[list[i64]].Value([2, 3]?)?
    print(unpack(choice))?
    equal = Pair[u16, u16](1, 2)?
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
        let source = format!("class Pair[A, B]:\n    a: A\n    b: B\nclass Other[A, B]:\n    a: A\n    b: B\ndef require[T](value: &Pair[T, T]) -> ():\n    pass\ndef main() -> ():\n    value = {actual}.unwrap()\n    require(&value)\n");
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
    Cell[T](value)
def main() -> Result[(), Failure]:
    mut cell = Cell[u8](5)?
    cell.replace(9)
    print(cell.get())?
    text = wrap[str]("hello")?
    print(text.get())?
    logged = Logged[list[i64]]([1, 2]?)?
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
def main() -> Result[(), Failure]:
    value = ByteChoice.Value(7)?
    match value:
        case Choice[u8].Empty:
            print(0)?
        case Choice[u8].Value(n):
            print(n)?
    nested = Envelope.Item(Choice[list[i64]].Value([2, 4]?)?)?
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
        ("value: Choice[u8] = Choice[i64].Value(1).unwrap()", "expected"),
        ("value: Choice = Choice[u8].Value(1).unwrap()", "requires 1 type arguments"),
        ("value: Choice[&i64] = Choice[&i64].Empty.unwrap()", "cannot contain references"),
        ("value = Choice[list[i64]].Value([1].unwrap()).unwrap()\n    drop(value)\n    drop(value)", "moved"),
    ] {
        let source = format!("enum Choice[T]:\n    Empty\n    Value(T)\ndef main() -> ():\n    {body}\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}
