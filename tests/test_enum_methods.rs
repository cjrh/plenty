mod support;

fn runs(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn shared_mutable_and_temporary_enum_receivers() {
    runs(
        r#"
enum Count:
    Value(i64)
    Empty
    def read(self) -> i64:
        match self:
            case Count.Value(n):
                *n
            case Count.Empty:
                0
    def increment(self: &mut Count, amount: i64) -> ():
        match self:
            case Count.Value(n):
                *n = *n + amount
            case Count.Empty:
                pass
def main() -> ():
    mut count = Count.Value(3)
    count.increment(4)
    print(count.read()).unwrap()
    print(Count.Value(9).read()).unwrap()
    reference = &count
    print(reference.read()).unwrap()
"#,
        "7\n9\n7\n",
    );
}

#[test]
fn generic_enums_method_parameters_defaults_and_nested_calls() {
    runs(
        r#"
enum Value[T]:
    Present(T)
    def echo[U](self, value: U) -> U:
        value
    def answer(self, extra: i64 = 2) -> i64:
        self.echo(40 + extra)
def main() -> ():
    value = Value[i64].Present(8)
    print(value.echo[str]("yes")).unwrap()
    print(value.answer()).unwrap()
    print(Value[str].Present("x").answer(extra=3)).unwrap()
"#,
        "yes\n42\n43\n",
    );
}

#[test]
fn enums_satisfy_shared_and_mutable_protocols() {
    runs(
        r#"
protocol Readable:
    def read(self) -> i64:
        pass
protocol Mutable:
    def set(self: &mut Mutable, n: i64) -> ():
        pass
enum Value[T]:
    Present(T)
    def read(self) -> i64:
        12
    def set(self: &mut Value, n: i64) -> ():
        pass
def read[T: Readable](value: &T) -> i64:
    value.read()
def update[T: Mutable](value: &mut T) -> ():
    value.set(7)
def main() -> ():
    mut value = Value[str].Present("message")
    update(&mut value)
    print(read(&value)).unwrap()
    print(read[Value[str]](&value)).unwrap()
"#,
        "12\n12\n",
    );
}

#[test]
fn parameterized_protocol_inference_uses_enum_methods() {
    runs(
        r#"
protocol Source[T]:
    def get(self) -> T:
        pass
enum SourceValue:
    Value(i64)
    def get(self) -> i64:
        match self:
            case SourceValue.Value(value):
                *value
def read[T, S: Source[T]](source: &S) -> T:
    source.get()
def main() -> ():
    value = SourceValue.Value(19)
    print(read(&value)).unwrap()
"#,
        "19\n",
    );
}

#[test]
fn invalid_enum_methods_have_actionable_diagnostics() {
    for (source, expected) in [
        (
            "enum E:\n    V\n    def V(self) -> ():\n        pass\n",
            "duplicate enum member",
        ),
        (
            "enum E:\n    def V(self) -> ():\n        pass\n    V\n",
            "duplicate variant",
        ),
        (
            "enum E:\n    V\n    def __del__(self: &mut E) -> ():\n        pass\n",
            "lifecycle hooks",
        ),
        (
            "enum E:\n    V\n    def read(self: E) -> ():\n        pass\n",
            "method receiver",
        ),
        (
            "enum E:\n    V\n    def read() -> ():\n        pass\n",
            "method receiver",
        ),
        (
            "enum E[T]:\n    V(T)\n    def read[T](self) -> ():\n        pass\n",
            "cannot shadow enum",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

#[test]
fn empty_variant_parentheses_are_reported_at_the_variant() {
    let error = support::check_source("enum E:\n    A()\n    B\n")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("2:5: declare a nullary variant without parentheses"),
        "{error}"
    );
}

#[test]
fn protocol_checks_reject_missing_wrong_and_method_generic_signatures() {
    let prefix = "protocol Readable:\n    def read(self) -> i64:\n        pass\ndef read[T: Readable](value: &T) -> ():\n    pass\n";
    for (method, expected) in [
        ("", "missing method `read`"),
        (
            "    def read(self) -> str:\n        'bad'\n",
            "signature of `read`",
        ),
        (
            "    def read(self: &mut E) -> i64:\n        1\n",
            "signature of `read`",
        ),
        (
            "    def read[T](self) -> i64:\n        1\n",
            "signature of `read`",
        ),
    ] {
        let source = format!("{prefix}enum E:\n    V\n{method}value = E.V\nread(&value)\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn mutable_method_cannot_use_shared_receiver() {
    let error = support::check_source(
        r#"
enum E:
    V
    def update(self: &mut E) -> ():
        pass
value = E.V
reference = &value
reference.update()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("mutable") || error.contains("shared"),
        "{error}"
    );
}

#[test]
fn collection_named_methods_dispatch_on_named_and_temporary_enums() {
    runs(
        r#"
enum E:
    V
    def unwrap(self, value: i64) -> i64:
        value
    def unwrap_or(self, value: i64) -> i64:
        value
    def ok_or(self, value: i64) -> i64:
        value
    def append(self, value: i64) -> i64:
        value
    def reserve(self, value: i64) -> i64:
        value
    def add(self, value: i64) -> i64:
        value
    def insert(self, value: i64) -> i64:
        value
def main() -> ():
    value = E.V
    print(value.unwrap(1)).unwrap()
    print(E.V.unwrap(2)).unwrap()
    print(E.V.unwrap_or(3)).unwrap()
    print(E.V.ok_or(4)).unwrap()
    print(E.V.append(5)).unwrap()
    print(E.V.reserve(6)).unwrap()
    print(E.V.add(7)).unwrap()
    print(E.V.insert(8)).unwrap()
"#,
        "1\n2\n3\n4\n5\n6\n7\n8\n",
    );
}

#[test]
fn enum_methods_preserve_transitive_worker_effect_checks() {
    let error = support::check_source(
        r#"
def seven() -> i64:
    7
enum E:
    V
    def run(self) -> i64:
        callback: Callable[[], i64] = seven
        callback()
def worker(value: E) -> i64:
    value.run()
with spawn(worker, E.V).unwrap():
    pass
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("worker") && error.contains("indirect callable"),
        "{error}"
    );
}

#[test]
fn receiver_reference_return_holds_the_original_loan() {
    let prefix = r#"
enum E:
    V(list[i64])
    def view(self) -> &E:
        self
    def update(self: &mut E) -> ():
        pass
mut value = E.V([1].unwrap())
reference = value.view()
"#;
    for action in ["value.update()", "moved = value"] {
        let source = format!("{prefix}{action}\nprint(reference).unwrap()\n");
        let directory = tempfile::tempdir().unwrap();
        let error =
            support::compile_source_to_executable(&source, &directory.path().join("program"))
                .expect_err(&source)
                .to_string();
        assert!(error.contains("borrow"), "{source}\n{error}");
    }
    runs(
        &format!("{prefix}print(reference).unwrap()\n"),
        "E.V([1])\n",
    );
}

#[test]
fn imported_generic_enum_visibility_and_private_helper_calls() {
    let directory = tempfile::tempdir().unwrap();
    let module = directory.path().join("values.plenty");
    let main = directory.path().join("main.plenty");
    std::fs::write(
        module,
        r#"
pub enum Value[T]:
    Present(T)
    def helper(self) -> i64:
        42
    pub def read(self) -> i64:
        self.helper()
"#,
    )
    .unwrap();
    std::fs::write(&main, "from values import Value\ndef main() -> ():\n    value = Value[str].Present('x')\n    print(value.read()).unwrap()\n").unwrap();
    let binary = directory.path().join("program");
    plenty::compile_file_to_executable(&main, &binary, None).unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    std::fs::write(&main, "from values import Value\ndef main() -> ():\n    value = Value[str].Present('x')\n    print(value.helper()).unwrap()\n").unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(error.contains("private to its defining module"), "{error}");
}

#[test]
fn private_enum_cannot_satisfy_foreign_module_bound() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("reader.plenty"),
        r#"
pub protocol Readable:
    def read(self) -> i64:
        pass
pub def read[T: Readable](value: &T) -> i64:
    value.read()
"#,
    )
    .unwrap();
    let main = directory.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
from reader import read
enum Hidden:
    V
    pub def read(self) -> i64:
        1
def main() -> ():
    value = Hidden.V
    print(read(&value)).unwrap()
"#,
    )
    .unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(
        error.contains("enum `Hidden` is private") && error.contains("declare it `pub enum`"),
        "{error}"
    );
}

#[test]
fn temporary_enum_method_drops_managed_payload_once() {
    runs(
        r#"
class Resource:
    name: str
    def __del__(self: &mut Resource) -> ():
        print(self.name).unwrap()
enum Event:
    Data(Resource)
    def read(self) -> i64:
        7
    def append(self, value: i64) -> i64:
        value
def main() -> ():
    print(Event.Data(Resource("first")).read()).unwrap()
    print(Event.Data(Resource("second")).append(9)).unwrap()
"#,
        "7\nfirst\n9\nsecond\n",
    );
}
