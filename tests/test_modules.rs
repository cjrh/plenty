//! File graph, name binding, privacy, and native execution contracts.
use std::process::Command;

fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    dir
}

#[test]
fn generic_protocol_visibility_uses_definition_scope_and_reports_the_call() {
    for parameterized in [false, true] {
        let api = if parameterized {
            "pub protocol Readable[T]:\n    def read(self) -> T:\n        pass\npub def read[T, R: Readable[T]](source: &R) -> T:\n    source.read()\n"
        } else {
            "pub protocol Readable:\n    def read(self) -> i64:\n        pass\npub def read[R: Readable](source: &R) -> i64:\n    source.read()\n"
        };
        for transitive in [false, true] {
            for (class_visibility, method_visibility, expected) in [
                ("", "pub ", "class `Cell` is private"),
                ("pub ", "", "`Cell.read` is private"),
                ("pub ", "pub ", ""),
            ] {
                let call = if transitive {
                    "bridge.read"
                } else {
                    "api.read"
                };
                let main = format!(
                    "import api\nimport bridge\n{class_visibility}class Cell:\n    value: i64\n    {method_visibility}def read(self) -> i64:\n        self.value\ndef main() -> Result[(), IoError]:\n    cell = Cell(7)\n    print({call}(&cell))?\n    Ok(())\n"
                );
                let files = [
                    ("api.plenty", api),
                    (
                        "bridge.plenty",
                        "import api\npub def read[T](source: &T) -> i64:\n    api.read(source)\n",
                    ),
                    ("main.plenty", main.as_str()),
                ];
                if expected.is_empty() {
                    run(&files, "main.plenty", "7\n");
                } else {
                    let dir = workspace(&files);
                    let error = plenty::check_file(&dir.path().join("main.plenty"), None)
                        .unwrap_err()
                        .to_string();
                    assert!(error.contains(expected), "{error}");
                    assert!(
                        error.lines().next().unwrap().contains("main.plenty:9:"),
                        "{error}"
                    );
                    assert_eq!(
                        error.contains("declare it `pub class`"),
                        class_visibility.is_empty(),
                        "{error}"
                    );
                }
            }
        }
    }
}

#[test]
fn std_io_private_stream_diagnostic_points_to_user_call() {
    let source = "import std.bytes\nimport std.io\nfrom std.bytes import Bytes, BytesError\nfrom std.io import StreamError\nclass Fake:\n    pub def read(self: &mut Fake, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:\n        Ok(0)\ndef main() -> Result[(), AllocError | BytesError]:\n    mut fake = Fake()\n    mut buffer = std.bytes.empty()?\n    result = std.io.read_exact(&mut fake, &mut buffer, 0, 0)\n    Ok(())\n";
    let dir = workspace(&[("main.plenty", source)]);
    let error = plenty::check_file(&dir.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with(&format!("{}:11:", dir.path().join("main.plenty").display())),
        "{error}"
    );
    assert!(error.contains("class `Fake` is private"), "{error}");
    assert!(error.contains("declare it `pub class`"), "{error}");
    let public = source.replace("class Fake:", "pub class Fake:");
    let dir = workspace(&[("main.plenty", &public)]);
    plenty::check_file(&dir.path().join("main.plenty"), None).unwrap();
}

