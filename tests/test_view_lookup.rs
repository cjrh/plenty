//! `v.get_ref(i)` on a slice view and `v.get_mut(i)` on an exclusive view
//! lend a row in place or return `Nothing`, with `list.get_ref`'s and
//! `list.get_mut`'s contracts (issue #890).
mod support;
use crate::test_text_bytes_fast_paths::runtime_calls;
use support::{assert_stdout, check_diagnostic as diagnostic};

const PRELUDE: &str = r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print("drop", self.label).unwrap()
"#;

fn program(body: &str) -> String {
    format!(
        "{PRELUDE}def main() -> Result[(), AllocError | IoError]:\n    mut xs = [[1]?, [2, 3]?, [4]?, [5]?]?\n{body}    Ok(())\n"
    )
}

#[test]
fn indices_count_within_the_view_and_from_its_end() {
    assert_stdout(
        "def main() -> Result[(), AllocError | IoError]:\n    mut xs = [0, 10, 20, 30, 40]?\n    v = &xs[1:4]\n    for i in [-4, -3, -1, 0, 2, 3, -9223372036854775808, 9223372036854775807]?:\n        print(v.get_ref(i))?\n    w = &mut xs[1:4]\n    print(w.get_mut(-1).is_some(), w.get_mut(3).is_some(), w.get_ref(0))?\n    e = &xs[2:2]\n    print(e.get_ref(0), e.get_ref(-1))?\n    print(xs)?\n    Ok(())\n",
        "Option[&i64].Nothing\n\
         Option[&i64].Some(10)\n\
         Option[&i64].Some(30)\n\
         Option[&i64].Some(10)\n\
         Option[&i64].Some(30)\n\
         Option[&i64].Nothing\n\
         Option[&i64].Nothing\n\
         Option[&i64].Nothing\n\
         True False Option[&i64].Some(10)\n\
         Option[&i64].Nothing Option[&i64].Nothing\n\
         [0, 10, 20, 30, 40]\n",
    );
}

#[test]
fn get_mut_changes_rows_of_every_kind_in_place() {
    assert_stdout(
        &format!(
            "{PRELUDE}def main() -> Result[(), AllocError | IoError]:\n    mut nums = [1, 2, 3, 4]?\n    mut rows = [[1]?, [2]?, [3]?]?\n    mut guards = [Guard(\"a\"), Guard(\"b\"), Guard(\"c\")]?\n    mut names = [\"x\", \"y\"]?\n    w = &mut nums[1:3]\n    match w.get_mut(-1):\n        case Some(n):\n            *n = 30\n        case Nothing:\n            pass\n    r = &mut rows[1:]\n    match r.get_mut(0):\n        case Some(row):\n            row.append(20)?\n        case Nothing:\n            pass\n    g = &mut guards[1:]\n    match g.get_mut(1):\n        case Some(guard):\n            old = replace(*guard, Guard(\"new\"))\n            print(\"took\", old.label)?\n        case Nothing:\n            pass\n    match g.get_ref(0):\n        case Some(guard):\n            print(guard.label)?\n        case Nothing:\n            pass\n    s = &names[1:]\n    print(s.get_ref(0), s.get_ref(1))?\n    print(nums, rows)?\n    print(guards[0].label, guards[1].label, guards[2].label)?\n    Ok(())\n"
        ),
        "took c\ndrop c\nb\nOption[&str].Some(\"y\") Option[&str].Nothing\n[1, 2, 30, 4] [[1], [2, 20], [3]]\na b new\ndrop a\ndrop b\ndrop new\n",
    );
}

#[test]
fn views_of_views_parameters_and_propagation() {
    assert_stdout(
        r#"
def last(v: &[list[i64]]) -> Option[&list[i64]]:
    v.get_ref(-1)

def bump(v: &mut [i64], i: i64) -> Option[()]:
    cell = v.get_mut(i)?
    *cell = 100
    Some(())

def index() -> i64:
    print("index").unwrap()
    -2

def main() -> Result[(), AllocError | IoError]:
    mut grid = [[1]?, [2]?, [3]?]?
    print(last(&grid[:2]), last(&grid[3:]))?
    mut nums = [1, 2, 3, 4, 5]?
    print(bump(&mut nums[1:4], 0), bump(&mut nums[1:4], -3), bump(&mut nums[1:4], 3))?
    outer = &mut nums[1:]
    inner = &mut outer[1:3]
    match inner.get_mut(index()):
        case Some(cell):
            *cell = 7
        case Nothing:
            pass
    print(nums)?
    Ok(())
"#,
        "Option[&list[i64]].Some([2]) Option[&list[i64]].Nothing\n\
         Option[()].Some(()) Option[()].Some(()) Option[()].Nothing\n\
         index\n\
         [1, 100, 7, 4, 5]\n",
    );
}

