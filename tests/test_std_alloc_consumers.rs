//! Selected ordinary boxes and the consuming Plenty generator collector.
mod support;
use super::test_std_alloc::assert_stdout;

#[test]
fn selected_boxes_preserve_scalar_inline_and_affine_content() {
    assert_stdout(
        r#"import std.alloc as alloc

class Child:
    id: i64
    def __del__(self: &mut Child) -> ():
        print("child", self.id).unwrap()

class Row:
    values: array[i64, 8]
    child: Box[Child]

enum Choice:
    Values(array[i64, 8], str)

def selected[T](owner: &alloc.Allocator, value: T) -> Box[T]:
    owner.box(value).unwrap()

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    mut scalar = owner.box(41).unwrap()
    reference = &mut *scalar
    *reference = 42
    print(*scalar)?
    boxed = selected(&owner, Row(array[i64, 8].filled(7), Box(Child(9))?))
    drop(owner)
    print(boxed.values[7], boxed.child.id)?
    row = *boxed
    print(row.values[0])?
    drop(row)
    root = alloc.system()
    print(*root.box[range[i64]](range(1, 5)).unwrap())?
    print(*root.box(Choice.Values(array[i64, 8].filled(3), ("retained" + " text")?)).unwrap())?
    Ok(())
"#,
        "42\n7 9\n7\nchild 9\nrange(1, 5, 1)\nChoice.Values(array[3, 3, 3, 3, 3, 3, 3, 3], \"retained text\")\n",
    );
}

#[test]
fn selected_list_sole_argument_inference_and_result_elements() {
    assert_stdout(
        r#"import std.alloc as alloc

def outcomes() -> Generator[Result[i64, AllocError]]:
    yield Ok(1)
    yield Err(AllocError.OutOfMemory)

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    print(owner.list(a for a in range(4))?)?
    print(owner.list[i64](a for a in range(0))?)?
    print(owner.list(outcomes())?)?
    print(owner.list(array[i64, 3].filled(a) for a in range(2))?)?
    Ok(())
"#,
        "[0, 1, 2, 3]\n[]\n[Result[i64, AllocError].Ok(1), Result[i64, AllocError].Err(AllocError.OutOfMemory)]\n[array[0, 0, 0], array[1, 1, 1]]\n",
    );
}

const POLICY: &str = r#"import std.alloc as alloc

pub class Policy:
    parent: alloc.Allocator
    left: i64
    error: AllocError
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        if self.left == 0:
            print("refused").unwrap()
            return Err(self.error)
        unchecked:
            self.left -= 1
        print("allocate").unwrap()
        self.parent.block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        print("release").unwrap()
        drop(storage)
    def __del__(self: &mut Policy) -> ():
        print("provider").unwrap()

class Child:
    id: i64
    def __del__(self: &mut Child) -> ():
        print("child", self.id).unwrap()
"#;

#[test]
fn selected_box_failure_returns_original_and_retry_relocates_once() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Row:
    data: array[i64, 9]
    child: Box[Child]
    text: str

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    for error in [AllocError.OutOfMemory, AllocError.CapacityOverflow]?:
        owner = alloc.allocator(&root, Policy(root.share(), 0, error)).unwrap()
        input = Row(array[i64, 9].filled(17), Box(Child(8))?, ("kept" + " text")?)
        match owner.box(input):
            case Ok(boxed):
                drop(boxed)
            case Err(alloc.BoxError.Allocation(original, cause)):
                print(cause, original.data[8], original.child.id, original.text)?
                retry = root.box(original).unwrap()
                print(retry.data[8], retry.text)?
                drop(retry)
        drop(owner)
    Ok(())
"#
        ),
        "refused\nAllocError.OutOfMemory 17 8 kept text\n17 kept text\nchild 8\nprovider\nrefused\nAllocError.CapacityOverflow 17 8 kept text\n17 kept text\nchild 8\nprovider\n",
    );
}

#[test]
fn selected_box_take_releases_storage_but_not_content_and_worker_drops_children_first() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def work(boxed: Box[Box[Child]]) -> ():
    print("worker", boxed.id).unwrap()
    drop(boxed)
    print("worker done").unwrap()

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    root = alloc.system()
    owner = alloc.allocator(&root, Policy(root.share(), 9, AllocError.OutOfMemory)).unwrap()
    boxed = owner.box(Child(1)).unwrap()
    child = *boxed
    print("taken", child.id)?
    drop(child)
    nested = owner.box(owner.box(Child(2)).unwrap()).unwrap()
    drop(owner)
    with spawn(work, nested).drop_job()? as task:
        task.join()
    print("joined")?
    Ok(())
