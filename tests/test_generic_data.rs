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
    let note = |instance: &str| {
        format!(
            "\n  4:9: note: in instantiation `{instance}`: cannot move out of a field"
        )
    };
    // A method body is checked for an instance only when the method is used
    // (issue #515), so an instance whose `get` would fail is accepted until
    // `get` is called.
    for code in [
        "def main() -> Result[(), AllocError]:\n    a = Cell(1)\n    b = Cell([1, 2]?)\n    Ok(())\n",
        "def size(cell: &Cell[list[i64]]) -> i64:\n    1\n\ndef main() -> ():\n    pass\n",
    ] {
        plenty::check_source(&format!("{cell}\n{code}")).unwrap();
    }
    let error = plenty::check_source(&format!(
        "{cell}\ndef main() -> Result[(), AllocError]:\n    b = Cell[list[i64]]([1, 2]?)\n    c = b.get()\n    Ok(())\n"
    ))
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        format!(
            "8:9: `Cell[list[i64]].get` copies `T` implicitly, but `list[i64]` is affine{}",
            note("Cell[list[i64]].get")
        )
    );
    // A method required by another generic body is attributed to the use
    // that required the outer instance.
    let error = plenty::check_source(&format!(
        "{cell}
def first[T](value: T) -> T:
    Cell(value).get()

def main() -> Result[(), AllocError]:
    a = first(1)
    b = first([1]?)
    Ok(())
"
    ))
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        format!(
            "11:9: `first[list[i64]]` requires `Cell[list[i64]].get`, which copies `T` \
             implicitly, but `list[i64]` is affine{}",
            note("Cell[list[i64]].get")
        )
    );
}

#[test]
fn unused_copying_methods_do_not_restrict_instances() {
    let holder = "class Holder[T]:
    item: T
    def take(self) -> T:
        self.item
    def borrow(self: &Holder[T]) -> &T:
        &self.item

class Guard:
    value: i64
    def __del__(self: &mut Guard) -> ():
        pass
";
    let output = run(&format!(
        "{holder}
def main() -> Result[(), IoError]:
    holder = Holder(Guard(3))
    print(holder.borrow().value)?
    Ok(())
"
    ));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "3\n");
    let error = check_source(&format!(
        "{holder}
def main() -> ():
    holder = Holder(Guard(3))
    guard = holder.take()
"
    ))
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "15:13: `Holder[Guard].take` copies `T` implicitly, but `Guard` is affine
  4:9: note: in instantiation `Holder[Guard].take`: cannot move out of a field"
    );
}

#[test]
fn protocol_conformance_and_destructors_check_unused_methods() {
    let guard = "class Guard:
    def __del__(self: &mut Guard) -> ():
        pass
";
    // A protocol bound requires the matched methods, even when the bounded
    // body never calls them.
    let error = check_source(&format!(
        "{guard}
protocol Sized:
    def size(self) -> i64:
        pass

class Holder[T]:
    item: T
    def size(self) -> i64:
        taken = self.item
        1

def accept[S: Sized](value: &S) -> i64:
    0

def main() -> ():
    holder = Holder(Guard())
    n = accept(&holder)
"
    ))
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "20:9: `accept[Holder[Guard]]` requires `Holder[Guard].size`, which copies `T` \
         implicitly, but `Guard` is affine
  12:17: note: in instantiation `Holder[Guard].size`: cannot move out of a field"
    );
    // Construction arms the destructor, so `__del__` is checked with the instance.
    let error = check_source(&format!(
        "{guard}
class Holder[T]:
    item: T
    def __del__(self: &mut Holder[T]) -> ():
        taken = self.item

def main() -> ():
    holder = Holder(Guard())
"
    ))
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "11:14: `Holder[Guard]` copies `T` implicitly, but `Guard` is affine
  8:17: note: in instantiation `Holder[Guard]`: cannot move out of a field"
    );
}