#[test]
fn a_live_result_borrows_the_view_and_its_owner() {
    for (body, message) in [
        (
            "    v = &xs[1:]\n    match v.get_ref(0):\n        case Some(row):\n            xs.append([9]?)?\n            print(row)?\n        case Nothing:\n            pass\n",
            "cannot modify or exclusively borrow `xs` while it is borrowed",
        ),
        (
            "    w = &mut xs[1:]\n    match w.get_mut(0):\n        case Some(row):\n            print(len(xs))?\n            row.append(4)?\n        case Nothing:\n            pass\n",
            "cannot read or borrow `xs` while it is exclusively borrowed",
        ),
        (
            "    w = &mut xs[1:]\n    match w.get_mut(0):\n        case Some(row):\n            other = w.get_ref(1)\n            row.append(4)?\n        case Nothing:\n            pass\n",
            "while it is exclusively borrowed",
        ),
        (
            "    w = &mut xs[1:]\n    match w.get_mut(0):\n        case Some(row):\n            w.swap(0, 1)\n            row.append(4)?\n        case Nothing:\n            pass\n",
            "while `xs` is exclusively borrowed",
        ),
        (
            "    v = &xs[:]\n    r = v.get_ref(0)\n    xs = []?\n    print(r)?\n",
            "while it is borrowed",
        ),
    ] {
        let error = diagnostic(&program(body));
        assert!(error.contains(message), "{body}: {error}");
    }
}

#[test]
fn the_loan_ends_at_the_results_last_use() {
    assert_stdout(
        &program(
            "    v = &xs[1:3]\n    match v.get_ref(1):\n        case Some(row):\n            print(row, xs[0], len(v), v.get_ref(0))?\n        case Nothing:\n            pass\n    w = &mut xs[2:]\n    match w.get_mut(0):\n        case Some(row):\n            row.append(6)?\n        case Nothing:\n            pass\n    w.swap(0, 1)\n    match w.get_mut(5):\n        case Some(row):\n            row.append(0)?\n        case Nothing:\n            pass\n    xs.append([7]?)?\n    print(xs)?\n",
        ),
        "[4] [1] 2 Option[&list[i64]].Some([2, 3])\n[[1], [2, 3], [5], [4, 6], [7]]\n",
    );
}

#[test]
fn receivers_and_arguments_are_checked() {
    for (body, message) in [
        (
            "    v = &xs[:]\n    r = v.get_mut(0)\n",
            "a shared slice view is read-only, so it has no `get_mut`",
        ),
        (
            "    v = &xs[:]\n    r = v.get_ref(0, 1)\n",
            "get_ref requires one i64 index argument",
        ),
        (
            "    w = &mut xs[:]\n    r = w.get_mut()\n",
            "get_mut requires one i64 index argument",
        ),
        (
            "    v = &xs[:]\n    r = v.get_ref(\"a\")\n",
            "expected i64, got str",
        ),
        (
            "    v = &xs[:]\n    r = v.get_ref(1.0)\n",
            "expected i64, got f64",
        ),
    ] {
        let error = diagnostic(&program(body));
        assert!(error.contains(message), "{body}: {error}");
    }
}

#[test]
fn a_lookup_compiles_inline_without_runtime_calls() {
    let source = r#"
def first(v: &[str], i: i64) -> Option[&str]:
    v.get_ref(i)

def main() -> Result[(), AllocError | IoError]:
    names = ["Ada", "Bea"]?
    print(first(&names[1:], 0))?
    Ok(())
"#;
    let calls = runtime_calls(source, "first");
    assert!(calls.is_empty(), "{calls:?}");
}
