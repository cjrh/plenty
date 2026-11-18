//! Compiled selected slices use the ordinary bounds contract, but an explicit provider.
use super::test_std_alloc::assert_stdout;

#[test]
fn selected_slice_matches_brackets_for_present_and_omitted_bounds() {
    assert_stdout(
        r#"import std.alloc as alloc

def main() -> Result[(), AllocError | IoError | RangeError]:
    out = alloc.system()
    xs = [0, 1, 2, 3, 4]?
    k = -2
    step = NonZero.try_from(k)?
    print(out.slice(&xs)?)?
    print(out.slice(&xs, 1, 4)?)?
    print(out.slice(&xs, stop=-1, step=-1)?)?
    print(out.slice(&xs, step=step)?, xs[::step]?)?
    print(out.slice[i64](&xs, i64.MIN, i64.MAX, 2)?)?
    print(out.slice(&xs, alloc.Unset.Unset, 3)?)?
    Ok(())
"#,
        "[0, 1, 2, 3, 4]\n[1, 2, 3]\n[]\n[4, 2, 0] [4, 2, 0]\n[0, 2, 4]\n[0, 1, 2]\n",
    );
}

#[test]
fn worker_slice_rolls_back_partial_copy_and_retains_the_output_provider() {
    use super::test_std_alloc_set_algebra::POLICY;
    assert_stdout(
        &format!(
            r#"{POLICY}
from std.atomic import Atomic, Ordering

def work(out: alloc.Allocator, xs: list[Box[i64]]) -> Result[list[Box[i64]], AllocError | IoError]:
    partial = budget("partial", 4, AllocError.CapacityOverflow)
    match partial.slice(&xs, step=-2):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    drop(partial)
    print(xs)?
    result = out.slice(&xs, step=-2)?
    drop(xs)
    drop(out)
    Ok(result)

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    hits = Shared(Atomic[u64](0))?
    handles = [hits.share()]?
    system = alloc.system()
    copies = system.slice(&handles)?
    drop(handles)
    drop(copies[0].fetch_add(1, Ordering.Relaxed))
    print(hits.load(Ordering.Relaxed))?
    xs = [Box(11)?, Box(22)?, Box(33)?]?
    with spawn(work, budget("output", 5), xs).drop_job()? as task:
        mut result = task.join()?
        print(result)?
        print(result.reserve(100).is_err())?
        drop(result)
    Ok(())
"#
        ),
        "1\nrefused partial\nAllocError.CapacityOverflow\npartial 0 4\n[Box(11), Box(22), Box(33)]\n[Box(33), Box(11)]\nrefused output\nTrue\noutput 0 5\n",
    );
}

#[test]
fn selected_slice_rejects_uncopyable_rows_and_invalid_steps() {
    use crate::test_support::check_error;
    for (decl, value, args, expected) in [
        (
            "class Guard:\n    def __del__(self) -> ():\n        pass",
            "Guard()",
            "",
            "cannot be copied",
        ),
        (
            "class Node:\n    next: Option[Box[Node]]",
            "Node(Nothing)",
            "",
            "recursive data",
        ),
        ("", "1", ", step=0", "a NonZero value cannot be zero"),
        ("", "1", ", step=len(xs)", "NonZero[i64]"),
    ] {
        let error = check_error(&format!(
            "import std.alloc as alloc\n{decl}\ndef main() -> ():\n    out = alloc.system()\n    xs = [{value}].unwrap()\n    drop(out.slice(&xs{args}))\n"
        ));
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn selected_slice_excludes_actor_programs_and_private_calls() {
    use crate::test_support::check_error;
    let error = check_error(
        "import std.alloc as alloc\nimport std.actor as actors\ndef unused(out: &alloc.Allocator, xs: &list[i64]) -> ():\n    drop(out.slice(xs))\ndef main() -> ():\n    drop(actors.heaps_live())\n",
    );
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
    let error =
        check_error("def main() -> ():\n    drop(__allocator_list_slice[i64](0, 0, 0, 0, 1))\n");
    assert!(
        error.contains("reserved for the standard library"),
        "{error}"
    );
}

#[test]
fn selected_slice_cannot_escape_a_view_or_shorten_a_worker_loan() {
    use crate::test_support::check_error;
    let error = check_error(
        r#"import std.alloc as alloc
class Window:
    data: &[i64]
def escape(out: &alloc.Allocator, xs: &list[Window]) -> Result[list[Window], AllocError]:
    out.slice(xs)
def main() -> ():
    pass
"#,
    );
    assert!(
        error.contains("view classes cannot be stored in collections"),
        "{error}"
    );
    let error = check_error(
        r#"import std.alloc as alloc

def work(out: &alloc.Allocator, xs: &list[i64]) -> Result[list[i64], AllocError]:
    out.slice(xs, step=-1)

def main() -> ():
    out = alloc.system()
    mut xs = [1].unwrap()
    with spawn(work, &out, &xs).drop_job().unwrap() as task:
        xs.append(2).unwrap()
        drop(task.join())
"#,
    );
    assert!(
        error.contains("cannot modify or exclusively borrow `xs`"),
        "{error}"
    );
}
