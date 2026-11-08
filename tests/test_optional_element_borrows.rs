//! `dict.get_ref`, `list.get_ref` and `list.get_mut` lend a stored value in
//! place or return `Nothing`, with the whole-collection loan of
//! `dict.get_mut` (issue #869).
mod support;
use crate::test_text_bytes_fast_paths::runtime_calls;
use support::{assert_stdout, check_diagnostic as diagnostic};

const PRELUDE: &str = r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print("drop", self.label).unwrap()

@copy
class Pair:
    a: i64
    b: i64
"#;

fn program(body: &str) -> String {
    format!(
        "{PRELUDE}def main() -> Result[(), AllocError | IoError]:\n    mut xs = [[1]?, [2, 3]?]?\n    mut d = {{\"a\": [1]?}}?\n{body}    Ok(())\n"
    )
}

#[test]
fn dict_get_ref_lends_present_values_of_every_kind() {
    assert_stdout(
        &format!(
            "{PRELUDE}def main() -> Result[(), AllocError | IoError]:\n    groups = {{\"a\": [1, 2]?}}?\n    guards = {{\"g\": Guard(\"kept\")}}?\n    pairs = {{\"p\": Pair(1, 2)}}?\n    counts = {{\"n\": 7}}?\n    match groups.get_ref(\"a\"):\n        case Some(group):\n            print(len(group), group)?\n        case Nothing:\n            pass\n    match guards.get_ref(\"g\"):\n        case Some(guard):\n            print(guard.label)?\n        case Nothing:\n            pass\n    match pairs.get_ref(\"p\"):\n        case Some(pair):\n            print(pair.a, pair.b)?\n        case Nothing:\n            pass\n    print(counts.get_ref(\"n\"), counts.get_ref(\"m\"), groups.get_ref(\"z\").is_some())?\n    print(groups, len(guards))?\n    Ok(())\n"
        ),
        "2 [1, 2]\nkept\n1 2\nOption[&i64].Some(7) Option[&i64].Nothing False\n{\"a\": [1, 2]} 1\ndrop kept\n",
    );
}

#[test]
fn list_lookups_handle_signed_and_extreme_indices() {
    assert_stdout(
        "def main() -> Result[(), AllocError | IoError]:\n    mut xs = [10, 20, 30]?\n    for i in [-4, -3, -1, 0, 2, 3, -9223372036854775808, 9223372036854775807]?:\n        print(xs.get_ref(i), xs.get_mut(i).is_some())?\n    empty: list[str] = []?\n    print(empty.get_ref(0), empty.get_ref(-1))?\n    print(xs)?\n    Ok(())\n",
        "Option[&i64].Nothing False\n\
         Option[&i64].Some(10) True\n\
         Option[&i64].Some(30) True\n\
         Option[&i64].Some(10) True\n\
         Option[&i64].Some(30) True\n\
         Option[&i64].Nothing False\n\
         Option[&i64].Nothing False\n\
         Option[&i64].Nothing False\n\
         Option[&str].Nothing Option[&str].Nothing\n\
         [10, 20, 30]\n",
    );
}

#[test]
fn list_get_mut_changes_elements_in_place() {
    assert_stdout(
        &format!(
            "{PRELUDE}def main() -> Result[(), AllocError | IoError]:\n    mut nums = [1, 2, 3]?\n    mut rows = [[1]?, [2]?]?\n    mut guards = [Guard(\"a\"), Guard(\"b\")]?\n    mut pairs = [Pair(1, 2)]?\n    flags = [True]?\n    match nums.get_mut(1):\n        case Some(n):\n            *n = 20\n        case Nothing:\n            pass\n    match rows.get_mut(-1):\n        case Some(row):\n            row.append(3)?\n        case Nothing:\n            pass\n    match guards.get_mut(0):\n        case Some(guard):\n            old = replace(*guard, Guard(\"new\"))\n            print(\"took\", old.label)?\n        case Nothing:\n            pass\n    match pairs.get_mut(0):\n        case Some(pair):\n            pair.b = 5\n        case Nothing:\n            pass\n    print(nums, rows, pairs[0].b, flags.get_ref(0))?\n    print(guards[0].label, guards[1].label)?\n    Ok(())\n"
        ),
        "took a\ndrop a\n[1, 20, 3] [[1], [2, 3]] 5 Option[&bool].Some(True)\nnew b\ndrop new\ndrop b\n",
    );
}

#[test]
fn nested_projected_and_propagated_lookups() {
    assert_stdout(
        r#"
class Store:
    rows: list[list[i64]]
    names: dict[str, list[str]]

    def row(self: &Store, i: i64) -> Option[&list[i64]]:
        self.rows.get_ref(i)

    def set(self: &mut Store, i: i64, j: i64) -> Option[()]:
        row = self.rows.get_mut(i)?
        cell = row.get_mut(j)?
        *cell = 10
        Some(())

def main() -> Result[(), AllocError | IoError]:
    mut s = Store([[1, 2]?, [3]?]?, {"a": ["x"]?}?)
    print(s.row(-1), s.row(2))?
    print(s.set(0, 1), s.set(0, 7), s.set(4, 0))?
    print(s.rows)?
    match s.names.get_ref("a"):
        case Some(names):
            print(names.get_ref(0))?
        case Nothing:
            pass
    mut grid = [[1, 2]?]?
    match grid[0].get_mut(-2):
        case Some(cell):
            *cell = 7
        case Nothing:
            pass
    mut boxed = Box([5, 6]?)?
    match boxed.get_mut(1):
        case Some(cell):
            *cell = 60
        case Nothing:
            pass
    print(grid, boxed.get_ref(1))?
    r = &mut grid
    match r.get_mut(0):
        case Some(row):
            row.append(3)?
        case Nothing:
            pass
    print(grid)?
    Ok(())
"#,
        "Option[&list[i64]].Some([3]) Option[&list[i64]].Nothing\n\
         Option[()].Some(()) Option[()].Nothing Option[()].Nothing\n\
         [[1, 10], [3]]\n\
         Option[&str].Some(\"x\")\n\
         [[7, 2]] Option[&i64].Some(60)\n\
         [[7, 2, 3]]\n",
    );
}

