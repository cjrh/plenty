//! Selected view materialization borrows elements and creates independent list storage.
use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;
use crate::test_support::check_error;

#[test]
fn copy_view_reads_only_the_selected_range_including_byte_views() {
    assert_stdout(
        r#"import std.alloc as alloc
import std.bytes as bytes_mod

def materialize[T](out: &alloc.Allocator, source: &[T]) -> Result[list[T], AllocError]:
    out.copy_view(source)

def main() -> Result[(), AllocError | IoError | bytes_mod.BytesError]:
    out = alloc.system()
    xs = [10, 20, 30, 40]?
    mut result = materialize(&out, &xs[1:3])?
    print(result, out.copy_view(&xs[2:2])?)?
    result[0] = 99
    print(result, xs)?
    packed = bytes_mod.from_bytes(b"a\xff\x00b")?
    words = [1u8, 128u8, 255u8, 4u8]?
    print(materialize(&out, &packed[1:3])?, materialize(&out, &words[1:3])?)?
    Ok(())
"#,
        "[20, 30] []\n[99, 30] [10, 20, 30, 40]\n[255, 0] [128, 255]\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn copy_view_uses_only_preallocated_provider_backing() {
    assert_stdout(
        r#"import std.alloc as alloc

pub class Preallocated:
    blocks: list[alloc.ByteBlock]
    pub def allocate(self: &mut Preallocated, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.blocks.pop().ok_or(AllocError.OutOfMemory)
    pub def release(self: &mut Preallocated, storage: alloc.ByteBlock) -> ():
        drop(storage)

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    # Oversized, aligned blocks cover both list storage and copied boxes.
    layout = alloc.layout(1024, 64).unwrap()
    mut blocks = list[alloc.ByteBlock]()?
    for i in range(8):
        blocks.append(root.block(&layout)?)?
    out = alloc.allocator(&root, Preallocated(blocks)).unwrap()
    xs = [Box(10)?, Box(20)?, Box(30)?, Box(40)?]?
    view = &xs[1:3]
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    result = out.copy_view(view)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    drop(xs)
    drop(out)
    print(result?)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n[Box(20), Box(30)]\n",
    );
}

#[test]
fn scoped_worker_copies_a_direct_view_without_extending_its_loan() {
    let source = r#"import std.alloc as alloc

def work(out: &alloc.Allocator, view: &[i64]) -> Result[list[i64], AllocError]:
    out.copy_view(view)

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    out = alloc.system()
    mut xs = [10, 20, 30, 40]?
    mut result = list[i64]()?
    with spawn(work, &out, &xs[1:3]).drop_job()? as task:
        result = task.join()?
    xs[1] = 99
    print(result, xs)?
    Ok(())
"#;
    assert_stdout(source, "[20, 30] [10, 99, 30, 40]\n");

    let conflict = source.replace(
        "        result = task.join()?\n    xs[1] = 99",
        "        xs[1] = 99\n        result = task.join()?",
    );
    let error = check_error(&conflict);
    assert!(
        error.contains("cannot modify or exclusively borrow `xs[1]` while `xs[1:3]` is borrowed"),
        "{error}"
    );
}

#[test]
fn worker_copy_view_unwinds_partial_elements_and_retains_the_selected_provider() {
    assert_stdout(
        &format!(
            r#"{POLICY}
from std.atomic import Atomic, Ordering

class Row:
    data: Box[i64]
    hits: Shared[Atomic[u64]]

def materialize(out: &alloc.Allocator, view: &[Row]) -> Result[list[Row], AllocError]:
    out.copy_view(view)

def work(out: alloc.Allocator, xs: list[Row]) -> Result[list[Row], AllocError | IoError]:
    view = &xs[1:3]
    # Enough for the list (three blocks) and first box, but not the second box.
    partial = budget("partial", 4, AllocError.CapacityOverflow)
    match materialize(&partial, view):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    drop(partial)
    result = materialize(&out, view)?
    print(xs[0].data, xs[1].data, xs[2].data)?
    drop(xs)
    drop(out)
    Ok(result)

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    hits = Shared(Atomic[u64](0))?
    xs = [Row(Box(11)?, hits.share()), Row(Box(22)?, hits.share()), Row(Box(33)?, hits.share())]?
    with spawn(work, budget("output", 6), xs).drop_job()? as task:
        mut result = task.join()?
        result.reserve(100)?
        print(result[0].data, result[1].data)?
        drop(result[0].hits.fetch_add(1, Ordering.Relaxed))
        print(hits.load(Ordering.Relaxed), result.reserve(200).is_err())?
        drop(result)
    Ok(())
"#
        ),
        "refused partial\nAllocError.CapacityOverflow\npartial 0 4\nBox(11) Box(22) Box(33)\nBox(22) Box(33)\nrefused output\n1 True\noutput 0 6\n",
    );
}

#[test]
fn share_view_retains_existing_payloads_and_selected_list_storage() {
    assert_stdout(
        &format!(
            r#"{POLICY}
from std.atomic import Atomic, Ordering

def main() -> Result[(), AllocError | IoError]:
    counter = Shared(Atomic[u64](0))?
    xs = [Shared(Atomic[u64](19))?, counter.share(), counter.share()]?
    refused = budget("failed", 0)
    print(refused.share_view(&xs[1:]).is_err())?
    drop(refused)
    out = budget("result", 3)
    mut result = out.share_view(&xs[1:])?
    drop(out)
    drop(xs)
    drop(result[0].fetch_add(5, Ordering.Relaxed))
    print(result[1].load(Ordering.Relaxed), counter.load(Ordering.Relaxed), result.reserve(100).is_err())?
    drop(result)
    empty = alloc.system().share_view(&[counter.share()]?[:0])?
    print(len(&empty))?
    Ok(())
"#
        ),
        "refused failed\nTrue\nfailed 0 0\nrefused result\n5 5 True\nresult 0 3\n0\n",
    );
}

#[test]
fn copy_view_rejects_noncopyable_elements_and_loans() {
    for (decl, element, expected) in [
        ("", "Shared[i64]", "handle.share()"),
        (
            "class Guard:\n    def __del__(self) -> ():\n        pass",
            "Guard",
            "cannot be copied",
        ),
        (
            "class Window:\n    data: &[i64]",
            "Window",
            "Window cannot be stored in a list",
        ),
    ] {
        let error = check_error(&format!(
            "import std.alloc as alloc\n{decl}\ndef bad(out: &alloc.Allocator, view: &[{element}]) -> ():\n    drop(out.copy_view(view))\ndef main() -> ():\n    pass\n"
        ));
        assert!(error.contains(expected), "{element}: {error}");
    }
}

#[test]
fn copy_view_does_not_shorten_worker_loans_or_admit_actor_heaps() {
    let error = check_error(
        r#"import std.alloc as alloc

def work(out: &alloc.Allocator, xs: &list[i64]) -> Result[list[i64], AllocError]:
    view = &xs[:]
    out.copy_view(view)

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
    let error = check_error(
        "import std.alloc as alloc\nimport std.actor as actors\ndef unused(out: &alloc.Allocator, view: &[i64]) -> ():\n    drop(out.copy_view(view))\ndef main() -> ():\n    drop(actors.heaps_live())\n",
    );
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
}
