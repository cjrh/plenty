mod support;
use support::{check_source, run};

#[test]
fn protocols_parameterize_read_and_write_contracts() {
    let output = run(r#"
protocol Readable[T]:
    def read(self) -> T:
        pass
protocol Writable[T]:
    def write(self: &mut Writable[T], value: T) -> ():
        pass
class Cell[T]:
    value: T
    def read(self) -> T:
        self.value
    def write(self: &mut Cell[T], value: T) -> ():
        self.value = value
    def send[S: Writable[T]](self, target: &mut S) -> ():
        target.write(self.value)
def read[T, R: Readable[T]](source: &R) -> T:
    source.read()
def put[T, W: Writable[T]](target: &mut W, value: T) -> ():
    target.write(value)
def main() -> Result[(), Failure]:
    source = Cell(7u8)
    mut target = Cell(1u8)
    source.send(&mut target)
    print(read(&target))?
    put(&mut target, 9u8)
    print(target.read())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n9\n");
}

#[test]
fn protocol_evidence_reaches_a_fixed_point_across_bounds() {
    let output = run(r#"
protocol Readable[T]:
    def read(self) -> T:
        pass
protocol Source[R]:
    def source(self) -> &R:
        pass
class Cell[T]:
    value: T
    def read(self) -> T:
        self.value
class Holder:
    cell: Cell[u8]
    def source(self) -> &Cell[u8]:
        &self.cell
def read[T, R: Readable[T], S: Source[R]](holder: &S) -> T:
    holder.source().read()
def main() -> Result[(), Failure]:
    holder = Holder(Cell(7u8))
    print(read(&holder))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
}

#[test]
fn protocol_inference_rejects_conflicts_and_unobservable_parameters() {
    for (source, expected) in [
        ("protocol Readable[T]:\n    def read(self) -> T:\n        pass\nclass Cell:\n    value: u8\n    def read(self) -> u8:\n        self.value\ndef use[T, R: Readable[T]](source: &R, other: T) -> ():\n    pass\ndef main() -> ():\n    cell = Cell(1)\n    use(&cell, 2u16)\n", "conflicting types"),
        ("protocol Marker[T]:\n    def size(self) -> i64:\n        pass\nclass Cell:\n    def size(self) -> i64:\n        1\ndef use[T, R: Marker[T]](source: &R) -> ():\n    pass\ndef main() -> ():\n    cell = Cell()\n    use(&cell)\n", "cannot infer type parameter"),
    ] {
        let error = check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn protocol_arguments_check_exact_signatures_and_arity() {
    let prefix = "protocol Readable[T]:\n    def read(self) -> T:\n        pass\nclass Cell:\n    value: u8\n    def read(self) -> u8:\n        self.value\n";
    for (function, body, expected) in [
        (
            "def read[R: Readable[u16]](source: &R) -> u16:\n    source.read()\n",
            "cell = Cell(1)\n    read(&cell)",
            "signature",
        ),
        (
            "def read[R: Readable](source: &R) -> ():\n    pass\n",
            "pass",
            "requires 1 type arguments",
        ),
    ] {
        let error = check_source(&format!(
            "{prefix}{function}def main() -> ():\n    {body}\n"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{error}");
    }
}
