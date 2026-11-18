//! Selected storage, with ordinary dict/set admission, mutation and transfer.
mod support;
use super::test_std_alloc::assert_stdout;

#[test]
fn selected_system_dict_and_set_accept_structural_keys_and_outlive_handles() {
    assert_stdout(
        r#"import std.alloc as alloc

class Key:
    id: i64

def make[K: KeyType](owner: &alloc.Allocator) -> Result[dict[K, str], AllocError]:
    owner.dict_with_capacity[K, str](1)

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    mut data = make[Key](&root)?
    mut keys = root.empty_set[Key]()?
    drop(root)
    for i in range(32):
        data.insert(Key(i), "value")?
        keys.add(Key(i))?
    probe = Key(31)
    print(len(data), data[&probe], len(keys), probe in keys)?
    Ok(())
"#,
        "32 value 32 True\n",
    );
}

const BUDGET: &str = r#"import std.alloc as alloc

pub class Budget:
    root: alloc.Allocator
    left: i64
    releases: i64
    pub def allocate(self: &mut Budget, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        if self.left == 0:
            print("refused").unwrap()
            return Err(AllocError.OutOfMemory)
        unchecked:
            self.left -= 1
        self.root.block(request)
    pub def release(self: &mut Budget, storage: alloc.ByteBlock) -> ():
        unchecked:
            self.releases += 1
        drop(storage)
    def __del__(self: &mut Budget) -> ():
        print("budget", self.left, "releases", self.releases).unwrap()

def budget(left: i64) -> alloc.Allocator:
    root = alloc.system()
    alloc.allocator(&root, Budget(root.share(), left, 0)).unwrap()

def report[T](result: Result[T, AllocError]) -> ():
    match result:
        case Err(error):
            print(error).unwrap()
        case Ok(value):
            print("unexpected success").unwrap()
            drop(value)
"#;

#[test]
fn custom_hash_owners_keep_selected_growth_and_cleanup_on_worker() {
    // Each empty owner takes two blocks. reserve(8) takes the remaining
    // three (buckets, order and rows); the ninth insertion needs new storage.
    assert_stdout(
        &format!(
            r#"{BUDGET}
def work(data: dict[i64, i64], keys: set[i64]) -> Result[(), AllocError | IoError]:
    mut d = data
    mut s = keys
    d.reserve(8)?
    s.reserve(8)?
    for i in range(8):
        d.insert(i, i)?
        s.add(i)?
    print(d.insert(8, 8).is_err(), s.add(8).is_err())?
    print(len(d), d[7], len(s), 7 in s)?
    drop(d)
    drop(s)
    Ok(())

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    owner = budget(5)
    other = budget(5)
    d = owner.empty_dict[i64, i64]()?
    s = other.empty_set[i64]()?
    drop(owner)
    drop(other)
    with spawn(work, d, s).drop_job()? as task:
        task.join()?
    print("joined")?
    Ok(())
"#
        ),
        "refused\nrefused\nTrue True\n8 7 8 True\nbudget 0 releases 5\nbudget 0 releases 5\njoined\n",
    );
}

#[test]
fn custom_constructors_check_capacity_before_provider_and_keep_its_errors() {
    assert_stdout(
        &format!(
            r#"{BUDGET}
@copy
enum Flag:
    First
    Second

def main() -> Result[(), IoError | AllocError]:
    owner = budget(1)
    report(owner.dict_with_capacity[str, i64](-1))
    report(owner.set_with_capacity[i64](i64.MAX))
    report(owner.set_with_capacity[Flag](-1))
    report(owner.set_with_capacity[Flag](i64.MAX))
    mut flags = owner.set_with_capacity[Flag](1000000)?
    report(owner.empty_set[Flag]())
    report(owner.dict_with_capacity[i64, str](4))
    report(owner.set_with_capacity[i64](4))
    drop(owner)
    flags.add(Flag.Second)?
    flags.add(Flag.First)?
    flags.reserve(1000000)?
    print(len(flags), flags.discard(Flag.First))?
    drop(flags)
    Ok(())
"#
        ),
        "AllocError.CapacityOverflow\nAllocError.CapacityOverflow\nAllocError.CapacityOverflow\nAllocError.CapacityOverflow\nrefused\nAllocError.OutOfMemory\nrefused\nAllocError.OutOfMemory\nrefused\nAllocError.OutOfMemory\n2 True\nbudget 0 releases 1\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn selected_system_hash_constructors_propagate_partial_allocation_failure() {
    assert_stdout(
        r#"import std.alloc as alloc

def main() -> Result[(), IoError]:
    owner = alloc.system()
    print("__test_fail_allocations_after_1__")?
    data = owner.dict_with_capacity[i64, i64](8)
    print("__test_restore_allocations__")?
    print("__test_fail_allocations_after_1__")?
    keys = owner.set_with_capacity[i64](8)
    print("__test_restore_allocations__")?
    print(data.is_err(), keys.is_err())?
    Ok(())
"#,
        "__test_fail_allocations_after_1__\n__test_restore_allocations__\n__test_fail_allocations_after_1__\n__test_restore_allocations__\nTrue True\n",
    );
}

#[test]
fn selected_dict_set_reject_invalid_types_and_private_intrinsic() {
    for (call, reason) in [
        ("owner.empty_dict[f64, str]()", "KeyType"),
        ("owner.empty_set[list[i64]]()", "KeyType"),
        ("owner.empty_dict[str, Generator[i64]]()", "generator"),
        ("owner.empty_dict[str, &i64]()", "reference"),
        (
            "owner.dict_with_capacity[str, list[Sender[i64]]](0)",
            "hidden task/channel ownership",
        ),
        (
            "__allocator_dict_set[set[i64]](0, 0)",
            "reserved for the standard library",
        ),
    ] {
        let error = support::check_error(&format!(
            "import std.alloc as alloc\ndef main() -> ():\n    owner = alloc.system()\n    drop({call})\n"
        ));
        assert!(error.contains(reason), "{call}: {error}");
    }
}

#[test]
fn selected_dict_still_rejects_worker_local_values() {
    let error = support::check_error(
        r#"import std.alloc as alloc

protocol Id:
    def id(self) -> i64:
        pass

def work(data: dict[i64, Box[Id]]) -> ():
    drop(data)

def main() -> ():
    owner = alloc.system()
    data = owner.empty_dict[i64, Box[Id]]().unwrap()
    with spawn(work, data).drop_job().unwrap() as task:
        task.join()
"#,
    );
    assert!(
        error.contains("not certified for worker threads"),
        "{error}"
    );
}

#[test]
fn selected_dict_set_in_unused_bodies_exclude_actors() {
    for call in [
        "owner.empty_dict[str, i64]()",
        "owner.set_with_capacity[i64](8)",
    ] {
        let error = support::check_error(&format!(
            "import std.alloc as alloc\nimport std.actor as actors\ndef unused(owner: &alloc.Allocator) -> ():\n    drop({call})\ndef main() -> ():\n    drop(actors.heaps_live())\n"
        ));
        assert!(
            error.contains("retained allocator use cannot be combined"),
            "{error}"
        );
    }
}
