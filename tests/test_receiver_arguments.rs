mod support;

fn run(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

const CELL: &str = r#"
class Cell:
    value: i64
    def read(self) -> i64:
        self.value
    def view(self) -> &i64:
        &self.value
    def set(self: &mut Cell, value: i64) -> ():
        self.value = value
    def add[T: IntType](self: &mut Cell, value: T) -> ():
        self.value = self.value + i64(value)
    def from_ref(self: &mut Cell, value: &i64) -> ():
        self.value = *value
    def from_ref_generic[T](self: &mut Cell, value: &i64, ignored: T) -> ():
        self.value = *value
    def bump(self: &mut Cell) -> i64:
        self.value += 1
        self.value
"#;

#[test]
fn bytes_fill_reads_exclusive_receiver_length() {
    run(
        r#"
import std.bytes
from std.bytes import Bytes, BytesError
def clear(buffer: &mut Bytes) -> Result[(), BytesError]:
    buffer.fill(0, buffer.len(), 0u8)
def main() -> Result[(), AllocError | BytesError | IoError]:
    mut buffer = std.bytes.from_list([1u8, 2u8]?)?
    clear(&mut buffer)?
    print(buffer.to_list()?)?
    Ok(())
"#,
        "[0, 0]\n",
    );
}

#[test]
fn owned_reborrowed_and_generic_receivers_allow_completed_reads() {
    run(
        &format!(
            r#"{CELL}
def twice(cell: &mut Cell) -> ():
    alias = &mut cell
    alias.set(alias.read() + alias.read())
def main() -> ():
    mut cell = Cell(2)
    cell.set(cell.read() + 1)
    cell.add(cell.read())
    cell.add[i64](cell.read())
    cell.set(*cell.view())
    twice(&mut cell)
    print(cell.value).unwrap()
"#
        ),
        "24\n",
    );
}

#[test]
fn receiver_and_arguments_keep_source_order_and_temporary_drop() {
    run(
        r#"
class Cell:
    value: i64
    def __del__(self) -> ():
        print("drop").unwrap()
    def set(self: &mut Cell, a: i64, b: i64) -> ():
        print(a * 10 + b).unwrap()
def receiver() -> Cell:
    print("receiver").unwrap()
    Cell(0)
def argument(n: i64) -> i64:
    print(n).unwrap()
    n
def main() -> ():
    receiver().set(argument(1), argument(2))
"#,
        "receiver\n1\n2\n12\ndrop\n",
    );
}

#[test]
fn projections_and_nested_reads_keep_the_reserved_place() {
    run(
        &format!(
            r#"{CELL}
class Pair:
    first: Cell
    second: Cell
def identity(n: i64) -> i64:
    n
def main() -> ():
    mut pair = Pair(Cell(4), Cell(9))
    pair.first.set(identity(pair.first.read()))
    pair.first.set(pair.second.bump())
    print(pair.first.value).unwrap()
"#
        ),
        "10\n",
    );
}

#[test]
fn boxed_and_borrowed_field_receivers_support_generic_arguments() {
    run(
        &format!(
            r#"{CELL}
class Pair:
    first: Cell
    second: Cell
def update(pair: &mut Pair) -> ():
    pair.first.add(pair.first.read())
    pair.first.add[i64](pair.first.read())
def main() -> ():
    mut cell = Box(Cell(3)).unwrap()
    cell.set(cell.read() + 1)
    cell.add(cell.read())
    cell.add[i64](cell.read())
    mut pair = Pair(Cell(2), Cell(0))
    update(&mut pair)
    print(cell.value, pair.first.value).unwrap()
"#
        ),
        "16 8\n",
    );
}

#[test]
fn explicit_generic_method_preserves_generator_specialization() {
    run(
        r#"
class Cell:
    value: i64
    def read(self) -> i64:
        self.value
    def add[T: IntType](self: &mut Cell, values: Generator[i64], extra: T) -> ():
        for value in values:
            self.value += value
        self.value += i64(extra)
def values() -> Generator[i64]:
    yield 2
def main() -> ():
    mut cell = Cell(3)
    cell.add[i64](values(), cell.read())
    print(cell.value).unwrap()
"#,
        "8\n",
    );
}

#[test]
fn argument_references_and_mutations_cannot_overlap_receiver() {
    for call in [
        "cell.from_ref(cell.view())",
        "cell.from_ref(&cell.value)",
        "cell.from_ref_generic(cell.view(), 0)",
        "cell.from_ref_generic[i64](cell.view(), 0)",
        "cell.set(cell.bump())",
        "cell.add(cell.bump())",
        "cell.add[i64](cell.bump())",
    ] {
        let source = format!("{CELL}\ndef main() -> ():\n    mut cell = Cell(1)\n    {call}\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("conflicting borrow"), "{call}: {error}");
    }
}

#[test]
fn arguments_cannot_move_receiver_or_extend_returned_mutable_borrow() {
    for source in [
        format!("{CELL}\ndef consume(cell: Cell) -> i64:\n    cell.value\ndef main() -> ():\n    mut cell = Cell(1)\n    cell.set(consume(cell))\n"),
        "class Cell:\n    value: i64\n    def select(self: &mut Cell, ignored: i64) -> &mut i64:\n        &mut self.value\n    def read(self) -> i64:\n        self.value\ndef main() -> ():\n    mut cell = Cell(1)\n    reference = cell.select(cell.read())\n    print(cell.read()).unwrap()\n    *reference = 2\n".to_owned(),
    ] {
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("conflicting borrow"), "{error}");
    }
}

#[test]
fn early_argument_failure_drops_temporary_receiver_once() {
    run(
        r#"
class Cell:
    value: i64
    def __del__(self) -> ():
        print("drop").unwrap()
    def set(self: &mut Cell, value: i64) -> ():
        print("called").unwrap()
def fail() -> Result[i64, str]:
    Err("stop")
def attempt() -> Result[(), str]:
    Cell(1).set(fail()?)
    Ok(())
def main() -> ():
    drop(attempt())
    print("done").unwrap()
"#,
        "drop\ndone\n",
    );
}
