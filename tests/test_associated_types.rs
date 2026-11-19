mod support;

use support::{assert_stdout, check_source};

const SOURCE: &str = r#"
protocol Source:
    type Item: KeyType

    def read(self) -> Option[Item]:
        pass

    def fallback(self) -> Item:
        pass

class Counter:
    def read(self) -> Option[i64]:
        Some(1)

    def fallback(self) -> i64:
        0

def read_once[S: Source](source: &S) -> Option[S.Item]:
    chosen: Option[S.Item] = source.read()
    chosen
"#;

const TRANSFER: &str = r#"
protocol Sink:
    type Input

    def put(self: &mut Sink, value: Input) -> ():
        pass

class Cell:
    value: i64

    def put(self: &mut Cell, value: i64) -> ():
        self.value = value

def transfer_one[D: Sink[Input = S.Item], S: Source](source: &S, dest: &mut D) -> ():
    match source.read():
        case Some(item):
            dest.put(item)
        case Nothing:
            pass
"#;

#[test]
fn projections_resolve_in_signatures_bodies_calls_and_function_values() {
    let source = format!(
        "{SOURCE}{TRANSFER}{}",
        r#"
def identity[T](value: T) -> T:
    value

def select[S: Source](source: &S, item: Option[S.Item]) -> S.Item:
    match item:
        case Some(value):
            identity[S.Item](value)
        case Nothing:
            source.fallback()

enum Number:
    One

    def read(self) -> Option[i64]:
        Some(2)

    def fallback(self) -> i64:
        0

def main() -> Result[(), IoError]:
    source = Counter()
    mut cell = Cell(0)
    result: Option[i64] = read_once(&source)
    print(result, read_once[Counter](&source))?
    transfer_one(&source, &mut cell)
    print(cell.value)?
    transfer_one[Cell, Number](&Number.One, &mut cell)
    print(cell.value)?
    selected: Option[i64] = Some(3)
    print(select(&source, selected), select[Counter](&source, Some(4)))?
    callback: Callable[[&Counter], Option[i64]] = read_once
    print(callback(&source))?
    Ok(())
"#
    );
    assert_stdout(
        &source,
        "Option[i64].Some(1) Option[i64].Some(1)\n1\n2\n3 4\nOption[i64].Some(1)\n",
    );
}

#[test]
fn ordinary_protocol_arguments_infer_alongside_associated_members() {
    assert_stdout(
        r#"
protocol Source[Tag]:
    type Item

    def read(self) -> (Tag, Option[Item]):
        pass

class Counter:
    def read(self) -> (str, Option[i64]):
        ("count", Some(1))

def tagged[Tag, S: Source[Tag, Item = i64]](source: &S) -> (Tag, Option[S.Item]):
    source.read()

def main() -> Result[(), IoError]:
    print(tagged(&Counter()))?
    print(tagged[str, Counter](&Counter()))?
    Ok(())
"#,
        "(\"count\", Option[i64].Some(1))\n(\"count\", Option[i64].Some(1))\n",
    );
}

#[test]
fn conflicts_in_unused_requirements_and_associated_bounds_reject_conformance() {
    let conflict = SOURCE.replace(
        "def fallback(self) -> i64:\n        0",
        "def fallback(self) -> str:\n        \"none\"",
    );
    assert_error(
        &conflict,
        "drop(read_once(&Counter()))",
        "conflicting types for `Item`",
    );
    assert_error(
        &conflict,
        "drop(read_once[Counter](&Counter()))",
        "conflicting types for `Item`",
    );

    let non_key = SOURCE
        .replace("Option[i64]", "Option[f64]")
        .replace(
            "def fallback(self) -> i64:\n        0",
            "def fallback(self) -> f64:\n        0.0",
        )
        .replace("Some(1)", "Some(1.0)");
    assert_error(
        &non_key,
        "drop(read_once(&Counter()))",
        "`Source.Item`: f64 does not satisfy KeyType",
    );
}

