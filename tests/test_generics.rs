mod support;

#[test]
fn explicit_functions_specialize_numeric_owned_and_borrowed_values() {
    let out = support::run(
        r#"
def identity[T](value: T) -> T:
    value
def add[T: IntType](a: T, b: T) -> T:
    a + b
def total[T: IntType](stop: T) -> T:
    mut value: T = 0
    for n in range[T](stop):
        value = value + n
    value
def first[T](values: &list[T]) -> &T:
    &values[0]
def down[T: IntType](n: T) -> T:
    if n == 0:
        return n
    down[T](n - 1)
print(identity[i64](3))
print(identity[str]("hello"))
print(add[u8](2, 3))
print(total[u16](5))
items = identity[list[i64]]([1, 2])
print(first[i64](&items))
print(items)
print(down[i32](1000))
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3\nhello\n5\n10\n1\n[1, 2]\n0\n"
    );
}

#[test]
fn invalid_type_arguments_and_concrete_bodies_are_diagnosed() {
    for source in [
        "def f[T](x: T) -> T:\n    x\nf(1)",
        "def f[T](x: T) -> T:\n    x\nf[i64, i32](1)",
        "def f[T: IntType](x: T) -> T:\n    x\nf[str]('bad')",
        "def f[T](x: T) -> T:\n    x + True\nf[i64](1)",
        "def f[T](x: T) -> T:\n    x\nf[u8](256)",
        "def f[T](x: T) -> T:\n    x\na = [1]\nb = f[list[i64]](a)\nprint(a)",
        "def f[T, T](x: T) -> T:\n    x",
        "def f[T](T: i64) -> i64:\n    T\nf[str](1)",
        "def f[T](x: T) -> T:\n    x\nf = 1\nf[i64](2)",
        "def f[T](x: T) -> ():\n    f[list[T]]([x])\nf[i64](1)",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}
