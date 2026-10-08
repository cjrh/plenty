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
fn generic_data_imports_keep_methods_and_constructors_visible() {
    run(&[
        ("data.plenty", "pub class Cell[T]:\n    pub value: T\n    pub def get(self) -> T:\n        self.value\npub enum Choice[T]:\n    Value(T)\npub def read[T](cell: &Cell[T]) -> T:\n    cell.get()\n"),
        ("main.plenty", "import data\nfrom data import Cell as Box\ndef main() -> Result[(), Failure]:\n    box = Box(7u8)?\n    print(data.read(&box))?\n    item = data.Choice[u8].Value(9)?\n    match item:\n        case data.Choice[u8].Value(value):\n            print(value)?\n    Ok(())\n"),
    ], "main.plenty", "7\n9\n");
}

#[test]
fn generic_data_cannot_bypass_member_privacy() {
    for body in [
        "cell = data.Cell(1)",
        "cell = data.Cell[i64](1)",
        "f: Callable[[i64], Result[data.Cell[i64], AllocError]] = data.Cell",
        "cell = data.make().unwrap()\n    print(cell.value).unwrap()",
        "cell = data.make().unwrap()\n    cell.hidden()",
    ] {
        let dir = workspace(&[
            ("data.plenty", "pub class Cell[T]:\n    value: T\n    def hidden(self) -> ():\n        pass\npub def make() -> Result[Cell[i64], AllocError]:\n    Cell(1)\n"),
            ("main.plenty", &format!("import data\ndef main() -> ():\n    {body}\n    pass\n")),
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
        let dir = workspace(&[("data.plenty", source), ("main.plenty", "import data\ndef main() -> ():\n    pass\n")]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), Some(dir.path())).unwrap_err().to_string();
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
        let error = plenty::check_file(&dir.path().join("main.plenty"), Some(dir.path())).unwrap_err().to_string();
        assert!(error.contains("private"), "{error}");
    }
}

#[test]
fn function_values_keep_import_visibility_and_definition_scope() {
    run(&[
        ("api.plenty", "def hidden(x: i64) -> i64:\n    x + 1\npub def identity[T](x: T) -> T:\n    x\npub def make() -> Callable[[i64], i64]:\n    def(x: i64) -> i64:\n        hidden(x)\n"),
        ("main.plenty", "import api\nfrom api import identity as keep\ndef main() -> Result[(), Failure]:\n    f = keep[u8]\n    print(f(7))?\n    print(api.make()(8))?\n    Ok(())\n"),
    ], "main.plenty", "7\n9\n");
    for api in [
        "class Hidden:\n    value: i64\npub def leak(callback: Callable[[Hidden], i64]) -> ():\n    pass\n",
        "class Hidden:\n    value: i64\npub def leak() -> Callable[[], Hidden]:\n    pass\n",
    ] {
        let dir = workspace(&[("api.plenty", api), ("main.plenty", "import api\ndef main() -> ():\n    pass\n")]);
        let error = plenty::check_file(&dir.path().join("main.plenty"), Some(dir.path())).unwrap_err().to_string();
        assert!(error.contains("private"), "{error}");
    }
}

#[test]
fn explicit_generics_resolve_imports_aliases_and_definition_scope() {
    run(&[
        ("maths.plenty", "def helper(x: i64) -> i64:\n    x + 1\npub def identity[T](x: T) -> T:\n    x\npub def bumped[T: IntType](x: T) -> i64:\n    helper(i64(x))\n"),
        ("main.plenty", "import maths\nfrom maths import identity as keep\ntype Count = u8\ndef main() -> ():\n    print(keep[Count](7)).unwrap()\n    print(maths.identity[list[i64]]([1, 2].unwrap())).unwrap()\n    print(maths.bumped[u16](4)).unwrap()\n"),
    ], "main.plenty", "7\n[1, 2]\n5\n");
}

#[test]
fn protocol_imports_do_not_activate_methods_and_preserve_visibility() {
    let api = "pub protocol Readable:\n    def read(self) -> i64:\n        pass\npub def read[T: Readable](source: &T) -> i64:\n    source.read()\n";
    let implementation =
        "pub class Box:\n    pub value: i64\n    pub def read(self) -> i64:\n        self.value\n";
    let main = "import api\nfrom data import Box\ndef main() -> ():\n    box = Box(7).unwrap()\n    print(box.read()).unwrap()\n    print(api.read[Box](&box)).unwrap()\n";
    run(
        &[
            ("api.plenty", api),
            ("data.plenty", implementation),
            ("main.plenty", main),
        ],
        "main.plenty",
        "7\n7\n",
    );
    let inferred_main = main.replace("api.read[Box]", "api.read");
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
    let inferred = private_main.replace("api.read[Box]", "api.read");
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
    print(data.Numbers.new()).unwrap()
    print(build()).unwrap()
"#,
            ),
        ],
        "main.plenty",
        "Result[list[i64], AllocError].Ok([])\nResult[list[i64], AllocError].Ok([42])\n",
    );
}

