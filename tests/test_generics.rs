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
print(identity[i64](3)).unwrap()
print(identity[str]("hello")).unwrap()
print(add[u8](2, 3)).unwrap()
print(total[u16](5)).unwrap()
items = identity[list[i64]]([1, 2].unwrap())
print(first[i64](&items)).unwrap()
print(items).unwrap()
print(down[i32](1000)).unwrap()
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
        "def f[T]() -> T:\n    1\nf()",
        "def f[T](x: T) -> T:\n    x\nf[i64, i32](1)",
        "def f[T: IntType](x: T) -> T:\n    x\nf[str]('bad')",
        "def f[T](x: T) -> T:\n    x + True\nf[i64](1)",
        "def f[T](x: T) -> T:\n    x\nf[u8](256)",
        "def f[T](x: T) -> T:\n    x\na = [1].unwrap()\nb = f[list[i64]](a)\nprint(a).unwrap()",
        "def f[T, T](x: T) -> T:\n    x",
        "def f[T](T: i64) -> i64:\n    T\nf[str](1)",
        "def f[T](x: T) -> T:\n    x\nf = 1\nf[i64](2)",
        "def f[T](x: T) -> ():\n    f[list[T]]([x].unwrap())\nf[i64](1)",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

fn rejection(source: &str) -> String {
    plenty::check_source(source).unwrap_err().to_string()
}

#[test]
fn instance_body_errors_are_reported_at_the_first_requiring_use() {
    // Found by the ownership pass: the body note precedes the pass's own notes.
    assert_eq!(
        rejection(
            "def twice[T](value: T) -> tuple[T, T]:
    (value, value)

def helper() -> Result[(), Failure]:
    a = twice(1)
    b = twice([1]?)
    Ok(())

def main() -> Result[(), Failure]:
    c = twice([2]?)
    helper()
"
        ),
        "6:9: cannot instantiate `twice[list[i64]]`: its definition is rejected
  2:13: note: in instantiation `twice[list[i64]]`: use of moved binding `value`
  2:6: note: `value` is moved here"
    );
    // Found while lowering the body.
    assert_eq!(
        rejection(
            "def add[T](a: T, b: T) -> T:
    a + b

def main() -> ():
    x = add[u8](1, 2)
    y = add[bool](True, False)
"
        ),
        "6:9: cannot instantiate `add[bool]`: its definition is rejected
  2:5: note: in instantiation `add[bool]`: operator `+` does not accept bool"
    );
    // Found by tail-call validation, after every body is lowered.
    assert_eq!(
        rejection(
            "def walk[T](value: T, seen: &i64, n: i64) -> i64:
    if n == 0:
        return 0
    local = n
    walk(value, &local, n - 1)

def main() -> ():
    zero = 0
    x = walk(1u8, &zero, 3)
"
        ),
        "9:9: cannot instantiate `walk[u8]`: its definition is rejected
  5:5: note: in instantiation `walk[u8]`: recursive call to `walk[u8]` is in tail position \
         but cannot be a tail call: the reference argument borrows local `local`, whose storage \
         does not survive the jump (borrow at 5:18)"
    );
}

#[test]
fn nested_instance_errors_name_the_outermost_use_and_the_failing_instance() {
    assert_eq!(
        rejection(
            "def twice[T](value: T) -> tuple[T, T]:
    (value, value)

def middle[T](value: T) -> tuple[T, T]:
    twice(value)

def outer[T](value: T) -> tuple[T, T]:
    middle(value)

def main() -> Result[(), Failure]:
    a = outer(1)
    b = outer([1]?)
    Ok(())
"
        ),
        "12:9: cannot instantiate `outer[list[i64]]`: its definition is rejected
  2:13: note: in instantiation `twice[list[i64]]`: use of moved binding `value`
  2:6: note: `value` is moved here"
    );
}

#[test]
fn generic_method_and_lifted_function_errors_are_reported_at_the_use() {
    assert_eq!(
        rejection(
            "class Holder:
    n: i64
    def both[T](self, value: T) -> tuple[T, T]:
        (value, value)

def main() -> Result[(), Failure]:
    holder = Holder(1)
    a = holder.both(1)
    b = holder.both([1]?)
    Ok(())
"
        ),
        "9:9: cannot instantiate `Holder.both[list[i64]]`: its definition is rejected
  4:17: note: in instantiation `Holder.both[list[i64]]`: use of moved binding `value`
  4:10: note: `value` is moved here"
    );
    // An anonymous function is lowered apart from the instance that contains it.
    assert_eq!(
        rejection(
            "def apply[T](value: T) -> i64:
    f = def(x: i64) -> i64:
        missing + x
    f(1)

def main() -> ():
    n = apply(1)
"
        ),
        "7:9: cannot instantiate `apply[i64]`: its definition is rejected
  3:9: note: in instantiation `apply[i64]`: unknown binding `missing`"
    );
}

#[test]
fn expanding_recursion_keeps_instance_names_readable() {
    let error = rejection(
        "def f[T](x: T) -> Result[(), Failure]:
    f[list[T]]([x]?)

def main() -> Result[(), Failure]:
    f[i64](1)
",
    );
    let nested = format!("f[{}", "list[".repeat(19));
    assert_eq!(
        error,
        format!(
            "5:5: cannot instantiate `f[i64]`: its definition is rejected
  2:16: note: in instantiation `{}...`: type nesting exceeds the implementation limit of 64",
            &nested[..96]
        )
    );
}

#[test]
fn only_checks_of_an_instance_body_move_to_the_use() {
    // A body is checked only per instance, so an error in it that no type
    // argument causes is still found through a use. It is reported like any
    // other, and its note keeps the definition position and message.
    let body = "def keep[T](value: T) -> T:
    missing
    value
";
    plenty::check_source(&format!("{body}\ndef main() -> ():\n    pass\n")).unwrap();
    assert_eq!(
        rejection(&format!("{body}\ndef main() -> ():\n    x = keep(1)\n")),
        "6:9: cannot instantiate `keep[i64]`: its definition is rejected
  2:5: note: in instantiation `keep[i64]`: unknown binding `missing`"
    );
    // Declaration checks need no instance and stay at the definition.
    assert_eq!(
        rejection("def keep[T: Missing](value: T) -> T:\n    value\n\ndef main() -> ():\n    x = keep(1)\n"),
        "1:13: unknown type `Missing`"
    );
    // A use that does not satisfy the declaration never reaches the body.
    assert_eq!(
        rejection("def add[T: IntType](a: T, b: T) -> T:\n    a + b\n\ndef main() -> ():\n    x = add('a', 'b')\n"),
        "5:9: str does not satisfy IntType"
    );
}
