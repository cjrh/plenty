//! Expected types through generic class constructors, `Shared(...)` and
//! `Box(...)`, and from the first continuing branch of an unannotated block
//! `if` or `match` to the branches after it (issue #797).

mod support;

fn run(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn reject(source: &str, message: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(message), "expected {message}, got {error}");
}

const UNTYPED: &str =
    "this branch's value has no type: no annotation or earlier continuing branch supplies one";

#[test]
fn issue_797_reproduction_nested_shared_mutex_argument() {
    run(
        r#"
from std.sync import Mutex

def size(state: Shared[Mutex[list[i64]]]) -> Result[i64, AllocError]:
    mut count = 0
    with state.lock() as items:
        items.append(4)?
        count = len(items)
    Ok(count)

def main() -> Result[(), AllocError | IoError]:
    print(size(Shared(Mutex([]?))?)?)?
    Ok(())
"#,
        "1\n",
    );
}

#[test]
fn constructors_take_the_expected_instance() {
    run(
        r#"
from std.sync import Mutex

class Pair[T]:
    first: T
    second: T

enum Invalid:
    Rejected

class Checked[T]:
    value: T
    def __init__(self, value: T) -> Result[(), Invalid]:
        self.value = value
        Ok(())

def total(pair: Pair[list[i64]]) -> Result[i64, OverflowError[i64]]:
    Ok(len(pair.first) + len(pair.second))

def main() -> Result[(), AllocError | IoError | Invalid | OverflowError[i64]]:
    pair: Pair[list[i64]] = Pair([]?, [1, 2]?)
    print(total(pair)?, total(Pair([]?, []?))?)?
    checked: Checked[dict[str, i64]] = Checked({}?)?
    print(len(checked.value))?
    lock: Mutex[Option[i64]] = Mutex(Nothing)
    shared: Shared[list[u8]] = Shared([200]?)?
    boxed: Box[dict[str, i64]] = Box({}?)?
    print(len(*shared), len(*boxed))?
    nested: Option[Shared[Mutex[list[i64]]]] = Some(Shared(Mutex([]?))?)
    print(nested.is_some())?
    drop(lock)
    Ok(())
"#,
        "2 0\n0\n1 0\nTrue\n",
    );
}

#[test]
fn constructor_inference_keeps_its_arguments_without_a_matching_context() {
    // An anonymous sum names no class instance, so the arguments choose it
    // and the value widens as before.
    run(
        r#"
class Cell[T]:
    value: T

def show(value: Cell[i64] | str) -> Result[(), IoError]:
    match value:
        case cell: Cell[i64]:
            print(cell.value)?
        case text: str:
            print(text)?
    Ok(())

def main() -> Result[(), IoError]:
    show(Cell(3))?
    show("text")?
    Ok(())
"#,
        "3\ntext\n",
    );
    reject(
        r#"
class Cell[T]:
    value: T

def main() -> Result[(), AllocError]:
    cell = Cell([]?)
    Ok(())
"#,
        "empty collection needs a type",
    );
    reject(
        r#"
class Cell[T]:
    value: T

def main() -> Result[(), AllocError]:
    cell: Cell[list[i64]] = Cell("text")
    Ok(())
"#,
        "expected list[i64], got str",
    );
}

#[test]
fn later_branches_take_the_first_continuing_branch_type() {
    run(
        r#"
def option(n: i64) -> Option[i64]:
    value = match n:
        case 0:
            Some(1)
        case _:
            Nothing
    value

def items(n: i64) -> Result[list[i64], AllocError]:
    values = if n == 0:
        [n]?
    elif n == 1:
        []?
    else:
        list()?
    Ok(values)

def table(flag: bool) -> Result[dict[str, i64], AllocError]:
    values = match flag:
        case True:
            {"a": 1}?
        case False:
            {}?
    Ok(values)

def small(flag: bool) -> u8:
    value = if flag:
        u8(3)
    else:
        200
    value

def skipped(n: i64) -> Result[Option[str], IoError]:
    # An exiting case contributes no type; the first continuing one does.
    value = match n:
        case 0:
            return Ok(Nothing)
        case 1:
            Some("one")
        case _:
            Nothing
    Ok(value)

def main() -> Result[(), AllocError | IoError]:
    print(option(0), option(1))?
    print(items(0)?, items(1)?, items(2)?)?
    print(table(True)?, table(False)?)?
    print(small(True), small(False))?
    print(skipped(0)?, skipped(1)?, skipped(2)?)?
    Ok(())
"#,
        "Option[i64].Some(1) Option[i64].Nothing\n\
         [0] [] []\n\
         {\"a\": 1} {}\n\
         3 200\n\
         Option[str].Nothing Option[str].Some(\"one\") Option[str].Nothing\n",
    );
}

#[test]
fn a_first_branch_without_a_type_suggests_an_annotation() {
    reject(
        r#"
def main() -> Result[(), AllocError]:
    flag = True
    value = match flag:
        case True:
            Nothing
        case False:
            Some(1)
    Ok(())
"#,
        &format!("{UNTYPED}; annotate the binding, as in `name: Option[i64] = ...`"),
    );
    reject(
        r#"
def main() -> Result[(), AllocError]:
    flag = True
    values = if flag:
        []?
    else:
        [1]?
    Ok(())
"#,
        "as in `name: list[i64] = ...`, or put a branch with a typed value first",
    );
    reject(
        r#"
def main() -> Result[(), AllocError]:
    flag = True
    values = if flag:
        return Ok(())
    else:
        {}?
    Ok(())
"#,
        "as in `name: dict[str, i64] = ...`",
    );
    // An annotation still types every branch, in any order.
    run(
        r#"
def main() -> Result[(), AllocError | IoError]:
    flag = True
    value: Option[i64] = match flag:
        case True:
            Nothing
        case False:
            Some(1)
    print(value)?
    Ok(())
"#,
        "Option[i64].Nothing\n",
    );
}

#[test]
fn branches_still_disagree_without_conversion() {
    reject(
        r#"
def main() -> Result[(), AllocError]:
    flag = True
    value = if flag:
        1
    else:
        "one"
    Ok(())
"#,
        "expected i64, got str",
    );
    // A borrowed text element does not become an owned one.
    reject(
        r#"
def pick(text: &str, flag: bool) -> Result[list[str], AllocError]:
    values = if flag:
        ["a"]?
    else:
        [text]?
    Ok(values)

def main() -> Result[(), AllocError]:
    drop(pick("b", True)?)
    Ok(())
"#,
        "expected list[str], got list[&str]",
    );
}