#[test]
fn unconstrained_generic_bodies_keep_private_access_lexical() {
    for (visibility, operation, expected) in [
        ("", "source.read()", "class `Cell` is private"),
        ("pub ", "source.hidden()", "`Cell.hidden` is private"),
        ("pub ", "source.value", "`Cell.value` is private"),
    ] {
        let api = format!("pub def read[T](source: &T) -> i64:\n    {operation}\n");
        let main = format!(
            "import api\n{visibility}class Cell:\n    value: i64\n    pub def read(self) -> i64:\n        self.value\n    def hidden(self) -> i64:\n        self.value\ndef main() -> Result[(), IoError]:\n    cell = Cell(7)\n    print(api.read(&cell))?\n    Ok(())\n"
        );
        let dir = workspace(&[("api.plenty", &api), ("main.plenty", &main)]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
        assert!(
            error.lines().next().unwrap().contains("main.plenty:10:"),
            "{error}"
        );
    }
    run(
        &[(
            "main.plenty",
            "protocol Readable:\n    def read(self) -> i64:\n        pass\nclass Cell:\n    value: i64\n    def read(self) -> i64:\n        self.value\ndef read[T: Readable](source: &T) -> i64:\n    source.read()\ndef main() -> Result[(), IoError]:\n    cell = Cell(7)\n    print(read(&cell))?\n    Ok(())\n",
        )],
        "main.plenty",
        "7\n",
    );
}

#[test]
fn structural_aliases_allow_forward_enums_and_preserve_managed_payloads() {
    run(
        &[(
            "main.plenty",
            r#"
type Real = Result[f64, Error]
type Optional = Option[Real]
type Text = Option[Result[str, Error]]
enum Error:
    Domain
def main() -> Result[(), AllocError | Error | IoError]:
    problem = Real.Err(Error.Domain)
    print(problem == Real.Err(Error.Domain))?
    print(Optional.Some(problem))?
    value = Text.Some(Ok('owned'))
    match value:
        case Some(result):
            print(result?)?
        case Nothing:
            pass
    Ok(())
"#,
        )],
        "main.plenty",
        "True\nOption[Result[f64, Error]].Some(Result[f64, Error].Err(Error.Domain))\nowned\n",
    );
}

#[test]
fn parameterized_protocols_are_explicit_imported_contracts() {
    run(
        &[
            (
                "api.plenty",
                "pub protocol Readable[T]:\n    def read(self) -> T:\n        pass\npub def read[T, R: Readable[T]](source: &R) -> T:\n    source.read()\n",
            ),
            (
                "main.plenty",
                "import api\npub class Cell:\n    value: u8\n    pub def read(self) -> u8:\n        self.value\ndef main() -> Result[(), IoError]:\n    cell = Cell(7)\n    print(api.read(&cell))?\n    Ok(())\n",
            ),
        ],
        "main.plenty",
        "7\n",
    );
    for api in [
        "class Hidden:\n    value: i64\npub protocol Readable[T]:\n    def read(self) -> T:\n        pass\npub def use[R: Readable[Hidden]](source: &R) -> ():\n    pass\n",
        "protocol Hidden[T]:\n    def read(self) -> T:\n        pass\npub def use[T, R: Hidden[T]](source: &R) -> ():\n    pass\n",
    ] {
        let dir = workspace(&[
            ("api.plenty", api),
            ("main.plenty", "import api\ndef main() -> ():\n    pass\n"),
        ]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), Some(dir.path()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("private"), "{error}");
    }
}

#[test]
fn channel_message_types_preserve_module_visibility_and_generic_inference() {
    run(
        &[
            (
                "messages.plenty",
                "pub def deliver[T](sender: &Sender[T], value: T) -> Result[(), SendError[T]]:\n    sender.send(value)\n",
            ),
            (
                "main.plenty",
                "import messages\ndef main() -> Result[(), AllocError | SendError[u8] | RecvError | IoError]:\n    sender, receiver = channel[u8](1)?\n    messages.deliver(&sender, 7u8)?\n    print(receiver.recv()?)?\n    Ok(())\n",
            ),
        ],
        "main.plenty",
        "7\n",
    );
    let dir = workspace(&[
        (
            "api.plenty",
            "class Hidden:\n    value: i64\npub def receive(receiver: &Receiver[Hidden]) -> ():\n    pass\n",
        ),
        ("main.plenty", "import api\ndef main() -> ():\n    pass\n"),
    ]);
    let error = plenty::check_file(&dir.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("private"), "{error}");
}

#[test]
fn generic_data_imports_keep_methods_and_constructors_visible() {
    run(
        &[
            (
                "data.plenty",
                "pub class Cell[T]:\n    pub value: T\n    pub def get(self) -> T:\n        self.value\npub enum Choice[T]:\n    Value(T)\npub def read[T](cell: &Cell[T]) -> T:\n    cell.get()\n",
            ),
            (
                "main.plenty",
                "import data\nfrom data import Cell as Crate\ndef main() -> Result[(), IoError]:\n    box = Crate(7u8)\n    print(data.read(&box))?\n    item = data.Choice[u8].Value(9)\n    match item:\n        case data.Choice.Value(value):\n            print(value)?\n    Ok(())\n",
            ),
        ],
        "main.plenty",
        "7\n9\n",
    );
}

#[test]
fn callable_data_constraints_preserve_imports_and_public_type_visibility() {
    run(
        &[
            (
                "handlers.plenty",
                "pub class Handler[T, R, F: Callable[[T], R]]:\n    pub callback: F\n    pub def call(self, value: T) -> R:\n        self.callback(value)\n",
            ),
            (
                "main.plenty",
                "from handlers import Handler\ndef main() -> Result[(), IoError | OverflowError[u8]]:\n    offset = 5u8\n    callback = def [offset](n: u8) -> Result[u8, OverflowError[u8]]:\n        Ok(n + offset)\n    handler = Handler(callback)\n    print(handler.call(3)?)?\n    Ok(())\n",
            ),
        ],
        "main.plenty",
        "8\n",
    );
    for declaration in [
        "pub class Handler[F: Callable[[Hidden], i64]]:\n    value: i64\n",
        "pub enum Job[F: OnceCallable[[], Hidden]]:\n    Empty\n",
    ] {
        let api = format!("class Hidden:\n    value: i64\n{declaration}");
        let dir = workspace(&[
            ("api.plenty", &api),
            ("main.plenty", "import api\ndef main() -> ():\n    pass\n"),
        ]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("private"), "{error}");
    }
}

#[test]
fn recursive_enum_instances_keep_identity_through_import_aliases() {
    run(
        &[
            (
                "forest.plenty",
                "pub enum Tree[T]:\n    Leaf(T)\n    Branch(Box[Tree[T]], Box[Tree[T]])\n",
            ),
            (
                "main.plenty",
                r#"
from forest import Tree as Expr
def total(tree: Expr[i64]) -> Result[i64, OverflowError[i64]]:
    match tree:
        case Expr.Leaf(value):
            Ok(value)
        case Expr.Branch(left, right):
            Ok(total(*left)? + total(*right)?)
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    tree = Expr[i64].Branch(Box(Expr[i64].Leaf(3))?, Box(Expr[i64].Leaf(7))?)
    print(total(tree)?)?
    Ok(())
"#,
            ),
        ],
        "main.plenty",
        "10\n",
    );
}

#[test]
fn recursive_declarations_preserve_field_and_public_signature_privacy() {
    let api = "pub class Node:\n    pub value: i64\n    next: Option[Box[Node]]\npub def make() -> Result[Node, AllocError]:\n    Ok(Node(7, Nothing))\n";
    run(
        &[
            ("api.plenty", api),
            (
                "main.plenty",
                "import api\ndef main() -> Result[(), AllocError | IoError]:\n    node = api.make()?\n    print(node.value)?\n    Ok(())\n",
            ),
        ],
        "main.plenty",
        "7\n",
    );
    let dir = workspace(&[
        ("api.plenty", api),
        (
            "main.plenty",
            "import api\ndef main() -> Result[(), AllocError | IoError]:\n    node = api.make()?\n    drop(node.next)\n    Ok(())\n",
        ),
    ]);
    let error = plenty::check_file(&dir.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("private"), "{error}");
    let dir = workspace(&[
        (
            "api.plenty",
            "class Hidden:\n    parent: Option[Box[Visible]]\npub class Visible:\n    pub hidden: Option[Box[Hidden]]\n",
        ),
        ("main.plenty", "import api\ndef main() -> ():\n    pass\n"),
    ]);
    let error = plenty::check_file(&dir.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("public signature exposes private type"),
        "{error}"
    );
}

#[test]
fn generic_data_cannot_bypass_member_privacy() {
    for body in [
        "cell = data.Cell(1)",
        "cell = data.Cell[i64](1)",
        "f: Callable[[i64], data.Cell[i64]] = data.Cell",
        "cell = data.make().unwrap()\n    print(cell.value).unwrap()",
        "cell = data.make().unwrap()\n    cell.hidden()",
    ] {
        let dir = workspace(&[
            (
                "data.plenty",
                "pub class Cell[T]:\n    value: T\n    def hidden(self) -> ():\n        pass\npub def make() -> Result[Cell[i64], AllocError]:\n    Ok(Cell(1))\n",
            ),
            (
                "main.plenty",
                &format!("import data\ndef main() -> ():\n    {body}\n    pass\n"),
            ),
        ]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), Some(dir.path()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("private"), "{error}");
    }
}

#[test]
fn generic_public_signatures_preserve_nominal_and_phantom_privacy() {
    for source in [
        "class Hidden[T]:\n    item: T\npub def expose[T](value: &Hidden[T]) -> ():\n    pass\n",
        "class Hidden:\n    item: i64\npub class Marker[T]:\n    pub number: i64\npub type Leak = Marker[Hidden]\n",
        "class Hidden[T]:\n    item: T\npub enum Exposed[T]:\n    Value(Hidden[T])\n",
    ] {
        let dir = workspace(&[
            ("data.plenty", source),
            ("main.plenty", "import data\ndef main() -> ():\n    pass\n"),
        ]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), Some(dir.path()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("private"), "{error}");
    }
}

#[test]
fn public_protocol_signatures_cannot_expose_private_requirements() {
    for source in [
        "protocol Hidden:\n    def read(self) -> i64:\n        pass\npub def read[T: Hidden](x: &T) -> i64:\n    x.read()\ndef main() -> ():\n    pass\n",
        "class Hidden:\n    value: i64\npub protocol Reader:\n    def read(self) -> Hidden:\n        pass\ndef main() -> ():\n    pass\n",
    ] {
        let dir = workspace(&[("main.plenty", source)]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), Some(dir.path()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("private"), "{error}");
    }
}

#[test]
fn function_values_keep_import_visibility_and_definition_scope() {
    run(
        &[
            (
                "api.plenty",
                "def hidden(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x + 1)\npub def identity[T](x: T) -> T:\n    x\npub def make() -> Callable[[i64], Result[i64, OverflowError[i64]]]:\n    def(x: i64) -> Result[i64, OverflowError[i64]]:\n        hidden(x)\n",
            ),
            (
                "main.plenty",
                "import api\nfrom api import identity as keep\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    f = keep[u8]\n    print(f(7))?\n    print(api.make()(8)?)?\n    Ok(())\n",
            ),
        ],
        "main.plenty",
        "7\n9\n",
    );
    for api in [
        "class Hidden:\n    value: i64\npub def leak(callback: Callable[[Hidden], i64]) -> ():\n    pass\n",
        "class Hidden:\n    value: i64\npub def leak() -> Callable[[], Hidden]:\n    pass\n",
    ] {
        let dir = workspace(&[
            ("api.plenty", api),
            ("main.plenty", "import api\ndef main() -> ():\n    pass\n"),
        ]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), Some(dir.path()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("private"), "{error}");
    }
}

#[test]
fn explicit_generics_resolve_imports_aliases_and_definition_scope() {
    run(
        &[
            (
                "maths.plenty",
                "def helper(x: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(x + 1)\npub def identity[T](x: T) -> T:\n    x\npub def bumped[T: IntType](x: T) -> Result[i64, OverflowError[i64]]:\n    helper(i64(x))\n",
            ),
            (
                "main.plenty",
                "import maths\nfrom maths import identity as keep\ntype Count = u8\ndef main() -> Result[(), OverflowError[i64]]:\n    print(keep[Count](7)).unwrap()\n    print(maths.identity[list[i64]]([1, 2].unwrap())).unwrap()\n    print(maths.bumped[u16](4)?).unwrap()\n    Ok(())\n",
            ),
        ],
        "main.plenty",
        "7\n[1, 2]\n5\n",
    );
}

#[test]
fn protocol_imports_do_not_activate_methods_and_preserve_visibility() {
    let api = "pub protocol Readable:\n    def read(self) -> i64:\n        pass\npub def read[T: Readable](source: &T) -> i64:\n    source.read()\n";
    let implementation = "pub class Crate:\n    pub value: i64\n    pub def read(self) -> i64:\n        self.value\n";
    let main = "import api\nfrom data import Crate\ndef main() -> ():\n    box = Crate(7)\n    print(box.read()).unwrap()\n    print(api.read[Crate](&box)).unwrap()\n";
    run(
        &[
            ("api.plenty", api),
            ("data.plenty", implementation),
            ("main.plenty", main),
        ],
        "main.plenty",
        "7\n7\n",
    );
    let inferred_main = main.replace("api.read[Crate]", "api.read");
    run(
        &[
            ("api.plenty", api),
            ("data.plenty", implementation),
            ("main.plenty", &inferred_main),
        ],
        "main.plenty",
        "7\n7\n",
    );
    let private = implementation.replace("pub def", "def");
    let private_main = main.replace("    print(box.read()).unwrap()\n", "");
    let workspace = workspace(&[
        ("api.plenty", api),
        ("data.plenty", &private),
        ("main.plenty", &private_main),
    ]);
    let error = plenty::check_file(
        &workspace.path().join("main.plenty"),
        Some(workspace.path()),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("private"), "{error}");
    let inferred = private_main.replace("api.read[Crate]", "api.read");
    std::fs::write(workspace.path().join("main.plenty"), inferred).unwrap();
    let error = plenty::check_file(
        &workspace.path().join("main.plenty"),
        Some(workspace.path()),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("private"), "{error}");
}

fn run(files: &[(&str, &str)], entry: &str, expected: &str) {
    let dir = workspace(files);
    let source = dir.path().join(entry);
    let output = dir.path().join("program");
    plenty::check_file(&source, Some(dir.path())).unwrap();
    plenty::compile_file_to_executable(&source, &output, Some(dir.path())).unwrap();
    for result in [
        Command::new(&output).output().unwrap(),
        Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg("--module-root")
            .arg(dir.path())
            .arg(&source)
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap(),
    ] {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, expected.as_bytes());
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn fallible_constructors_resolve_imported_collection_aliases() {
    run(
        &[
            ("data.plenty", "pub type Numbers = list[i64]\n"),
            (
                "main.plenty",
                r#"
import data
from data import Numbers as Values
def build() -> Result[data.Numbers, AllocError]:
    mut values = Values.with_capacity(1)?
    values.append(42)?
    Ok(values)
def main() -> ():
    print(str.repr(data.Numbers.new()).unwrap()).unwrap()
    print(str.repr(build()).unwrap()).unwrap()
"#,
            ),
        ],
        "main.plenty",
        "Result[list[i64], AllocError].Ok([])\nResult[list[i64], AllocError].Ok([42])\n",
    );
}

#[test]
fn nominal_and_generator_constructors_resolve_imports() {
    run(
        &[
            (
                "data.plenty",
                r#"
pub class Number:
    pub value: i64
pub enum Message:
    Value(i64)
pub def numbers() -> Generator[i64]:
    yield 5
"#,
            ),
            (
                "main.plenty",
                r#"
import data
from data import Number as N
def collect() -> Result[list[i64], AllocError]:
    source = data.numbers()
    list[i64].from(source)
def main() -> ():
    print(N.new(3)).unwrap()
    print(data.Message.Value.new(4)).unwrap()
    print(str.repr(collect()).unwrap()).unwrap()
"#,
            ),
        ],
        "main.plenty",
        "data.Number(value=3)\ndata.Message.Value(4)\nResult[list[i64], AllocError].Ok([5])\n",
    );
}

#[test]
fn fallible_class_constructor_preserves_private_visibility() {
    reject(
        &[
            ("data.plenty", "pub class Secret:\n    value: i64\n"),
            (
                "main.plenty",
                "import data\ndef main() -> ():\n    drop(data.Secret.new(1))\n",
            ),
        ],
        "__new__` is private",
    );
}

fn reject(files: &[(&str, &str)], expected: &str) {
    let dir = workspace(files);
    let source = dir.path().join("main.plenty");
    let output = dir.path().join("program");
    for error in [
        plenty::check_file(&source, None).unwrap_err(),
        plenty::compile_file_to_executable(&source, &output, None).unwrap_err(),
    ] {
        let message = error.to_string();
        assert!(
            message.contains(expected),
            "expected {expected:?}: {message}"
        );
        assert!(
            message.contains(".plenty"),
            "missing source path: {message}"
        );
    }
    assert!(!output.exists());
}

#[test]
fn absolute_imports_aliases_forward_calls_and_private_helpers() {
    run(
        &[
            (
                "app/main.plenty",
                "import math.arithmetic\nfrom math.arithmetic import twice as double\nimport math.arithmetic as numbers\ndef main() -> Result[(), OverflowError[i64]]:\n    print(math.arithmetic.twice(10)?).unwrap()\n    print(double(11)?).unwrap()\n    print(numbers.twice(12)?).unwrap()\n    Ok(())\n",
            ),
            (
                "math/arithmetic.plenty",
                "pub def twice(n: i64) -> Result[i64, OverflowError[i64]]:\n    helper(n)\ndef helper(n: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(n * 2)\ndef main() -> ():\n    print('not an entrypoint').unwrap()\n",
            ),
        ],
        "app/main.plenty",
        "20\n22\n24\n",
    );
}

#[test]
fn imported_main_is_an_ordinary_function_and_library_check_needs_no_entry() {
    let dir = workspace(&[(
        "lib.plenty",
        "pub def main(n: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(n + 1)\n",
    )]);
    plenty::check_module_file(&dir.path().join("lib.plenty"), None).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--check-module")
        .arg(dir.path().join("lib.plenty"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    run(
        &[
            (
                "main.plenty",
                "import lib\ndef main() -> Result[(), OverflowError[i64]]:\n    print(lib.main(41)?).unwrap()\n    Ok(())\n",
            ),
            (
                "lib.plenty",
                "pub def main(n: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(n + 1)\n",
            ),
        ],
        "main.plenty",
        "42\n",
    );
    let dir = workspace(&[("empty.plenty", "pub type Count = u32\n")]);
    plenty::check_module_file(&dir.path().join("empty.plenty"), None).unwrap();
    assert!(plenty::check_file(&dir.path().join("empty.plenty"), None).is_err());
}

const MODEL: &str = "pub class Counter:\n    value: i64\n    pub def __init__(self, value: i64) -> ():\n        self.value = value\n    pub def get(self) -> i64:\n        self.value\n    pub def increment(self: &mut Counter) -> Result[(), OverflowError[i64]]:\n        self.value = self.value + 1\n        Ok(())\n    def hidden(self) -> i64:\n        self.value\n    def __del__(self) -> ():\n        print(self.value).unwrap()\npub def peek(x: &Counter) -> i64:\n    x.value\n";

#[test]
fn public_methods_construction_and_private_drop_work_across_modules() {
    run(
        &[
            (
                "main.plenty",
                "from model import Counter, peek\ndef main() -> Result[(), OverflowError[i64]]:\n    mut counter = Counter(40)\n    counter.increment()?\n    print(counter.get()).unwrap()\n    print(peek(&counter)).unwrap()\n    Ok(())\n",
            ),
            ("model.plenty", MODEL),
        ],
        "main.plenty",
        "41\n41\n41\n",
    );
    for operation in [
        "print(c.value).unwrap()",
        "c.value = 42",
        "r = &c.value",
        "r = &mut c.value",
        "print(c.hidden()).unwrap()",
    ] {
        let source = format!(
            "import model\ndef main() -> ():\n    mut c = model.Counter(1)\n    {operation}\n"
        );
        reject(
            &[("main.plenty", &source), ("model.plenty", MODEL)],
            "private",
        );
    }
}

#[test]
fn public_fields_generated_constructors_and_qualified_enum_patterns() {
    run(
        &[
            (
                "main.plenty",
                "import model as m\nfrom model import Event as E, Count\ndef read(x: m.Event) -> i64:\n    match x:\n        case m.Event.Value(n):\n            n\n        case E.Empty:\n            0\ndef main() -> ():\n    mut p = m.Point(3)\n    p.x = 4\n    print(p.x).unwrap()\n    print(read(m.Event.Value(42))).unwrap()\n    n: Count = Count(7)\n    print(n).unwrap()\n",
            ),
            (
                "model.plenty",
                "pub type Count = u32\npub class Point:\n    pub x: i64\npub enum Event:\n    Value(i64)\n    Empty\n",
            ),
        ],
        "main.plenty",
        "4\n42\n7\n",
    );
}

#[test]
fn private_constructors_and_aliases_cannot_bypass_visibility() {
    for body in [
        "pub class Secret:\n    value: i64\n",
        "pub class Secret:\n    pub value: i64\n    def __init__(self, value: i64) -> ():\n        self.value = value\n",
    ] {
        reject(
            &[
                (
                    "main.plenty",
                    "from lib import Secret\ntype Alias = Secret\ndef main() -> ():\n    value = Alias(42).unwrap()\n",
                ),
                ("lib.plenty", body),
            ],
            "__new__` is private",
        );
    }
    run(
        &[
            (
                "main.plenty",
                "from lib import make\ndef main() -> ():\n    value = make()\n    print(value.get()).unwrap()\n",
            ),
            (
                "lib.plenty",
                "pub class Secret:\n    value: i64\n    pub def get(self) -> i64:\n        self.value\npub def make() -> Secret:\n    Secret(42)\n",
            ),
        ],
        "main.plenty",
        "42\n",
    );
}

#[test]
fn public_signatures_cannot_expose_private_nominal_types() {
    for api in [
        "pub def f(x: Hidden) -> ():\n    pass\n",
        "pub type Exposed = Alias\n",
        "pub enum Exposed:\n    Value(list[Alias])\n",
        "pub class Exposed:\n    pub value: Option[Alias]\n",
        "pub def f() -> Result[Hidden, str]:\n    Ok(Hidden(1))\n",
    ] {
        let lib = format!("class Hidden:\n    value: i64\ntype Alias = Hidden\n{api}");
        reject(
            &[
                ("main.plenty", "import lib\ndef main() -> ():\n    pass\n"),
                ("lib.plenty", &lib),
            ],
            "public signature exposes private type",
        );
    }
}

#[test]
fn diamond_imports_share_identity_and_same_named_types_stay_distinct() {
    run(
        &[
            (
                "main.plenty",
                "import left, right\ndef main() -> ():\n    print(right.read(left.make())).unwrap()\n",
            ),
            (
                "left.plenty",
                "from model import Point\npub def make() -> Point:\n    Point(42)\n",
            ),
            (
                "right.plenty",
                "from model import Point\npub def read(p: Point) -> i64:\n    p.x\n",
            ),
            ("model.plenty", "pub class Point:\n    pub x: i64\n"),
        ],
        "main.plenty",
        "42\n",
    );
    reject(
        &[
            (
                "main.plenty",
                "import left, right\ndef main() -> ():\n    p: left.Point = right.Point(1)\n",
            ),
            ("left.plenty", "pub class Point:\n    pub x: i64\n"),
            ("right.plenty", "pub class Point:\n    pub x: i64\n"),
        ],
        "expected left.Point, got right.Point",
    );
}

#[test]
fn import_failures_do_not_run_initializers_or_expose_transitive_names() {
    reject(
        &[(
            "main.plenty",
            "import absent\ndef main() -> ():\n    pass\n",
        )],
        "loading `absent`",
    );
    reject(
        &[
            ("main.plenty", "import lib\ndef main() -> ():\n    pass\n"),
            ("lib.plenty", "print('must not run').unwrap()\n"),
        ],
        "module scope",
    );
    for use_site in [
        "from lib import hidden",
        "import lib\ndef main() -> ():\n    lib.hidden()",
    ] {
        reject(
            &[
                ("main.plenty", use_site),
                ("lib.plenty", "def hidden() -> ():\n    pass\n"),
            ],
            "private",
        );
    }
    reject(
        &[
            (
                "main.plenty",
                "from a import f\ndef main() -> ():\n    f()\n",
            ),
            ("a.plenty", "from b import f\n"),
            ("b.plenty", "pub def f() -> ():\n    pass\n"),
        ],
        "private or is not a declared export",
    );
    reject(
        &[
            ("main.plenty", "import a\ndef main() -> ():\n    b.f()\n"),
            ("a.plenty", "import b\n"),
            ("b.plenty", "pub def f() -> ():\n    pass\n"),
        ],
        "unknown binding",
    );
    reject(
        &[("main.plenty", "pub import lib\n"), ("lib.plenty", "")],
        "re-exports",
    );
}

/// A directory is a namespace only when some module path through it names a
/// `.plenty` or `.plentyi` file; others can sit beside a same-named module.
#[test]
fn directories_without_modules_do_not_conflict_with_same_named_modules() {
    let main = "import oauth\ndef main() -> ():\n    print(oauth.NAME).unwrap()\n";
    let module = "pub const NAME: str = \"oauth\"\n";
    let dir = workspace(&[
        ("main.plenty", main),
        ("oauth.plenty", module),
        ("oauth/mod.rs", "pub fn token() {}\n"),
        ("oauth/assets/logo.svg", ""),
        // Names that no import can spell.
        ("oauth/not-a-module.plenty", ""),
        ("oauth/not-a-namespace/x.plenty", ""),
        ("oauth/.hidden/x.plenty", ""),
        ("std/README.md", ""),
    ]);
    std::fs::create_dir(dir.path().join("oauth/empty")).unwrap();
    std::os::unix::fs::symlink(".", dir.path().join("oauth/cycle")).unwrap();
    // Links leaving the source root name nothing an import could load, and
    // the walk does not follow them.
    let outside = workspace(&[("x.plenty", "")]);
    std::os::unix::fs::symlink(outside.path(), dir.path().join("oauth/ext")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("x.plenty"),
        dir.path().join("oauth/y.plenty"),
    )
    .unwrap();
    std::os::unix::fs::symlink("/", dir.path().join("oauth/sys")).unwrap();
    // A directory named like a module is not one.
    std::fs::create_dir(dir.path().join("oauth/dir.plenty")).unwrap();
    let source = dir.path().join("main.plenty");
    plenty::check_file(&source, None).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"oauth\n");

    // Each of these makes `oauth/` a namespace again, so `import oauth` is
    // ambiguous: a direct or nested module, an interface, or a module reached
    // through a link.
    for (path, link) in [
        ("oauth/x.plenty", None),
        ("oauth/x.plentyi", None),
        ("oauth/inner/deeper/x.plenty", None),
        ("oauth/linked", Some("../lib")),
        ("oauth/linked.plenty", Some("../lib/x.plenty")),
    ] {
        let dir = workspace(&[
            ("main.plenty", main),
            ("oauth.plenty", module),
            ("oauth/mod.rs", ""),
            ("lib/x.plenty", ""),
        ]);
        let path = dir.path().join(path);
        match link {
            Some(target) => std::os::unix::fs::symlink(target, &path).unwrap(),
            None => {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, "").unwrap();
            }
        }
        let message = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("ambiguous module `oauth`"),
            "{path:?}: {message}"
        );
    }

    // An unreadable directory may hold modules, so it keeps the conflict, and
    // the diagnostic names it.
    let dir = workspace(&[
        ("main.plenty", main),
        ("oauth.plenty", module),
        ("oauth/locked/mod.rs", ""),
    ]);
    let locked = dir.path().join("oauth/locked");
    let mode = |mode| {
        std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(mode))
            .unwrap()
    };
    mode(0o000);
    let readable = std::fs::read_dir(&locked).is_ok();
    let result = plenty::check_file(&dir.path().join("main.plenty"), None);
    mode(0o755);
    // A privileged user reads it anyway, and it holds no modules.
    if !readable {
        let message = result.unwrap_err().to_string();
        assert!(message.contains("ambiguous module `oauth`"), "{message}");
        let note = format!(
            "note: {} could not be read",
            locked.canonicalize().unwrap().display()
        );
        assert!(message.contains(&note), "{message}");
    }
}