#[test]
fn nominal_and_generator_constructors_resolve_imports() {
    run(&[
        ("data.plenty", r#"
pub class Number:
    pub value: i64
pub enum Message:
    Value(i64)
pub def numbers() -> Generator[i64]:
    yield 5
"#),
        ("main.plenty", r#"
import data
from data import Number as N
def collect() -> Result[list[i64], AllocError]:
    source = data.numbers()
    list[i64].from(source)
def main() -> ():
    print(N.new(3)).unwrap()
    print(data.Message.Value.new(4)).unwrap()
    print(collect()).unwrap()
"#),
    ], "main.plenty", "Result[data.Number, AllocError].Ok(data.Number(value=3))\nResult[data.Message, AllocError].Ok(data.Message.Value(4))\nResult[list[i64], AllocError].Ok([5])\n");
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
    run(&[
        ("app/main.plenty", "import math.arithmetic\nfrom math.arithmetic import twice as double\nimport math.arithmetic as numbers\ndef main() -> ():\n    print(math.arithmetic.twice(10)).unwrap()\n    print(double(11)).unwrap()\n    print(numbers.twice(12)).unwrap()\n"),
        ("math/arithmetic.plenty", "pub def twice(n: i64) -> i64:\n    helper(n)\ndef helper(n: i64) -> i64:\n    n * 2\ndef main() -> ():\n    print('not an entrypoint').unwrap()\n"),
    ], "app/main.plenty", "20\n22\n24\n");
}

#[test]
fn imported_main_is_an_ordinary_function_and_library_check_needs_no_entry() {
    let dir = workspace(&[("lib.plenty", "pub def main(n: i64) -> i64:\n    n + 1\n")]);
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
                "import lib\ndef main() -> ():\n    print(lib.main(41)).unwrap()\n",
            ),
            ("lib.plenty", "pub def main(n: i64) -> i64:\n    n + 1\n"),
        ],
        "main.plenty",
        "42\n",
    );
    let dir = workspace(&[("empty.plenty", "pub type Count = u32\n")]);
    plenty::check_module_file(&dir.path().join("empty.plenty"), None).unwrap();
    assert!(plenty::check_file(&dir.path().join("empty.plenty"), None).is_err());
}

const MODEL: &str = "pub class Counter:\n    value: i64\n    pub def __init__(self, value: i64) -> ():\n        self.value = value\n    pub def get(self) -> i64:\n        self.value\n    pub def increment(self: &mut Counter) -> ():\n        self.value = self.value + 1\n    def hidden(self) -> i64:\n        self.value\n    def __del__(self) -> ():\n        print(self.value).unwrap()\npub def peek(x: &Counter) -> i64:\n    x.value\n";

#[test]
fn public_methods_construction_and_private_drop_work_across_modules() {
    run(&[
        ("main.plenty", "from model import Counter, peek\ndef main() -> ():\n    mut counter = Counter(40).unwrap()\n    counter.increment()\n    print(counter.get()).unwrap()\n    print(peek(&counter)).unwrap()\n"),
        ("model.plenty", MODEL),
    ], "main.plenty", "41\n41\n41\n");
    for operation in [
        "print(c.value).unwrap()",
        "c.value = 42",
        "r = &c.value",
        "r = &mut c.value",
        "print(c.hidden()).unwrap()",
    ] {
        let source = format!(
            "import model\ndef main() -> ():\n    mut c = model.Counter(1).unwrap()\n    {operation}\n"
        );
        reject(
            &[("main.plenty", &source), ("model.plenty", MODEL)],
            "private",
        );
    }
}