"#
        ),
        "allocate\nrelease\ntaken 1\nchild 1\nallocate\nallocate\nworker 2\nchild 2\nrelease\nrelease\nprovider\nworker done\njoined\n",
    );
}

#[test]
fn selected_collector_failures_clean_frame_current_item_and_partial_output() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def source(capture: Child) -> Generator[Child]:
    print("resume", capture.id).unwrap()
    for i in range(20):
        print("yield", i).unwrap()
        yield Child(i)

def main() -> Result[(), IoError]:
    root = alloc.system()
    for budget in range(4):
        print("budget", budget)?
        owner = alloc.allocator(&root, Policy(root.share(), budget, AllocError.OutOfMemory)).unwrap()
        print(owner.list(source(Child(99))).is_err())?
        drop(owner)
    Ok(())
"#
        ),
        // Empty-list construction needs its entry state and object; first growth
        // then takes one row allocation. Failure must never resume early.
        "budget 0\nrefused\nchild 99\nTrue\nprovider\nbudget 1\nallocate\nrefused\nrelease\nchild 99\nTrue\nprovider\nbudget 2\nallocate\nallocate\nresume 99\nyield 0\nrefused\nchild 0\nchild 99\nrelease\nrelease\nTrue\nprovider\nbudget 3\nallocate\nallocate\nresume 99\nyield 0\nallocate\nyield 1\nyield 2\nyield 3\nyield 4\nrefused\nchild 4\nchild 99\nchild 0\nchild 1\nchild 2\nchild 3\nrelease\nrelease\nrelease\nTrue\nprovider\n",
    );
}

#[test]
fn selected_collector_releases_shared_and_mutable_capture_loans_on_failure() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def shared(values: &list[i64]) -> Generator[i64]:
    for value in values:
        yield *value

def exclusive(values: &mut list[i64]) -> Generator[i64]:
    while len(*values) > 0:
        yield values.pop().unwrap()

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    owner = alloc.allocator(&root, Policy(root.share(), 0, AllocError.CapacityOverflow)).unwrap()
    mut values = [1, 2]?
    print(owner.list(shared(&values)).is_err())?
    values.append(3)?
    print(owner.list[i64](exclusive(&mut values)).is_err())?
    values.append(4)?
    print(values)?
    Ok(())
"#
        ),
        "refused\nTrue\nrefused\nTrue\n[1, 2, 3, 4]\nprovider\n",
    );
}