#[test]
fn cycles_collisions_and_source_root_boundaries_are_diagnosed() {
    reject(
        &[
            ("main.plenty", "import a\ndef main() -> ():\n    pass\n"),
            ("a.plenty", "import b\n"),
            ("b.plenty", "import a\n"),
        ],
        "a.plenty ->",
    );
    reject(
        &[
            ("main.plenty", "import a.b\ndef main() -> ():\n    pass\n"),
            ("a.plenty", ""),
            ("a/b.plenty", ""),
        ],
        "ambiguous module",
    );
    reject(
        &[
            (
                "main.plenty",
                "import lib as print\ndef main() -> ():\n    pass\n",
            ),
            ("lib.plenty", ""),
        ],
        "conflicts with the builtin `print`, a prelude name; bind the import under another name, as in `import lib as ...`",
    );
    let dir = workspace(&[
        (
            "app/main.plenty",
            "import lib\ndef main() -> ():\n    pass\n",
        ),
        ("lib.plenty", ""),
    ]);
    let entry = dir.path().join("app/main.plenty");
    assert!(plenty::check_file(&entry, None).is_err());
    plenty::check_file(&entry, Some(dir.path())).unwrap();
    assert!(
        plenty::check_file(
            &dir.path().join("lib.plenty"),
            Some(&dir.path().join("app"))
        )
        .is_err()
    );
}

