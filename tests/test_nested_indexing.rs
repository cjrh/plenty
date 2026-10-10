//! Brackets after a name are an index when the name is a local binding and
//! type arguments when it is a declaration (issue #23).
mod support;
use support::{check_source, run};

fn stdout(source: &str) -> String {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn error(source: &str) -> String {
    check_source(source).unwrap_err().to_string()
}

#[test]
fn an_indexed_name_indexes_a_local() {
    let output = stdout(
        r#"
def main() -> Result[(), Failure]:
    mut a = [2, 0, 1]?
    b = [1, 2, 0]?
    c = [0, 2, 1]?
    idx = [0, 2]?
    k = 1
    print(b[b[k]])?
    print(a[b[c[k]]])?
    print(a[idx[1]])?
    print(a[b[k] + 0])?
    print(a[(k)])?
    a[b[k]] = 7
    a[b[c[k]]] = 8
    print(a)?
    Ok(())
"#,
    );
    assert_eq!(output, "0\n2\n1\n1\n0\n[8, 0, 7]\n");
}

#[test]
fn dotted_local_bases_index_through_fields() {
    let output = stdout(
        r#"
class Table:
    items: list[i64]
    order: list[i64]
def main() -> Result[(), Failure]:
    mut table = Table([10, 20, 30]?, [2, 0, 1]?)
    picks = [1, 2, 0]?
    k = 0
    print(table.items[table.order[k]])?
    print(table.items[picks[table.order[k]]])?
    print(picks[table.order[picks[k]]])?
    table.items[table.order[k]] = 31
    print(table.items)?
    Ok(())
"#,
    );
    assert_eq!(output, "30\n10\n1\n[10, 20, 31]\n");
}

#[test]
fn a_local_shadowing_a_generic_function_is_indexed() {
    let output = stdout(
        r#"
def identity[T](value: T) -> T:
    value
def pick(identity: &list[i64], order: &list[i64], k: i64) -> i64:
    identity[order[k]]
def main() -> Result[(), Failure]:
    values = [4, 5, 6]?
    order = [2, 0, 1]?
    print(pick(&values, &order, 0))?
    print(identity[u8](7))?
    Ok(())
"#,
    );
    assert_eq!(output, "6\n7\n");
}

#[test]
fn indexed_callables_take_arguments() {
    let output = stdout(
        r#"
class Holder:
    callbacks: list[Callable[[i64], i64]]
def double(x: i64) -> i64:
    x * 2
def triple(x: i64) -> i64:
    x * 3
def main() -> Result[(), Failure]:
    handlers = [double, triple]?
    holder = Holder([double, triple]?)
    indices = [1, 0]?
    i = 0
    print(handlers[indices[i]](5))?
    print(holder.callbacks[indices[i]](5))?
    print(holder.callbacks[indices[indices[i]]](5))?
    print(holder.callbacks[i](5))?
    Ok(())
"#,
    );
    assert_eq!(output, "15\n15\n10\n10\n");
}

#[test]
fn nested_index_expressions_evaluate_once() {
    let output = stdout(
        r#"
class Counter:
    value: i64
    def next(self: &mut Counter) -> i64:
        self.value = self.value + 1
        self.value - 1
def main() -> Result[(), Failure]:
    mut a = [0, 0, 0]?
    b = [2, 1, 0]?
    mut counter = Counter(0)
    a[b[counter.next()]] = 5
    print(counter.value)?
    print(a[b[counter.next()]])?
    print(counter.value)?
    a[b[b[counter.next()]]] = 9
    print(counter.value)?
    print(a)?
    Ok(())
"#,
    );
    assert_eq!(output, "1\n0\n2\n3\n[0, 0, 9]\n");
}

#[test]
fn declarations_keep_type_arguments() {
    let output = stdout(
        r#"
type Count = u8
class Number:
    value: i64
    def convert[T: IntType](self) -> T:
        T(self.value)
def identity[T](value: T) -> T:
    value
def first[T](values: &list[list[T]]) -> T:
    values[0][0]
def main() -> Result[(), Failure]:
    keep = identity[u8]
    print(keep(7))?
    print(identity[Count](8))?
    print(identity[list[list[i64]]]([[1]?, [2]?]?))?
    nested = [[3]?]?
    print(first[i64](&nested))?
    number = Number(9)
    print(number.convert[Count]())?
    Ok(())
"#,
    );
    assert_eq!(output, "7\n8\n[[1], [2]]\n3\n9\n");
}

// The base decides: a type argument may share its name with a local binding.
#[test]
fn a_type_argument_named_like_a_local_stays_a_type_argument() {
    let output = stdout(
        r#"
type count = u8
def identity[T](value: T) -> T:
    value
def main() -> Result[(), Failure]:
    count = 300
    print(identity[count](7))?
    print(count)?
    Ok(())
"#,
    );
    assert_eq!(output, "7\n300\n");
}

#[test]
fn imported_generics_keep_type_arguments() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("maths.plenty"),
        "pub def identity[T](x: T) -> T:\n    x\n",
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        "import maths\nfrom maths import identity as keep\ntype Count = u8\ndef main() -> Result[(), Failure]:\n    order = [1, 0]?\n    k = 0\n    print(keep[Count](7))?\n    print(maths.identity[list[i64]]([1, 2]?))?\n    print(maths.identity[i64](order[order[k]]))?\n    Ok(())\n",
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, Some(dir.path())).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n[1, 2]\n0\n");
}

#[test]
fn two_indices_are_reported_at_the_second() {
    let source = "def main() -> Result[(), Failure]:\n    b = [1, 0]?\n    k = 1\n    print(b[k, k])?\n    Ok(())\n";
    let message = error(source);
    assert!(message.contains("4:16"), "{message}");
    assert!(
        message.contains("indexing takes one expression"),
        "{message}"
    );
    let nested = "def main() -> Result[(), Failure]:\n    b = [1, 0]?\n    k = 1\n    print(b[b[k, k]])?\n    Ok(())\n";
    let message = error(nested);
    assert!(message.contains("4:18"), "{message}");
    assert!(
        message.contains("indexing takes one expression"),
        "{message}"
    );
}

#[test]
fn type_syntax_cannot_index_a_local() {
    for (index, column) in [
        ("b[list[i64]]", 13),
        ("b[Callable[[i64], i64]]", 13),
        ("b[b[dict[str, i64]]]", 15),
        ("b[list[i64]](1)", 13),
    ] {
        let source = format!(
            "def main() -> Result[(), Failure]:\n    b = [1, 0]?\n    print({index})?\n    Ok(())\n"
        );
        let message = error(&source);
        assert!(message.contains(&format!("3:{column}:")), "{message}");
        assert!(
            message.contains("`b` is a local binding, so these brackets index it"),
            "{message}"
        );
    }
}
