//! Compiled selected copies cross the typed std/native boundary in both build modes.
use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;
use crate::test_support::check_error;

#[test]
fn selected_copy_reads_typed_references_and_returns_inline_results() {
    assert_stdout(
        &format!(
            r#"{POLICY}
enum Either:
    Small(i16)
    Wide(range[i64], array[i64, 8])

def duplicate[T](out: &alloc.Allocator, value: &T) -> Result[T, AllocError]:
    out.copy(value)

def main() -> Result[(), AllocError | IoError]:
    out = budget("none", 0)
    rows = [-91i16]?
    print(duplicate(&out, &rows[0])?, out.copy[f32](-1.25f32)?)?
    empty = array[i64, 0].filled(0)
    print(out.copy(&empty)?)?
    print(out.copy[()](&()).is_ok())?
    text = ("shared long" + " string")?
    value = Some(Either.Wide(range(2, 9), array[i64, 8].filled(17)))
    a = out.copy(&text)?
    b = duplicate(&out, &value)?
    drop(text)
    drop(value)
    print(a)?
    match b:
        case Some(e):
            match e:
                case Either.Wide(span, data):
                    print(span, data[7])?
                case Either.Small(n):
                    print(n)?
        case Nothing:
            pass
    Ok(())
"#
        ),
        "-91 -1.25\narray[]\nTrue\nshared long string\nrange(2, 9, 1) 17\nnone 0 0\n",
    );
}

#[test]
fn selected_copy_on_worker_unwinds_nested_failure_and_retains_only_shared_content() {
    assert_stdout(
        &format!(
            r#"{POLICY}
from std.atomic import Atomic, Ordering

class Bundle:
    data: Box[list[i64]]
    hits: Shared[Atomic[u64]]
    wide: array[i64, 8]

enum Item:
    Empty
    Full(Bundle, str)

def first(data: &list[i64]) -> i64:
    data[0]

def work(out: alloc.Allocator) -> Result[(), AllocError | IoError]:
    owner = budget("source", 4)
    mut values = owner.list_with_capacity[i64](1)?
    values.append(11)?
    hits = Shared(Atomic[u64](0))?
    source = Item.Full(Bundle(owner.box(values).unwrap(), hits.share(), array[i64, 8].filled(9)), ("allocated" + " label")?)
    drop(owner)
    partial = budget("partial", 3, AllocError.CapacityOverflow)
    match partial.copy(&source):
        case Err(e):
            print(e)?
        case Ok(v):
            drop(v)
    drop(partial)
    result = out.copy(&source)?
    drop(out)
    match &source:
        case Item.Full(b, text):
            print(first(&b.data), b.wide[7], *text)?
        case Item.Empty:
            pass
    drop(source)
    match result:
        case Item.Full(b, text):
            drop(b.hits.fetch_add(1, Ordering.Relaxed))
            print(first(&b.data), b.wide[7], text, hits.load(Ordering.Relaxed))?
        case Item.Empty:
            pass
    Ok(())

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    with spawn(work, budget("output", 4)).drop_job()? as task:
        task.join()?
    Ok(())
"#
        ),
        "refused partial\nAllocError.CapacityOverflow\npartial 0 3\n11 9 allocated label\nsource 0 4\n11 9 allocated label 1\noutput 0 4\n",
    );
}

#[test]
fn zero_width_flat_field_does_not_displace_its_owned_sibling() {
    assert_stdout(
        r#"import std.alloc as alloc
class Tiny:
    a: array[i64, 0]
    b: array[i64, 0]
class Outer:
    tiny: Tiny
    xs: list[i64]

def main() -> Result[(), AllocError | IoError]:
    tiny = Tiny(array[i64, 0].filled(0), array[i64, 0].filled(1))
    v = Outer(tiny, [42]?)
    a = alloc.system()
    w = a.copy(&v)?
    drop(v)
    print(w.xs)?
    Ok(())
"#,
        "[42]\n",
    );
}

#[test]
fn selected_copy_rejects_ineligible_types() {
    for (decl, value, expected) in [
        ("", "Shared(3).unwrap()", "handle.share()"),
        (
            "class Guard:\n    def __del__(self) -> ():\n        pass",
            "[Guard()].unwrap()",
            "cannot be copied",
        ),
        (
            "def generate() -> Generator[i64]:\n    yield 3",
            "generate()",
            "cannot be copied",
        ),
        ("", "def [n]() -> i64:\n        n", "cannot be copied"),
        (
            "class Node:\n    next: Option[Box[Node]]",
            "Node(Nothing)",
            "recursive data",
        ),
    ] {
        let error = check_error(&format!(
            "import std.alloc as alloc\n{decl}\ndef main() -> ():\n    n = 1\n    owner = alloc.system()\n    x = {value}\n    drop(owner.copy(&x))\n"
        ));
        assert!(error.contains(expected), "{value}: {error}");
    }
}

#[test]
fn selected_copy_rejects_named_class_with_slice_field() {
    let error = check_error(
        r#"import std.alloc as alloc
class Window:
    data: &[i64]

def main() -> ():
    out = alloc.system()
    xs = [1].unwrap()
    window = Window(&xs[:])
    drop(out.copy(&window))
"#,
    );
    assert!(
        error.contains("Allocator.copy result cannot carry a loan: `Window`"),
        "{error}"
    );
}

#[test]
fn selected_copy_excludes_actors_and_private_calls() {
    let error = check_error(
        "import std.alloc as alloc\nimport std.actor as actors\ndef unused(out: &alloc.Allocator, value: &i64) -> ():\n    drop(out.copy(value))\ndef main() -> ():\n    drop(actors.heaps_live())\n",
    );
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
    let error = check_error("def main() -> ():\n    drop(__allocator_copy[i64](0, 0))\n");
    assert!(
        error.contains("reserved for the standard library"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn inline_copy_requires_no_heap_scratch() {
    assert_stdout(
        r#"import std.alloc as alloc

enum Item:
    Empty
    Values(str, array[i64, 24], range[i64])

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    original = Item.Values(("heap-backed" + " text")?, array[i64, 24].filled(31), range(7))
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    result = owner.copy(&original)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    drop(original)
    match result?:
        case Item.Values(label, xs, span):
            print(label, xs[23], len(span))?
        case Item.Empty:
            pass
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\nheap-backed text 31 7\n",
    );
}

#[test]
fn copying_on_worker_keeps_source_borrowed_until_join() {
    let error = check_error(
        r#"import std.alloc as alloc

def work(out: &alloc.Allocator, source: &list[i64]) -> Result[list[i64], AllocError]:
    out.copy(source)

def main() -> ():
    out = alloc.system()
    mut source = [1].unwrap()
    with spawn(work, &out, &source).drop_job().unwrap() as task:
        source.append(2).unwrap()
        drop(task.join())
"#,
    );
    assert!(
        error.contains("cannot modify or exclusively borrow `source`"),
        "{error}"
    );
}