/// `import p` and `import p.b` coexist only when both name their own module
/// paths (issue #568). Locally, `p.plenty` beside `p/b.plenty` is ambiguous,
/// so a local `p` could enclose `p.b` only as an alias, which is rejected:
/// `p.f` and `p.b.f` would then belong to unrelated modules.
#[test]
fn an_alias_cannot_enclose_a_module_path() {
    let x = ("x.plenty", "pub def f() -> i64:\n    1\n");
    let b = ("p/b.plenty", "pub def f() -> i64:\n    2\n");
    for (first, second) in [
        ("import x as p", "import p.b"),
        ("import p.b", "import x as p"),
    ] {
        reject(
            &[
                x,
                b,
                (
                    "main.plenty",
                    &format!("{first}\n{second}\ndef main() -> ():\n    pass\n"),
                ),
            ],
            &format!(
                "`{second}` conflicts with `{first}`: the alias `p` cannot enclose a module path; bind the nested module under another name, such as `import p.b as b`"
            ),
        );
    }
    run(
        &[
            x,
            b,
            (
                "main.plenty",
                "import x as p\nimport p.b as b\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    print(p.f() * 10 + b.f())?\n    Ok(())\n",
            ),
        ],
        "main.plenty",
        "12\n",
    );
    // A module path cannot extend a declaration or an imported item either.
    for (main, expected) in [
        (
            "import p.b\nclass p:\n    v: i64\n",
            "main.plenty:1:8: import binding `p.b` conflicts with the class `p` declared at line 2",
        ),
        (
            "from x import f as p\nimport p.b\n",
            "main.plenty:2:8: import binding `p.b` conflicts with `from x import f as p` at line 1",
        ),
        (
            "import p.b\nfrom x import f as p\n",
            "main.plenty:2:20: import binding `p` conflicts with the module namespace `p` bound by `import p.b` at line 1",
        ),
    ] {
        reject(
            &[
                x,
                b,
                (
                    "main.plenty",
                    &format!("{main}def main() -> ():\n    pass\n"),
                ),
            ],
            expected,
        );
    }
}