#[test]
fn selected_box_rejects_private_intrinsic_borrows_and_hidden_endpoints() {
    for (source, expected) in [
        (
            "def main() -> ():\n    mut x = 1\n    drop(__allocator_box[i64](0, &mut x))\n",
            "reserved for the standard library",
        ),
        (
            "import std.alloc as alloc\ndef main() -> ():\n    owner = alloc.system()\n    x = 1\n    drop(owner.box(&x))\n",
            "borrow",
        ),
        (
            "import std.alloc as alloc\ndef main() -> ():\n    owner = alloc.system()\n    sender, receiver = channel[i64](1).unwrap()\n    drop(owner.box(sender))\n    drop(receiver)\n",
            "hidden task/channel ownership",
        ),
    ] {
        let error = support::check_error(source);
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn selected_boxes_retain_copyable_fields_and_owned_resources() {
    assert_stdout(
        r#"import std.alloc as alloc

@copy
class Text:
    value: str

class Local:
    file: File

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    text = Text(("copy" + " retained")?)
    boxed = owner.box(text).unwrap()
    print(text.value, boxed.value)?
    drop(boxed)
    print(text.value)?
    local = owner.box(Local(open("/dev/null", "w")?)).unwrap()
    drop(local)
    Ok(())
"#,
        "copy retained copy retained\ncopy retained\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn selected_box_has_only_its_selected_storage_allocation() {
    for successes in 0..=1 {
        assert_stdout(
            &format!(
                r#"import std.alloc as alloc
class Child:
    value: array[i64, 9]
    def __del__(self: &mut Child) -> ():
        print("child", self.value[8]).unwrap()
def main() -> Result[(), IoError]:
    root = alloc.system()
    input = Child(array[i64, 9].filled(42))
    print("__test_fail_allocations_after_{successes}__")?
    result = root.box(input)
    print("__test_restore_allocations__")?
    print(result.is_ok())?
    drop(result)
    Ok(())
"#
            ),
            &format!(
                "__test_fail_allocations_after_{successes}__\n__test_restore_allocations__\n{}\nchild 42\n",
                if successes == 1 { "True" } else { "False" }
            ),
        );
    }
}

#[test]
fn selected_box_unit_matches_ordinary_box_support() {
    assert_stdout(
        r#"import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    ordinary = Box(())?
    owner = alloc.system()
    selected = owner.box[()](()).unwrap()
    print(ordinary, selected)?
    Ok(())
"#,
        "Box(()) Box(())\n",
    );
}

#[test]
fn selected_boxes_pack_narrow_scalars_and_floating_point() {
    assert_stdout(
        r#"import std.alloc as alloc

def main() -> Result[(), IoError]:
    owner = alloc.system()
    print(*owner.box(-7i8).unwrap(), *owner.box(255u8).unwrap())?
    print(*owner.box(1.25).unwrap(), *owner.box(True).unwrap())?
    Ok(())
"#,
        "-7 255\n1.25 True\n",
    );
}

#[test]
fn selected_consumers_in_unused_bodies_still_exclude_actors() {
    for body in [
        "drop(owner.box(1))",
        "drop(owner.list(n for n in range(3)))",
    ] {
        let error = support::check_error(&format!(
            "import std.alloc as alloc\nimport std.actor as actors\ndef unused(owner: &alloc.Allocator) -> ():\n    {body}\ndef main() -> ():\n    drop(actors.heaps_live())\n"
        ));
        assert!(
            error.contains("retained allocator use cannot be combined"),
            "{error}"
        );
    }
}

#[test]
fn selected_collector_growth_failure_ends_borrowed_frame_loans() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def shared(values: &list[i64]) -> Generator[i64]:
    for value in values:
        yield *value

def exclusive(values: &mut list[i64]) -> Generator[i64]:
    while len(*values) > 0:
        yield values.pop().unwrap()

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    mut values = [1, 2]?
    shared_owner = alloc.allocator(&root, Policy(root.share(), 2, AllocError.CapacityOverflow)).unwrap()
    match shared_owner.list[i64](shared(&values)):
        case Err(error):
            print(error)?
        case Ok(output):
            drop(output)
    values.append(3)?
    print(values)?
    drop(shared_owner)
    mutable_owner = alloc.allocator(&root, Policy(root.share(), 2, AllocError.OutOfMemory)).unwrap()
    match mutable_owner.list(exclusive(&mut values)):
        case Err(error):
            print(error)?
        case Ok(output):
            drop(output)
    values.append(4)?
    print(values)?
    drop(mutable_owner)
    Ok(())
"#
        ),
        "allocate\nallocate\nrefused\nrelease\nrelease\nAllocError.CapacityOverflow\n[1, 2, 3]\nprovider\nallocate\nallocate\nrefused\nrelease\nrelease\nAllocError.OutOfMemory\n[1, 2, 4]\nprovider\n",
    );
}

#[test]
fn selected_boxes_cannot_hide_borrowing_frames_or_transfer_local_contents_to_workers() {
    let borrowed = support::check_error(
        r#"import std.alloc as alloc

def source(value: &i64) -> Generator[i64]:
    yield *value

def main() -> ():
    owner = alloc.system()
    value = 3
    drop(owner.box(source(&value)))
"#,
    );
    assert!(borrowed.contains("borrow"), "{borrowed}");
    let source = r#"import std.alloc as alloc

protocol Id:
    def id(self) -> i64:
        pass

class Item:
    def id(self) -> i64:
        7

def work(value: Box[Box[Id]]) -> ():
    drop(value)

def main() -> ():
    owner = alloc.system()
    erased = Box[Id](Item()).unwrap()
    boxed = owner.box(erased).unwrap()
    ACTION
"#;
    assert_stdout(
        &source.replace("ACTION", "inner = *boxed\n    print(inner.id()).unwrap()"),
        "7\n",
    );
    let worker = support::check_error(&source.replace(
        "ACTION",
        "with spawn(work, boxed).drop_job().unwrap() as task:\n        task.join()",
    ));
    assert!(
        worker.contains("not certified for worker threads"),
        "{worker}"
    );
}

#[test]
fn selected_collector_result_keeps_provider_for_later_growth_and_drop() {
    assert_stdout(
        &format!(
            r#"{POLICY}
def make(root: &alloc.Allocator) -> list[i64]:
    owner = alloc.allocator(root, Policy(root.share(), 9, AllocError.OutOfMemory)).unwrap()
    owner.list(n for n in range(4)).unwrap()

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    mut values = make(&root)
    print("collected", len(values))?
    values.append(4)?
    print(values)?
    drop(values)
    Ok(())
"#
        ),
        "allocate\nallocate\nallocate\ncollected 4\nallocate\nrelease\n[0, 1, 2, 3, 4]\nrelease\nrelease\nrelease\nprovider\n",
    );
}
