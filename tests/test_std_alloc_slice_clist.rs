use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;
use crate::test_support::check_error;

#[test]
fn selected_mixed_columns_follow_slice_bounds_and_keep_row_bits() {
    assert_stdout(
        r#"import std.alloc as alloc

class Row:
    flag: bool
    short: i16
    wide: u64
    number: f32

def main() -> Result[(), AllocError | IoError | RangeError]:
    out = alloc.system()
    mut rows = clist[Row]()
    rows.append(Row(True, -32768i16, u64.MAX, -0.0f32)).unwrap()
    rows.append(Row(False, 7i16, 8u64, 1.25f32)).unwrap()
    rows.append(Row(True, -2i16, 6u64, 2.5f32)).unwrap()
    k = -2
    copy = out.slice_clist[Row](&rows, alloc.Unset.Unset, step=NonZero.try_from(k)?)?
    print(copy[0].short, copy[-1].flag, copy[-1].wide, copy[-1].number)?
    print(out.slice_clist(&rows, i64.MIN, i64.MAX)? == rows)?
    empty = out.slice_clist(&rows, stop=-1, step=-1)?
    print(empty.len())?
    rows[0].short = 0i16
    print(copy[-1].short, rows[0].short)?
    Ok(())
"#,
        "-2 True 18446744073709551615 -0.0\nTrue\n0\n-32768 0\n",
    );
}

#[test]
fn selected_empty_retains_provider_through_worker_growth_and_drop() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Item:
    value: i64

def make() -> Result[clist[Item], AllocError]:
    rows = clist[Item]()
    owner = budget("worker", 2, AllocError.CapacityOverflow)
    owner.slice_clist(&rows)

def work(values: clist[Item]) -> Result[i64, IoError | AllocError]:
    mut items = values
    print(items.len(), items.capacity())?
    items.append(Item(9)).unwrap()
    match items.reserve(100):
        case Err(error):
            print(error, items[-1].value)?
        case Ok(_):
            pass
    # Bracket slicing produces an ordinary owner, not a child of the provider.
    mut independent = items[::-1]?
    independent.reserve(100)?
    independent[0].value = 3
    print(items.remove(0).unwrap().value, items.len())?
    Ok(independent[0].value)

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    with spawn(work, make()?).drop_job()? as job:
        print(job.join()?)?
    Ok(())
"#
        ),
        "0 0\nrefused worker\nAllocError.CapacityOverflow 9\n9 0\nworker 0 2\n3\n",
    );
}

#[test]
fn refused_slice_preserves_the_source_and_never_reuses_its_provider() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Row:
    value: i64

def main() -> Result[(), AllocError | IoError]:
    mut rows = clist[Row]()
    rows.append(Row(11)).unwrap()
    a = budget("source", 1)
    source = a.slice_clist(&rows)?
    out = budget("output", 0)
    match out.slice_clist(&source):
        case Err(error):
            print(error, source[0].value)?
        case Ok(value):
            drop(value)
    print(out.slice_clist(&source, stop=0).is_err())?
    drop(source)
    Ok(())
"#
        ),
        "refused output\nAllocError.OutOfMemory 11\nrefused output\nTrue\noutput 0 0\nsource 0 1\n",
    );
}

#[test]
fn slice_clist_obeys_caller_row_privacy_even_with_cached_specializations() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("model.plenty"),
        r#"import std.alloc as alloc
pub class Row:
    private_value: i64
pub def make(out: &alloc.Allocator) -> Result[clist[Row], AllocError]:
    rows = clist[Row]()
    out.slice_clist(&rows, step=-1)
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    let source = "from model import make, Row\nimport std.alloc as alloc\ndef main() -> Result[(), AllocError]:\n    out = alloc.system()\n    rows = make(&out)?\n    STATEMENT\n    Ok(())\n";
    std::fs::write(&main, source.replace("STATEMENT", "drop(rows)")).unwrap();
    plenty::check_file(&main, None).unwrap();
    for call in [
        "out.slice_clist(&rows, step=-1)",
        "out.slice_clist[Row](&rows, 0, 1, 1)",
    ] {
        std::fs::write(&main, source.replace("STATEMENT", &format!("drop({call})"))).unwrap();
        let err = plenty::check_file(&main, None).unwrap_err().to_string();
        assert!(err.contains("private"), "{err}");
    }
}

#[test]
fn slice_clist_keeps_the_existing_step_actor_and_intrinsic_boundaries() {
    let source = "import std.alloc as alloc\nclass Row:\n    n: i32\ndef use_slice(out: &alloc.Allocator, rows: &clist[Row]) -> ():\n    drop(out.slice_clist(rows, step=STEP))\ndef main() -> ():\n    pass\n";
    for (step, diagnostic) in [("0", "cannot be zero"), ("rows.len()", "NonZero[i64]")] {
        let error = check_error(&source.replace("STEP", step));
        assert!(error.contains(diagnostic), "{error}");
    }
    let error = check_error(&format!(
        "import std.actor as actors\n{}",
        source
            .replace("STEP", "1")
            .replace("    pass", "    drop(actors.heaps_live())")
    ));
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
    let error =
        check_error("def main() -> ():\n    drop(__allocator_clist_slice[i64](0, 0, 0, 0, 1))\n");
    assert!(
        error.contains("reserved for the standard library"),
        "{error}"
    );
}

#[test]
fn selected_clist_does_not_relax_schema_or_worker_borrow_rules() {
    let source = r#"import std.alloc as alloc
class Row:
    n: i64

def work(out: &alloc.Allocator, rows: &clist[Row]) -> Result[clist[Row], AllocError]:
    out.slice_clist(rows)

def main() -> ():
    out = alloc.system()
    mut rows = clist[Row]()
    with spawn(work, &out, &rows).drop_job().unwrap() as job:
        rows.clear()
        drop(job.join())
"#;
    let err = check_error(source);
    assert!(
        err.contains("cannot modify or exclusively borrow `rows`"),
        "{err}"
    );
    let err = check_error(&source.replace("n: i64", "n: str"));
    assert!(err.contains("primitive scalar fields"), "{err}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn selected_columns_gather_without_ordinary_storage_or_scratch() {
    assert_stdout(
        r#"import std.alloc as alloc
pub class Preallocated:
    blocks: list[alloc.ByteBlock]
    pub def allocate(self: &mut Preallocated, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.blocks.pop().ok_or(AllocError.OutOfMemory)
    pub def release(self: &mut Preallocated, storage: alloc.ByteBlock) -> ():
        drop(storage)
class Row:
    small: i8
    value: f64

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    layout = alloc.layout(1024, 64).unwrap()
    blocks = [root.block(&layout)?, root.block(&layout)?]?
    out = alloc.allocator(&root, Preallocated(blocks)).unwrap()
    mut rows = clist[Row]()
    rows.append(Row(1i8, 3.25)).unwrap()
    rows.append(Row(2i8, 4.5)).unwrap()
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    mut result = out.slice_clist(&rows, step=-1)?
    result.reserve(16)?
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(result[0].small, result[1].value)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n2 3.25\n",
    );
}