/// `from` imports declarations, never a module, and a module path is not a
/// value (issue #587). The module is looked for only after an import fails;
/// a name that is no module keeps the original diagnostic.
#[test]
fn from_cannot_import_a_module() {
    let helpers = ("util/helpers.plenty", "pub def f() -> i64:\n    1\n");
    let deep = ("a/b/c.plenty", "pub def g() -> i64:\n    2\n");
    let module = |path: &str, binding: &str| {
        format!(
            "`from` imports declarations, and `{path}` is a module; use `import {path}` or `import {path} as {binding}`"
        )
    };
    for (main, message) in [
        (
            "from util import helpers\n",
            module("util.helpers", "helpers"),
        ),
        (
            "from util import helpers as h\n",
            module("util.helpers", "h"),
        ),
        ("from a.b import c\n", module("a.b.c", "c")),
        ("from a import b\n", "loading `a`".to_string()),
        ("from util import missing\n", "loading `util`".to_string()),
        (
            "from util.helpers import missing\n",
            "`util.helpers.missing` is private or is not a declared export".to_string(),
        ),
    ] {
        reject(
            &[
                helpers,
                deep,
                (
                    "main.plenty",
                    &format!("{main}def main() -> ():\n    pass\n"),
                ),
            ],
            &message,
        );
    }
    for (source, message) in [
        ("from std import json\n", module("std.json", "json")),
        ("from std.http import server\n", module("std.http.server", "server")),
        ("from std import missing\n", "unknown standard library module `std`".to_string()),
        ("from std.http import missing\n", "`std.http.missing` is private or is not a declared export".to_string()),
        (
            "import std.http\nimport std.http.server\ndef f() -> ():\n    x = std.http.server\n",
            "`std.http.server` is a module, not a declaration; name one of its declarations, such as `std.http.server.".to_string(),
        ),
    ] {
        let error = plenty::check_source(&format!("{source}def main() -> ():\n    pass\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(&message), "expected {message:?}: {error}");
    }
}

#[test]
fn in_memory_api_never_searches_the_filesystem_for_imports() {
    for source in [
        "import lib\ndef main() -> ():\n    pass\n",
        "from std.math import gcd\nimport lib.util\ndef main() -> ():\n    pass\n",
    ] {
        let error = plenty::check_source(source).unwrap_err().to_string();
        assert!(error.contains("without a source root"), "{error}");
        assert!(error.contains("only `std` modules"), "{error}");
    }
    let error = plenty::check_source("import std.missing\ndef main() -> ():\n    pass\n")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("unknown standard library module `std.missing`"),
        "{error}"
    );
}

#[test]
fn in_memory_api_imports_embedded_std_modules() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    plenty::compile_source_to_executable(
        "import std.math\nfrom std.math import MathError, gcd\nfrom std.fmt import FormatError\n\
         def root(x: f64) -> Result[f64, MathError]:\n    std.math.sqrt(x)\n\
         def main() -> Result[(), AllocError | FormatError | IoError | MathError]:\n\
         \x20   print(gcd(12, 18))?\n    print(f\"{root(9.0)?:>5}\"?)?\n    Ok(())\n",
        &executable,
    )
    .unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "6\n  3.0\n");
}

#[test]
fn imported_generators_enums_and_tail_recursion_keep_their_identities() {
    run(
        &[
            (
                "main.plenty",
                "import data\ndef main() -> Result[(), OverflowError[i64]]:\n    for value in data.values():\n        match value:\n            case data.Reading.Value(n):\n                print(n).unwrap()\n            case data.Reading.Empty:\n                pass\n    print(data.count(100_000)?).unwrap()\n    Ok(())\n",
            ),
            (
                "data.plenty",
                "pub enum Reading:\n    Value(i64)\n    Empty\npub def values() -> Generator[Reading]:\n    yield Reading.Value(42)\n    yield Reading.Empty\npub def count(n: i64) -> Result[i64, OverflowError[i64]]:\n    if n == 0:\n        Ok(0)\n    else:\n        count(n - 1)\n",
            ),
        ],
        "main.plenty",
        "42\n0\n",
    );
}

