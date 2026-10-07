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
fn explicit_generics_resolve_imports_aliases_and_definition_scope() {
    run(&[
        ("maths.plenty", "def helper(x: i64) -> i64:\n    x + 1\npub def identity[T](x: T) -> T:\n    x\npub def bumped[T: IntType](x: T) -> i64:\n    helper(i64(x))\n"),
        ("main.plenty", "import maths\nfrom maths import identity as keep\ntype Count = u8\ndef main() -> ():\n    print(keep[Count](7))\n    print(maths.identity[list[i64]]([1, 2]))\n    print(maths.bumped[u16](4))\n"),
    ], "main.plenty", "7\n[1, 2]\n5\n");
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
    mut values = Values.try_with_capacity(1)?
    values.try_append(42)?
    Ok(values)
def main() -> ():
    print(data.Numbers.try_new())
    print(build())
"#,
            ),
        ],
        "main.plenty",
        "Result[list[i64], AllocError].Ok([])\nResult[list[i64], AllocError].Ok([42])\n",
    );
}

#[test]
fn fallible_nominal_and_generator_constructors_resolve_imports() {
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
    source = data.numbers.try_new()?
    list[i64].try_from(source)
def main() -> ():
    print(N.try_new(3))
    print(data.Message.Value.try_new(4))
    print(collect())
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
                "import data\ndef main() -> ():\n    drop(data.Secret.try_new(1))\n",
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
        ("app/main.plenty", "import math.arithmetic\nfrom math.arithmetic import twice as double\nimport math.arithmetic as numbers\ndef main() -> ():\n    print(math.arithmetic.twice(10))\n    print(double(11))\n    print(numbers.twice(12))\n"),
        ("math/arithmetic.plenty", "pub def twice(n: i64) -> i64:\n    helper(n)\ndef helper(n: i64) -> i64:\n    n * 2\ndef main() -> ():\n    print('not an entrypoint')\n"),
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
                "import lib\ndef main() -> ():\n    print(lib.main(41))\n",
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

const MODEL: &str = "pub class Counter:\n    value: i64\n    pub def __init__(self, value: i64) -> ():\n        self.value = value\n    pub def get(self) -> i64:\n        self.value\n    pub def increment(self: &mut Counter) -> ():\n        self.value = self.value + 1\n    def hidden(self) -> i64:\n        self.value\n    def __del__(self) -> ():\n        print(self.value)\npub def peek(x: &Counter) -> i64:\n    x.value\n";

#[test]
fn public_methods_construction_and_private_drop_work_across_modules() {
    run(&[
        ("main.plenty", "from model import Counter, peek\ndef main() -> ():\n    mut counter = Counter(40)\n    counter.increment()\n    print(counter.get())\n    print(peek(&counter))\n"),
        ("model.plenty", MODEL),
    ], "main.plenty", "41\n41\n41\n");
    for operation in [
        "print(c.value)",
        "c.value = 42",
        "r = &c.value",
        "r = &mut c.value",
        "print(c.hidden())",
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
    run(&[
        ("main.plenty", "import model as m\nfrom model import Event as E, Count\ndef read(x: m.Event) -> i64:\n    match x:\n        case m.Event.Value(n):\n            n\n        case E.Empty:\n            0\ndef main() -> ():\n    mut p = m.Point(3)\n    p.x = 4\n    print(p.x)\n    print(read(m.Event.Value(42)))\n    n: Count = Count(7)\n    print(n)\n"),
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
            ("main.plenty", "from lib import Secret\ntype Alias = Secret\ndef main() -> ():\n    value = Alias(42)\n"),
            ("lib.plenty", body),
        ], "__new__` is private");
    }
    run(&[
        ("main.plenty", "from lib import make\ndef main() -> ():\n    value = make()\n    print(value.get())\n"),
        ("lib.plenty", "pub class Secret:\n    value: i64\n    pub def get(self) -> i64:\n        self.value\npub def make() -> Secret:\n    Secret(42)\n"),
    ], "main.plenty", "42\n");
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
                "import left, right\ndef main() -> ():\n    print(right.read(left.make()))\n",
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
            ("lib.plenty", "print('must not run')\n"),
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
        ("main.plenty", "import data\ndef main() -> ():\n    for value in data.values():\n        match value:\n            case data.Reading.Value(n):\n                print(n)\n            case data.Reading.Empty:\n                pass\n    print(data.count(100_000))\n"),
        ("data.plenty", "pub enum Reading:\n    Value(i64)\n    Empty\npub def values() -> Generator[Reading]:\n    yield Reading.Value(42)\n    yield Reading.Empty\npub def count(n: i64) -> i64:\n    if n == 0:\n        0\n    else:\n        count(n - 1)\n"),
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
                "import pkg.a, pkg.b\ndef main() -> ():\n    print(pkg.a.f() + pkg.b.f())\n",
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
                "pub def bad() -> ():\n    a = [1]\n    b = a\n    print(a)\n",
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
        ("main.plenty", "import original, alias\ndef main() -> ():\n    p: original.Point = alias.Point(42)\n    print(p.x)\n"),
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
