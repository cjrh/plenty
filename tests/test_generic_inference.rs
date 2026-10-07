mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn protocols_infer_shared_mutable_and_named_reference_arguments() {
    native(
        r#"
protocol Readable:
    def read(self) -> str:
        pass
class Message:
    text: str
    def read(self) -> str:
        self.text
def read_message[T: Readable](source: &T) -> str:
    source.read()
def replace[T](target: &mut T, value: T) -> ():
    *target = value
def view[T](source: &T) -> &T:
    source
def main() -> Result[(), Failure]:
    message = Message("hello")?
    print(read_message(&message))?
    borrowed = &message
    print(read_message(borrowed))?
    mut value = 1u8
    reference = &mut value
    replace(reference, 7u8)
    print(view(&value))?
    explicit = view[u8](&value)
    print(*explicit)?
    Ok(())
"#,
        "hello\nhello\n7\n7\n",
    );
}

#[test]
fn structural_types_aliases_and_nested_calls_infer_without_return_context() {
    native(r#"
type Count = u16
def identity[T](value: T) -> T:
    value
def first[T](values: &list[T]) -> &T:
    &values[0]
def key[K, V](values: &dict[K, V], k: K) -> &V:
    &values[k]
def pair[A, B](values: tuple[A, B]) -> A:
    a, b = values
    a
def success[T, E](value: Result[T, E]) -> Result[T, E]:
    Ok(value?)
def optional[T](value: Option[T]) -> Option[T]:
    Some(value?)
def count[T: IntType](values: range[T]) -> T:
    mut total: T = 0
    for n in values:
        total = total + n
    total
def main() -> Result[(), Failure]:
    n: Count = 42
    print(identity(identity(n)))?
    values = identity([1, 2]?)
    print(first(&values))?
    mapping = {"a": 7u8}?
    print(key(&mapping, "a"))?
    print(identity({1u8, 2u8}?))?
    print(pair((3u32, "cm")?))?
    r: Result[u64, str] = Ok(18446744073709551615u64)
    print(success(r))?
    print(optional(Some(1u8)))?
    print(count(range[u8](4)))?
    print(identity(2.5))?
    print(1 + identity[u8](2))?
    Ok(())
"#, "42\n1\n7\n{1, 2}\n3\nResult[u64, str].Ok(18446744073709551615)\nOption[u8].Some(1)\n6\n2.5\n3\n");
}

#[test]
fn recursive_specializations_generators_and_scoped_bindings() {
    native(
        r#"
def down[T: IntType](n: T) -> T:
    if n == 0:
        return n
    down(n - 1)
def once[T](value: T) -> Generator[T]:
    yield value
def collect[T](source: Generator[T]) -> Result[list[T], AllocError]:
    [item for item in source]
def identity[T](x: T) -> T:
    x
def main() -> Result[(), Failure]:
    print(down(10000i32))?
    print(collect(once(8u8))?)?
    values = [Some(2u8), Nothing]?
    for value in values:
        match value:
            case Some(n):
                print(identity(n))?
            case Nothing:
                print("missing")?
    print([identity(n) for n in range[u8](3)]?)?
    Ok(())
"#,
        "0\n[8]\n2\nmissing\n[0, 1, 2]\n",
    );
}

#[test]
fn argument_effects_and_cleanup_happen_once_in_source_order() {
    native(
        r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
def make(name: str) -> Result[Guard, Failure]:
    print(name)?
    Ok(Guard(name)?)
def keep[T](a: T, b: T) -> T:
    a
def main() -> Result[(), Failure]:
    guard = keep(make("left")?, make("right")?)
    print("returned")?
    Ok(())
"#,
        "left\nright\nright\nreturned\nleft\n",
    );
}

#[test]
fn inference_does_not_hide_type_conflicts_missing_information_or_invalid_bounds() {
    for (source, expected) in [
        ("def same[T](a: T, b: T) -> T:\n    a\nsame(1u8, 2i64)", "conflicting types for `T`"),
        ("def same[T](a: T, b: T) -> T:\n    a\nsame(1, 2u8)", "conflicting types for `T`"),
        ("def same[T](a: T, b: T) -> T:\n    a\nsame(1u8, 2)", "conflicting types for `T`"),
        ("def make[T]() -> T:\n    1\nx: i64 = make()", "cannot infer type parameter `T`"),
        ("def ignore[T, U](x: T) -> T:\n    x\nignore(1)", "cannot infer type parameter `U`"),
        ("def take[T](values: list[T]) -> ():\n    pass\ntake(1)", "does not match parameter type"),
        ("def number[T: IntType](x: T) -> T:\n    x\nnumber('bad')", "does not satisfy IntType"),
        ("def identity[T](x: T) -> T:\n    x\nidentity = 1\nidentity(2)", "not callable"),
        ("def identity[T](x: T) -> T:\n    x\na = [1].unwrap()\nb = identity(a)\nprint(a).unwrap()", "moved"),
        ("def use[T](x: &mut T, y: &mut T) -> ():\n    pass\nmut n = 1\nuse(&mut n, &mut n)", "borrow"),
        ("def view[T](x: &T) -> &T:\n    x\ndef bad() -> &i64:\n    n = 1\n    view(&n)", "reference parameter"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}