#[test]
fn namespace_roots_and_locals_do_not_implicitly_import_names() {
    reject(
        &[
            (
                "main.plenty",
                "import first as pkg\nimport pkg.second\ndef main() -> ():\n    pass\n",
            ),
            ("first.plenty", ""),
            ("pkg/second.plenty", ""),
        ],
        "conflicts",
    );
    run(
        &[
            (
                "main.plenty",
                "import pkg.a, pkg.b\ndef main() -> Result[(), OverflowError[i64]]:\n    print(pkg.a.f() + pkg.b.f()).unwrap()\n    Ok(())\n",
            ),
            ("pkg/a.plenty", "pub def f() -> i64:\n    1\n"),
            ("pkg/b.plenty", "pub def f() -> i64:\n    2\n"),
        ],
        "main.plenty",
        "3\n",
    );
    reject(
        &[
            (
                "main.plenty",
                "import lib\ndef main() -> ():\n    lib = 1\n    lib.f()\n",
            ),
            ("lib.plenty", "pub def f() -> ():\n    pass\n"),
        ],
        "method",
    );
    reject(
        &[
            (
                "main.plenty",
                "def hidden() -> ():\n    pass\nimport lib\ndef main() -> ():\n    lib.f()\n",
            ),
            ("lib.plenty", "pub def f() -> ():\n    hidden()\n"),
        ],
        "unknown function",
    );
}

#[test]
fn imported_ownership_errors_retain_the_defining_source_path() {
    reject(
        &[
            ("main.plenty", "import lib\ndef main() -> ():\n    pass\n"),
            (
                "lib.plenty",
                "pub def bad() -> ():\n    a = [1].unwrap()\n    b = a\n    print(a).unwrap()\n",
            ),
        ],
        "lib.plenty",
    );
}