#[test]
fn equality_and_call_arguments_cannot_change_signature_evidence() {
    let mismatch = TRANSFER.replace("value: i64", "value: str");
    let source = format!("{SOURCE}{mismatch}");
    for call in [
        "transfer_one(&Counter(), &mut cell)",
        "transfer_one[Cell, Counter](&Counter(), &mut cell)",
    ] {
        assert_error(
            &source,
            &format!("mut cell = Cell(\"zero\")\n    {call}"),
            "equality for `D.Input` requires `S.Item` = i64, but `D.Input` is str",
        );
    }

    let fixed = format!(
        "{SOURCE}{}",
        r#"
def take[S: Source[Item = str]](source: &S) -> S.Item:
    source.fallback()

def accept[S: Source](value: S.Item, source: &S) -> S.Item:
    value
"#
    );
    assert_error(
        &fixed,
        "drop(take(&Counter()))",
        "equality for `S.Item` requires str",
    );
    assert_error(
        &fixed,
        r#"drop(accept("text", &Counter()))"#,
        "expected i64, got str",
    );
    assert_error(
        &fixed,
        "result: Option[str] = read_once(&Counter())",
        "expected Option[str]",
    );
}

#[test]
fn declarations_reject_missing_evidence_and_invalid_projections() {
    assert_error(
        "protocol Source:\n    type Item\n    def read(self) -> i64:\n        pass\ndef use[S: Source[Item = i64]](source: &S) -> ():\n    pass\n",
        "pass",
        "associated type `Source.Item` must be determined",
    );
    assert_error(
        &SOURCE.replace("chosen: Option[S.Item]", "chosen: Option[S.Other]"),
        "pass",
        "protocol `Source` has no associated type `Other`",
    );
    assert_error(
        &SOURCE.replace("S: Source]", "S: Source[Item = i64, Item = i64]]"),
        "pass",
        "duplicate equality constraint for `Source.Item`",
    );
    assert_error(
        SOURCE,
        "value: Box[Source] = Box[Source](Counter())",
        "cannot box protocol `Source` with associated types",
    );
}

#[test]
fn public_projections_keep_module_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let api = dir.path().join("api.plenty");
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &api,
        r#"
pub protocol Source:
    type Item

    def read(self) -> Item:
        pass

pub def read[S: Source](source: &S) -> S.Item:
    source.read()
"#,
    )
    .unwrap();
    let main_source = r#"
import api

pub class Counter:
    pub def read(self) -> i64:
        1

def main() -> ():
    result: i64 = api.read(&Counter())
    drop(result)
"#;
    std::fs::write(&main, main_source).unwrap();
    plenty::check_file(&main, None).unwrap();
    std::fs::write(&main, main_source.replace("pub def read", "def read")).unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(error.contains("`Counter.read` is private"), "{error}");
}

#[test]
fn qualified_type_names_still_resolve_when_the_prefix_is_a_type_parameter() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("api.plenty"),
        r#"
pub class Record:
    pub def get(self) -> i64:
        9
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
import api as S

protocol Source:
    type Item

    def read(self) -> Item:
        pass

class Counter:
    def read(self) -> i64:
        1

def extract[S](unused: S, record: &S.Record) -> i64:
    record.get()

def combine[S: Source](source: &S, record: &S.Record) -> S.Item:
    drop(record.get())
    source.read()

def main() -> ():
    record = S.Record()
    drop(extract(0, &record))
    drop(combine(&Counter(), &record))
"#,
    )
    .unwrap();
    plenty::check_file(&main, None).unwrap();
}

#[test]
fn projections_do_not_replace_qualified_types_or_constructors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("api.plenty"),
        r#"
pub class Item:
    value: i64

    pub def __init__(self: &mut Item, value: i64) -> ():
        self.value = value
        drop(print(self.value))
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
import api as other

protocol Source:
    type Item

    def read(self) -> Item:
        pass

class Counter:
    def read(self) -> i64:
        1

def use[api: Source](source: &api, record: &other.Item) -> api.Item:
    drop(other.Item(2))
    source.read()

def main() -> ():
    record = other.Item(9)
    drop(use(&Counter(), &record))
    drop(use[Counter](&Counter(), &record))
"#,
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"9\n2\n2\n");
}

