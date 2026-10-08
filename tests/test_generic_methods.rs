mod support;
use support::{check_source, run};

#[test]
fn explicit_method_arguments_support_output_only_types_and_temporaries() {
    let output = run(r#"
class Number:
    value: i64
    def convert[T: IntType](self) -> T:
        T(self.value)
    def borrow[T](self, unused: T) -> &i64:
        &self.value
def convert[T: IntType](number: &Number) -> T:
    number.convert[T]()
def main() -> Result[(), Failure]:
    number = Number(7)?
    value: u8 = number.convert[u8]() + 1
    print(value)?
    print((Number(9)?).convert[u16]())?
    print(convert[u32](&number))?
    borrowed = number.borrow[u8](1)
    print(*borrowed)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "8\n9\n7\n7\n");
}

#[test]
fn method_type_arguments_do_not_change_callable_field_indexing() {
    let output = run("class Table:\n    callbacks: list[Callable[[i64], i64]]\ndef double(x: i64) -> i64:\n    x * 2\ndef main() -> Result[(), Failure]:\n    table = Table([double]?)?\n    index = 0\n    print(table.callbacks[index](7))?\n    Ok(())\n");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "14\n");
}

#[test]
fn explicit_method_arguments_keep_borrow_conflicts() {
    let error = check_source("class Cell:\n    value: i64\n    def borrow[T](self, unused: T) -> &i64:\n        &self.value\ndef main() -> ():\n    mut cell = Cell(1).unwrap()\n    reference = cell.borrow[u8](1)\n    cell.value = 2\n    print(*reference).unwrap()\n").unwrap_err().to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn methods_infer_parameters_and_callback_bounds() {
    let output = run(r#"
class Cell[T]:
    value: T
    def map[U, F: Callable[[T], U]](self, f: &F) -> Result[Cell[U], AllocError]:
        Cell(f(self.value))
class Counter:
    value: i64
    def add[T: IntType](self: &mut Counter, value: T) -> ():
        self.value = self.value + i64(value)
def double(value: u8) -> u16:
    u16(value) * 2
def main() -> Result[(), Failure]:
    cell = Cell(7u8)?
    callback = double
    print(cell.map(&callback)?)?
    mut counter = Counter(1)?
    counter.add(2u8)
    counter.add(3u16)
    print(counter.value)?
    print((Cell(4u8)?).map(&callback)?)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Cell[u16](value=14)\n6\nCell[u16](value=8)\n"
    );
}

#[test]
fn method_generics_preserve_borrow_and_declaration_rules() {
    for (source, expected) in [
        ("class Cell[T]:\n    value: T\n    def same[T](self, x: T) -> T:\n        x\n", "cannot shadow"),
        ("class Cell:\n    value: i64\n    def __init__[T](self, x: T) -> ():\n        self.value = 1\n", "lifecycle methods"),
        ("class Cell:\n    value: i64\n    def borrow[T](self, unused: T) -> &i64:\n        &self.value\ndef main() -> ():\n    value = Cell(1).unwrap().borrow(2)\n    print(*value).unwrap()\n", "named receiver"),
        ("class Cell:\n    value: i64\n    def add[T: IntType](self: &mut Cell, x: T) -> ():\n        self.value = self.value + i64(x)\ndef main() -> ():\n    cell = Cell(1).unwrap()\n    cell.add(2u8)\n", "mutable"),
    ] {
        let source = if source.contains("def main") { source.to_owned() } else { format!("{source}def main() -> ():\n    pass\n") };
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}