#[cfg(unix)]
#[test]
fn canonical_paths_deduplicate_symlinks_and_reject_root_escape() {
    use std::os::unix::fs::symlink;
    let dir = workspace(&[
        (
            "main.plenty",
            "import original, alias\ndef main() -> ():\n    p: original.Point = alias.Point(42)\n    print(p.x).unwrap()\n",
        ),
        ("original.plenty", "pub class Point:\n    pub x: i64\n"),
    ]);
    symlink(
        dir.path().join("original.plenty"),
        dir.path().join("alias.plenty"),
    )
    .unwrap();
    plenty::check_file(&dir.path().join("main.plenty"), None).unwrap();
    let other = workspace(&[("external.plenty", "")]);
    symlink(
        other.path().join("external.plenty"),
        dir.path().join("escape.plenty"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.plenty"),
        "import escape\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
    let error = plenty::check_file(&dir.path().join("main.plenty"), None).unwrap_err();
    assert!(error.to_string().contains("outside the source root"));
}

#[test]
fn imported_generic_body_errors_are_reported_in_the_importing_file() {
    let dir = workspace(&[
        (
            "cells.plenty",
            "pub class Cell[T]:\n    pub value: T\n    pub def get(self) -> T:\n        self.value\n\npub def twice[T](value: T) -> tuple[T, T]:\n    (value, value)\n\npub def wrap[T](value: T) -> T:\n    Cell(value).get()\n",
        ),
        (
            "class.plenty",
            "import cells\n\ndef main() -> Result[(), AllocError | IoError]:\n    cell = cells.wrap([1, 2]?)\n    Ok(())\n",
        ),
        (
            "function.plenty",
            "import cells\n\ndef main() -> Result[(), AllocError | IoError]:\n    pair = cells.twice([1, 2]?)\n    Ok(())\n",
        ),
    ]);
    let path = |name: &str| dir.path().join(name).canonicalize().unwrap();
    let cells = path("cells.plenty");
    let cells = cells.display();
    let main = path("class.plenty");
    assert_eq!(
        plenty::check_file(&main, None).unwrap_err().to_string(),
        format!(
            "{}:4:12: `cells.wrap[list[i64]]` requires `cells.Cell[list[i64]].get`, which \
             copies `T`, but `list[i64]` cannot be copied\n  \
             {cells}:4:9: note: in instantiation `cells.Cell[list[i64]].get`: cannot move out of a field",
            main.display()
        )
    );
    let main = path("function.plenty");
    assert_eq!(
        plenty::check_file(&main, None).unwrap_err().to_string(),
        format!(
            "{}:4:12: `cells.twice[list[i64]]` copies `T`, but `list[i64]` cannot be copied\n  \
             {cells}:7:13: note: in instantiation `cells.twice[list[i64]]`: use of moved binding `value`\n  \
             {cells}:7:6: note: `value` is moved here",
            main.display()
        )
    );
}

/// Name conflicts with builtins, declarations, and earlier imports name the
/// identifier, its origin with a line, and a fix, at the conflicting name
/// (issue #801).
#[test]
fn name_conflicts_explain_their_origin() {
    let main = "\ndef main() -> ():\n    pass\n";
    for (source, expected) in [
        (
            "def spawn() -> ():\n    pass\n",
            "main.plenty:1:5: cannot redefine builtin `spawn` as a function: it is a prelude name, available without an import; rename the function",
        ),
        (
            "def set() -> ():\n    pass\n",
            "main.plenty:1:5: cannot redefine builtin `set` as a function",
        ),
        (
            "class set:\n    v: i64\n",
            "main.plenty:1:1: cannot redefine builtin `set` as a type: it is a prelude name, available without an import; rename the declaration",
        ),
        (
            "from std.sync import Shared\n",
            "main.plenty:1:22: import binding `Shared` conflicts with the builtin `Shared`: it is a prelude name, available without an import, and `std.sync` does not export it; remove the import",
        ),
        (
            "import std.math as pages\ndef pages() -> ():\n    pass\n",
            "main.plenty:1:20: import binding `pages` conflicts with the function `pages` declared at line 2; rename the function or bind the import under another name, as in `import std.math as ...`",
        ),
        (
            "from std.math import sqrt\nclass sqrt:\n    v: i64\n",
            "main.plenty:1:22: import binding `sqrt` conflicts with the class `sqrt` declared at line 2; rename the class or bind the import under another name, as in `from std.math import sqrt as ...`",
        ),
        (
            "from std.math import sqrt\nfrom std.math import sqrt\n",
            "main.plenty:2:22: `from std.math import sqrt` repeats the import at line 1; remove the redundant import",
        ),
        (
            "import std.math as m\nimport std.json as m\n",
            "main.plenty:2:20: import binding `m` conflicts with `import std.math as m` at line 1; bind one of them under another name, as in `import std.json as ...`",
        ),
        (
            "from std.math import sqrt as m\nimport std.json as m\n",
            "main.plenty:2:20: import binding `m` conflicts with `from std.math import sqrt as m` at line 1",
        ),
    ] {
        let dir = workspace(&[("main.plenty", &format!("{source}{main}"))]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "expected {expected:?}: {error}");
    }
}

/// A function-local binding may shadow an imported name, and importing a
/// builtin's own declaration is not a conflict (issue #801).
#[test]
fn local_shadowing_and_identical_builtin_imports_are_allowed() {
    for source in [
        "import std.math as math\ndef main() -> ():\n    math = 3\n    if math > 2:\n        pass\n",
        "from std.math import sqrt\ndef main() -> ():\n    sqrt = 2\n    if sqrt > 1:\n        pass\n",
        "from std.concurrent import ThreadPoolExecutor\ndef main() -> ():\n    pass\n",
    ] {
        let dir = workspace(&[("main.plenty", source)]);
        if let Err(error) = plenty::check_file(&dir.path().join("main.plenty"), None) {
            panic!("rejected:\n{source}\n{error}");
        }
    }
}

#[test]
fn replacement_builtin_is_bound_in_its_defining_module() {
    run(
        &[
            (
                "api.plenty",
                r#"
pub class Cell:
    pub value: i64
    pub def swap(self: &mut Cell, value: i64) -> i64:
        replace(self.value, value)
pub def swap[T](items: &mut list[T], value: T) -> T:
    replace(items[0], value)
"#,
            ),
            (
                "main.plenty",
                r#"
import api
def replace(left: i64, right: i64) -> i64:
    right
def main() -> ():
    mut cell = api.Cell(7)
    mut items = [11].unwrap()
    print(cell.swap(8)).unwrap()
    print(api.swap(&mut items, 12)).unwrap()
    print(cell.value).unwrap()
    print(items[0]).unwrap()
    print(replace(2, 3)).unwrap()
    operation = replace
    print(operation(4, 5)).unwrap()
"#,
            ),
        ],
        "main.plenty",
        "7\n11\n8\n12\n3\n5\n",
    );
}

#[test]
fn replacement_user_aliases_and_local_bindings_keep_their_identity() {
    run(
        &[
            (
                "helper.plenty",
                "pub def choose[T](value: T) -> T:\n    value\n",
            ),
            (
                "api.plenty",
                r#"
from helper import choose as replace
pub def direct() -> i64:
    replace(7)
pub def explicit() -> i64:
    replace[i64](8)
pub def indirect() -> i64:
    operation = replace[i64]
    operation(9)
"#,
            ),
            (
                "main.plenty",
                r#"
import api
from helper import choose
def main() -> ():
    print(api.direct()).unwrap()
    print(api.explicit()).unwrap()
    print(api.indirect()).unwrap()
    replace = choose[i64]
    print(replace(10)).unwrap()
"#,
            ),
        ],
        "main.plenty",
        "7\n8\n9\n10\n",
    );
}

#[test]
fn replacement_in_unused_std_test_methods_is_not_shadowed_by_the_entrypoint() {
    for declaration in [
        "def replace(callback: &mut Closure[[f64], f64], limit: f64) -> ():\n    pass",
        "def replace[T](value: T) -> T:\n    value",
        "type replace = i64",
    ] {
        let source = format!(
            "import std.test\n{declaration}\ndef main() -> ():\n    std.test.check(True, \"ok\").unwrap()\n"
        );
        let dir = workspace(&[("main.plenty", &source)]);
        plenty::check_file(&dir.path().join("main.plenty"), None).unwrap();
    }
}

#[test]
fn replacement_builtin_is_not_an_indirect_or_generic_user_function() {
    // A value use resolves in the writing module, which declares no
    // `replace`; a generic call is the builtin and reports its written
    // name, as `iter[i64](xs)` does (issue #898).
    for (expression, expected) in [
        ("replace", "unknown binding `api.replace`"),
        ("replace[i64]", "`api.replace` is not a generic function"),
        ("replace[i64](7)", "`replace` is not a generic function"),
    ] {
        let api = format!("pub def invalid() -> ():\n    operation = {expression}\n");
        let dir = workspace(&[
            ("api.plenty", &api),
            (
                "main.plenty",
                "import api\ndef replace[T](value: T) -> T:\n    value\ndef main() -> ():\n    pass\n",
            ),
        ]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expression}: {error}");
        assert!(!error.contains("$builtin"), "{error}");
    }
}

#[test]
fn replacement_builtin_diagnostics_use_the_public_spelling() {
    let dir = workspace(&[("main.plenty", "def main() -> ():\n    replace()\n")]);
    let error = plenty::check_file(&dir.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("replace takes a mutable field"), "{error}");
    assert!(!error.contains("$builtin"), "{error}");
}

/// Pipeline sources and loop adapters bind in the module that writes the call,
/// so entrypoint declarations of the same names do not capture them (issue
/// #898).
#[test]
fn iteration_builtins_are_bound_in_their_defining_module() {
    run(
        &[
            (
                "api.plenty",
                r#"
pub def total(values: &list[i64]) -> i64:
    iter(values).sum().unwrap()
pub def loops(values: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut n = 0
    for i, v in enumerate(values):
        n += i * *v
    for a, b in zip(values, values):
        n += *a * *b
    for v in reversed(values):
        n += *v
    Ok(n)
pub def counts(values: &list[i64]) -> Result[i64, OverflowError[i64]]:
    Ok(enumerate(values).count() + zip(values, values).count())
pub def any_set(flags: &list[bool]) -> bool:
    iter(flags).any()
pub def first[T](values: &list[T]) -> i64:
    iter(values).count()
pub def doubled(values: &list[i64]) -> list[i64]:
    [*v for v in iter(values)].unwrap()
"#,
            ),
            (
                "main.plenty",
                r#"
import api
def iter(value: i64) -> i64:
    value
def enumerate(value: i64) -> i64:
    value
def zip(left: i64, right: i64) -> i64:
    left
def reversed(value: i64) -> i64:
    value
def any(value: i64) -> i64:
    value
def all(value: i64) -> i64:
    value
def main() -> ():
    values = [1, 2].unwrap()
    print(api.total(&values)).unwrap()
    print(api.loops(&values).unwrap()).unwrap()
    print(api.counts(&values).unwrap()).unwrap()
    print(api.any_set(&[False, True].unwrap())).unwrap()
    print(api.first(&values)).unwrap()
    print(api.doubled(&values)).unwrap()
    print(iter(4), enumerate(5), zip(6, 7), reversed(8), any(9), all(10)).unwrap()
    operation = iter
    print(operation(11)).unwrap()
"#,
            ),
        ],
        "main.plenty",
        "3\n10\n4\nTrue\n2\n[1, 2]\n4 5 6 8 9 10\n11\n",
    );
}

#[test]
fn imported_iteration_names_shadow_only_their_importing_module() {
    run(
        &[
            (
                "helper.plenty",
                "pub def iter(value: i64) -> i64:\n    value\n",
            ),
            (
                "api.plenty",
                "from helper import iter\npub def direct() -> i64:\n    iter(3)\n",
            ),
            (
                "main.plenty",
                r#"
import api
def main() -> ():
    values = [1, 2].unwrap()
    print(api.direct()).unwrap()
    print(iter(&values).count()).unwrap()
"#,
            ),
        ],
        "main.plenty",
        "3\n2\n",
    );
}

#[test]
fn iteration_builtins_are_not_indirect_user_functions() {
    for name in ["iter", "enumerate", "zip", "reversed", "any", "all"] {
        let api = format!("pub def invalid() -> ():\n    operation = {name}\n");
        let main = format!(
            "import api\ndef {name}(value: i64) -> i64:\n    value\ndef main() -> ():\n    pass\n"
        );
        let dir = workspace(&[("api.plenty", &api), ("main.plenty", &main)]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!("api.{name}")), "{error}");
        assert!(!error.contains("$builtin"), "{error}");
    }
}

#[test]
fn iteration_builtin_diagnostics_ignore_entrypoint_declarations() {
    for (call, expected) in [
        ("any(flags)", "any is not a builtin function"),
        ("all(flags)", "all is not a builtin function"),
        ("reversed(flags)", "reversed is supported directly"),
    ] {
        let api = format!("pub def invalid(flags: &list[bool]) -> ():\n    {call}\n");
        let dir = workspace(&[
            ("api.plenty", &api),
            (
                "main.plenty",
                "import api\ndef any(value: i64) -> i64:\n    value\ndef all(value: i64) -> i64:\n    value\ndef enumerate(value: i64) -> i64:\n    value\ndef reversed(value: i64) -> i64:\n    value\ndef main() -> ():\n    pass\n",
            ),
        ]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{call}: {error}");
        assert!(!error.contains("$builtin"), "{error}");
    }
}

#[test]
fn generic_iteration_builtin_calls_name_the_builtin() {
    let dir = workspace(&[
        (
            "api.plenty",
            "pub def invalid(xs: &list[i64]) -> i64:\n    iter[i64](xs).count()\n",
        ),
        (
            "main.plenty",
            "import api\ndef iter[T](value: T) -> T:\n    value\ndef main() -> ():\n    pass\n",
        ),
    ]);
    let error = plenty::check_file(&dir.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("`iter` is not a generic function"),
        "{error}"
    );
    assert!(!error.contains("api.iter"), "{error}");
    assert!(!error.contains("$builtin"), "{error}");
}

#[test]
fn short_extensions_preserve_module_identity_and_interface_trust() {
    let untrusted = workspace(&[(
        "main.p",
        "extern def call() -> () = \"call\"\ndef main() -> ():\n    pass\n",
    )]);
    assert!(
        plenty::check_file(&untrusted.path().join("main.p"), None)
            .unwrap_err()
            .to_string()
            .contains("trusted .plentyi or .pi")
    );
    for entry in ["main.plenty", "main.p"] {
        for source in ["helper.plenty", "helper.p"] {
            for interface in ["native.plentyi", "native.pi"] {
                let dir = workspace(&[
                    (
                        entry,
                        "import helper\nimport native\ndef main() -> ():\n    pass\n",
                    ),
                    (source, "pub def value() -> i64:\n    42\n"),
                    (interface, "extern def abs(value: i32) -> i32 = \"abs\"\n"),
                ]);
                plenty::check_file(&dir.path().join(entry), None).unwrap();
            }
        }
    }
}

#[test]
fn every_extension_collision_is_ambiguous_and_names_both_files() {
    let extensions = ["plenty", "p", "plentyi", "pi"];
    for (i, first) in extensions.iter().enumerate() {
        for second in &extensions[i + 1..] {
            let a = format!("helper.{first}");
            let b = format!("helper.{second}");
            let dir = workspace(&[
                ("main.p", "import helper\ndef main() -> ():\n    pass\n"),
                (&a, "pub def value() -> i64:\n    1\n"),
                (&b, "pub def value() -> i64:\n    2\n"),
            ]);
            let error = plenty::check_file(&dir.path().join("main.p"), None)
                .unwrap_err()
                .to_string();
            assert!(error.contains("ambiguous module"), "{error}");
            assert!(error.contains(&a) && error.contains(&b), "{error}");
        }
    }
}

#[test]
fn short_extensions_keep_namespace_and_standard_library_boundaries() {
    for name in ["std.p", "std.pi"] {
        let dir = workspace(&[("main.p", "def main() -> ():\n    pass\n"), (name, "")]);
        assert!(
            plenty::check_file(&dir.path().join("main.p"), None)
                .unwrap_err()
                .to_string()
                .contains("reserved")
        );
    }
    for name in ["helper.p", "helper.pi"] {
        let dir = workspace(&[
            (
                "main.p",
                "import helper.child\ndef main() -> ():\n    pass\n",
            ),
            (name, ""),
            ("helper/child.p", ""),
        ]);
        assert!(
            plenty::check_file(&dir.path().join("main.p"), None)
                .unwrap_err()
                .to_string()
                .contains("ambiguous module")
        );
    }
}

#[test]
fn short_files_distinguish_asset_directories_from_module_namespaces() {
    for extension in ["plenty", "p", "plentyi", "pi"] {
        let dir = workspace(&[
            ("main.p", "import helper\ndef main() -> ():\n    pass\n"),
            ("helper.p", "pub def value() -> i64:\n    1\n"),
            ("helper/readme.txt", "assets only"),
        ]);
        let entry = dir.path().join("main.p");
        plenty::check_file(&entry, None).unwrap();
        let nested = dir.path().join(format!("helper/child.{extension}"));
        std::fs::write(&nested, "").unwrap();
        let error = plenty::check_file(&entry, None).unwrap_err().to_string();
        assert!(error.contains("ambiguous module `helper`"), "{error}");
        std::fs::remove_file(nested).unwrap();
        let standard = dir.path().join("std");
        std::fs::create_dir(&standard).unwrap();
        std::fs::write(standard.join(format!("child.{extension}")), "").unwrap();
        let error = plenty::check_file(&entry, None).unwrap_err().to_string();
        assert!(error.contains("`std` is reserved"), "{error}");
    }
}

/// Entry-module declarations are unqualified, so a user type named like a
/// template's type parameter must not capture it when the template's callable
/// bound or signature is checked: `G` in `std.error.map_error`, `W` in
/// `std.fmt`, `T` and `G` in a library module (issue #915).
#[test]
fn entry_type_names_do_not_capture_template_type_parameters() {
    run(
        &[(
            "main.plenty",
            r#"import std.json
from std.error import map_error, context, Context

@serial
class G[T]:
    value: T

@serial
class T[X]:
    inner: X

@serial
class E:
    code: i64

class F[A]:
    a: A

class W[A]:
    a: A

class R[A]:
    a: A

def main() -> Result[(), Failure]:
    opts = std.json.codec_options()
    g = std.json.load[G[i64]](b"{\"value\":1}", &opts)?
    print(g.value)?
    t = std.json.load[T[G[E]]](b"{\"inner\":{\"value\":{\"code\":7}}}", &opts)?
    print(t.inner.value.code)?
    text = std.json.dump(&t, &std.json.limits())?
    print(len(text))?
    r: Result[i64, i64] = Err(3)
    m = map_error(r, def (e: i64) -> F[i64]: F(e))
    match m:
        case Err(f):
            print(f.a)?
        case Ok(_):
            pass
    failed: Result[i64, E] = Err(E(4))
    c = context(failed, "reading")
    match c:
        case Err(ctx):
            print(ctx.cause.code)?
            print(f"{ctx}"?)?
        case Ok(_):
            pass
    Ok(())
"#,
        )],
        "main.plenty",
        "1\n7\n30\n3\n4\nstd.error.Context[E](operation=\"reading\", subject=std.error.Subject.Absent, cause=E(code=4))\n",
    );
    run(
        &[
            (
                "lib.plenty",
                r#"pub def wrap[G](value: G) -> Option[G]:
    Some(value)

pub def apply[T, G, F: OnceCallable[[T], G]](value: T, f: F) -> G:
    mut call = f
    call(value)

pub class Pair[G]:
    pub left: G
    pub right: G
    pub def make[T](self, extra: T, a: G, b: G) -> tuple[Pair[G], T]:
        (Pair(b, a), extra)
"#,
            ),
            (
                "main.plenty",
                r#"from lib import wrap, apply, Pair

class G[A]:
    a: A

class T[A]:
    a: A

class F:
    n: i64

def main() -> Result[(), IoError]:
    w = wrap(G(1))
    match w:
        case Some(g):
            print(g.a)?
        case Nothing:
            pass
    f = apply(3, def (x: i64) -> F: F(x))
    print(f.n)?
    p = Pair(T(1), T(2))
    (q, extra) = p.make(F(9), T(3), T(4))
    print(q.left.a)?
    print(extra.n)?
    Ok(())
"#,
            ),
        ],
        "main.plenty",
        "1\n3\n4\n9\n",
    );
    // A template's own parameter shadows an entry class of the same name in
    // its own module too, including in a callable bound and a protocol.
    run(
        &[(
            "main.plenty",
            r#"class G[A]:
    a: A

protocol Show[W]:
    def show(self, w: W) -> W:
        pass

class Shower:
    n: i64
    def show(self, w: i64) -> i64:
        w

class W[A]:
    a: A

def ident[G](value: G) -> G:
    value

def call[G, F: Callable[[G], G]](value: G, f: F) -> G:
    f(value)

def shown[S: Show[i64]](s: &S) -> i64:
    s.show(1)

def main() -> Result[(), IoError]:
    print(ident(3))?
    print(call(4, def (x: i64) -> i64: x))?
    print(shown(&Shower(2)))?
    g = G(W(1))
    print(g.a.a)?
    Ok(())
"#,
        )],
        "main.plenty",
        "3\n4\n1\n1\n",
    );
}