#[test]
fn atomic_types_supply_and_accept_associated_evidence() {
    let source = r#"
from std.atomic import Atomic

protocol Source:
    type Item: IntType

    def read(self) -> Atomic[Item]:
        pass

    def fallback(self) -> Item:
        pass

class Counter:
    def read(self) -> Atomic[i64]:
        Atomic[i64](42)

    def fallback(self) -> i64:
        42

def accept[S: Source](source: &S, value: Atomic[S.Item]) -> Atomic[S.Item]:
    value
"#;
    let main = "drop(accept(&Counter(), Atomic[i64](1)))\n    drop(accept[Counter](&Counter(), Atomic[i64](2)))";
    check_source(&format!("{source}\ndef main() -> ():\n    {main}\n")).unwrap();
    let conflict = source.replace("def fallback(self) -> i64:", "def fallback(self) -> u64:");
    assert_error(
        &conflict,
        "drop(accept(&Counter(), Atomic[i64](1)))",
        "conflicting types for `Item`",
    );
}

#[test]
fn exported_atomic_signatures_keep_their_type_arguments() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("api.plenty"),
        r#"
from std.atomic import Atomic

pub protocol Source:
    type Item: IntType

    def read(self) -> Atomic[Item]:
        pass

pub def read[S: Source](source: &S) -> Atomic[S.Item]:
    source.read()

pub def accept[S: Source](source: &S, value: Atomic[S.Item]) -> Atomic[S.Item]:
    value

pub def pass_atomic[T: IntType](value: Atomic[T]) -> Atomic[T]:
    value
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
import api
from std.atomic import Atomic

pub class Counter:
    pub def read(self) -> Atomic[i64]:
        Atomic[i64](42)

def main() -> ():
    counter = Counter()
    drop(api.accept(&counter, api.read(&counter)))
    drop(api.accept[Counter](&counter, api.pass_atomic(Atomic[i64](1))))
"#,
    )
    .unwrap();
    plenty::check_file(&main, None).unwrap();
}

#[test]
fn const_arguments_cannot_use_associated_type_members() {
    for written in [
        "array[i64, Item]",
        "Bounded[i64, 0, Item]",
        "Count[Item]",
        "Mode[Item]",
    ] {
        let protocol = format!(
            r#"
class Count[const N: i64]:
    value: i64

enum Mode[const N: i64]:
    Empty

protocol Source:
    type Item

    def read(self) -> {written}:
        pass
"#
        );
        assert_error(
            &protocol,
            "pass",
            "an associated type cannot be used as a const argument",
        );
    }
    let source = r#"
const count: i64 = 2

protocol Source:
    type Item

    def read(self) -> array[Item, count]:
        pass

class Pair:
    def read(self) -> array[i64, 2]:
        [1, 2]

def read[S: Source](source: &S) -> array[S.Item, count]:
    source.read()

def main() -> ():
    drop(read(&Pair()))
"#;
    check_source(source).unwrap();
    let invalid = source.replace("array[S.Item, count]", "array[i64, S.Item]");
    let error = check_source(&invalid).unwrap_err().to_string();
    assert!(
        error.contains("an associated type cannot be used as a const argument"),
        "{error}"
    );
}

#[test]
fn private_module_members_keep_their_diagnostic_with_a_shadowing_type_parameter() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("api.plenty"), "class Hidden:\n    pass\n").unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        "import api as S\ndef use[S](value: S, secret: &S.Hidden) -> ():\n    pass\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(
        error.contains("`S.Hidden` is private or is not a declared export"),
        "{error}"
    );
}

#[test]
fn public_associated_equalities_cannot_expose_private_array_elements() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("api.plenty"),
        r#"
class Secret:
    id: i64

pub protocol Source:
    type Item

    def read(self) -> Item:
        pass

pub def read[S: Source[Item = array[Secret, 2]]](source: &S) -> S.Item:
    source.read()
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(&main, "import api\ndef main() -> ():\n    pass\n").unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(
        error.contains("public signature exposes private type `api.Secret`"),
        "{error}"
    );
}

fn assert_error(prefix: &str, main: &str, expected: &str) {
    let program = format!("{prefix}\ndef main() -> ():\n    {main}\n");
    let error = check_source(&program).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}