#[test]
fn public_fields_generated_constructors_and_qualified_enum_patterns() {
    run(&[
        ("main.plenty", "import model as m\nfrom model import Event as E, Count\ndef read(x: m.Event) -> i64:\n    match x:\n        case m.Event.Value(n):\n            n\n        case E.Empty:\n            0\ndef main() -> ():\n    mut p = m.Point(3).unwrap()\n    p.x = 4\n    print(p.x).unwrap()\n    print(read(m.Event.Value(42).unwrap())).unwrap()\n    n: Count = Count(7)\n    print(n).unwrap()\n"),
        ("model.plenty", "pub type Count = u32\npub class Point:\n    pub x: i64\npub enum Event:\n    Value(i64)\n    Empty\n"),
    ], "main.plenty", "4\n42\n7\n");
}

#[test]
fn private_constructors_and_aliases_cannot_bypass_visibility() {
    for body in [
        "pub class Secret:\n    value: i64\n",
        "pub class Secret:\n    pub value: i64\n    def __init__(self, value: i64) -> ():\n        self.value = value\n",
    ] {
        reject(&[
            ("main.plenty", "from lib import Secret\ntype Alias = Secret\ndef main() -> ():\n    value = Alias(42).unwrap()\n"),
            ("lib.plenty", body),
        ], "__new__` is private");
    }
    run(&[
        ("main.plenty", "from lib import make\ndef main() -> ():\n    value = make()\n    print(value.get()).unwrap()\n"),
        ("lib.plenty", "pub class Secret:\n    value: i64\n    pub def get(self) -> i64:\n        self.value\npub def make() -> Secret:\n    Secret(42).unwrap()\n"),
    ], "main.plenty", "42\n");
}

#[test]
fn public_signatures_cannot_expose_private_nominal_types() {
    for api in [
        "pub def f(x: Hidden) -> ():\n    pass\n",
        "pub type Exposed = Alias\n",
        "pub enum Exposed:\n    Value(list[Alias])\n",
        "pub class Exposed:\n    pub value: Option[Alias]\n",
        "pub def f() -> Result[Hidden, str]:\n    Ok(Hidden(1).unwrap())\n",
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
                "from model import Point\npub def make() -> Point:\n    Point(42).unwrap()\n",
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
                "import left, right\ndef main() -> ():\n    p: left.Point = right.Point(1).unwrap()\n",
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
        "conflicts with an existing name",
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
    assert!(plenty::check_file(
        &dir.path().join("lib.plenty"),
        Some(&dir.path().join("app"))
    )
    .is_err());
}

#[test]
fn in_memory_api_never_searches_the_filesystem_for_imports() {
    let error = plenty::check_source("import lib\ndef main() -> ():\n    pass\n").unwrap_err();
    assert!(error.to_string().contains("file-based compilation"));
}

#[test]
fn imported_generators_enums_and_tail_recursion_keep_their_identities() {
    run(&[
        ("main.plenty", "import data\ndef main() -> ():\n    for value in data.values():\n        match value:\n            case data.Reading.Value(n):\n                print(n).unwrap()\n            case data.Reading.Empty:\n                pass\n    print(data.count(100_000)).unwrap()\n"),
        ("data.plenty", "pub enum Reading:\n    Value(i64)\n    Empty\npub def values() -> Generator[Reading]:\n    yield Reading.Value(42).unwrap()\n    yield (Reading.Empty).unwrap()\npub def count(n: i64) -> i64:\n    if n == 0:\n        0\n    else:\n        count(n - 1)\n"),
    ], "main.plenty", "42\n0\n");
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
                "import pkg.a, pkg.b\ndef main() -> ():\n    print(pkg.a.f() + pkg.b.f()).unwrap()\n",
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
        ("main.plenty", "import original, alias\ndef main() -> ():\n    p: original.Point = alias.Point(42).unwrap()\n    print(p.x).unwrap()\n"),
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