#[test]
fn the_key_or_index_is_observed_once_before_the_receiver() {
    assert_stdout(
        r#"
def key() -> str:
    print("key").unwrap()
    "a"

def index() -> i64:
    print("index").unwrap()
    -1

def main() -> Result[(), AllocError | IoError]:
    mut d = {"a": [1]?}?
    mut xs = [1, 2]?
    match d.get_mut(key()):
        case Some(v):
            v.append(2)?
        case Nothing:
            pass
    print(d.get_ref(key()), xs.get_ref(index()))?
    match xs.get_mut(index()):
        case Some(v):
            *v = 9
        case Nothing:
            pass
    print(d, xs)?
    Ok(())
"#,
        "key\nkey\nindex\nOption[&list[i64]].Some([1, 2]) Option[&i64].Some(2)\nindex\n{\"a\": [1, 2]} [1, 9]\n",
    );
}

#[test]
fn a_live_result_borrows_the_whole_collection() {
    for (body, message) in [
        (
            "    match xs.get_ref(0):\n        case Some(v):\n            xs.append([4]?)?\n            print(v)?\n        case Nothing:\n            pass\n",
            "cannot modify or exclusively borrow `xs` while it is borrowed",
        ),
        (
            "    match d.get_ref(\"a\"):\n        case Some(v):\n            d.insert(\"b\", [2]?)?\n            print(v)?\n        case Nothing:\n            pass\n",
            "cannot modify or exclusively borrow `d` while it is borrowed",
        ),
        (
            "    match xs.get_mut(1):\n        case Some(v):\n            print(len(xs))?\n            v.append(4)?\n        case Nothing:\n            pass\n",
            "cannot read or borrow `xs` while it is exclusively borrowed",
        ),
        (
            "    match xs.get_mut(0):\n        case Some(v):\n            other = xs.get_ref(1)\n            v.append(4)?\n        case Nothing:\n            pass\n",
            "cannot read or borrow `xs` while it is exclusively borrowed",
        ),
        (
            "    v = xs.get_ref(0)\n    xs = []?\n    print(v)?\n",
            "while it is borrowed",
        ),
    ] {
        let error = diagnostic(&program(body));
        assert!(error.contains(message), "{body}: {error}");
    }
}

#[test]
fn shared_results_allow_reads_and_absent_results_hold_no_loan() {
    assert_stdout(
        &program(
            "    match xs.get_ref(1):\n        case Some(v):\n            print(v, xs[0], len(xs), xs.get_ref(0))?\n        case Nothing:\n            pass\n    match xs.get_mut(9):\n        case Some(v):\n            v.append(0)?\n        case Nothing:\n            xs.append([9]?)?\n    match d.get_ref(\"b\"):\n        case Some(v):\n            print(v)?\n        case Nothing:\n            d.insert(\"b\", [2]?)?\n    print(xs, d)?\n",
        ),
        "[2, 3] [1] 2 Option[&list[i64]].Some([1])\n[[1], [2, 3], [9]] {\"a\": [1], \"b\": [2]}\n",
    );
}

#[test]
fn receivers_and_arguments_are_checked() {
    for (body, message) in [
        (
            "    ys = [1]?\n    match ys.get_mut(0):\n        case Some(v):\n            *v = 2\n        case Nothing:\n            pass\n",
            "immutable binding",
        ),
        (
            "    view = &xs\n    match view.get_mut(0):\n        case Some(v):\n            v.append(2)?\n        case Nothing:\n            pass\n",
            "cannot borrow shared reference as mutable",
        ),
        (
            "    print([1]?.get_ref(0))?\n",
            "get_ref requires a list or dictionary binding or class field",
        ),
        (
            "    v = xs.get_ref(0, 1)\n",
            "get_ref requires one i64 index argument",
        ),
        ("    v = d.get_ref()\n", "get_ref requires one key argument"),
        ("    v = xs.get_mut(\"a\")\n", "expected i64, got str"),
        ("    v = d.get_ref(1)\n", "expected str, got i64"),
    ] {
        let error = diagnostic(&program(body));
        assert!(error.contains(message), "{body}: {error}");
    }
}

#[test]
fn a_lookup_is_one_runtime_call_that_counts_nothing() {
    let source = r#"
def first(names: &list[str], i: i64) -> Option[&str]:
    names.get_ref(i)

def entry(groups: &dict[str, list[str]], key: &str) -> Option[&list[str]]:
    groups.get_ref(key)

def main() -> Result[(), AllocError | IoError]:
    names = ["Ada"]?
    groups = {"a": ["Bea"]?}?
    print(first(&names, 0), entry(&groups, "a"))?
    Ok(())
"#;
    for (function, lookup) in [("first", "plenty_op_606"), ("entry", "plenty_op_113")] {
        let calls = runtime_calls(source, function);
        assert_eq!(
            calls.iter().filter(|name| *name == lookup).count(),
            1,
            "{function}: {calls:?}"
        );
        assert!(
            !calls
                .iter()
                .any(|name| name == "plenty_retain" || name == "plenty_release"),
            "{function}: {calls:?}"
        );
    }
}
